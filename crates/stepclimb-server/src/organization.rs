// SPDX-License-Identifier: AGPL-3.0-or-later
//! The caller's organization and its settings under `/api/v1/organization`
//! (docs/specs/m0/02-organizations-and-accounts.md B12 to B14 and API).
//!
//! A principal only ever sees its own organization: every route acts in the credential's scope,
//! and no route takes an organization id. Reading the organization needs a principal and no
//! permission; renaming it and reading or changing its settings need `organization.manage`,
//! which is grantable only at organization scope. The slug never changes through the API (Q3).
//!
//! The settings are the typed `stepclimb_core::OrgSettings` and `OrgSettingsPatch`. A patch
//! names only the keys it changes; each key it names that is no setting is listed in the 422 as
//! `settings.unknown_key`, and a known key whose value does not match its type is a 400
//! `request.malformed`, as for any body that does not match the schema (B2). `local_sign_in` is
//! read here but written only through `/api/v1/admin/local-sign-in` with `provider.manage`
//! (docs/specs/m0/04-local-accounts.md Q2), and `admission` only through
//! `/api/v1/admin/admission` with `admission.manage` (docs/specs/m0/07-admission.md Q2), so a
//! patch naming either is listed in the 422 as `settings.read_only`.
//!
//! `embedding.return_origins` lists the origins a deep link's `return` URL may point at for the
//! web app to show its "Back to" link (docs/specs/m1/03-modules-and-items.md B17): at most 20
//! entries, each `https://host[:port]` without a path. An entry written any other way is listed
//! in the 422 as `settings.return_origin_invalid` at its index, and a 21st entry is refused as
//! `settings.return_origins_too_many` at the list. Every signed-in user reads the list through
//! `GET /api/v1/me`, which is how the web app decides whether to show the link.
//!
//! A patch that sets `email_matching` to `true` changes how unknown identities sign in: one may
//! then be linked to an existing user by email (docs/specs/m0/07-admission.md B5). It needs
//! `provider.manage` at organization scope (403 `permission.denied`) and, beyond it, every
//! catalog permission at organization scope (403 `permission.escalation`), the rule of `PUT
//! /api/v1/admin/local-sign-in`. Both checks run before anything is read, whatever the stored
//! value, so they apply to `true` alone, not to a change; `false` needs only
//! `organization.manage`.
//!
//! `GET /api/v1/organization/entitlements` shows the organization's plan and what it allows, with
//! every catalog feature and limit (docs/specs/m0/10-entitlements-framework.md B12). It needs
//! `organization.manage`, and reads the plan through the request's [`RequestEntitlements`], the
//! same load every `Require<Feature>` of the request checks.

use std::collections::BTreeMap;

use axum::{Json, extract::State, http::StatusCode};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use stepclimb_core::{
    OrgSettings as CoreSettings, OrgSettingsPatch as CorePatch,
    feature::{self, Limit},
    grants::{Grants, Target},
    id::Uuid,
    org_plans::{OrgPlan, PlanSource as CoreSource},
    organizations,
    perm::{OrgManage, ProviderManage},
};
use utoipa::ToSchema;

use crate::{
    AppState,
    audit::ClientAddress,
    entitlements::Entitlements as RequestEntitlements,
    error::{ApiError, FieldError},
    extract::ApiJson,
    openapi::problem_responses,
    request_org,
    require::Require,
    roles::{
        code::{NAME_EMPTY, NAME_TOO_LONG, TEXT_CONTAINS_NUL},
        refuse_unless_holds_every_permission,
    },
    session::Authenticated,
    users::code::TZ_UNKNOWN,
};

/// The path of the caller's organization.
pub const ORGANIZATION_PATH: &str = "/api/v1/organization";

/// The path of the caller's organization settings.
pub const SETTINGS_PATH: &str = "/api/v1/organization/settings";

/// The path of the caller's organization's entitlements.
pub const ENTITLEMENTS_PATH: &str = "/api/v1/organization/entitlements";

/// The codes this module produces.
pub mod code {
    /// In a 422's `errors`: the patch names a key that is no organization setting.
    pub const SETTINGS_UNKNOWN_KEY: &str = "settings.unknown_key";
    /// In a 422's `errors`: the patch names a setting another route writes.
    pub const SETTINGS_READ_ONLY: &str = "settings.read_only";
    /// In a 422's `errors`: an entry of `embedding.return_origins` is no `https://host[:port]`.
    pub const RETURN_ORIGIN_INVALID: &str = "settings.return_origin_invalid";
    /// In a 422's `errors`: `embedding.return_origins` has more than 20 entries.
    pub const RETURN_ORIGINS_TOO_MANY: &str = "settings.return_origins_too_many";
}

/// The settings this route reads but does not write: the local sign-in switches belong to
/// `/api/v1/admin/local-sign-in` (spec 04 Q2), the admission switches to
/// `/api/v1/admin/admission` (spec 07 Q2).
pub const READ_ONLY_SETTINGS: &[&str] = CorePatch::WRITTEN_ELSEWHERE;

problem_responses! {
    OrgWriteForbidden => "`permission.denied`: the caller does not hold `organization.manage` at organization scope. `auth.csrf_refused`: the request did not come from this site.",
    SettingsWriteForbidden => "`permission.denied`: the caller does not hold `organization.manage` at organization scope, or the patch sets `email_matching` to `true` and the caller does not hold `provider.manage` there. `permission.escalation`: the patch sets `email_matching` to `true`, which changes how people sign in, and the caller does not hold every catalog permission at organization scope. `auth.csrf_refused`: the request did not come from this site.",
    SettingsInvalid => "`request.invalid`: the patch breaks a rule; `errors` lists each one. `settings.unknown_key` marks a key that is no organization setting; `settings.read_only` a setting this route does not change (`local_sign_in`: `PUT /api/v1/admin/local-sign-in`; `admission`: `PUT /api/v1/admin/admission`); `tz.unknown` a `default_tz` that is no IANA time zone; `settings.return_origin_invalid` an entry of `embedding.return_origins` that is not written `https://host[:port]`; `settings.return_origins_too_many` more than 20 of them.",
}

/// The caller's organization.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, ToSchema)]
pub struct Organization {
    #[schema(value_type = String, format = "uuid")]
    pub id: Uuid,
    /// Fixed when the organization is created.
    pub slug: String,
    pub name: String,
}

impl From<organizations::Organization> for Organization {
    fn from(org: organizations::Organization) -> Self {
        Self {
            id: org.id,
            slug: org.slug,
            name: org.name,
        }
    }
}

/// A change to the organization; an omitted member is left as it is. A member sent as `null`
/// is malformed: omit it instead.
#[derive(Clone, Debug, Default, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct OrganizationPatch {
    /// Trimmed; 1 to 200 characters.
    #[serde(default, deserialize_with = "present")]
    #[schema(value_type = String)]
    pub name: Option<String>,
}

/// A member that, when present, must be a value: `null` fails the parse (400), and only an
/// omitted member reads as `None`, through `#[serde(default)]`.
pub(crate) fn present<'de, D, T>(deserializer: D) -> Result<Option<T>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: Deserialize<'de>,
{
    T::deserialize(deserializer).map(Some)
}

/// Every organization setting with its effective value: the stored one, or the setting's
/// default when none is stored.
//
// The document's form of `stepclimb_core::OrgSettings`, which the routes serialize;
// `tests/organization.rs` checks the core type's answer against this schema.
#[derive(ToSchema)]
pub struct OrgSettings {
    /// The IANA time zone a user created without one gets; `UTC` unless set.
    pub default_tz: String,
    /// Whether a sign-in through an identity the organization does not know yet may be linked
    /// to the existing user with the same email address: only through a provider whose config
    /// sets `trusted_for_email_matching`, only when that provider marked the address verified,
    /// and only to a local or just-in-time user whose stored email is recorded as verified.
    /// `false` unless set.
    pub email_matching: bool,
    /// The local sign-in method switches; read-only here, changed through
    /// `PUT /api/v1/admin/local-sign-in`.
    pub local_sign_in: LocalMethods,
    /// Who admission lets in; read-only here, changed through `PUT /api/v1/admin/admission`.
    pub admission: AdmissionSwitches,
    pub embedding: Embedding,
}

/// How host sites link into the organization's courses.
//
// The document's form of `stepclimb_core::Embedding`, which the routes serialize.
#[derive(ToSchema)]
pub struct Embedding {
    /// The origins a deep link's `return` URL may point at for the web app to show a "Back to"
    /// link, each `https://host[:port]` in lowercase without a path; empty unless set.
    pub return_origins: Vec<String>,
}

/// The whole new `embedding`: each member is required.
//
// The document's form of `stepclimb_core::EmbeddingPatch`.
#[derive(Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct EmbeddingPatch {
    /// At most 20 entries, each `https://host[:port]` without a path, in any letter case; a
    /// host outside ASCII is written in its `xn--` form. Stored in lowercase, each once.
    pub return_origins: Vec<String>,
}

/// The admission switches as the organization settings show them.
//
// The document's form of `stepclimb_core::settings::AdmissionSwitches`.
#[derive(ToSchema)]
pub struct AdmissionSwitches {
    /// Whether a person the organization has no user for is created as a user when they first
    /// sign in through an identity provider; `false` unless set. Off, they are refused with
    /// `auth.signup_closed`.
    pub signup_allowed: bool,
    /// Whether only users already provisioned may sign in; `false` unless set.
    pub sync_only: bool,
}

/// The local sign-in method switches as the organization settings show them.
//
// The document's form of `stepclimb_core::local_sign_in::LocalMethods`.
#[derive(ToSchema)]
pub struct LocalMethods {
    pub passkey: bool,
    pub password: bool,
    pub totp: bool,
    pub magic_link: bool,
    /// Every password sign-in needs a TOTP code.
    pub totp_required: bool,
}

/// A partial change to the organization settings: one optional member per setting this route
/// writes; only the members present change. A member that is no setting, or a setting another
/// route writes, is refused.
//
// The document's form of `stepclimb_core::OrgSettingsPatch`, whose `deny_unknown_fields` this
// repeats so the document says `additionalProperties: false`; `tests/organization.rs` checks
// it declares the members of `OrgSettings` but `READ_ONLY_SETTINGS`.
#[derive(Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct OrgSettingsPatch {
    /// An IANA time zone, matched in any letter case and stored in the database's spelling.
    #[serde(default)]
    #[schema(value_type = String)]
    pub default_tz: Option<String>,
    /// `true` needs `provider.manage` and every catalog permission at organization scope, as
    /// well as `organization.manage`.
    #[serde(default)]
    #[schema(value_type = bool)]
    pub email_matching: Option<bool>,
    /// Replaces the return origins.
    #[serde(default)]
    #[schema(value_type = EmbeddingPatch)]
    pub embedding: Option<EmbeddingPatch>,
}

/// Read the caller's organization.
#[utoipa::path(
    get,
    path = "/api/v1/organization",
    operation_id = "getOrganization",
    tag = "organization",
    responses(
        (status = 200, description = "The caller's organization", body = Organization),
        (status = 401, response = crate::openapi::Unauthenticated),
        (status = 404, response = crate::openapi::OrganizationNotFound),
        (status = 500, response = crate::openapi::InternalError),
        (status = 503, response = crate::openapi::NoOrganization),
    )
)]
pub async fn get_organization(
    principal: Authenticated,
    State(state): State<AppState>,
) -> Result<Json<Organization>, ApiError> {
    let org = organizations::get(&state.db, principal.scope())
        .await
        .map_err(org_error)?;
    Ok(Json(org.into()))
}

/// Rename the caller's organization. A patch without `name` changes nothing.
#[utoipa::path(
    patch,
    path = "/api/v1/organization",
    operation_id = "updateOrganization",
    tag = "organization",
    extensions(("x-stepclimb-mutates" = json!(true))),
    request_body = OrganizationPatch,
    responses(
        (status = 200, description = "The organization after the change", body = Organization),
        (status = 400, response = crate::openapi::BadRequest),
        (status = 401, response = crate::openapi::Unauthenticated),
        (status = 403, response = OrgWriteForbidden),
        (status = 404, response = crate::openapi::OrganizationNotFound),
        (status = 413, response = crate::openapi::PayloadTooLarge),
        (status = 415, response = crate::openapi::UnsupportedMediaType),
        (status = 422, response = crate::openapi::UnprocessableEntity),
        (status = 500, response = crate::openapi::InternalError),
        (status = 503, response = crate::openapi::NoOrganization),
    )
)]
pub async fn update_organization(
    require: Require<OrgManage>,
    client: ClientAddress,
    State(state): State<AppState>,
    ApiJson(body): ApiJson<OrganizationPatch>,
) -> Result<Json<Organization>, ApiError> {
    let scope = require.principal().scope();
    let org = match body.name {
        Some(name) => {
            let actor = client.actor(require.principal());
            organizations::rename(&state.db, scope, &name, state.clock.now(), actor).await
        }
        None => organizations::get(&state.db, scope).await,
    }
    .map_err(org_error)?;
    Ok(Json(org.into()))
}

/// Read the organization settings, every key with its effective value.
#[utoipa::path(
    get,
    path = "/api/v1/organization/settings",
    operation_id = "getOrganizationSettings",
    tag = "organization",
    responses(
        (status = 200, description = "Every setting with its effective value", body = OrgSettings),
        (status = 401, response = crate::openapi::Unauthenticated),
        (status = 403, response = crate::openapi::PermissionDenied),
        (status = 404, response = crate::openapi::OrganizationNotFound),
        (status = 500, response = crate::openapi::InternalError),
        (status = 503, response = crate::openapi::NoOrganization),
    )
)]
pub async fn get_settings(
    require: Require<OrgManage>,
    State(state): State<AppState>,
) -> Result<Json<CoreSettings>, ApiError> {
    let settings = organizations::settings(&state.db, require.principal().scope())
        .await
        .map_err(org_error)?;
    Ok(Json(settings))
}

/// Change the settings the patch names; the others keep their value. A patch naming no setting
/// writes nothing.
#[utoipa::path(
    patch,
    path = "/api/v1/organization/settings",
    operation_id = "updateOrganizationSettings",
    tag = "organization",
    extensions(("x-stepclimb-mutates" = json!(true))),
    request_body = OrgSettingsPatch,
    responses(
        (status = 200, description = "Every setting after the change", body = OrgSettings),
        (status = 400, response = crate::openapi::BadRequest),
        (status = 401, response = crate::openapi::Unauthenticated),
        (status = 403, response = SettingsWriteForbidden),
        (status = 404, response = crate::openapi::OrganizationNotFound),
        (status = 413, response = crate::openapi::PayloadTooLarge),
        (status = 415, response = crate::openapi::UnsupportedMediaType),
        (status = 422, response = SettingsInvalid),
        (status = 500, response = crate::openapi::InternalError),
        (status = 503, response = crate::openapi::NoOrganization),
    )
)]
pub async fn update_settings(
    require: Require<OrgManage>,
    client: ClientAddress,
    State(state): State<AppState>,
    ApiJson(body): ApiJson<Map<String, Value>>,
) -> Result<Json<CoreSettings>, ApiError> {
    let scope = require.principal().scope();
    let settings = match parse_patch(body)? {
        Some(patch) => {
            refuse_email_matching_unless_allowed(&patch, require.grants())?;
            let actor = client.actor(require.principal());
            organizations::update_settings(&state.db, scope, &patch, state.clock.now(), actor).await
        }
        None => organizations::settings(&state.db, scope).await,
    }
    .map_err(org_error)?;
    Ok(Json(settings))
}

/// The checks of a `patch` that turns `email_matching` on (module documentation): 403
/// `permission.denied` without `provider.manage` at organization scope, then 403
/// `permission.escalation` without every catalog permission there. Any other patch passes.
fn refuse_email_matching_unless_allowed(
    patch: &CorePatch,
    grants: &Grants,
) -> Result<(), ApiError> {
    if patch.email_matching != Some(true) {
        return Ok(());
    }
    grants.check::<ProviderManage>(&Target::Organization)?;
    refuse_unless_holds_every_permission(
        grants,
        "Matching by email lets a sign-in reach an existing user's account, including users with permissions you do not hold. Only someone holding every permission can turn it on.",
    )
}

/// The organization's plan and what it allows: every feature and limit of the server's catalogs.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, ToSchema)]
#[schema(as = Entitlements)]
pub struct OrgEntitlements {
    pub plan: PlanRef,
    /// Every catalog feature, by key, with whether the plan includes it.
    pub features: BTreeMap<String, bool>,
    /// Every catalog limit, by key, with the most units the plan allows, or `null` for no limit.
    pub limits: BTreeMap<String, Option<u64>>,
}

/// Which plan the organization runs on and how it got it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, ToSchema)]
pub struct PlanRef {
    /// The plan's key; `unlimited` for a self-hosted organization.
    pub key: String,
    pub source: PlanSource,
}

/// How the organization got its plan: a subscription (`stripe`), granted by the operator
/// (`grant`), the non-profit plan (`nonprofit`), or the self-hosted `unlimited` plan
/// (`selfhost`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, ToSchema)]
#[serde(rename_all = "lowercase")]
pub enum PlanSource {
    Stripe,
    Grant,
    Nonprofit,
    Selfhost,
}

impl From<CoreSource> for PlanSource {
    fn from(source: CoreSource) -> Self {
        match source {
            CoreSource::Stripe => Self::Stripe,
            CoreSource::Grant => Self::Grant,
            CoreSource::Nonprofit => Self::Nonprofit,
            CoreSource::Selfhost => Self::Selfhost,
        }
    }
}

impl From<&OrgPlan> for OrgEntitlements {
    /// Lists [`feature::ALL`] and [`Limit::ALL`], not the plan row's keys: a key the catalog no
    /// longer has is left out, and one the row lacks shows as the plan reads it (B7).
    fn from(plan: &OrgPlan) -> Self {
        let rules = &plan.entitlements;
        Self {
            plan: PlanRef {
                key: plan.plan.clone(),
                source: plan.source.into(),
            },
            features: feature::ALL
                .iter()
                .map(|key| ((*key).to_owned(), rules.has_key(key)))
                .collect(),
            limits: Limit::ALL
                .iter()
                .map(|limit| (limit.key().to_owned(), rules.limit(*limit)))
                .collect(),
        }
    }
}

/// Read the organization's plan, every catalog feature with whether the plan includes it, and
/// every catalog limit with its value.
#[utoipa::path(
    get,
    path = "/api/v1/organization/entitlements",
    operation_id = "getOrganizationEntitlements",
    tag = "organization",
    responses(
        (status = 200, description = "The plan and what it allows", body = OrgEntitlements),
        (status = 401, response = crate::openapi::Unauthenticated),
        (status = 403, response = crate::openapi::PermissionDenied),
        (status = 404, response = crate::openapi::OrganizationNotFound),
        (status = 500, response = crate::openapi::InternalError),
        (status = 503, response = crate::openapi::NoOrganization),
    )
)]
pub async fn get_entitlements(
    _: Require<OrgManage>,
    entitlements: RequestEntitlements,
) -> Json<OrgEntitlements> {
    Json(entitlements.plan().into())
}

/// The most refused keys a 422 lists, so a large body cannot make an equally large answer.
pub const MAX_UNKNOWN_KEYS: usize = 20;

const MALFORMED_PATCH: &str = "The request body is not valid JSON for this operation.";

/// The patch in `body`, `None` when it names no key. In B2's order: 400 when a member is `null` (no
/// setting is nullable; omit a member to leave it as it is), then 422 listing the first
/// [`MAX_UNKNOWN_KEYS`] keys that are no setting or are [`READ_ONLY_SETTINGS`], then 400 when a
/// known key's value does not match its type.
fn parse_patch(body: Map<String, Value>) -> Result<Option<CorePatch>, ApiError> {
    if body.is_empty() {
        return Ok(None);
    }
    if body.values().any(Value::is_null) {
        return Err(ApiError::malformed(MALFORMED_PATCH));
    }
    let unknown: Vec<FieldError> = body
        .keys()
        .filter_map(|key| {
            let code = if READ_ONLY_SETTINGS.contains(&key.as_str()) {
                code::SETTINGS_READ_ONLY
            } else if !CorePatch::knows(key) {
                code::SETTINGS_UNKNOWN_KEY
            } else {
                return None;
            };
            Some(FieldError::at(&[key], code))
        })
        .take(MAX_UNKNOWN_KEYS)
        .collect();
    if !unknown.is_empty() {
        return Err(ApiError::invalid(unknown));
    }
    serde_json::from_value(Value::Object(body))
        .map(Some)
        .map_err(|_| ApiError::malformed(MALFORMED_PATCH))
}

/// The answer for an organizations repository error.
fn org_error(error: organizations::Error) -> ApiError {
    use organizations::Error as E;
    let invalid = |code| ApiError::invalid(vec![FieldError::at(&["name"], code)]);
    match error {
        E::NotFound => ApiError::new(
            StatusCode::NOT_FOUND,
            request_org::code::ORG_NOT_FOUND,
            "The request's organization cannot be determined.",
        ),
        E::NameEmpty => invalid(NAME_EMPTY),
        E::NameTooLong => invalid(NAME_TOO_LONG),
        E::NameContainsNul => invalid(TEXT_CONTAINS_NUL),
        E::DefaultTzUnknown => ApiError::invalid(vec![FieldError::at(&["default_tz"], TZ_UNKNOWN)]),
        E::ReturnOriginsInvalid(indexes) => ApiError::invalid(
            indexes
                .iter()
                .map(|index| {
                    let index = index.to_string();
                    let path = ["embedding", "return_origins", index.as_str()];
                    FieldError::at(&path, code::RETURN_ORIGIN_INVALID)
                })
                .collect(),
        ),
        E::ReturnOriginsTooMany => ApiError::invalid(vec![FieldError::at(
            &["embedding", "return_origins"],
            code::RETURN_ORIGINS_TOO_MANY,
        )]),
        other @ (E::SlugInvalid
        | E::SlugReserved
        | E::SlugTaken
        | E::StoredSettings(_)
        | E::Plan(_)
        | E::OrgPlan(_)
        | E::LocalProvider(_)
        | E::Time(_)
        | E::Database(_)) => ApiError::internal(&other),
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    /// The API's spellings are the stored ones, so a source added to the core cannot reach the
    /// API under another name.
    #[test]
    fn plan_sources_serialize_as_their_stored_spelling() {
        for source in CoreSource::ALL {
            let json = serde_json::to_value(PlanSource::from(source)).unwrap();
            assert_eq!(json, source.as_str());
        }
    }

    #[test]
    fn a_patch_naming_the_local_sign_in_switches_is_refused() {
        let body: Map<String, Value> = serde_json::from_value(json!({
            "local_sign_in": { "passkey": false },
            "default_tz": "UTC",
            "zeta": 1,
        }))
        .unwrap();
        let problem = parse_patch(body).unwrap_err().problem();
        assert_eq!(problem.status, 422);
        let fields: Vec<_> = problem
            .errors
            .iter()
            .map(|e| (e.field.as_str(), e.code.as_str()))
            .collect();
        assert_eq!(
            fields,
            [
                ("/local_sign_in", "settings.read_only"),
                ("/zeta", "settings.unknown_key")
            ]
        );
    }

    #[test]
    fn a_patch_naming_the_admission_switches_is_refused() {
        let body: Map<String, Value> =
            serde_json::from_value(json!({ "admission": { "signup_allowed": true } })).unwrap();
        let problem = parse_patch(body).unwrap_err().problem();
        assert_eq!(problem.status, 422);
        assert_eq!(problem.errors[0].field, "/admission");
        assert_eq!(problem.errors[0].code, "settings.read_only");
    }

    #[test]
    fn a_patch_lists_every_unknown_key() {
        let body: Map<String, Value> = serde_json::from_str(r#"{"zeta": 1, "a/b": true}"#).unwrap();
        let problem = parse_patch(body).unwrap_err().problem();
        assert_eq!(problem.status, 422);
        assert_eq!(problem.code, "request.invalid");
        let fields: Vec<_> = problem
            .errors
            .iter()
            .map(|e| (e.field.as_str(), e.code.as_str()))
            .collect();
        assert_eq!(
            fields,
            [
                ("/a~1b", "settings.unknown_key"),
                ("/zeta", "settings.unknown_key")
            ]
        );
        assert_eq!(parse_patch(Map::new()).unwrap(), None);
    }

    #[test]
    fn a_patch_lists_at_most_twenty_unknown_keys() {
        let body: Map<String, Value> = (0..25).map(|i| (format!("k{i:02}"), json!(1))).collect();
        let problem = parse_patch(body).unwrap_err().problem();
        assert_eq!(problem.status, 422);
        let fields: Vec<_> = problem.errors.iter().map(|e| e.field.as_str()).collect();
        let first: Vec<String> = (0..MAX_UNKNOWN_KEYS).map(|i| format!("/k{i:02}")).collect();
        assert_eq!(fields, first);
    }

    #[test]
    fn a_null_member_is_malformed_whatever_its_key() {
        for body in [json!({ "no_such_key": null }), json!({ "a": 1, "b": null })] {
            let Value::Object(body) = body else {
                unreachable!()
            };
            let problem = parse_patch(body).unwrap_err().problem();
            assert_eq!(
                (problem.status, problem.code.as_str()),
                (400, "request.malformed")
            );
        }
    }

    #[test]
    fn a_rename_patch_refuses_null_and_reads_an_omitted_name_as_none() {
        assert!(serde_json::from_value::<OrganizationPatch>(json!({ "name": null })).is_err());
        let omitted: OrganizationPatch = serde_json::from_value(json!({})).unwrap();
        assert_eq!(omitted.name, None);
        let named: OrganizationPatch = serde_json::from_value(json!({ "name": "A" })).unwrap();
        assert_eq!(named.name.as_deref(), Some("A"));
    }

    #[test]
    fn repository_errors_map_to_their_status_code_and_field() {
        let cases = [
            (organizations::Error::NotFound, 404, "org.not_found", None),
            (
                organizations::Error::NameEmpty,
                422,
                "request.invalid",
                Some(("/name", "name.empty")),
            ),
            (
                organizations::Error::NameTooLong,
                422,
                "request.invalid",
                Some(("/name", "name.too_long")),
            ),
            (
                organizations::Error::NameContainsNul,
                422,
                "request.invalid",
                Some(("/name", "text.contains_nul")),
            ),
            (
                organizations::Error::DefaultTzUnknown,
                422,
                "request.invalid",
                Some(("/default_tz", "tz.unknown")),
            ),
            (
                organizations::Error::ReturnOriginsTooMany,
                422,
                "request.invalid",
                Some((
                    "/embedding/return_origins",
                    "settings.return_origins_too_many",
                )),
            ),
            (organizations::Error::SlugTaken, 500, "internal.error", None),
        ];
        for (error, status, code, field) in cases {
            let problem = org_error(error).problem();
            assert_eq!((problem.status, problem.code.as_str()), (status, code));
            let fields: Vec<_> = problem
                .errors
                .iter()
                .map(|e| (e.field.as_str(), e.code.as_str()))
                .collect();
            assert_eq!(fields, field.into_iter().collect::<Vec<_>>(), "{code}");
        }
    }
}
