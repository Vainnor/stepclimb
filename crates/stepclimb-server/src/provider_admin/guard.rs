// SPDX-License-Identifier: AGPL-3.0-or-later
//! The rules a provider write must pass beyond `provider.manage`, each read under the
//! organization row's lock the write holds (spec 06 I5; module docs of [`super`]).

use std::collections::BTreeSet;

use axum::http::StatusCode;
use serde_json::{Map, Value};
use sqlx::PgConnection;
use stepclimb_auth::provider_config::TRUSTED_FOR_EMAIL_MATCHING;
use stepclimb_core::{
    OrgScope,
    admission_rules::{self, Stored},
    admission_settings,
    grants::{Grants, Target},
    identity_providers::{self, IdentityProvider, Kind},
    local_sign_in,
    perm::{AdmissionManage, Permission},
};

use super::code::PROVIDER_ATTRIBUTE_IN_USE;
use crate::{
    error::{ApiError, FieldError},
    local_sign_in::code::LOCAL_LAST_METHOD,
    roles::refuse_unless_holds_every_permission,
};

/// The members of `claims` an email match reads besides the anchors.
const EMAIL_CLAIMS: [&str; 2] = ["/claims/email", "/claims/email_verified"];

const TRUST_DETAIL: &str = "A provider trusted for email matching can sign people in as the existing users whose verified email it asserts, including users with permissions you do not hold. Only someone holding every permission can trust a provider, or change the issuer, endpoints, subject or email claims of a trusted one.";

const ANCHOR_DETAIL: &str = "Identities link users to this provider. Changing its issuer, endpoints or subject would sign them in as whoever the new provider says, including users with permissions you do not hold. Only someone holding every permission can change them.";

const ATTRIBUTE_DETAIL: &str = "Admission rules test this attribute, so changing where it is read from changes who the rules let in. Changing the mapping of an attribute an admission rule tests also needs admission.manage.";

const SIGNUP_DETAIL: &str = "Sign-up is on, so everyone who can sign in through this provider becomes a user. Turning on a provider, or changing who can sign in through an enabled one, also needs admission.manage.";

/// A provider as a write would leave it.
pub(super) struct Proposed<'a> {
    pub kind: Kind,
    /// The whole stored config the write would store.
    pub config: &'a Map<String, Value>,
    pub enabled: bool,
}

/// A stored provider and how many identities link users to it.
pub(super) struct Current<'a> {
    pub provider: &'a IdentityProvider,
    pub identities: i64,
}

/// What the rules read about the organization and the caller.
pub(super) struct Guard<'a> {
    grants: &'a Grants,
    signup_allowed: bool,
    local_has_primary: bool,
    /// The organization's own admission rules, which every provider's sign-ins must pass.
    organization_rules: Stored,
}

impl<'a> Guard<'a> {
    /// Lock the organization's row until the transaction ends (the lock every settings write,
    /// and every provider write here, takes first) and read sign-up and the local methods
    /// under it.
    pub(super) async fn lock(
        conn: &mut PgConnection,
        scope: OrgScope,
        grants: &'a Grants,
    ) -> Result<Self, ApiError> {
        let local = local_sign_in::get_for_update(&mut *conn, scope)
            .await
            .map_err(|e| ApiError::internal(&e))?;
        let switches = admission_settings::get(&mut *conn, scope)
            .await
            .map_err(|e| ApiError::internal(&e))?;
        let organization_rules = admission_rules::organization_list(&mut *conn, scope)
            .await
            .map_err(|e| ApiError::internal(&e))?;
        Ok(Self {
            grants,
            signup_allowed: switches.signup_allowed,
            local_has_primary: local.has_primary(),
            organization_rules,
        })
    }

    /// Refuse `after` unless the caller may make it, `before` being the stored row (`None` for
    /// a new provider):
    ///
    /// 1. changing what a provider with identities is (its kind, issuer, authorize origin,
    ///    token or userinfo URL, or subject pointer: [`anchors_changed`]) needs every
    ///    permission (403 `permission.escalation`);
    /// 2. trusting a provider for email matching, or changing those members or its email
    ///    claims while it is trusted, needs every permission (403 `permission.escalation`);
    /// 3. with sign-up on, turning a provider on, or changing those members while it stays on,
    ///    needs `admission.manage` too (403 `permission.denied`);
    /// 4. an attribute an admission rule tests ([`Guard::check_attributes`]) cannot be removed
    ///    (422), and mapping it differently needs `admission.manage` (403 `permission.denied`).
    pub(super) fn check(
        &self,
        before: Option<&Current<'_>>,
        after: &Proposed<'_>,
    ) -> Result<(), ApiError> {
        let anchors_changed = before.is_some_and(|c| anchors_changed(c.provider, after));
        if anchors_changed && before.is_some_and(|c| c.identities > 0) {
            refuse_unless_holds_every_permission(self.grants, ANCHOR_DETAIL)?;
        }
        let newly_trusted = before.is_none_or(|c| {
            !trusted(&c.provider.config)
                || anchors_changed
                || email_claims_changed(c.provider, after)
        });
        if trusted(after.config) && newly_trusted {
            refuse_unless_holds_every_permission(self.grants, TRUST_DETAIL)?;
        }
        let newly_open = before.is_none_or(|c| !c.provider.enabled || anchors_changed);
        if self.signup_allowed && after.enabled && newly_open && !self.holds::<AdmissionManage>() {
            return Err(ApiError::new(
                StatusCode::FORBIDDEN,
                crate::require::code::PERMISSION_DENIED,
                SIGNUP_DETAIL,
            ));
        }
        self.check_attributes(before.map(|c| c.provider), after)
    }

    /// The attributes the organization's rules and `before`'s own rules test are read from
    /// where the provider maps them, so a different mapping changes who the rules let in.
    /// Removing one answers 422 `provider.config_invalid` with `provider.attribute_in_use` at
    /// each; mapping one differently, a new provider's mapping included, needs
    /// `admission.manage` (403 `permission.denied`). A list that does not parse could test
    /// any attribute, so it counts as testing them all.
    fn check_attributes(
        &self,
        before: Option<&IdentityProvider>,
        after: &Proposed<'_>,
    ) -> Result<(), ApiError> {
        let tested = self.tested(before);
        let empty = Map::new();
        let stored = before.and_then(|p| mapped(&p.config)).unwrap_or(&empty);
        let proposed = mapped(after.config).unwrap_or(&empty);
        let removed: Vec<FieldError> = stored
            .keys()
            .filter(|name| tested.contains(name) && !proposed.contains_key(*name))
            .take(super::MAX_ERRORS)
            .map(|name| {
                let path = ["config", "claims", "attributes", name.as_str()];
                FieldError::at(&path, PROVIDER_ATTRIBUTE_IN_USE)
            })
            .collect();
        if !removed.is_empty() {
            return Err(ApiError::new(
                StatusCode::UNPROCESSABLE_ENTITY,
                super::code::PROVIDER_CONFIG_INVALID,
                "Admission rules test these attributes, so the provider must keep mapping them. Change the rules first.",
            )
            .with_errors(removed));
        }
        let remapped = proposed
            .iter()
            .any(|(name, pointer)| tested.contains(name) && stored.get(name) != Some(pointer));
        if remapped && !self.holds::<AdmissionManage>() {
            return Err(ApiError::new(
                StatusCode::FORBIDDEN,
                crate::require::code::PERMISSION_DENIED,
                ATTRIBUTE_DETAIL,
            ));
        }
        Ok(())
    }

    /// The attribute names the organization's rules and `provider`'s own rules test.
    fn tested(&self, provider: Option<&IdentityProvider>) -> Tested {
        let own = provider.map_or(Ok(Vec::new()), admission_rules::provider_rules);
        match (&self.organization_rules, &own) {
            (Ok(organization), Ok(own)) => Tested::Names(
                admission_rules::attribute_names(organization)
                    .into_iter()
                    .chain(admission_rules::attribute_names(own))
                    .map(str::to_owned)
                    .collect(),
            ),
            _ => Tested::Every,
        }
    }

    fn holds<P: Permission>(&self) -> bool {
        self.grants.holds_key(P::KEY, &Target::Organization)
    }

    /// 409 `local.last_method` when `current` is enabled, the write leaves it off (`still_on`
    /// false: disabled or deleted), the local methods offer no primary way in, and no other
    /// outside provider of the organization is enabled (`04-local-accounts.md` B2's rule, from
    /// the providers' side).
    pub(super) async fn refuse_lockout(
        &self,
        conn: &mut PgConnection,
        scope: OrgScope,
        current: &IdentityProvider,
        still_on: bool,
    ) -> Result<(), ApiError> {
        if !current.enabled || still_on || self.local_has_primary {
            return Ok(());
        }
        let others = identity_providers::list_enabled(&mut *conn, scope)
            .await
            .map_err(|e| ApiError::internal(&e))?
            .into_iter()
            .filter(|p| p.id != current.id)
            .count();
        if others > 0 {
            return Ok(());
        }
        Err(ApiError::new(
            StatusCode::CONFLICT,
            LOCAL_LAST_METHOD,
            "This is the organization's last way to sign in: no local method is offered and no other provider is enabled. Turn on a local method or another provider first.",
        ))
    }
}

/// The attribute names admission rules test.
enum Tested {
    Names(BTreeSet<String>),
    /// A list does not parse, so any attribute may be tested.
    Every,
}

impl Tested {
    fn contains(&self, name: &str) -> bool {
        match self {
            Self::Names(names) => names.contains(name),
            Self::Every => true,
        }
    }
}

/// `config`'s `claims.attributes`: attribute names to JSON Pointers, as the config types write
/// them for both kinds; `None` when absent.
fn mapped(config: &Map<String, Value>) -> Option<&Map<String, Value>> {
    config
        .get("claims")
        .and_then(|claims| claims.get("attributes"))
        .and_then(Value::as_object)
}

/// Whether `config` trusts the provider for email matching: the member is exactly `true`, as
/// admission reads it (`07-admission.md` B5).
pub(super) fn trusted(config: &Map<String, Value>) -> bool {
    config.get(TRUSTED_FOR_EMAIL_MATCHING) == Some(&Value::Bool(true))
}

/// Whether `after` changes what `before` is: the anchors `stepclimb.toml` may not change
/// under identities (`crate::config_providers::changed_anchors`: the kind, the `oidc` issuer,
/// the `oauth2` endpoints' origins and subject pointer), and here also the whole `oauth2`
/// token and userinfo URLs. Those two decide whose identity a sign-in reads, and one host can
/// serve another account's user endpoint under a different path; the file keeps comparing
/// origins, since its operator owns the instance.
fn anchors_changed(before: &IdentityProvider, after: &Proposed<'_>) -> bool {
    let changed = crate::config_providers::changed_anchors(
        before.kind,
        &before.config,
        after.kind,
        after.config,
    );
    !changed.is_empty() || (before.kind == Kind::Oauth2 && endpoint_urls_changed(before, after))
}

/// Whether the `oauth2` `token` or `userinfo` URL differs, compared whole after parsing
/// (scheme, host, port, path and query), or the stored one does not parse.
fn endpoint_urls_changed(before: &IdentityProvider, after: &Proposed<'_>) -> bool {
    let url = |config: &Map<String, Value>, name: &str| {
        config
            .get(name)
            .and_then(Value::as_str)
            .and_then(|text| url::Url::parse(text).ok())
    };
    ["token", "userinfo"].iter().any(|name| {
        let stored = url(&before.config, name);
        stored.is_none() || stored != url(after.config, name)
    })
}

fn email_claims_changed(before: &IdentityProvider, after: &Proposed<'_>) -> bool {
    let stored = Value::Object(before.config.clone());
    let proposed = Value::Object(after.config.clone());
    EMAIL_CLAIMS
        .iter()
        .any(|pointer| stored.pointer(pointer) != proposed.pointer(pointer))
}

#[cfg(test)]
mod tests {
    use serde_json::json;
    use stepclimb_core::{
        id::{Uuid, new_id},
        perm::{self, ProviderManage},
        role_assignments::{GrantScope, UserGrant},
    };

    use super::*;

    #[expect(
        clippy::disallowed_methods,
        reason = "unit tests of the provider rules need grants and have no credential"
    )]
    fn grants(keys: &[&str]) -> Grants {
        let grant = UserGrant {
            assignment_id: new_id(),
            role_id: new_id(),
            role_key: "custom".into(),
            permissions: keys.iter().map(|&k| k.to_owned()).collect(),
            scope: GrantScope::Organization,
        };
        Grants::from_assignments(OrgScope::instance(new_id()), [grant])
    }

    fn every() -> Vec<&'static str> {
        perm::ALL.iter().map(|p| p.key()).collect()
    }

    fn config(value: Value) -> Map<String, Value> {
        let Value::Object(map) = value else {
            panic!("an object")
        };
        map
    }

    fn sso(trusted: bool, email: &str) -> Map<String, Value> {
        config(json!({
            "issuer": "https://idp.example.test/realm",
            "claims": { "email": email, "email_verified": "/email_verified" },
            TRUSTED_FOR_EMAIL_MATCHING: trusted,
            "label": "SSO",
        }))
    }

    fn stored(config: Map<String, Value>, enabled: bool) -> IdentityProvider {
        IdentityProvider {
            id: Uuid::nil(),
            key: "sso".into(),
            kind: Kind::Oidc,
            config,
            enabled,
            created_at: std::time::SystemTime::UNIX_EPOCH,
            updated_at: std::time::SystemTime::UNIX_EPOCH,
        }
    }

    fn guard(grants: &Grants, signup_allowed: bool) -> Guard<'_> {
        Guard {
            grants,
            signup_allowed,
            local_has_primary: true,
            organization_rules: Ok(Vec::new()),
        }
    }

    fn code(result: Result<(), ApiError>) -> Option<&'static str> {
        result.err().map(|e| e.code())
    }

    const ESCALATION: Option<&str> = Some("permission.escalation");

    /// Trusting a provider for email matching, or changing its email claims or anchors while
    /// trusted, needs every permission.
    #[test]
    fn trust_for_email_matching_needs_every_permission() {
        let manager = grants(&[ProviderManage::KEY]);
        let admin = grants(&every());
        let trusted = sso(true, "/email");
        let new = Proposed {
            kind: Kind::Oidc,
            config: &trusted,
            enabled: false,
        };
        assert_eq!(code(guard(&manager, false).check(None, &new)), ESCALATION);
        assert_eq!(code(guard(&admin, false).check(None, &new)), None);

        let untrusted = stored(sso(false, "/email"), false);
        let turned_on = Current {
            provider: &untrusted,
            identities: 0,
        };
        assert_eq!(
            code(guard(&manager, false).check(Some(&turned_on), &new)),
            ESCALATION
        );

        let row = stored(sso(true, "/email"), false);
        let current = Current {
            provider: &row,
            identities: 0,
        };
        let relabelled = sso(true, "/email");
        let same = Proposed {
            config: &relabelled,
            ..new
        };
        assert_eq!(
            code(guard(&manager, false).check(Some(&current), &same)),
            None
        );
        let other_claim = sso(true, "/mail");
        let changed = Proposed {
            config: &other_claim,
            ..new
        };
        assert_eq!(
            code(guard(&manager, false).check(Some(&current), &changed)),
            ESCALATION
        );
        let mut other_issuer = sso(true, "/email");
        other_issuer.insert("issuer".into(), json!("https://other.example.test"));
        let moved = Proposed {
            config: &other_issuer,
            ..new
        };
        assert_eq!(
            code(guard(&manager, false).check(Some(&current), &moved)),
            ESCALATION
        );
        let untrusting = sso(false, "/mail");
        let off = Proposed {
            config: &untrusting,
            ..new
        };
        assert_eq!(
            code(guard(&manager, false).check(Some(&current), &off)),
            None
        );
    }

    #[test]
    fn a_linked_providers_anchors_need_every_permission_and_its_label_does_not() {
        let manager = grants(&[ProviderManage::KEY]);
        let row = stored(sso(false, "/email"), false);
        let linked = Current {
            provider: &row,
            identities: 1,
        };
        let mut moved = sso(false, "/email");
        moved.insert("issuer".into(), json!("https://other.example.test"));
        let proposed = |config| Proposed {
            kind: Kind::Oidc,
            config,
            enabled: false,
        };
        assert_eq!(
            code(guard(&manager, false).check(Some(&linked), &proposed(&moved))),
            ESCALATION
        );
        let unlinked = Current {
            identities: 0,
            ..linked
        };
        assert_eq!(
            code(guard(&manager, false).check(Some(&unlinked), &proposed(&moved))),
            None
        );
        let same = sso(false, "/mail");
        assert_eq!(
            code(guard(&manager, false).check(Some(&linked), &proposed(&same))),
            None
        );
    }

    #[test]
    fn with_sign_up_on_opening_a_provider_needs_admission_manage() {
        let manager = grants(&[ProviderManage::KEY]);
        let both = grants(&[ProviderManage::KEY, AdmissionManage::KEY]);
        let config = sso(false, "/email");
        let on = Proposed {
            kind: Kind::Oidc,
            config: &config,
            enabled: true,
        };
        let denied = Some("permission.denied");
        assert_eq!(code(guard(&manager, true).check(None, &on)), denied);
        assert_eq!(code(guard(&both, true).check(None, &on)), None);
        assert_eq!(code(guard(&manager, false).check(None, &on)), None);
        let off = Proposed {
            enabled: false,
            ..on
        };
        assert_eq!(code(guard(&manager, true).check(None, &off)), None);
        let row = stored(sso(false, "/email"), true);
        let enabled = Current {
            provider: &row,
            identities: 0,
        };
        assert_eq!(code(guard(&manager, true).check(Some(&enabled), &on)), None);
    }

    fn oauth2(token: &str, userinfo: &str) -> Map<String, Value> {
        config(json!({
            "authorize": "https://accounts.example.test/oauth/authorize",
            "token": token,
            "userinfo": userinfo,
            "claims": { "subject": "/data/cid" },
            "label": "Accounts",
        }))
    }

    fn stored_oauth2(config: Map<String, Value>) -> IdentityProvider {
        IdentityProvider {
            kind: Kind::Oauth2,
            ..stored(config, false)
        }
    }

    /// One host can serve another account's user endpoint under another path, so the API
    /// compares the token and userinfo URLs whole, not only their origins.
    #[test]
    fn a_same_origin_endpoint_path_change_is_an_anchor_change() {
        let manager = grants(&[ProviderManage::KEY]);
        let token = "https://accounts.example.test/oauth/token";
        let userinfo = "https://accounts.example.test/api/user";
        let row = stored_oauth2(oauth2(token, userinfo));
        let linked = Current {
            provider: &row,
            identities: 1,
        };
        let check = |config: &Map<String, Value>| {
            let after = Proposed {
                kind: Kind::Oauth2,
                config,
                enabled: false,
            };
            code(guard(&manager, false).check(Some(&linked), &after))
        };
        assert_eq!(check(&oauth2(token, userinfo)), None);
        for (token, userinfo) in [
            (token, "https://accounts.example.test/api/other-user"),
            (token, "https://accounts.example.test/api/user?id=1"),
            ("https://accounts.example.test/other/token", userinfo),
            (token, "https://accounts.example.test:8443/api/user"),
        ] {
            assert_eq!(
                check(&oauth2(token, userinfo)),
                ESCALATION,
                "{token} {userinfo}"
            );
        }
        let mut authorize = oauth2(token, userinfo);
        authorize.insert(
            "authorize".into(),
            json!("https://accounts.example.test/other/authorize"),
        );
        assert_eq!(check(&authorize), None);
    }

    fn mapping(attributes: Value) -> Map<String, Value> {
        let mut config = sso(false, "/email");
        config["claims"]["attributes"] = attributes;
        config
    }

    fn with_rules(grants: &Grants, organization_rules: Stored) -> Guard<'_> {
        Guard {
            organization_rules,
            ..guard(grants, false)
        }
    }

    fn rules(value: Value) -> Vec<admission_rules::Rule> {
        let Value::Array(list) = value else {
            panic!("a list")
        };
        admission_rules::parse_list(&list, &[], None).expect("valid rules")
    }

    /// An attribute an admission rule tests: removing it is 422, mapping it differently needs
    /// `admission.manage`; an attribute no rule tests needs `provider.manage` alone.
    #[test]
    fn an_attribute_a_rule_tests_needs_admission_manage_and_cannot_be_removed() {
        let manager = grants(&[ProviderManage::KEY]);
        let both = grants(&[ProviderManage::KEY, AdmissionManage::KEY]);
        let mut own = mapping(json!({ "rating": "/rating", "team": "/team" }));
        own.insert(
            admission_rules::PROVIDER_MEMBER.into(),
            json!([{ "attribute": "team", "op": "exists" }]),
        );
        let row = stored(own, false);
        let current = Current {
            provider: &row,
            identities: 0,
        };
        let organization = rules(json!([{ "attribute": "rating", "op": "in", "value": ["S3"] }]));
        let check = |grants: &Grants, attributes: Value| {
            let config = mapping(attributes);
            let after = Proposed {
                kind: Kind::Oidc,
                config: &config,
                enabled: false,
            };
            with_rules(grants, Ok(organization.clone()))
                .check(Some(&current), &after)
                .err()
        };
        let denied = Some("permission.denied");
        let unchanged = json!({ "rating": "/rating", "team": "/team" });
        assert_eq!(check(&manager, unchanged).map(|e| e.code()), None);
        let added = json!({ "rating": "/rating", "team": "/team", "region": "/region" });
        assert_eq!(check(&manager, added).map(|e| e.code()), None);
        for remapped in [
            json!({ "rating": "/always-s3", "team": "/team" }),
            json!({ "rating": "/rating", "team": "/everyone" }),
        ] {
            assert_eq!(check(&manager, remapped.clone()).map(|e| e.code()), denied);
            assert_eq!(check(&both, remapped).map(|e| e.code()), None);
        }
        let removed = check(&both, json!({ "team": "/team" })).expect("refused");
        assert_eq!(removed.code(), "provider.config_invalid");
        assert_eq!(removed.status(), StatusCode::UNPROCESSABLE_ENTITY);

        let unreadable = with_rules(&manager, Err(admission_rules::Unreadable("org".into())));
        let config = mapping(json!({ "rating": "/rating", "team": "/team", "region": "/r" }));
        let after = Proposed {
            kind: Kind::Oidc,
            config: &config,
            enabled: false,
        };
        assert_eq!(code(unreadable.check(Some(&current), &after)), denied);
    }

    /// A new provider mapping an attribute the organization's rules test needs
    /// `admission.manage`: its mapping decides what those rules read for its sign-ins.
    #[test]
    fn a_new_provider_mapping_a_tested_attribute_needs_admission_manage() {
        let manager = grants(&[ProviderManage::KEY]);
        let both = grants(&[ProviderManage::KEY, AdmissionManage::KEY]);
        let organization = rules(json!([{ "attribute": "rating", "op": "exists" }]));
        let tested = mapping(json!({ "rating": "/rating" }));
        let untested = mapping(json!({ "region": "/region" }));
        let new = |config| Proposed {
            kind: Kind::Oidc,
            config,
            enabled: false,
        };
        let with = |grants| with_rules(grants, Ok(organization.clone()));
        let denied = Some("permission.denied");
        assert_eq!(code(with(&manager).check(None, &new(&tested))), denied);
        assert_eq!(code(with(&both).check(None, &new(&tested))), None);
        assert_eq!(code(with(&manager).check(None, &new(&untested))), None);
    }
}
