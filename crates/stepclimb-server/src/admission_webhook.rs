// SPDX-License-Identifier: AGPL-3.0-or-later
//! The admission webhook under `/api/v1/admin/admission/webhook`
//! (docs/specs/m0/07-admission.md B13, B17, B24, Q2, Q7, API).
//!
//! An organization has at most one webhook, which admission calls on every sign-in through an
//! outside provider (Q3 (a), spec 07 issue 6). Every route needs `admission.manage` at
//! organization scope. What sets the webhook up or keeps it working also needs the plan's
//! `admission_webhook` feature (spec 10 Q1): creating it, changing its URL, enabling it and
//! rotating its secret. Reading, disabling and deleting it do not, so an organization whose plan
//! loses the feature keeps a read-only webhook it can still turn off or remove (spec 10 B11,
//! the plan's "over-limit features become read-only"). The webhook call at sign-in checks no
//! feature, so a downgrade cannot remove a gate an organization configured.
//!
//! Rotation takes `Require<AdmissionWebhook>` before its permission (spec 10 B9). Whether a
//! `PUT` needs the feature depends on its body, so it checks the feature in the handler, after
//! the permission and before anything else; its target is the caller's organization, so no id
//! can be found missing first. A new webhook is disabled unless the body enables it: an admin
//! creates it, gives the receiver the secret, then enables it.
//!
//! A webhook can refuse every user who signs in through an outside provider, including users
//! holding permissions the caller lacks; creating, changing, rotating the secret of (which
//! refuses every such sign-in until the receiver has the new secret) and deleting it are
//! therefore refused with 403 `permission.escalation` unless the caller holds every catalog
//! permission at organization scope, the rule of the admission rules and `PUT
//! /api/v1/admin/local-sign-in`. The check runs before anything is read. Reading needs only
//! `admission.manage`. Two rotations that read the same secret do not both win: the second
//! answers 409 `admission.webhook_secret_changed`, so no caller is handed a secret already
//! replaced.
//!
//! The signing secret is a Standard Webhooks secret (Q7 (a), `stepclimb_auth::webhook`) that
//! the server generates. The answer that creates the webhook, and the answer of a rotation,
//! carry it once; no other answer, log line or audit entry does. It is stored sealed with the
//! key ring, bound to the organization, `admission_webhooks`, the row's id and `secret`
//! ([`seal_secret`]), so a sealed value copied to another row does not open.
//!
//! The URL is checked on save ([`check_url`]): an absolute `https` URL (`http` only with the
//! insecure-providers switch) of at most [`MAX_URL_CHARS`] characters, with no fragment, white
//! space, control character, user name or password, and an address written as its host must
//! pass the outbound URL policy for the instance's tenancy (B17, `05-oidc.md` Q4). A host name
//! is not resolved on save; the client checks every address it resolves to at each connection.

use axum::{Json, extract::State, http::StatusCode};
use serde::{Deserialize, Serialize};
use stepclimb_auth::{
    encryption::{EncryptError, KeyRing},
    outbound::UrlPolicy,
    webhook::Secret,
};
use stepclimb_core::{
    OrgScope,
    admission_webhooks::{self as repo, Change, Fresh, Saved},
    feature::AdmissionWebhook as WebhookFeature,
    id::{Uuid, new_id},
    organizations,
    perm::AdmissionManage,
};
use url::Url;
use utoipa::ToSchema;

use crate::{
    AppState,
    audit::ClientAddress,
    entitlements::Entitlements,
    error::{ApiError, FieldError},
    extract::ApiJson,
    openapi::problem_responses,
    organization::present,
    request_org,
    require::Require,
    roles::refuse_unless_holds_every_permission,
    timestamp,
};

/// The path of the webhook.
pub const WEBHOOK_PATH: &str = "/api/v1/admin/admission/webhook";

/// The path that rotates its secret.
pub const ROTATE_SECRET_PATH: &str = "/api/v1/admin/admission/webhook/rotate-secret";

/// The longest URL accepted, in characters.
pub const MAX_URL_CHARS: usize = 2048;

/// The codes this module produces.
pub mod code {
    /// The organization has no webhook (404).
    pub const WEBHOOK_NOT_FOUND: &str = "admission.webhook_not_found";
    /// In a 422's `errors`: the URL is not an absolute `http(s)` URL this route accepts.
    pub const WEBHOOK_URL_INVALID: &str = "admission.webhook_url_invalid";
    /// In a 422's `errors`: the outbound URL policy refuses the URL.
    pub const WEBHOOK_URL_REFUSED: &str = "admission.webhook_url_refused";
    /// In a 422's `errors`: creating the webhook needs a URL.
    pub const WEBHOOK_URL_REQUIRED: &str = "admission.webhook_url_required";
    /// Another rotation replaced the secret since this one read it (409).
    pub const WEBHOOK_SECRET_CHANGED: &str = "admission.webhook_secret_changed";
}

/// The table and column the secret is sealed for (`stepclimb_auth::encryption` context rules).
const SECRET_TABLE: &[u8] = b"admission_webhooks";
const SECRET_COLUMN: &[u8] = b"secret";

/// Why a webhook write is refused with `permission.escalation`.
const ESCALATION_DETAIL: &str = "The admission webhook decides every sign-in through an outside provider, including those of users with permissions you do not hold. Only someone holding every permission can change it.";

problem_responses! {
    AdmissionWebhookReadForbidden => "`permission.denied`: the caller does not hold `admission.manage` at organization scope.",
    AdmissionWebhookWriteForbidden => "`permission.denied`: the caller does not hold `admission.manage` at organization scope. `entitlement.feature_unavailable`: the body creates the webhook, sets its URL or enables it, and the organization's plan does not include `admission_webhook`; a body that only disables it needs no feature. `permission.escalation`: the caller does not hold every catalog permission at organization scope; the webhook decides the sign-ins of every user, including users with permissions the caller lacks. `auth.csrf_refused`: the request did not come from this site.",
    AdmissionWebhookRotateForbidden => "`entitlement.feature_unavailable`: the organization's plan does not include `admission_webhook`. `permission.denied`: the caller does not hold `admission.manage` at organization scope. `permission.escalation`: the caller does not hold every catalog permission at organization scope; until the receiver has the new secret, every sign-in through an outside provider is refused. `auth.csrf_refused`: the request did not come from this site.",
    AdmissionWebhookDeleteForbidden => "`permission.denied`: the caller does not hold `admission.manage` at organization scope. `permission.escalation`: the caller does not hold every catalog permission at organization scope; removing the webhook lets in people it refuses. `auth.csrf_refused`: the request did not come from this site.",
    AdmissionWebhookNotFound => "`admission.webhook_not_found`: the organization has no admission webhook. `org.not_found`: the request's organization cannot be determined.",
    AdmissionWebhookSecretChanged => "`admission.webhook_secret_changed`: another rotation replaced the secret while this one ran; its secret is the current one. Rotate again for a new one.",
    AdmissionWebhookInvalid => "`request.invalid`: `errors` names `/url` with `admission.webhook_url_invalid` (not an absolute `https` URL of at most 2,048 characters without a fragment, white space or control character), `admission.webhook_url_refused` (the outbound URL policy refuses it: a user name or password, `http` or a loopback address without the instance's insecure-providers switch, a private network address on an instance with several organizations, or a link-local, multicast or reserved address) or `admission.webhook_url_required` (the organization has no webhook yet and the body names no URL).",
}

/// The organization's admission webhook, without its secret.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, ToSchema)]
pub struct AdmissionWebhook {
    /// Where admission POSTs each sign-in through an outside provider. Not a secret: every
    /// `admission.manage` holder reads it and the audit log records it.
    pub url: String,
    /// Whether admission calls it. Off, sign-ins skip the webhook. A new webhook is off until
    /// enabled.
    pub enabled: bool,
    /// RFC 3339, UTC.
    #[schema(format = DateTime)]
    pub created_at: String,
}

impl AdmissionWebhook {
    fn of(webhook: &repo::AdmissionWebhook) -> Result<Self, ApiError> {
        Ok(Self {
            url: webhook.url.clone(),
            enabled: webhook.enabled,
            created_at: timestamp::rfc3339(webhook.created_at)
                .map_err(|error| ApiError::internal(&error))?,
        })
    }
}

/// The webhook after a `PUT`, with its signing secret when this call created it.
// No `Debug`, so the secret cannot reach a log line through `{:?}`.
#[derive(Serialize, ToSchema)]
pub struct SavedAdmissionWebhook {
    pub url: String,
    pub enabled: bool,
    /// RFC 3339, UTC.
    #[schema(format = DateTime)]
    pub created_at: String,
    /// Present only when this call created the webhook: the Standard Webhooks signing secret,
    /// `whsec_` and 32 bytes in base64. This is the only answer that carries it: give it to the
    /// receiver now. Rotate it to get a new one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub secret: Option<String>,
}

/// A new signing secret, shown once.
// No `Debug`, so the secret cannot reach a log line through `{:?}`.
#[derive(Serialize, ToSchema)]
pub struct AdmissionWebhookSecret {
    /// The Standard Webhooks signing secret, `whsec_` and 32 bytes in base64. The old one stops
    /// signing at once: sign-ins through outside providers are refused until the receiver has
    /// this one.
    pub secret: String,
}

/// A change to the webhook: send only what you change. An absent member keeps its stored
/// value; creating the webhook needs `url`, and `enabled` is then `false` unless sent, so the
/// receiver can be given the secret first. A member sent as `null` is malformed: omit it
/// instead. Without the plan's `admission_webhook` feature, only `{"enabled": false}` (or `{}`)
/// is accepted.
#[derive(Clone, Debug, Default, PartialEq, Eq, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct AdmissionWebhookPatch {
    /// An absolute `https` URL, at most 2,048 characters, with no fragment, user name or
    /// password, kept exactly as sent. It is not a secret: it is returned to every
    /// `admission.manage` holder and written to the audit log. Authenticate the receiver with
    /// the signing secret, never with a token in a query parameter.
    #[serde(default, deserialize_with = "present")]
    #[schema(value_type = String)]
    pub url: Option<String>,
    /// Whether admission calls the webhook.
    #[serde(default, deserialize_with = "present")]
    #[schema(value_type = bool)]
    pub enabled: Option<bool>,
}

impl AdmissionWebhookPatch {
    /// Whether the change sets the webhook up or turns it on, which needs the plan's
    /// `admission_webhook` feature: any URL (creating needs one) or `enabled: true`.
    fn needs_feature(&self) -> bool {
        self.url.is_some() || self.enabled == Some(true)
    }
}

/// Read the organization's admission webhook. The signing secret is never returned.
#[utoipa::path(
    get,
    path = "/api/v1/admin/admission/webhook",
    operation_id = "getAdmissionWebhook",
    tag = "admission",
    responses(
        (status = 200, description = "The admission webhook", body = AdmissionWebhook),
        (status = 401, response = crate::openapi::Unauthenticated),
        (status = 403, response = AdmissionWebhookReadForbidden),
        (status = 404, response = AdmissionWebhookNotFound),
        (status = 500, response = crate::openapi::InternalError),
        (status = 503, response = crate::openapi::NoOrganization),
    )
)]
pub async fn get_admission_webhook(
    require: Require<AdmissionManage>,
    State(state): State<AppState>,
) -> Result<Json<AdmissionWebhook>, ApiError> {
    let webhook = repo::get(&state.db, require.principal().scope())
        .await
        .map_err(webhook_error)?
        .ok_or_else(webhook_not_found)?;
    Ok(Json(AdmissionWebhook::of(&webhook)?))
}

/// Create the organization's admission webhook, or change the members the body names. The
/// answer of the call that creates it carries the signing secret, once.
#[utoipa::path(
    put,
    path = "/api/v1/admin/admission/webhook",
    operation_id = "updateAdmissionWebhook",
    tag = "admission",
    extensions(("x-stepclimb-mutates" = json!(true))),
    request_body = AdmissionWebhookPatch,
    responses(
        (status = 200, description = "The webhook after the change, with `secret` when this call created it", body = SavedAdmissionWebhook),
        (status = 400, response = crate::openapi::BadRequest),
        (status = 401, response = crate::openapi::Unauthenticated),
        (status = 403, response = AdmissionWebhookWriteForbidden),
        (status = 404, response = crate::openapi::OrganizationNotFound),
        (status = 413, response = crate::openapi::PayloadTooLarge),
        (status = 415, response = crate::openapi::UnsupportedMediaType),
        (status = 422, response = AdmissionWebhookInvalid),
        (status = 500, response = crate::openapi::InternalError),
        (status = 503, response = crate::openapi::NoOrganization),
    )
)]
pub async fn update_admission_webhook(
    require: Require<AdmissionManage>,
    entitlements: Entitlements,
    client: ClientAddress,
    State(state): State<AppState>,
    ApiJson(body): ApiJson<AdmissionWebhookPatch>,
) -> Result<Json<SavedAdmissionWebhook>, ApiError> {
    if body.needs_feature() {
        entitlements.check::<WebhookFeature>()?;
    }
    refuse_unless_holds_every_permission(require.grants(), ESCALATION_DETAIL)?;
    let principal = require.principal();
    let scope = principal.scope();
    if let Some(url) = &body.url {
        let tenancy = organizations::tenancy(&state.db)
            .await
            .map_err(|e| ApiError::internal(&e))?;
        let policy = state.providers.client(tenancy).policy();
        check_url(url, policy)
            .map_err(|code| ApiError::invalid(vec![FieldError::at(&["url"], code)]))?;
    }
    let change = Change {
        url: body.url.as_deref(),
        enabled: body.enabled,
    };
    let secret = Secret::generate().map_err(|e| ApiError::internal(&e))?;
    let id = new_id();
    let sealed =
        seal_secret(&state.keys, scope, id, &secret).map_err(|e| ApiError::internal(&e))?;
    let fresh = Fresh {
        id,
        sealed_secret: &sealed,
    };
    let now = state.clock.now();
    let saved = repo::save(
        &state.db,
        scope,
        change,
        &fresh,
        now,
        client.actor(principal),
    )
    .await
    .map_err(webhook_error)?;
    let (webhook, secret) = match &saved {
        Saved::Created(webhook) => (webhook, Some(secret.expose_whsec())),
        Saved::Updated(webhook) | Saved::Unchanged(webhook) => (webhook, None),
    };
    let AdmissionWebhook {
        url,
        enabled,
        created_at,
    } = AdmissionWebhook::of(webhook)?;
    Ok(Json(SavedAdmissionWebhook {
        url,
        enabled,
        created_at,
        secret,
    }))
}

/// Replace the webhook's signing secret with a new one, returned once. The old secret stops
/// signing at once.
#[utoipa::path(
    post,
    path = "/api/v1/admin/admission/webhook/rotate-secret",
    operation_id = "rotateAdmissionWebhookSecret",
    tag = "admission",
    extensions(("x-stepclimb-mutates" = json!(true))),
    responses(
        (status = 200, description = "The new signing secret", body = AdmissionWebhookSecret),
        (status = 401, response = crate::openapi::Unauthenticated),
        (status = 403, response = AdmissionWebhookRotateForbidden),
        (status = 404, response = AdmissionWebhookNotFound),
        (status = 409, response = AdmissionWebhookSecretChanged),
        (status = 500, response = crate::openapi::InternalError),
        (status = 503, response = crate::openapi::NoOrganization),
    )
)]
pub async fn rotate_admission_webhook_secret(
    _: Require<WebhookFeature>,
    require: Require<AdmissionManage>,
    client: ClientAddress,
    State(state): State<AppState>,
) -> Result<Json<AdmissionWebhookSecret>, ApiError> {
    refuse_unless_holds_every_permission(require.grants(), ESCALATION_DETAIL)?;
    let principal = require.principal();
    let scope = principal.scope();
    let webhook = repo::get(&state.db, scope)
        .await
        .map_err(webhook_error)?
        .ok_or_else(webhook_not_found)?;
    let secret = Secret::generate().map_err(|e| ApiError::internal(&e))?;
    let sealed =
        seal_secret(&state.keys, scope, webhook.id, &secret).map_err(|e| ApiError::internal(&e))?;
    let now = state.clock.now();
    repo::rotate_secret(
        &state.db,
        scope,
        webhook.id,
        &webhook.sealed_secret,
        &sealed,
        now,
        client.actor(principal),
    )
    .await
    .map_err(webhook_error)?;
    Ok(Json(AdmissionWebhookSecret {
        secret: secret.expose_whsec(),
    }))
}

/// Delete the organization's admission webhook; sign-ins no longer call it.
#[utoipa::path(
    delete,
    path = "/api/v1/admin/admission/webhook",
    operation_id = "deleteAdmissionWebhook",
    tag = "admission",
    extensions(("x-stepclimb-mutates" = json!(true))),
    responses(
        (status = 204, description = "The webhook and its secret are deleted"),
        (status = 401, response = crate::openapi::Unauthenticated),
        (status = 403, response = AdmissionWebhookDeleteForbidden),
        (status = 404, response = AdmissionWebhookNotFound),
        (status = 500, response = crate::openapi::InternalError),
        (status = 503, response = crate::openapi::NoOrganization),
    )
)]
pub async fn delete_admission_webhook(
    require: Require<AdmissionManage>,
    client: ClientAddress,
    State(state): State<AppState>,
) -> Result<StatusCode, ApiError> {
    refuse_unless_holds_every_permission(require.grants(), ESCALATION_DETAIL)?;
    let principal = require.principal();
    let now = state.clock.now();
    repo::delete(&state.db, principal.scope(), now, client.actor(principal))
        .await
        .map_err(webhook_error)?;
    Ok(StatusCode::NO_CONTENT)
}

/// `secret`, sealed for the `secret` column of the scope's webhook `id`.
pub fn seal_secret(
    keys: &KeyRing,
    scope: OrgScope,
    id: Uuid,
    secret: &Secret,
) -> Result<Vec<u8>, EncryptError> {
    let org_id = scope.org_id();
    keys.encrypt(secret.expose_bytes(), &secret_context(&org_id, &id))
}

/// `[org_id, "admission_webhooks", id, "secret"]`.
pub fn secret_context<'a>(org_id: &'a Uuid, id: &'a Uuid) -> [&'a [u8]; 4] {
    [
        org_id.as_bytes(),
        SECRET_TABLE,
        id.as_bytes(),
        SECRET_COLUMN,
    ]
}

/// The code a 422 gives `url` under `policy`, or nothing when it is accepted.
pub fn check_url(url: &str, policy: UrlPolicy) -> Result<(), &'static str> {
    // The URL parser drops surrounding blanks and inner tabs and newlines, so the parsed URL
    // would not be the string kept; such a string is refused instead.
    let plain = !url.chars().any(|c| c.is_whitespace() || c.is_control());
    let length = url.chars().count();
    let parsed = Url::parse(url).ok().filter(|parsed| {
        plain
            && (1..=MAX_URL_CHARS).contains(&length)
            && matches!(parsed.scheme(), "https" | "http")
            && parsed.fragment().is_none()
    });
    let Some(parsed) = parsed else {
        return Err(code::WEBHOOK_URL_INVALID);
    };
    policy
        .check_url(&parsed)
        .map_err(|_| code::WEBHOOK_URL_REFUSED)
}

fn webhook_not_found() -> ApiError {
    ApiError::new(
        StatusCode::NOT_FOUND,
        code::WEBHOOK_NOT_FOUND,
        "The organization has no admission webhook.",
    )
}

/// The answer for an admission webhook repository error.
fn webhook_error(error: repo::Error) -> ApiError {
    match error {
        repo::Error::NotFound => webhook_not_found(),
        repo::Error::OrgNotFound => ApiError::new(
            StatusCode::NOT_FOUND,
            request_org::code::ORG_NOT_FOUND,
            "The request's organization cannot be determined.",
        ),
        repo::Error::SecretChanged => ApiError::new(
            StatusCode::CONFLICT,
            code::WEBHOOK_SECRET_CHANGED,
            "Another rotation replaced the secret while this one ran. Rotate again for a new one.",
        ),
        repo::Error::UrlRequired => {
            ApiError::invalid(vec![FieldError::at(&["url"], code::WEBHOOK_URL_REQUIRED)])
        }
        other => ApiError::internal(&other),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const STRICT: UrlPolicy = UrlPolicy::new(false, false);
    const INSECURE: UrlPolicy = UrlPolicy::new(true, true);

    #[test]
    fn an_https_url_with_a_query_is_accepted() {
        for url in [
            "https://hooks.example.test/admission",
            "https://hooks.example.test:8443/a?tenant=1",
            "https://93.184.215.14/hook",
        ] {
            assert_eq!(check_url(url, STRICT), Ok(()), "{url}");
        }
        assert_eq!(check_url("http://127.0.0.1:9000/hook", INSECURE), Ok(()));
    }

    #[test]
    fn a_malformed_url_is_invalid() {
        let long = format!("https://hooks.example.test/{}", "a".repeat(MAX_URL_CHARS));
        for url in [
            "",
            "hooks.example.test/admission",
            "/admission",
            "ftp://hooks.example.test/",
            "https://hooks.example.test/#part",
            " https://hooks.example.test/",
            "https://hooks.example.test/a b",
            "https://hooks.example.test/\n",
            long.as_str(),
        ] {
            assert_eq!(
                check_url(url, INSECURE),
                Err(code::WEBHOOK_URL_INVALID),
                "{url:?}"
            );
        }
        let longest = format!("https://h.example.test/{}", "a".repeat(MAX_URL_CHARS - 23));
        assert_eq!(longest.chars().count(), MAX_URL_CHARS);
        assert_eq!(check_url(&longest, INSECURE), Ok(()));
    }

    #[test]
    fn the_outbound_policy_refuses_its_classes() {
        for url in [
            "http://hooks.example.test/",
            "https://user:pass@hooks.example.test/",
            "https://127.0.0.1/",
            "https://10.0.0.1/",
            "https://169.254.169.254/latest/meta-data/",
            "https://[fe80::1]/",
        ] {
            assert_eq!(
                check_url(url, STRICT),
                Err(code::WEBHOOK_URL_REFUSED),
                "{url}"
            );
        }
        assert_eq!(
            check_url("https://10.0.0.1/", UrlPolicy::new(false, true)),
            Ok(())
        );
        assert_eq!(
            check_url("https://169.254.169.254/", INSECURE),
            Err(code::WEBHOOK_URL_REFUSED)
        );
    }

    #[test]
    fn only_a_body_that_sets_up_or_enables_the_webhook_needs_the_feature() {
        let patch = |url: Option<&str>, enabled| AdmissionWebhookPatch {
            url: url.map(str::to_owned),
            enabled,
        };
        assert!(patch(Some("https://h.example.test/"), None).needs_feature());
        assert!(patch(Some("https://h.example.test/"), Some(false)).needs_feature());
        assert!(patch(None, Some(true)).needs_feature());
        assert!(!patch(None, Some(false)).needs_feature());
        assert!(!patch(None, None).needs_feature());
    }

    #[test]
    fn a_body_names_only_what_it_changes() {
        let parse = |v: serde_json::Value| serde_json::from_value::<AdmissionWebhookPatch>(v);
        assert_eq!(
            parse(serde_json::json!({})).unwrap(),
            AdmissionWebhookPatch::default()
        );
        let off = parse(serde_json::json!({ "enabled": false })).unwrap();
        assert_eq!((off.url, off.enabled), (None, Some(false)));
        assert!(parse(serde_json::json!({ "url": null })).is_err());
        assert!(parse(serde_json::json!({ "enabled": null })).is_err());
        assert!(parse(serde_json::json!({ "enabled": "false" })).is_err());
        assert!(parse(serde_json::json!({ "secret": "whsec_x" })).is_err());
    }

    #[test]
    fn repository_refusals_map_to_their_status_code_and_field() {
        let missing = webhook_error(repo::Error::NotFound).problem();
        assert_eq!(
            (missing.status, missing.code.as_str()),
            (404, "admission.webhook_not_found")
        );
        let org = webhook_error(repo::Error::OrgNotFound).problem();
        assert_eq!((org.status, org.code.as_str()), (404, "org.not_found"));
        let required = webhook_error(repo::Error::UrlRequired).problem();
        assert_eq!(
            (required.status, required.code.as_str()),
            (422, "request.invalid")
        );
        assert_eq!(required.errors[0].field, "/url");
        assert_eq!(required.errors[0].code, "admission.webhook_url_required");
        let changed = webhook_error(repo::Error::SecretChanged).problem();
        assert_eq!(
            (changed.status, changed.code.as_str()),
            (409, "admission.webhook_secret_changed")
        );
    }
}
