// SPDX-License-Identifier: AGPL-3.0-or-later
//! The admission switches and rules under `/api/v1/admin/admission`
//! (docs/specs/m0/07-admission.md B8 to B10, B12, B23, Q1, Q2, API).
//!
//! Reading and changing them need `admission.manage` at organization scope, the one permission
//! that covers who may sign in (Q2 (b)); the generic settings `PATCH` refuses the `admission`
//! key as `settings.read_only`. A `PUT` names only what it changes: an absent switch, an absent
//! `rules`, an absent `rules.organization` or `rules.providers`, and a provider `rules.providers`
//! does not name each keep their stored value, so one admin's change cannot undo another's made
//! in the meantime. A list that is named is replaced whole.
//!
//! The switches go through the repository's `admission_settings::update`, which applies them
//! under the organization row's lock and writes, with its `organization.update_settings`
//! entry, only a change; it refuses to turn `sync_only` on (422
//! `admission.sync_only_unavailable`) until admission applies it. No escalation rule applies to
//! them: they decide whether a person the organization has no user for is let in, and never
//! how an existing user signs in or what they may do, since a just-in-time user gets no role.
//!
//! The rules go through `admission_rules`: the lists are read under the organization row's and
//! the providers' row locks, the change is checked against them and against the attributes
//! each provider's config maps (422 `request.invalid` listing at most [`MAX_ERRORS`] problems),
//! and only the lists that differ are written, each with its audit entry. Rules run on every
//! sign-in through an outside provider (Q3 (a)), so a change can refuse users who hold
//! permissions the caller lacks: a body that names any list is refused with 403
//! `permission.escalation`, before the lists are read or checked, unless the caller holds
//! every catalog permission at organization scope, the rule of `PUT
//! /api/v1/admin/local-sign-in`. A stored list that does not parse makes `GET` a 500; a `PUT`
//! that names it replaces it.

use std::collections::{BTreeMap, BTreeSet};

use axum::{Json, extract::State, http::StatusCode};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sqlx::PgConnection;
use stepclimb_auth::{oauth2_config::Oauth2Config, oidc_config::OidcConfig};
use stepclimb_core::{
    OrgScope,
    admission_rules::{self as rules_repo, Problem, RuleSets, RulesInput, Stored, Unreadable},
    admission_settings::{self as repo, AdmissionPatch, AdmissionSwitches},
    audit::Actor,
    grants::Grants,
    identity_providers::{IdentityProvider, Kind},
    perm::AdmissionManage,
};
use utoipa::ToSchema;

use crate::{
    AppState,
    audit::ClientAddress,
    error::{ApiError, FieldError},
    extract::ApiJson,
    openapi::problem_responses,
    organization::present,
    request_org,
    require::Require,
    roles::refuse_unless_holds_every_permission,
};

/// The path of the admission settings.
pub const ADMISSION_PATH: &str = "/api/v1/admin/admission";

/// The most problems one 422 lists.
pub const MAX_ERRORS: usize = 20;

/// The codes this module produces.
pub mod code {
    /// In a 422's `errors`: `sync_only` cannot be turned on before admission applies it.
    pub const SYNC_ONLY_UNAVAILABLE: &str = "admission.sync_only_unavailable";
    pub use stepclimb_core::admission_rules::code::{
        ATTRIBUTE_UNKNOWN, PROVIDER_UNKNOWN, RULE_INVALID,
    };
}

problem_responses! {
    AdmissionWriteForbidden => "`permission.denied`: the caller does not hold `admission.manage` at organization scope. `permission.escalation`: the body names a rule list, which applies to every user who signs in through an outside provider, and the caller does not hold every catalog permission at organization scope. `auth.csrf_refused`: the request did not come from this site.",
    AdmissionInvalid => "`request.invalid`: the change breaks a rule; `errors` lists each one (at most 20), each with a JSON Pointer to the member. `admission.sync_only_unavailable` marks `sync_only` turned on, which this release refuses. `admission.rule_invalid` marks a rule that is not one of the documented forms or breaks a limit; `admission.attribute_unknown` an attribute no provider the list applies to maps; `admission.provider_unknown` a key in `rules.providers` that is not an outside provider of the organization.",
}

/// Who admission lets in.
#[derive(Clone, Debug, PartialEq, Serialize, ToSchema)]
pub struct AdmissionSettings {
    /// Whether a person the organization has no user for is created as a user when they first
    /// sign in through an identity provider. Off, they are refused with `auth.signup_closed`.
    pub signup_allowed: bool,
    /// Whether only users already provisioned may sign in. This release accepts only `false`:
    /// `true` answers 422 with `admission.sync_only_unavailable`.
    pub sync_only: bool,
    pub rules: AdmissionRules,
}

impl AdmissionSettings {
    /// The answer for `switches` and `sets`; a 500 when a list in `sets` does not parse.
    fn new(switches: AdmissionSwitches, sets: &RuleSets) -> Result<Self, ApiError> {
        let AdmissionSwitches {
            signup_allowed,
            sync_only,
        } = switches;
        Ok(Self {
            signup_allowed,
            sync_only,
            rules: AdmissionRules::of(sets)?,
        })
    }
}

/// The rules every sign-in through an outside provider must pass (Q3 (a)): every rule of the
/// organization's list and of the provider's own list. A sign-in with a local account runs no
/// rules. A refused person is answered `auth.admission_refused`.
#[derive(Clone, Debug, PartialEq, Serialize, ToSchema)]
pub struct AdmissionRules {
    /// The rules every outside provider's sign-ins must pass. Their attributes must be ones
    /// some provider maps; a provider that does not map one fails the rule.
    #[schema(value_type = Vec<AdmissionRule>)]
    pub organization: Value,
    /// Each outside provider of the organization, enabled or not, by key, with the rules its
    /// sign-ins must pass besides the organization's; `[]` when it has none.
    #[schema(value_type = BTreeMap<String, Vec<AdmissionRule>>)]
    pub providers: BTreeMap<String, Value>,
}

impl AdmissionRules {
    /// The lists of `sets`; a 500 when one does not parse.
    fn of(sets: &RuleSets) -> Result<Self, ApiError> {
        sets.check().map_err(rules_error)?;
        let json = |stored: &Stored| {
            stored
                .as_ref()
                .map(|rules| rules_repo::to_json(rules))
                .map_err(|Unreadable(at)| rules_error(rules_repo::Error::StoredRules(at.clone())))
        };
        let providers = sets
            .providers
            .iter()
            .map(|p| Ok((p.provider.key.clone(), json(&p.rules)?)))
            .collect::<Result<_, ApiError>>()?;
        Ok(Self {
            organization: json(&sets.organization)?,
            providers,
        })
    }
}

/// One admission rule: a condition on one mapped attribute, or a group of rules. An attribute
/// that is absent or `null` fails its condition, whatever the `op`; one of a type the rule
/// cannot compare refuses the sign-in.
//
// The document's form of `stepclimb_core::admission_rules::Rule`; requests are parsed by
// `admission_rules::parse_list`, so each problem gets a pointer, and `tests/admission_rules.rs`
// checks answers against this schema.
#[derive(ToSchema)]
#[serde(untagged)]
pub enum AdmissionRule {
    Condition(AdmissionCondition),
    Any(AdmissionAny),
    All(AdmissionAll),
}

/// A test of one mapped attribute. Rules see attributes exactly as the provider's claims map
/// them, verified or not: an attribute mapped from an email claim holds the address the
/// provider sent even when it did not vouch for it. To rely on such a value, map the
/// provider's verification claim as an attribute too and require it alongside, as in
/// `{"attribute": "email_verified", "op": "equals", "value": true}`.
#[derive(ToSchema)]
pub struct AdmissionCondition {
    /// The attribute's name, as the provider's `claims.attributes` maps it; 1 to 1,000
    /// characters.
    pub attribute: String,
    pub op: AdmissionOp,
    /// `equals`: a string, integer or boolean. `in` and `not_in`: 1 to 100 of them, all of one
    /// type. `matches`: a regular expression (Rust `regex` syntax, at most 1,000 characters),
    /// matched against the whole value. `exists`: absent.
    pub value: Option<AdmissionValue>,
}

/// What a condition asks of its attribute.
#[derive(ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum AdmissionOp {
    /// The attribute is the value.
    Equals,
    /// The attribute is one of the values.
    In,
    /// The attribute is present and none of the values.
    NotIn,
    /// The attribute is present, with any value but `null`.
    Exists,
    /// The attribute is a string the pattern matches whole.
    Matches,
}

/// A condition's value.
#[derive(ToSchema)]
#[serde(untagged)]
pub enum AdmissionValue {
    Text(String),
    Integer(i64),
    Boolean(bool),
    List(Vec<AdmissionScalar>),
}

/// One value of an `in` or `not_in` list.
#[derive(ToSchema)]
#[serde(untagged)]
pub enum AdmissionScalar {
    Text(String),
    Integer(i64),
    Boolean(bool),
}

/// Holds when at least one of its 1 to 50 rules holds. Groups nest at most 4 deep.
#[derive(ToSchema)]
pub struct AdmissionAny {
    #[schema(no_recursion)]
    pub any: Vec<AdmissionRule>,
}

/// Holds when every one of its 1 to 50 rules holds. Groups nest at most 4 deep.
#[derive(ToSchema)]
pub struct AdmissionAll {
    #[schema(no_recursion)]
    pub all: Vec<AdmissionRule>,
}

/// A change to the admission switches and rules: send only what you change. An absent member
/// keeps its stored value, and `{}` changes nothing. A member sent as `null` is malformed: omit
/// it instead.
#[derive(Clone, Debug, Default, PartialEq, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct AdmissionSettingsPatch {
    /// Whether a person the organization has no user for is created as a user when they first
    /// sign in through an identity provider.
    #[serde(default, deserialize_with = "present")]
    #[schema(value_type = bool)]
    pub signup_allowed: Option<bool>,
    /// Whether only users already provisioned may sign in. This release accepts only `false`:
    /// `true` answers 422 with `admission.sync_only_unavailable`.
    #[serde(default, deserialize_with = "present")]
    #[schema(value_type = bool)]
    pub sync_only: Option<bool>,
    #[serde(default, deserialize_with = "present")]
    #[schema(value_type = AdmissionRulesPatch)]
    pub rules: Option<AdmissionRulesPatch>,
}

/// The rule lists to replace: each list named replaces the stored one whole, and a list not
/// named keeps its rules. At most 50 rules per list.
#[derive(Clone, Debug, Default, PartialEq, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct AdmissionRulesPatch {
    /// The organization's list.
    #[serde(default, deserialize_with = "present")]
    #[schema(value_type = Vec<AdmissionRule>)]
    pub organization: Option<Vec<Value>>,
    /// Provider lists by provider key; a provider not named keeps its list. `[]` empties one.
    #[serde(default, deserialize_with = "present")]
    #[schema(value_type = BTreeMap<String, Vec<AdmissionRule>>)]
    pub providers: Option<BTreeMap<String, Vec<Value>>>,
}

/// Read the organization's admission switches and rules.
#[utoipa::path(
    get,
    path = "/api/v1/admin/admission",
    operation_id = "getAdmissionSettings",
    tag = "admission",
    responses(
        (status = 200, description = "The admission switches and rules", body = AdmissionSettings),
        (status = 401, response = crate::openapi::Unauthenticated),
        (status = 403, response = crate::openapi::PermissionDenied),
        (status = 404, response = crate::openapi::OrganizationNotFound),
        (status = 500, response = crate::openapi::InternalError),
        (status = 503, response = crate::openapi::NoOrganization),
    )
)]
pub async fn get_admission_settings(
    require: Require<AdmissionManage>,
    State(state): State<AppState>,
) -> Result<Json<AdmissionSettings>, ApiError> {
    let scope = require.principal().scope();
    let mut conn = state
        .db
        .acquire()
        .await
        .map_err(|e| ApiError::internal(&e))?;
    let switches = repo::get(&mut *conn, scope)
        .await
        .map_err(admission_error)?;
    let sets = rules_repo::get(&mut conn, scope)
        .await
        .map_err(rules_error)?;
    Ok(Json(AdmissionSettings::new(switches, &sets)?))
}

/// Change the admission switches and rule lists the body names; send only what you change. A
/// member the body leaves out keeps its value, and a body naming only current values changes
/// nothing.
#[utoipa::path(
    put,
    path = "/api/v1/admin/admission",
    operation_id = "updateAdmissionSettings",
    tag = "admission",
    extensions(("x-stepclimb-mutates" = json!(true))),
    request_body = AdmissionSettingsPatch,
    responses(
        (status = 200, description = "The switches and rules after the change", body = AdmissionSettings),
        (status = 400, response = crate::openapi::BadRequest),
        (status = 401, response = crate::openapi::Unauthenticated),
        (status = 403, response = AdmissionWriteForbidden),
        (status = 404, response = crate::openapi::OrganizationNotFound),
        (status = 413, response = crate::openapi::PayloadTooLarge),
        (status = 415, response = crate::openapi::UnsupportedMediaType),
        (status = 422, response = AdmissionInvalid),
        (status = 500, response = crate::openapi::InternalError),
        (status = 503, response = crate::openapi::NoOrganization),
    )
)]
pub async fn update_admission_settings(
    require: Require<AdmissionManage>,
    client: ClientAddress,
    State(state): State<AppState>,
    ApiJson(body): ApiJson<AdmissionSettingsPatch>,
) -> Result<Json<AdmissionSettings>, ApiError> {
    let principal = require.principal();
    let scope = principal.scope();
    let actor = client.actor(principal);
    let now = state.clock.now();
    let AdmissionSettingsPatch {
        signup_allowed,
        sync_only,
        rules,
    } = body;
    let switches = AdmissionPatch {
        signup_allowed,
        sync_only,
    };
    let mut tx = state.db.begin().await.map_err(|e| ApiError::internal(&e))?;
    let switches = repo::update(&mut *tx, scope, switches, now, actor)
        .await
        .map_err(admission_error)?;
    let sets = match rules {
        Some(patch) => {
            let write = RulesWrite { scope, now, actor };
            write.apply(&mut tx, require.grants(), &patch).await?
        }
        None => rules_repo::get(&mut tx, scope).await.map_err(rules_error)?,
    };
    let answer = AdmissionSettings::new(switches, &sets)?;
    tx.commit().await.map_err(|e| ApiError::internal(&e))?;
    Ok(Json(answer))
}

/// Who changes the rules, where and when.
struct RulesWrite {
    scope: OrgScope,
    now: std::time::SystemTime,
    actor: Actor,
}

impl RulesWrite {
    /// Apply `patch` on `conn`, a transaction: refuse an escalation when the patch names a list,
    /// before anything is read, so a caller without every permission learns nothing about
    /// which attributes the providers map; then lock the lists, check the change and write the
    /// lists that differ. Returns every list after the change; a list that did not parse and
    /// that the patch replaced reads as its new rules.
    async fn apply(
        &self,
        conn: &mut PgConnection,
        grants: &Grants,
        patch: &AdmissionRulesPatch,
    ) -> Result<RuleSets, ApiError> {
        let names_a_list = patch.organization.is_some()
            || patch
                .providers
                .as_ref()
                .is_some_and(|lists| !lists.is_empty());
        if names_a_list {
            refuse_unless_holds_every_permission(grants, ESCALATION_DETAIL)?;
        }
        let current = rules_repo::lock(&mut *conn, self.scope)
            .await
            .map_err(rules_error)?;
        let input = RulesInput {
            organization: patch.organization.as_deref(),
            providers: patch.providers.as_ref(),
        };
        let change =
            rules_repo::validate(&current, input, mapped_attributes).map_err(rule_problems)?;
        if change.is_empty() {
            return Ok(current);
        }
        rules_repo::update(conn, self.scope, current, change, self.now, self.actor)
            .await
            .map_err(rules_error)
    }
}

/// The attribute names `provider`'s config maps (`05-oidc.md` B13,
/// `06-oauth2-and-vatsim-preset.md` B6); none for a config that does not read, which can never
/// sign anyone in.
fn mapped_attributes(provider: &IdentityProvider) -> BTreeSet<String> {
    let names = match provider.kind {
        Kind::Oidc => OidcConfig::from_stored(&provider.config)
            .ok()
            .map(|config| config.settings.claims.attributes.into_keys().collect()),
        Kind::Oauth2 => Oauth2Config::from_stored(&provider.config)
            .ok()
            .map(|config| {
                config
                    .settings
                    .claims
                    .profile
                    .attributes
                    .into_keys()
                    .collect()
            }),
        Kind::Local => None,
    };
    names.unwrap_or_default()
}

/// Why a rule change is refused with `permission.escalation`.
const ESCALATION_DETAIL: &str = "Admission rules apply to every user who signs in through an outside provider, including users with permissions you do not hold. Only someone holding every permission can change them.";

/// The 422 for a rule change's problems, at most [`MAX_ERRORS`] of them.
fn rule_problems(problems: Vec<Problem>) -> ApiError {
    let errors = problems
        .iter()
        .take(MAX_ERRORS)
        .map(|problem| {
            let path: Vec<&str> = problem.path.iter().map(String::as_str).collect();
            FieldError::at(&path, problem.code)
        })
        .collect();
    ApiError::invalid(errors)
}

/// The answer for an admission rules repository error.
fn rules_error(error: rules_repo::Error) -> ApiError {
    match error {
        rules_repo::Error::NotFound => org_not_found(),
        other => ApiError::internal(&other),
    }
}

fn org_not_found() -> ApiError {
    ApiError::new(
        StatusCode::NOT_FOUND,
        request_org::code::ORG_NOT_FOUND,
        "The request's organization cannot be determined.",
    )
}

/// The answer for an admission settings repository error.
fn admission_error(error: repo::Error) -> ApiError {
    match error {
        repo::Error::SyncOnlyUnavailable => ApiError::invalid(vec![FieldError::at(
            &["sync_only"],
            code::SYNC_ONLY_UNAVAILABLE,
        )]),
        repo::Error::NotFound => org_not_found(),
        other => ApiError::internal(&other),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_body_names_only_what_it_changes() {
        let parse = |v: serde_json::Value| serde_json::from_value::<AdmissionSettingsPatch>(v);
        assert_eq!(
            parse(serde_json::json!({})).unwrap(),
            AdmissionSettingsPatch::default()
        );
        let one = parse(serde_json::json!({ "signup_allowed": true })).unwrap();
        assert_eq!(
            (one.signup_allowed, one.sync_only, one.rules),
            (Some(true), None, None)
        );
        let rules = parse(serde_json::json!({ "rules": { "providers": { "sso": [] } } }))
            .unwrap()
            .rules
            .expect("rules");
        assert_eq!(rules.organization, None);
        assert_eq!(
            rules.providers.expect("providers")["sso"],
            Vec::<Value>::new()
        );
    }

    #[test]
    fn a_null_an_unknown_member_or_a_non_boolean_is_malformed() {
        let parse = |v: serde_json::Value| serde_json::from_value::<AdmissionSettingsPatch>(v);
        assert!(parse(serde_json::json!({ "signup_allowed": null })).is_err());
        assert!(parse(serde_json::json!({ "sync_only": null })).is_err());
        assert!(parse(serde_json::json!({ "rules": null })).is_err());
        assert!(parse(serde_json::json!({ "rules": { "organization": null } })).is_err());
        assert!(parse(serde_json::json!({ "rules": { "providers": [] } })).is_err());
        assert!(parse(serde_json::json!({ "rules": { "orgs": [] } })).is_err());
        assert!(parse(serde_json::json!({ "sync_only": "false" })).is_err());
    }

    #[test]
    fn repository_refusals_map_to_their_status_code_and_field() {
        let sync = admission_error(repo::Error::SyncOnlyUnavailable).problem();
        assert_eq!((sync.status, sync.code.as_str()), (422, "request.invalid"));
        assert_eq!(sync.errors[0].field, "/sync_only");
        assert_eq!(sync.errors[0].code, "admission.sync_only_unavailable");
        let missing = admission_error(repo::Error::NotFound).problem();
        assert_eq!(
            (missing.status, missing.code.as_str()),
            (404, "org.not_found")
        );
        let broken = serde_json::from_str::<u8>("x").unwrap_err();
        let stored = admission_error(repo::Error::StoredSettings(broken)).problem();
        assert_eq!(stored.status, 500);
    }
}
