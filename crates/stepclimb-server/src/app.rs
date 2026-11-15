// SPDX-License-Identifier: AGPL-3.0-or-later
//! The HTTP application.

use axum::{
    Router,
    extract::Request,
    http::{HeaderValue, header},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::{MethodRouter, delete, get, patch, post, put},
};
use sqlx::PgPool;
use stepclimb_auth::encryption::KeyRing;
use stepclimb_core::clock::Clock;
use tower_http::trace::TraceLayer;
use utoipa_scalar::{Scalar, Servable};

use crate::{
    accounts, appearance, auth, branding,
    csrf::{self, OriginAllowlist},
    enrollment,
    error::ApiError,
    health, identities, local_sign_in, me, my_passkeys, openapi, organization, passkey_enrollment,
    passkey_sign_in, provider_sign_in,
    providers::Providers,
    rate_limit::RateLimiter,
    role_assignments, roles, spa, users,
    web_origin::WebOrigin,
};

/// What every handler can reach.
#[derive(Clone)]
pub struct AppState {
    pub db: PgPool,
    /// The time every expiry and lifetime rule reads; tests pass a manual clock.
    pub clock: Clock,
    /// The AES-256-GCM keys for secrets the server reads back, from configuration.
    pub keys: KeyRing,
    /// The origin users open the instance at (`public_url`).
    pub public_url: WebOrigin,
    /// Further origins the CSRF check accepts (`allowed_origins`).
    pub allowed_origins: Vec<WebOrigin>,
    /// The sign-in and token rate limits and the proxies trusted to name a client's address.
    pub rate_limits: RateLimiter,
    /// The outbound clients for identity providers and their discovery cache
    /// (`allow_insecure_providers`).
    pub providers: Providers,
}

/// The `Strict-Transport-Security` value: one year, for this host only. Subdomains of the
/// public host may be other services of the operator's, which this server does not speak for.
pub const HSTS: &str = "max-age=31536000";

/// The `Cache-Control` value of every `/api/v1` response: API answers carry personal data, which
/// neither a shared cache nor the browser's may keep.
pub const API_CACHE_CONTROL: &str = "no-store";

/// The whole application: probes, sign-out, the signed-in user, the API document and its UI,
/// and the SPA for the rest. An unknown `/api/` path and a known path with the wrong method
/// answer problem details. Every unsafe request passes the CSRF check first, every `/api/v1`
/// response carries `Cache-Control: no-store`, and with an `https` public URL every response
/// carries HSTS.
pub fn router(state: AppState) -> Router {
    let allowlist = OriginAllowlist::new(&state.public_url, &state.allowed_origins);
    let https = state.public_url.is_https();
    let mut app = Router::new();
    for (path, handler) in routes() {
        app = app.route(path, handler);
    }
    let app = app
        .route(OPENAPI_JSON, get(openapi_json))
        .merge(Scalar::with_url(DOCS, openapi::document()))
        // Applies to the routes above, so it stays after the last of them.
        .method_not_allowed_fallback(method_not_allowed)
        .fallback(spa::serve)
        // Around every route and both fallbacks, so no handler runs before it.
        .layer(middleware::from_fn_with_state(
            allowlist,
            csrf::refuse_cross_site,
        ))
        // Outside the CSRF check, so its refusals carry the header too.
        .layer(middleware::from_fn(no_store_for_api));
    with_hsts(app, https)
        .layer(TraceLayer::new_for_http())
        .with_state(state)
}

/// The path of the API document, which is not an operation in itself.
pub const OPENAPI_JSON: &str = "/api/openapi.json";

/// The path of the API document's UI.
pub const DOCS: &str = "/docs";

/// Every route of the API, by path. [`router`] mounts these and only these besides
/// [`OPENAPI_JSON`] and [`DOCS`]; a test checks each path is an operation of the OpenAPI
/// document, and another that each takes exactly the methods the document lists, so a route
/// cannot escape the walk of spec 08 T17.
pub fn routes() -> Vec<(&'static str, MethodRouter<AppState>)> {
    vec![
        ("/healthz", get(health::healthz)),
        ("/readyz", get(health::readyz)),
        ("/auth/sign-out", post(auth::sign_out)),
        (
            enrollment::ENROLLMENT_START_PATH,
            post(enrollment::start_enrollment),
        ),
        (
            passkey_enrollment::ENROLLMENT_PASSKEY_PATH,
            post(passkey_enrollment::enroll_passkey),
        ),
        (
            passkey_sign_in::PASSKEY_START_PATH,
            post(passkey_sign_in::start_passkey_sign_in),
        ),
        (
            passkey_sign_in::PASSKEY_FINISH_PATH,
            post(passkey_sign_in::finish_passkey_sign_in),
        ),
        (
            provider_sign_in::START_PATH,
            get(provider_sign_in::start_provider_sign_in),
        ),
        (branding::THEME_CSS_PATH, get(branding::theme_css)),
        (me::ME_PATH, get(me::get_me)),
        (
            appearance::MY_APPEARANCE_PATH,
            put(appearance::update_my_appearance),
        ),
        (
            identities::MY_IDENTITIES_PATH,
            get(identities::list_my_identities),
        ),
        (
            "/api/v1/me/identities/{identity_id}",
            delete(identities::delete_my_identity),
        ),
        (
            my_passkeys::MY_PASSKEYS_PATH,
            get(my_passkeys::list_my_passkeys).post(my_passkeys::create_my_passkey),
        ),
        (
            my_passkeys::MY_PASSKEYS_START_PATH,
            post(my_passkeys::start_my_passkey),
        ),
        (
            my_passkeys::MY_PASSKEY_PATH,
            patch(my_passkeys::update_my_passkey).delete(my_passkeys::delete_my_passkey),
        ),
        (
            "/api/v1/users/{user_id}/identities",
            get(identities::list_user_identities).post(identities::create_user_identity),
        ),
        (
            "/api/v1/users/{user_id}/identities/{identity_id}",
            delete(identities::delete_user_identity),
        ),
        (
            organization::ORGANIZATION_PATH,
            get(organization::get_organization).patch(organization::update_organization),
        ),
        (
            organization::SETTINGS_PATH,
            get(organization::get_settings).patch(organization::update_settings),
        ),
        (
            organization::ENTITLEMENTS_PATH,
            get(organization::get_entitlements),
        ),
        (
            accounts::ACCOUNTS_PATH,
            get(accounts::list_accounts).post(accounts::create_account),
        ),
        (
            "/api/v1/accounts/{account_id}",
            get(accounts::get_account)
                .patch(accounts::update_account)
                .delete(accounts::delete_account),
        ),
        (
            "/api/v1/accounts/{account_id}/move",
            post(accounts::move_account),
        ),
        ("/api/v1/admin/permissions", get(roles::list_permissions)),
        (
            roles::ROLES_PATH,
            get(roles::list_roles).post(roles::create_role),
        ),
        (
            "/api/v1/admin/roles/{role_id}",
            get(roles::get_role)
                .patch(roles::update_role)
                .delete(roles::delete_role),
        ),
        (
            users::USERS_PATH,
            get(users::list_users).post(users::create_user),
        ),
        (
            "/api/v1/users/{user_id}",
            get(users::get_user).patch(users::update_user),
        ),
        (
            "/api/v1/admin/users/{user_id}/enrollment-links",
            post(enrollment::create_enrollment_link),
        ),
        (
            role_assignments::ROLE_ASSIGNMENTS_PATH,
            get(role_assignments::list_role_assignments)
                .post(role_assignments::create_role_assignment),
        ),
        (
            "/api/v1/admin/role-assignments/{assignment_id}",
            delete(role_assignments::delete_role_assignment),
        ),
        (
            branding::BRANDING_PATH,
            get(branding::get_branding).put(branding::update_branding),
        ),
        (
            local_sign_in::LOCAL_SIGN_IN_PATH,
            get(local_sign_in::get_local_sign_in).put(local_sign_in::update_local_sign_in),
        ),
    ]
}

/// HSTS on every response, refusals included, when the public URL is `https`. The server never
/// terminates TLS itself and reads no proxy's `X-Forwarded-Proto` (of the forwarding headers it
/// reads only `X-Forwarded-For`, for the rate limits), so the configured public URL is what says
/// the instance is served over HTTPS.
fn with_hsts(app: Router<AppState>, https: bool) -> Router<AppState> {
    if https {
        app.layer(middleware::map_response(add_hsts))
    } else {
        app
    }
}

/// `Cache-Control: no-store` on the response to any request under `/api/v1`: answers, errors,
/// refusals and unknown paths alike. The path is read before the request runs, since only the
/// request knows it.
async fn no_store_for_api(request: Request, next: Next) -> Response {
    let path = request.uri().path();
    let api = path == "/api/v1" || path.starts_with("/api/v1/");
    let mut response = next.run(request).await;
    if api {
        response.headers_mut().insert(
            header::CACHE_CONTROL,
            HeaderValue::from_static(API_CACHE_CONTROL),
        );
    }
    response
}

async fn add_hsts(mut response: Response) -> Response {
    response.headers_mut().insert(
        header::STRICT_TRANSPORT_SECURITY,
        HeaderValue::from_static(HSTS),
    );
    response
}

/// A matched path with another method: 405 as problem details. axum still sets `Allow`.
async fn method_not_allowed() -> ApiError {
    ApiError::method_not_allowed()
}

async fn openapi_json() -> impl IntoResponse {
    (
        [(header::CONTENT_TYPE, "application/json")],
        openapi::document_json(),
    )
}
