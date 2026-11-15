// SPDX-License-Identifier: AGPL-3.0-or-later
//! The organization's sign-in providers under `/api/v1/admin/providers`
//! (docs/specs/m0/06-oauth2-and-vatsim-preset.md B14 to B17, Q2, Q5, API; `05-oidc.md` B1, Q1).
//!
//! Every route needs `provider.manage` at organization scope and sees only the `oidc` and
//! `oauth2` rows: the `local` row's methods are `PUT /api/v1/admin/local-sign-in`'s (B17).
//! A config is validated by its kind's config type (B4, `05-oidc.md` B1) under the outbound URL
//! policy the instance is under now (`05-oidc.md` Q4: private addresses only with one
//! organization), and its client secret is sealed to the row with the key ring. No answer, log
//! line or audit entry holds the secret: answers carry `client_secret_set` and the config
//! without it (`identity_providers::public_config`).
//!
//! A `PATCH` names what it changes: `label`, `enabled`, and `config` members, each named member
//! replacing the stored one whole and an absent `client_secret` keeping the stored secret. A
//! named `client_secret` is always a change: the stored secret is not opened to compare. The
//! kind and the key never change (Q5 (a)).
//!
//! Rules beyond the permission, all checked under the organization row's lock
//! ([`guard`]):
//!
//! - creating or changing an `oidc` provider needs the plan's `oidc_providers` feature, an
//!   `oauth2` one `oauth2_providers` (`05-oidc.md` Q1 (a)); deleting needs neither. The route
//!   serves both kinds, so the check reads the kind (the body's on create, the row's on
//!   change) after the permission check, and with one feature off a change to an unknown id
//!   answers 404;
//! - a row `stepclimb.toml` owns is read-only: 409 `provider.config_owned` (Q2 (a));
//! - deleting a provider with identities answers 409 `provider.in_use` (Q5 (a)); disabling it
//!   keeps them;
//! - disabling or deleting the organization's last way to sign in answers 409
//!   `local.last_method`;
//! - trusting a provider for email matching, changing what a provider with identities is, and,
//!   with sign-up on, turning a provider on, each need more than `provider.manage`
//!   ([`guard::Guard::check`]).
//!
//! Each write goes through `identity_providers::admin`, which writes the config through
//! `identity_providers::replace_config` (a row keeps its admission rules) and the audit entry.

mod guard;
pub mod schema;

use std::fmt;

use axum::{
    Json,
    extract::State,
    http::{StatusCode, header},
    response::{IntoResponse, Response},
};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use sqlx::PgConnection;
use stepclimb_auth::{
    encryption::KeyRing,
    oauth2_config::{self, Oauth2Config},
    oidc_config::{self, OidcConfig},
    outbound::UrlPolicy,
    provider_config::{ConfigErrors, ProviderRow, SealedSecret},
};
use stepclimb_core::{
    OrgScope,
    feature::{Oauth2Providers, OidcProviders},
    id::{Uuid, new_id},
    identity_providers::{
        self as providers, IdentityProvider, Kind, LABEL,
        admin::{self as repo, Change, NewProvider as NewRow},
        public_config,
    },
    organizations,
    perm::ProviderManage,
};
use utoipa::{IntoParams, ToSchema};

use self::guard::{Current, Guard, Proposed};
use crate::{
    AppState,
    audit::ClientAddress,
    entitlements::Entitlements,
    error::{ApiError, FieldError},
    extract::{ApiJson, ApiPath, ApiQuery},
    openapi::problem_responses,
    organization::present,
    pagination::{Page, PageParams},
    provider_sign_in::redirect_uri,
    require::Require,
    web_origin::WebOrigin,
};

/// The path of the providers collection; a provider is at `{PROVIDERS_PATH}/{provider_id}`.
pub const PROVIDERS_PATH: &str = "/api/v1/admin/providers";

/// The most problems one 422 lists.
pub const MAX_ERRORS: usize = 20;

/// The codes this module produces.
pub mod code {
    /// The organization has no `oidc` or `oauth2` provider with this id (404).
    pub const PROVIDER_NOT_FOUND: &str = "provider.not_found";
    /// Another provider of the organization has the key (409).
    pub const PROVIDER_KEY_TAKEN: &str = "provider.key_taken";
    /// `stepclimb.toml` declares the provider, so the API cannot change it (409).
    pub const PROVIDER_CONFIG_OWNED: &str = "provider.config_owned";
    /// Identities still link users to the provider; disable it instead (409).
    pub const PROVIDER_IN_USE: &str = "provider.in_use";
    /// The kind is not `oidc` or `oauth2` (422).
    pub const PROVIDER_KIND_UNSUPPORTED: &str = "provider.kind_unsupported";
    /// This server has no preset of that name (422).
    pub const PROVIDER_PRESET_UNKNOWN: &str = "provider.preset_unknown";
    /// The config breaks a rule; `errors` lists each one, under `/config` (422).
    pub const PROVIDER_CONFIG_INVALID: &str = "provider.config_invalid";
    /// In a 422's `errors`: the key is not 1 to 32 lowercase letters, digits and hyphens
    /// starting with a letter or digit, or it is `local`.
    pub const PROVIDER_KEY_INVALID: &str = "provider.key_invalid";
    /// In a 422's `errors`: the label is empty, longer than 200 characters or holds a control
    /// character.
    pub const PROVIDER_LABEL_INVALID: &str = "provider.label_invalid";
}

problem_responses! {
    ProvidersForbidden => "`permission.denied`: the caller does not hold `provider.manage` at organization scope.",
    ProviderWriteForbidden => "`entitlement.feature_unavailable`: the organization's plan does not include the provider's kind (`oidc_providers` for `oidc`, `oauth2_providers` for `oauth2`). `permission.denied`: the caller does not hold `provider.manage` at organization scope; or sign-up is on and the write turns the provider on, or changes the issuer, endpoint origins or subject pointer of an enabled one, without `admission.manage`. `permission.escalation`: the write trusts the provider for email matching, or changes the issuer, endpoint origins, subject pointer or email claims of a trusted one, or changes the issuer, endpoint origins or subject pointer of a provider identities link users to, and the caller does not hold every catalog permission at organization scope. `auth.csrf_refused`: the request did not come from this site.",
    ProviderDeleteForbidden => "`permission.denied`: the caller does not hold `provider.manage` at organization scope. `auth.csrf_refused`: the request did not come from this site.",
    ProviderNotFound => "`provider.not_found`: the organization has no `oidc` or `oauth2` provider with this id. `org.not_found`: the request's organization cannot be determined.",
    ProviderCreateConflict => "`provider.key_taken`: another provider of the organization has the key.",
    ProviderUpdateConflict => "`provider.config_owned`: `stepclimb.toml` declares the provider; change it there. `local.last_method`: disabling it would leave the organization no way to sign in.",
    ProviderDeleteConflict => "`provider.config_owned`: `stepclimb.toml` declares the provider; remove it there, and the server disables it. `provider.in_use`: identities still link users to the provider; disable it instead. `local.last_method`: deleting it would leave the organization no way to sign in.",
    ProviderInvalid => "`provider.config_invalid`: the config breaks a rule; `errors` lists each one (at most 20) with a JSON Pointer under `/config` and a `provider.*` code. `provider.kind_unsupported`: the kind is not `oidc` or `oauth2`. `provider.preset_unknown`: this server has no such preset. `request.invalid`: `errors` names `/key` (`provider.key_invalid`) or `/label` (`provider.label_invalid`).",
}

/// A provider's protocol.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, ToSchema)]
#[serde(rename_all = "lowercase")]
pub enum ProviderKind {
    /// OpenID Connect.
    Oidc,
    /// OAuth 2 with a JSON user endpoint.
    Oauth2,
}

/// Who owns a provider's row.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, ToSchema)]
#[serde(rename_all = "lowercase")]
pub enum ProviderSource {
    /// Created through this API, which may change and delete it.
    Api,
    /// Declared in `stepclimb.toml`: read-only here.
    Config,
}

/// An `oidc` or `oauth2` provider of the organization.
#[derive(Clone, Debug, PartialEq, Serialize, ToSchema)]
pub struct Provider {
    #[schema(value_type = String, format = "uuid")]
    pub id: Uuid,
    /// The `{provider}` of `/auth/{provider}/start`; fixed once created.
    pub key: String,
    pub kind: ProviderKind,
    /// The text of the provider's sign-in button.
    pub label: String,
    /// Whether people can sign in through it. A disabled provider keeps its identities.
    pub enabled: bool,
    pub source: ProviderSource,
    /// The URI to register at the provider as the client's redirect URI.
    pub redirect_uri: String,
    /// Whether the provider has a client secret. The secret itself is never answered.
    pub client_secret_set: bool,
    /// The kind's configuration, without the client secret.
    #[schema(value_type = schema::ProviderConfig)]
    pub config: Map<String, Value>,
}

impl Provider {
    /// The answer for `provider`; a 500 for a row this API cannot show (the local row, or one
    /// without a label), which neither this API nor startup writes.
    fn of(provider: &IdentityProvider, public_url: &WebOrigin) -> Result<Self, ApiError> {
        let kind = match provider.kind {
            Kind::Oidc => ProviderKind::Oidc,
            Kind::Oauth2 => ProviderKind::Oauth2,
            Kind::Local => return Err(ApiError::internal(&Unshowable("the local row"))),
        };
        let label = provider
            .label()
            .ok_or_else(|| ApiError::internal(&Unshowable("a row without a label")))?;
        let from_file = provider.is_from_config_file();
        Ok(Self {
            id: provider.id,
            key: provider.key.clone(),
            kind,
            label: label.to_owned(),
            enabled: provider.enabled,
            source: if from_file {
                ProviderSource::Config
            } else {
                ProviderSource::Api
            },
            redirect_uri: redirect_uri(public_url, &provider.key),
            client_secret_set: from_file || provider.config.contains_key(providers::CLIENT_SECRET),
            config: public_config(&provider.config),
        })
    }
}

/// A stored provider row this API cannot answer with.
#[derive(Debug, thiserror::Error)]
#[error("the providers API cannot show {0}")]
struct Unshowable(&'static str);

fn yes() -> bool {
    true
}

/// A provider to create.
//
// `Debug` names the config's members, never their values: the config holds the client secret
// in the clear.
#[derive(Clone, PartialEq, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct NewProvider {
    /// 1 to 32 lowercase letters, digits and hyphens, starting with a letter or digit; not
    /// `local`. It is part of the redirect URI and never changes.
    pub key: String,
    /// `oidc` or `oauth2`; anything else answers 422 `provider.kind_unsupported`.
    pub kind: String,
    /// A preset to take the provider's endpoints and claims from. This server has none, so any
    /// value answers 422 `provider.preset_unknown`.
    #[serde(default, deserialize_with = "present")]
    #[schema(value_type = String)]
    pub preset: Option<String>,
    /// 1 to 200 characters without control characters.
    pub label: String,
    /// Whether people can sign in through it; `true` when omitted.
    #[serde(default = "yes")]
    pub enabled: bool,
    /// The kind's configuration: the members of an `OidcProviderConfig` or an
    /// `Oauth2ProviderConfig` and `client_secret`, which is stored encrypted and never answered.
    /// Members with a default may be left out.
    #[schema(value_type = std::collections::BTreeMap<String, Value>)]
    pub config: Map<String, Value>,
}

/// A change to a provider: send only what you change. A member sent as `null` is malformed.
//
// `Debug` names the config's members, never their values: they can hold the client secret.
#[derive(Clone, Default, PartialEq, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct ProviderPatch {
    /// 1 to 200 characters without control characters.
    #[serde(default, deserialize_with = "present")]
    #[schema(value_type = String)]
    pub label: Option<String>,
    #[serde(default, deserialize_with = "present")]
    #[schema(value_type = bool)]
    pub enabled: Option<bool>,
    /// Config members to replace, each whole; members not named keep their value, and an
    /// absent `client_secret` keeps the stored secret.
    #[serde(default, deserialize_with = "present")]
    #[schema(value_type = std::collections::BTreeMap<String, Value>)]
    pub config: Option<Map<String, Value>>,
}

/// The names of `config`'s members, for a `Debug` that must not show a value.
fn member_names(config: &Map<String, Value>) -> Vec<&str> {
    config.keys().map(String::as_str).collect()
}

impl fmt::Debug for NewProvider {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("NewProvider")
            .field("key", &self.key)
            .field("kind", &self.kind)
            .field("preset", &self.preset)
            .field("label", &self.label)
            .field("enabled", &self.enabled)
            .field("config", &member_names(&self.config))
            .finish()
    }
}

impl fmt::Debug for ProviderPatch {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ProviderPatch")
            .field("label", &self.label)
            .field("enabled", &self.enabled)
            .field("config", &self.config.as_ref().map(member_names))
            .finish()
    }
}

/// The provider's id in the path.
#[derive(Debug, Clone, Copy, Deserialize, IntoParams)]
#[into_params(parameter_in = Path)]
pub struct ProviderIdParam {
    /// The provider's id.
    #[param(value_type = String, format = "uuid")]
    pub provider_id: Uuid,
}

/// List the organization's `oidc` and `oauth2` providers, by key.
#[utoipa::path(
    get,
    path = "/api/v1/admin/providers",
    operation_id = "listProviders",
    tag = "providers",
    params(PageParams),
    responses(
        (status = 200, description = "One page of providers", body = Page<Provider>),
        (status = 400, response = crate::openapi::BadRequest),
        (status = 401, response = crate::openapi::Unauthenticated),
        (status = 403, response = ProvidersForbidden),
        (status = 404, response = crate::openapi::OrganizationNotFound),
        (status = 500, response = crate::openapi::InternalError),
        (status = 503, response = crate::openapi::NoOrganization),
    )
)]
pub async fn list_providers(
    require: Require<ProviderManage>,
    State(state): State<AppState>,
    ApiQuery(params): ApiQuery<PageParams>,
) -> Result<Json<Page<Provider>>, ApiError> {
    let page = params.parse::<String>()?;
    let scope = require.principal().scope();
    let after = page.after().map(String::as_str);
    let rows = repo::list(&state.db, scope, after, page.fetch_limit())
        .await
        .map_err(|error| match error {
            // Only the cursor's key is a caller's string here.
            repo::Error::Provider(providers::Error::ContainsNul { .. }) => {
                ApiError::invalid_cursor()
            }
            other => provider_error(other),
        })?;
    let items = rows
        .iter()
        .map(|row| Provider::of(row, &state.public_url))
        .collect::<Result<Vec<_>, _>>()?;
    Ok(Json(page.page(items, |provider| provider.key.clone())?))
}

/// Read one provider.
#[utoipa::path(
    get,
    path = "/api/v1/admin/providers/{provider_id}",
    operation_id = "getProvider",
    tag = "providers",
    params(ProviderIdParam),
    responses(
        (status = 200, description = "The provider", body = Provider),
        (status = 400, response = crate::openapi::BadRequest),
        (status = 401, response = crate::openapi::Unauthenticated),
        (status = 403, response = ProvidersForbidden),
        (status = 404, response = ProviderNotFound),
        (status = 500, response = crate::openapi::InternalError),
        (status = 503, response = crate::openapi::NoOrganization),
    )
)]
pub async fn get_provider(
    require: Require<ProviderManage>,
    State(state): State<AppState>,
    ApiPath(ProviderIdParam { provider_id }): ApiPath<ProviderIdParam>,
) -> Result<Json<Provider>, ApiError> {
    let provider = repo::get(&state.db, require.principal().scope(), provider_id)
        .await
        .map_err(provider_error)?
        .ok_or_else(provider_not_found)?;
    Ok(Json(Provider::of(&provider, &state.public_url)?))
}

/// Create an `oidc` or `oauth2` provider.
#[utoipa::path(
    post,
    path = "/api/v1/admin/providers",
    operation_id = "createProvider",
    tag = "providers",
    extensions(("x-stepclimb-mutates" = json!(true))),
    request_body = NewProvider,
    responses(
        (
            status = 201,
            description = "The provider is created",
            body = Provider,
            headers(("Location" = String, description = "The provider's path"))
        ),
        (status = 400, response = crate::openapi::BadRequest),
        (status = 401, response = crate::openapi::Unauthenticated),
        (status = 403, response = ProviderWriteForbidden),
        (status = 404, response = crate::openapi::OrganizationNotFound),
        (status = 409, response = ProviderCreateConflict),
        (status = 413, response = crate::openapi::PayloadTooLarge),
        (status = 415, response = crate::openapi::UnsupportedMediaType),
        (status = 422, response = ProviderInvalid),
        (status = 500, response = crate::openapi::InternalError),
        (status = 503, response = crate::openapi::NoOrganization),
    )
)]
pub async fn create_provider(
    require: Require<ProviderManage>,
    entitlements: Entitlements,
    client: ClientAddress,
    State(state): State<AppState>,
    ApiJson(body): ApiJson<NewProvider>,
) -> Result<Response, ApiError> {
    let kind = new_kind(&body)?;
    require_kind_feature(&entitlements, kind)?;
    check_key(&body.key)?;
    check_label(&body.label)?;
    let principal = require.principal();
    let scope = principal.scope();
    let mut tx = state.db.begin().await.map_err(|e| ApiError::internal(&e))?;
    let guard = Guard::lock(&mut tx, scope, require.grants()).await?;
    let id = new_id();
    let sealing = Sealing {
        policy: policy(&state, &mut tx).await?,
        keys: &state.keys,
        row: ProviderRow {
            org_id: scope.org_id(),
            provider_id: id,
        },
    };
    let config = sealing.config(kind, body.config, None, &body.label)?;
    let enabled = body.enabled;
    guard.check(
        None,
        &Proposed {
            kind,
            config: &config,
            enabled,
        },
    )?;
    let row = NewRow {
        id,
        key: &body.key,
        kind,
        config: &config,
        enabled,
    };
    let actor = client.actor(principal);
    let created = repo::create(&mut tx, scope, &row, state.clock.now(), actor)
        .await
        .map_err(provider_error)?;
    tx.commit().await.map_err(|e| ApiError::internal(&e))?;
    let location = format!("{PROVIDERS_PATH}/{}", created.id);
    let answer = Provider::of(&created, &state.public_url)?;
    Ok((
        StatusCode::CREATED,
        [(header::LOCATION, location)],
        Json(answer),
    )
        .into_response())
}

/// Change a provider's label, whether it is enabled, or members of its config. Its key and
/// kind never change.
#[utoipa::path(
    patch,
    path = "/api/v1/admin/providers/{provider_id}",
    operation_id = "updateProvider",
    tag = "providers",
    extensions(("x-stepclimb-mutates" = json!(true))),
    params(ProviderIdParam),
    request_body = ProviderPatch,
    responses(
        (status = 200, description = "The provider after the change", body = Provider),
        (status = 400, response = crate::openapi::BadRequest),
        (status = 401, response = crate::openapi::Unauthenticated),
        (status = 403, response = ProviderWriteForbidden),
        (status = 404, response = ProviderNotFound),
        (status = 409, response = ProviderUpdateConflict),
        (status = 413, response = crate::openapi::PayloadTooLarge),
        (status = 415, response = crate::openapi::UnsupportedMediaType),
        (status = 422, response = ProviderInvalid),
        (status = 500, response = crate::openapi::InternalError),
        (status = 503, response = crate::openapi::NoOrganization),
    )
)]
pub async fn update_provider(
    require: Require<ProviderManage>,
    entitlements: Entitlements,
    client: ClientAddress,
    State(state): State<AppState>,
    ApiPath(ProviderIdParam { provider_id }): ApiPath<ProviderIdParam>,
    ApiJson(body): ApiJson<ProviderPatch>,
) -> Result<Json<Provider>, ApiError> {
    let principal = require.principal();
    let scope = principal.scope();
    let mut tx = state.db.begin().await.map_err(|e| ApiError::internal(&e))?;
    let guard = Guard::lock(&mut tx, scope, require.grants()).await?;
    let current = locked(&mut tx, scope, provider_id).await?;
    require_kind_feature(&entitlements, current.kind)?;
    refuse_config_owned(&current)?;
    let (config, enabled) = patched(&state, &mut tx, scope, &current, body).await?;
    let identities = providers::identity_count(&mut *tx, scope, current.id)
        .await
        .map_err(|e| ApiError::internal(&e))?;
    let before = Current {
        provider: &current,
        identities,
    };
    let after = Proposed {
        kind: current.kind,
        config: &config,
        enabled,
    };
    guard.check(Some(&before), &after)?;
    guard
        .refuse_lockout(&mut tx, scope, &current, enabled)
        .await?;
    let change = Change {
        config: &config,
        enabled,
    };
    let actor = client.actor(principal);
    let updated = repo::update(&mut tx, scope, &current, &change, state.clock.now(), actor)
        .await
        .map_err(provider_error)?;
    tx.commit().await.map_err(|e| ApiError::internal(&e))?;
    Ok(Json(Provider::of(&updated, &state.public_url)?))
}

/// Delete a provider no identity links a user to. To keep its identities, disable it instead.
#[utoipa::path(
    delete,
    path = "/api/v1/admin/providers/{provider_id}",
    operation_id = "deleteProvider",
    tag = "providers",
    extensions(("x-stepclimb-mutates" = json!(true))),
    params(ProviderIdParam),
    responses(
        (status = 204, description = "The provider is deleted"),
        (status = 400, response = crate::openapi::BadRequest),
        (status = 401, response = crate::openapi::Unauthenticated),
        (status = 403, response = ProviderDeleteForbidden),
        (status = 404, response = ProviderNotFound),
        (status = 409, response = ProviderDeleteConflict),
        (status = 500, response = crate::openapi::InternalError),
        (status = 503, response = crate::openapi::NoOrganization),
    )
)]
pub async fn delete_provider(
    require: Require<ProviderManage>,
    client: ClientAddress,
    State(state): State<AppState>,
    ApiPath(ProviderIdParam { provider_id }): ApiPath<ProviderIdParam>,
) -> Result<StatusCode, ApiError> {
    let principal = require.principal();
    let scope = principal.scope();
    let mut tx = state.db.begin().await.map_err(|e| ApiError::internal(&e))?;
    let guard = Guard::lock(&mut tx, scope, require.grants()).await?;
    let current = locked(&mut tx, scope, provider_id).await?;
    refuse_config_owned(&current)?;
    guard
        .refuse_lockout(&mut tx, scope, &current, false)
        .await?;
    let actor = client.actor(principal);
    repo::delete(&mut tx, scope, &current, state.clock.now(), actor)
        .await
        .map_err(provider_error)?;
    tx.commit().await.map_err(|e| ApiError::internal(&e))?;
    Ok(StatusCode::NO_CONTENT)
}

/// The scope's provider `id`, locked until the transaction ends, or 404.
async fn locked(
    conn: &mut PgConnection,
    scope: OrgScope,
    id: Uuid,
) -> Result<IdentityProvider, ApiError> {
    repo::lock(conn, scope, id)
        .await
        .map_err(provider_error)?
        .ok_or_else(provider_not_found)
}

/// The config and `enabled` `current` would have after `patch`. A patch naming no config
/// member keeps the stored config as it is (with the new label); one naming any is validated
/// whole, the named members over the stored ones.
async fn patched(
    state: &AppState,
    conn: &mut PgConnection,
    scope: OrgScope,
    current: &IdentityProvider,
    patch: ProviderPatch,
) -> Result<(Map<String, Value>, bool), ApiError> {
    if let Some(label) = &patch.label {
        check_label(label)?;
    }
    let label = match (patch.label.as_deref(), current.label()) {
        (Some(label), _) | (None, Some(label)) => label.to_owned(),
        (None, None) => return Err(invalid(&["label"], code::PROVIDER_LABEL_INVALID)),
    };
    let enabled = patch.enabled.unwrap_or(current.enabled);
    let mut config = match patch.config {
        None => current.config.clone(),
        Some(members) => {
            let mut merged = public_config(&current.config);
            merged.extend(members);
            let sealing = Sealing {
                policy: policy(state, conn).await?,
                keys: &state.keys,
                row: ProviderRow {
                    org_id: scope.org_id(),
                    provider_id: current.id,
                },
            };
            let stored = sealed_secret(current);
            sealing.config(current.kind, merged, stored, &label)?
        }
    };
    config.remove(stepclimb_core::admission_rules::PROVIDER_MEMBER);
    config.insert(LABEL.into(), label.into());
    Ok((config, enabled))
}

/// How a config is validated and sealed for one row.
struct Sealing<'a> {
    policy: UrlPolicy,
    keys: &'a KeyRing,
    row: ProviderRow,
}

impl Sealing<'_> {
    /// `input` validated as `kind`'s config and sealed to the row, with the label; `stored` is
    /// the row's sealed secret, kept when `input` names none.
    fn config(
        &self,
        kind: Kind,
        input: Map<String, Value>,
        stored: Option<SealedSecret>,
        label: &str,
    ) -> Result<Map<String, Value>, ApiError> {
        let input = Value::Object(input);
        let sealed = |e: stepclimb_auth::encryption::EncryptError| ApiError::internal(&e);
        let mut config = match kind {
            Kind::Oidc => oidc_config::validate(&input, self.policy, stored)
                .map_err(config_invalid)?
                .seal(self.keys, self.row)
                .map_err(sealed)?
                .to_json(),
            Kind::Oauth2 => oauth2_config::validate(&input, self.policy, stored)
                .map_err(config_invalid)?
                .seal(self.keys, self.row)
                .map_err(sealed)?
                .to_json(),
            Kind::Local => return Err(kind_unsupported()),
        };
        config.insert(LABEL.into(), label.into());
        Ok(config)
    }
}

/// The sealed secret of `provider`'s stored config, when it reads.
fn sealed_secret(provider: &IdentityProvider) -> Option<SealedSecret> {
    match provider.kind {
        Kind::Oidc => OidcConfig::from_stored(&provider.config)
            .ok()
            .and_then(|c| c.sealed_secret()),
        Kind::Oauth2 => Oauth2Config::from_stored(&provider.config)
            .ok()
            .and_then(|c| c.sealed_secret()),
        Kind::Local => None,
    }
}

/// The outbound URL policy the instance is under now: the insecure switch, and private
/// addresses only with one organization (`05-oidc.md` Q4).
async fn policy(state: &AppState, conn: &mut PgConnection) -> Result<UrlPolicy, ApiError> {
    let tenancy = organizations::tenancy(&mut *conn)
        .await
        .map_err(|e| ApiError::internal(&e))?;
    Ok(state.providers.client(tenancy).policy())
}

/// The kind a new provider's body names, or 422.
fn new_kind(body: &NewProvider) -> Result<Kind, ApiError> {
    if body.preset.is_some() {
        return Err(ApiError::new(
            StatusCode::UNPROCESSABLE_ENTITY,
            code::PROVIDER_PRESET_UNKNOWN,
            "This server has no provider preset of that name.",
        )
        .with_errors(vec![FieldError::at(
            &["preset"],
            code::PROVIDER_PRESET_UNKNOWN,
        )]));
    }
    match Kind::parse(&body.kind) {
        Some(kind @ (Kind::Oidc | Kind::Oauth2)) => Ok(kind),
        Some(Kind::Local) | None => Err(kind_unsupported()),
    }
}

/// 403 `entitlement.feature_unavailable` when the plan lacks `kind`'s feature.
fn require_kind_feature(entitlements: &Entitlements, kind: Kind) -> Result<(), ApiError> {
    match kind {
        Kind::Oidc => entitlements.check::<OidcProviders>(),
        Kind::Oauth2 => entitlements.check::<Oauth2Providers>(),
        Kind::Local => Err(provider_not_found()),
    }
}

fn check_key(key: &str) -> Result<(), ApiError> {
    if providers::valid_key(key) && key != stepclimb_core::local_sign_in::PROVIDER_KEY {
        Ok(())
    } else {
        Err(invalid(&["key"], code::PROVIDER_KEY_INVALID))
    }
}

fn check_label(label: &str) -> Result<(), ApiError> {
    if providers::valid_label(label) {
        Ok(())
    } else {
        Err(invalid(&["label"], code::PROVIDER_LABEL_INVALID))
    }
}

fn invalid(path: &[&str], code: &'static str) -> ApiError {
    ApiError::invalid(vec![FieldError::at(path, code)])
}

/// 409 `provider.config_owned` for a row `stepclimb.toml` declares.
fn refuse_config_owned(provider: &IdentityProvider) -> Result<(), ApiError> {
    if !provider.is_from_config_file() {
        return Ok(());
    }
    Err(ApiError::new(
        StatusCode::CONFLICT,
        code::PROVIDER_CONFIG_OWNED,
        "stepclimb.toml declares this provider, so it can be changed only there.",
    ))
}

fn kind_unsupported() -> ApiError {
    ApiError::new(
        StatusCode::UNPROCESSABLE_ENTITY,
        code::PROVIDER_KIND_UNSUPPORTED,
        "This API creates only oidc and oauth2 providers; local sign-in is set under /api/v1/admin/local-sign-in.",
    )
    .with_errors(vec![FieldError::at(&["kind"], code::PROVIDER_KIND_UNSUPPORTED)])
}

/// 422 `provider.config_invalid` with each problem under `/config`, at most [`MAX_ERRORS`].
fn config_invalid(ConfigErrors(problems): ConfigErrors) -> ApiError {
    let errors = problems
        .iter()
        .take(MAX_ERRORS)
        .map(|problem| FieldError {
            field: format!("/config{}", problem.pointer),
            code: problem.code.to_owned(),
        })
        .collect();
    ApiError::new(
        StatusCode::UNPROCESSABLE_ENTITY,
        code::PROVIDER_CONFIG_INVALID,
        "The provider config breaks one or more rules; `errors` lists them.",
    )
    .with_errors(errors)
}

fn provider_not_found() -> ApiError {
    ApiError::new(
        StatusCode::NOT_FOUND,
        code::PROVIDER_NOT_FOUND,
        "No provider with this id exists in this organization.",
    )
}

/// The answer for an admin repository error.
fn provider_error(error: repo::Error) -> ApiError {
    use repo::Error as E;
    match error {
        E::NotFound => provider_not_found(),
        E::KeyTaken => ApiError::new(
            StatusCode::CONFLICT,
            code::PROVIDER_KEY_TAKEN,
            "Another provider of this organization has this key.",
        ),
        E::InUse => ApiError::new(
            StatusCode::CONFLICT,
            code::PROVIDER_IN_USE,
            "Identities still link users to this provider. Disable it instead, which keeps them.",
        ),
        E::KeyInvalid => invalid(&["key"], code::PROVIDER_KEY_INVALID),
        E::LabelInvalid => invalid(&["label"], code::PROVIDER_LABEL_INVALID),
        E::KindUnsupported => kind_unsupported(),
        other @ (E::SourceMember | E::Provider(_) | E::Time(_) | E::Database(_)) => {
            ApiError::internal(&other)
        }
    }
}

#[cfg(test)]
mod tests;
