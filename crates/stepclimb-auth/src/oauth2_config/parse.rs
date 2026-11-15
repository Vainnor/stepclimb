// SPDX-License-Identifier: AGPL-3.0-or-later
//! The rules of the module docs' table, member by member, collecting every problem.

use serde_json::{Map, Value};

use super::{
    ConfigErrors, Oauth2Claims, Oauth2Config, Oauth2ConfigDraft, Oauth2Settings, SealedSecret,
    StoredConfigError, TRUSTED_FOR_EMAIL_MATCHING, code,
};
use crate::claims::ProfileMapping;
use crate::outbound::UrlPolicy;
use crate::provider_config::walk::{self, Walk};

/// The members of an `oauth2` config.
const MEMBERS: [&str; 10] = [
    "authorize",
    "token",
    "userinfo",
    "client_id",
    "client_secret",
    "scopes",
    "pkce",
    "claims",
    "allow_local_passkeys",
    TRUSTED_FOR_EMAIL_MATCHING,
];

/// The members of `claims`.
const CLAIMS: [&str; 7] = [
    "subject",
    "given_name",
    "family_name",
    "email",
    "email_verified",
    "groups",
    "attributes",
];

pub(super) fn draft(
    config: &Value,
    policy: UrlPolicy,
    stored: Option<SealedSecret>,
) -> Result<Oauth2ConfigDraft, ConfigErrors> {
    let mut walk = Walk::default();
    let Some(map) = walk::object(&mut walk, config) else {
        return walk.finish(None);
    };
    walk::unknown_members(&mut walk, &[], map, &MEMBERS);
    let settings = settings(&mut walk, map, Some(policy));
    let secret = walk::input_secret(&mut walk, map.get("client_secret"), stored);
    let draft = settings
        .zip(secret)
        .map(|(settings, secret)| Oauth2ConfigDraft { settings, secret });
    walk.finish(draft)
}

pub(super) fn stored(config: &Map<String, Value>) -> Result<Oauth2Config, StoredConfigError> {
    let mut walk = Walk::default();
    let settings = settings(&mut walk, config, None);
    let secret = walk::stored_secret(&mut walk, config);
    let config = settings
        .zip(secret)
        .map(|(settings, secret)| Oauth2Config { settings, secret });
    walk.finish_stored(config)
}

/// Every member but the secret. `policy` is `None` for a stored config.
fn settings(
    walk: &mut Walk,
    map: &Map<String, Value>,
    policy: Option<UrlPolicy>,
) -> Option<Oauth2Settings> {
    let endpoint = |walk: &mut Walk, name| walk::url(walk, name, map.get(name), policy, true);
    let authorize = endpoint(walk, "authorize").filter(|url| no_reserved_parameter(walk, url));
    let token = endpoint(walk, "token");
    let userinfo = endpoint(walk, "userinfo");
    let client_id = walk::required_text(walk, &["client_id"], map.get("client_id"));
    let scopes = walk::scopes(walk, map.get("scopes"), &[]);
    let pkce = walk::flag(walk, "pkce", map.get("pkce"), true);
    let claims = claims(walk, map.get("claims"));
    let allow_local_passkeys = walk::flag(
        walk,
        "allow_local_passkeys",
        map.get("allow_local_passkeys"),
        false,
    );
    let trusted_for_email_matching = walk::flag(
        walk,
        TRUSTED_FOR_EMAIL_MATCHING,
        map.get(TRUSTED_FOR_EMAIL_MATCHING),
        false,
    );
    Some(Oauth2Settings {
        authorize: authorize?,
        token: token?,
        userinfo: userinfo?,
        client_id: client_id?,
        scopes: scopes?,
        pkce: pkce?,
        claims: claims?,
        allow_local_passkeys: allow_local_passkeys?,
        trusted_for_email_matching: trusted_for_email_matching?,
    })
}

/// The parameters the authorization request sets (spec 06 B5; `05-oidc.md` B5 for `nonce`). One
/// already in the `authorize` URL's query would be sent twice, and a provider may read either.
pub(crate) const RESERVED_PARAMETERS: [&str; 8] = [
    "response_type",
    "client_id",
    "redirect_uri",
    "scope",
    "state",
    "code_challenge",
    "code_challenge_method",
    "nonce",
];

/// False, with a problem, when the query of `authorize` (already a valid URL) names a
/// [`RESERVED_PARAMETERS`] entry, compared after percent-decoding so `%73tate` is `state`.
fn no_reserved_parameter(walk: &mut Walk, authorize: &str) -> bool {
    let reserved = url::Url::parse(authorize).is_ok_and(|url| {
        url.query_pairs()
            .any(|(name, _)| RESERVED_PARAMETERS.contains(&name.as_ref()))
    });
    if reserved {
        walk.fail(&["authorize"], code::URL_PARAMETER_RESERVED);
    }
    !reserved
}

fn claims(walk: &mut Walk, value: Option<&Value>) -> Option<Oauth2Claims> {
    let map = match value {
        None => {
            walk.fail(&["claims"], code::FIELD_REQUIRED);
            return None;
        }
        Some(Value::Object(map)) => map,
        Some(_) => {
            walk.fail(&["claims"], code::FIELD_TYPE);
            return None;
        }
    };
    walk::unknown_members(walk, &["claims"], map, &CLAIMS);
    let subject = match map.get("subject") {
        None => {
            walk.fail(&["claims", "subject"], code::FIELD_REQUIRED);
            None
        }
        Some(value) => walk::pointer(walk, &["claims", "subject"], value),
    };
    let given_name = walk::optional_pointer(walk, map, "given_name");
    let family_name = walk::optional_pointer(walk, map, "family_name");
    let email = walk::optional_pointer(walk, map, "email");
    let email_verified = walk::optional_pointer(walk, map, "email_verified");
    let groups = walk::optional_pointer(walk, map, "groups");
    let attributes = walk::attributes(walk, map.get("attributes"));
    Some(Oauth2Claims {
        subject: subject?,
        profile: ProfileMapping {
            given_name: given_name?,
            family_name: family_name?,
            email: email?,
            email_verified: email_verified?,
            groups: groups?,
            attributes: attributes?,
        },
    })
}
