// SPDX-License-Identifier: AGPL-3.0-or-later
//! `GET /.well-known/stepclimb`, the discovery document of the request's organization
//! (docs/specs/m0/08-sessions-and-csrf.md B38; `11-app-shell.md` B13, Q4).
//!
//! The document is public: the sign-in screen reads it before anyone has signed in, and the
//! Tauri app (M5) reads it when a user links a server. It therefore carries only:
//!
//! - the organization's `name`;
//! - `sign_in`, the methods the organization offers: the local entry first when local sign-in is
//!   offered, with the local methods `LocalSignIn::offered()` returns (`totp` only beside
//!   `password`, as it is a second step after one), then each enabled outside
//!   provider, sorted by key, with its `key`, `kind` and the `label` member of its config. No
//!   other member of a provider's config, which holds its sealed client secret, and no row id
//!   reaches the document;
//! - `theme`, the absolute URL of the organization's stylesheet fingerprinted with its hash, as
//!   the page the server serves links it (`spa`);
//! - `oauth`, the absolute URLs of the OAuth 2.1 endpoints the apps sign in through: the
//!   authorization, token and revocation endpoints, with the `issuer` the authorization
//!   endpoint's redirects carry as `iss` (RFC 9207).
//!
//! The organization comes from `RequestOrg`, and every read is scoped to it. The answer is
//! `Cache-Control: no-cache`: it changes whenever an admin switches a method or saves branding,
//! so a cache must ask again on every use.

use axum::{
    Json,
    extract::State,
    http::{HeaderValue, header},
    response::{IntoResponse, Response},
};
use serde::Serialize;
use stepclimb_core::{
    OrgScope,
    identity_providers::{self, IdentityProvider, Kind},
    local_sign_in::{self, Method},
    organizations,
};
use utoipa::ToSchema;

use crate::{
    AppState,
    branding::{self, THEME_CSS_PATH, THEME_CSS_VERSION_PARAM},
    error::ApiError,
    oauth_authorize::AUTHORIZE_PATH,
    oauth_token::{REVOKE_PATH, TOKEN_PATH},
    request_org::{self, RequestOrg},
    spa::is_css_hash,
    web_origin::WebOrigin,
};

/// The path of the discovery document.
pub const DISCOVERY_PATH: &str = "/.well-known/stepclimb";

/// The document's `Cache-Control`: revalidated on every use, since a switched-off method or a
/// saved branding change must show on the next read.
pub const DISCOVERY_CACHE_CONTROL: &str = "no-cache";

/// The request organization's discovery document.
#[derive(Debug, Serialize, ToSchema)]
pub struct Discovery {
    /// The organization's name.
    pub name: String,
    /// The sign-in methods the organization offers, in the order a sign-in screen lists them:
    /// local sign-in first when it is offered, then the outside providers by key. Empty when it
    /// offers none.
    pub sign_in: Vec<SignInMethod>,
    /// The organization's theme.
    pub theme: DiscoveryTheme,
    /// The OAuth 2.1 endpoints Stepclimb's own apps sign in through.
    pub oauth: DiscoveryOauth,
}

/// The OAuth 2.1 server's issuer and endpoints, as absolute URLs on the public URL.
#[derive(Debug, Serialize, ToSchema)]
pub struct DiscoveryOauth {
    /// The server's issuer identifier, the public URL. Every answer the authorization endpoint
    /// sends to a redirect URI carries it as `iss` (RFC 9207); an app checks the two agree, and
    /// that this value is the URL it fetched the document from.
    pub issuer: String,
    /// `<public URL>/oauth/authorize`, which the app opens in the system browser with its PKCE
    /// challenge.
    pub authorization_endpoint: String,
    /// `<public URL>/oauth/token`, where the app exchanges its code and refreshes its tokens.
    pub token_endpoint: String,
    /// `<public URL>/oauth/revoke`, where the app ends its sign-in (RFC 7009).
    pub revocation_endpoint: String,
}

/// One sign-in method the organization offers.
#[derive(Debug, Serialize, ToSchema)]
pub struct SignInMethod {
    /// `local` for local sign-in; otherwise the provider's key, the `{provider}` of
    /// `/auth/{provider}/start`.
    pub key: String,
    /// What the method speaks. A preset provider is the kind it speaks (`oauth2`).
    pub kind: SignInKind,
    /// The provider's configured label, such as "Sign in with Example"; `null` when its config
    /// holds none, and always for local sign-in, whose labels are the web app's.
    #[schema(required = true)]
    pub label: Option<String>,
    /// Local sign-in only: the local methods the organization offers. `totp` is the second step
    /// after `password` and is listed only beside it.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schema(required = false, nullable = false)]
    pub methods: Option<Vec<LocalMethod>>,
}

/// The protocol of a sign-in method.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum SignInKind {
    /// The organization's own accounts.
    Local,
    /// An OpenID Connect provider.
    Oidc,
    /// An OAuth 2 provider with a JSON user endpoint.
    Oauth2,
}

/// A local sign-in method.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum LocalMethod {
    Passkey,
    Password,
    Totp,
    MagicLink,
}

impl From<Method> for LocalMethod {
    fn from(method: Method) -> Self {
        match method {
            Method::Passkey => Self::Passkey,
            Method::Password => Self::Password,
            Method::Totp => Self::Totp,
            Method::MagicLink => Self::MagicLink,
        }
    }
}

/// The organization's theme, as the apps load it.
#[derive(Debug, Serialize, ToSchema)]
pub struct DiscoveryTheme {
    /// The absolute URL of the organization's stylesheet, `<public URL>/theme.css?v=<SHA-256>`,
    /// which is cached for a year; a branding change gives it another hash.
    pub stylesheet: String,
}

/// The discovery document of the request's organization: its name, the sign-in methods it
/// offers and its theme.
#[utoipa::path(
    get,
    path = "/.well-known/stepclimb",
    operation_id = "getDiscovery",
    tag = "auth",
    responses(
        (
            status = 200,
            description = "The organization's discovery document. `Cache-Control: no-cache`.",
            body = Discovery,
            headers(("Cache-Control" = String, description = "`no-cache`"))
        ),
        (status = 404, response = crate::openapi::OrganizationNotFound),
        (status = 500, response = crate::openapi::InternalError),
        (status = 503, response = crate::openapi::NoOrganization),
    )
)]
pub async fn get_discovery(
    org: RequestOrg,
    State(state): State<AppState>,
) -> Result<Response, ApiError> {
    let scope = org.scope();
    let name = organization_name(&state, scope).await?;
    let sign_in = sign_in_methods(&state, scope).await?;
    let served = branding::served(&state, Ok(org)).await?;
    let document = Discovery {
        name,
        sign_in,
        theme: DiscoveryTheme {
            stylesheet: stylesheet_url(&state.public_url, &served.css_hash),
        },
        oauth: DiscoveryOauth {
            issuer: crate::oauth_authorize::issuer(&state),
            authorization_endpoint: public_endpoint(&state.public_url, AUTHORIZE_PATH),
            token_endpoint: public_endpoint(&state.public_url, TOKEN_PATH),
            revocation_endpoint: public_endpoint(&state.public_url, REVOKE_PATH),
        },
    };
    let cache = [(
        header::CACHE_CONTROL,
        HeaderValue::from_static(DISCOVERY_CACHE_CONTROL),
    )];
    Ok((cache, Json(document)).into_response())
}

async fn organization_name(state: &AppState, scope: OrgScope) -> Result<String, ApiError> {
    match organizations::get(&state.db, scope).await {
        Ok(organization) => Ok(organization.name),
        Err(organizations::Error::NotFound) => Err(org_not_found()),
        Err(error) => Err(ApiError::internal(&error)),
    }
}

/// The local entry when local sign-in is offered, then every enabled outside provider.
async fn sign_in_methods(state: &AppState, scope: OrgScope) -> Result<Vec<SignInMethod>, ApiError> {
    let local = match local_sign_in::get(&state.db, scope).await {
        Ok(local) => local,
        Err(local_sign_in::Error::NotFound) => return Err(org_not_found()),
        Err(error) => return Err(ApiError::internal(&error)),
    };
    let providers = identity_providers::list_enabled(&state.db, scope)
        .await
        .map_err(|error| ApiError::internal(&error))?;
    let offered = local.offered();
    let mut methods = Vec::with_capacity(providers.len() + 1);
    if !offered.is_empty() {
        methods.push(SignInMethod {
            key: local_sign_in::PROVIDER_KEY.to_owned(),
            kind: SignInKind::Local,
            label: None,
            methods: Some(offered.into_iter().map(LocalMethod::from).collect()),
        });
    }
    methods.extend(providers.iter().filter_map(provider_method));
    Ok(methods)
}

/// A provider's entry: its key, kind and label, and nothing else of its row. `list_enabled`
/// never returns the local row; were it to, the row is left out, since its methods are the
/// local entry's.
fn provider_method(provider: &IdentityProvider) -> Option<SignInMethod> {
    let kind = match provider.kind {
        Kind::Oidc => SignInKind::Oidc,
        Kind::Oauth2 => SignInKind::Oauth2,
        Kind::Local => return None,
    };
    let label = provider.label().map(str::to_owned);
    Some(SignInMethod {
        key: provider.key.clone(),
        kind,
        label,
        methods: None,
    })
}

/// `<public URL>/theme.css?v=<hash>`, or without the `v` when the stored hash is not a SHA-256,
/// as `spa` links it, so nothing but hex digits reaches the URL.
fn stylesheet_url(public_url: &WebOrigin, css_hash: &str) -> String {
    if is_css_hash(css_hash) {
        format!("{public_url}{THEME_CSS_PATH}?{THEME_CSS_VERSION_PARAM}={css_hash}")
    } else {
        format!("{public_url}{THEME_CSS_PATH}")
    }
}

/// `path` on the public URL, the origin users open the instance at.
fn public_endpoint(public_url: &WebOrigin, path: &str) -> String {
    format!("{public_url}{path}")
}

fn org_not_found() -> ApiError {
    ApiError::new(
        axum::http::StatusCode::NOT_FOUND,
        request_org::code::ORG_NOT_FOUND,
        "The request's organization cannot be determined.",
    )
}

#[cfg(test)]
mod tests {
    use std::time::SystemTime;

    use serde_json::{Map, Value, json};
    use stepclimb_core::id::Uuid;

    use super::*;

    fn provider(kind: Kind, config: Value) -> IdentityProvider {
        let Value::Object(config) = config else {
            panic!("a config object")
        };
        IdentityProvider {
            id: Uuid::nil(),
            key: "example".into(),
            kind,
            config,
            enabled: true,
            created_at: SystemTime::UNIX_EPOCH,
            updated_at: SystemTime::UNIX_EPOCH,
        }
    }

    #[test]
    fn a_provider_entry_carries_its_key_kind_and_label_only() {
        let config = json!({
            "label": "Sign in with Example",
            "issuer": "https://idp.example.org",
            "client_id": "client-id-value",
            "client_secret": { "sealed": "sealed-secret-value" },
        });
        let entry = provider_method(&provider(Kind::Oidc, config)).expect("an entry");
        let entry = serde_json::to_value(entry).unwrap();
        assert_eq!(
            entry,
            json!({ "key": "example", "kind": "oidc", "label": "Sign in with Example" })
        );
    }

    #[test]
    fn a_label_that_is_not_a_string_is_null() {
        for config in [json!({}), json!({ "label": 7 }), json!({ "label": null })] {
            let entry = provider_method(&provider(Kind::Oauth2, config)).expect("an entry");
            assert_eq!(entry.label, None);
            assert_eq!(entry.kind, SignInKind::Oauth2);
        }
    }

    #[test]
    fn the_local_row_is_never_a_provider_entry() {
        assert!(provider_method(&provider(Kind::Local, Value::Object(Map::new()))).is_none());
    }

    #[test]
    fn the_stylesheet_is_fingerprinted_only_with_a_sha256() {
        let origin = WebOrigin::parse("https://lms.example.org/").unwrap();
        let hash = "a".repeat(64);
        assert_eq!(
            stylesheet_url(&origin, &hash),
            format!("https://lms.example.org/theme.css?v={hash}")
        );
        for bad in ["", "abc", &"A".repeat(64), &format!("{}\"", "a".repeat(63))] {
            assert_eq!(
                stylesheet_url(&origin, bad),
                "https://lms.example.org/theme.css"
            );
        }
    }

    #[test]
    fn the_oauth_endpoints_are_on_the_public_url() {
        for public in ["https://lms.example.org/", "https://lms.example.org"] {
            let origin = WebOrigin::parse(public).unwrap();
            assert_eq!(
                public_endpoint(&origin, AUTHORIZE_PATH),
                "https://lms.example.org/oauth/authorize"
            );
            assert_eq!(
                public_endpoint(&origin, TOKEN_PATH),
                "https://lms.example.org/oauth/token"
            );
            assert_eq!(
                public_endpoint(&origin, REVOKE_PATH),
                "https://lms.example.org/oauth/revoke"
            );
        }
    }

    #[test]
    fn every_local_method_has_its_api_spelling() {
        for method in Method::ALL {
            let spelled = serde_json::to_value(LocalMethod::from(method)).unwrap();
            assert_eq!(spelled, method.as_str());
        }
    }
}
