// SPDX-License-Identifier: AGPL-3.0-or-later
//! The admission switches under `/api/v1/admin/admission`
//! (docs/specs/m0/07-admission.md B12, B23, Q2, API).
//!
//! Reading and changing the switches need `admission.manage` at organization scope, the one
//! permission that covers who may sign in (Q2 (b)); the generic settings `PATCH` refuses the
//! `admission` key as `settings.read_only`. A `PUT` names only the switches it changes: an
//! absent one keeps its stored value, so one admin's change cannot undo another's made in the
//! meantime. The repository's `admission_settings::update` applies the body to the switches
//! under the organization row's lock and writes, with its `organization.update_settings` entry,
//! only a change; it
//! refuses to turn `sync_only` on (422 `admission.sync_only_unavailable`) until admission
//! applies it.
//!
//! No escalation rule applies: the switches decide whether a person the organization has no
//! user for is let in, and never how an existing user signs in or what they may do, since a
//! just-in-time user gets no role.

use axum::{Json, extract::State, http::StatusCode};
use serde::{Deserialize, Serialize};
use stepclimb_core::{
    admission_settings::{self as repo, AdmissionPatch, AdmissionSwitches},
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
};

/// The path of the admission settings.
pub const ADMISSION_PATH: &str = "/api/v1/admin/admission";

/// The codes this module produces.
pub mod code {
    /// In a 422's `errors`: `sync_only` cannot be turned on before admission applies it.
    pub const SYNC_ONLY_UNAVAILABLE: &str = "admission.sync_only_unavailable";
}

problem_responses! {
    AdmissionWriteForbidden => "`permission.denied`: the caller does not hold `admission.manage` at organization scope. `auth.csrf_refused`: the request did not come from this site.",
    AdmissionInvalid => "`request.invalid`: the change breaks a rule; `errors` lists each one. `admission.sync_only_unavailable` marks `sync_only` turned on, which this release refuses.",
}

/// Who admission lets in.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, ToSchema)]
pub struct AdmissionSettings {
    /// Whether a person the organization has no user for is created as a user when they first
    /// sign in through an identity provider. Off, they are refused with `auth.signup_closed`.
    pub signup_allowed: bool,
    /// Whether only users already provisioned may sign in. This release accepts only `false`:
    /// `true` answers 422 with `admission.sync_only_unavailable`.
    pub sync_only: bool,
}

impl From<AdmissionSwitches> for AdmissionSettings {
    fn from(switches: AdmissionSwitches) -> Self {
        let AdmissionSwitches {
            signup_allowed,
            sync_only,
        } = switches;
        Self {
            signup_allowed,
            sync_only,
        }
    }
}

/// A change to the admission switches: send only what you change. An absent member keeps its
/// stored value, and `{}` changes nothing. A member sent as `null` is malformed: omit it instead.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize, ToSchema)]
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
}

impl From<AdmissionSettingsPatch> for AdmissionPatch {
    fn from(body: AdmissionSettingsPatch) -> Self {
        Self {
            signup_allowed: body.signup_allowed,
            sync_only: body.sync_only,
        }
    }
}

/// Read the organization's admission switches.
#[utoipa::path(
    get,
    path = "/api/v1/admin/admission",
    operation_id = "getAdmissionSettings",
    tag = "admission",
    responses(
        (status = 200, description = "The admission switches", body = AdmissionSettings),
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
    let switches = repo::get(&state.db, require.principal().scope())
        .await
        .map_err(admission_error)?;
    Ok(Json(switches.into()))
}

/// Change the admission switches the body names; send only what you change. A switch the body
/// leaves out keeps its value, and a body naming only current values changes nothing.
#[utoipa::path(
    put,
    path = "/api/v1/admin/admission",
    operation_id = "updateAdmissionSettings",
    tag = "admission",
    extensions(("x-stepclimb-mutates" = json!(true))),
    request_body = AdmissionSettingsPatch,
    responses(
        (status = 200, description = "The switches after the change", body = AdmissionSettings),
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
    let actor = client.actor(principal);
    let now = state.clock.now();
    let after = repo::update(&state.db, principal.scope(), body.into(), now, actor)
        .await
        .map_err(admission_error)?;
    Ok(Json(after.into()))
}

/// The answer for an admission settings repository error.
fn admission_error(error: repo::Error) -> ApiError {
    match error {
        repo::Error::SyncOnlyUnavailable => ApiError::invalid(vec![FieldError::at(
            &["sync_only"],
            code::SYNC_ONLY_UNAVAILABLE,
        )]),
        repo::Error::NotFound => ApiError::new(
            StatusCode::NOT_FOUND,
            request_org::code::ORG_NOT_FOUND,
            "The request's organization cannot be determined.",
        ),
        other => ApiError::internal(&other),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_body_names_only_the_switches_it_changes() {
        let parse = |v: serde_json::Value| serde_json::from_value::<AdmissionSettingsPatch>(v);
        assert_eq!(
            parse(serde_json::json!({})).unwrap(),
            AdmissionSettingsPatch::default()
        );
        let one = parse(serde_json::json!({ "signup_allowed": true })).unwrap();
        assert_eq!(
            AdmissionPatch::from(one),
            AdmissionPatch {
                signup_allowed: Some(true),
                sync_only: None
            }
        );
        let both = parse(serde_json::json!({ "signup_allowed": false, "sync_only": false }));
        assert_eq!(
            AdmissionPatch::from(both.unwrap()),
            AdmissionPatch {
                signup_allowed: Some(false),
                sync_only: Some(false)
            }
        );
    }

    #[test]
    fn a_null_an_unknown_member_or_a_non_boolean_is_malformed() {
        let parse = |v: serde_json::Value| serde_json::from_value::<AdmissionSettingsPatch>(v);
        assert!(parse(serde_json::json!({ "signup_allowed": null })).is_err());
        assert!(parse(serde_json::json!({ "sync_only": null })).is_err());
        assert!(parse(serde_json::json!({ "rules": [] })).is_err());
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
