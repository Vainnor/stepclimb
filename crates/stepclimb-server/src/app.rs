// SPDX-License-Identifier: AGPL-3.0-or-later
//! The HTTP application.

use axum::{
    Router,
    http::{HeaderValue, header},
    middleware,
    response::{IntoResponse, Response},
    routing::{MethodRouter, get, post},
};
use sqlx::PgPool;
use stepclimb_auth::encryption::KeyRing;
use stepclimb_core::clock::Clock;
use tower_http::trace::TraceLayer;
use utoipa_scalar::{Scalar, Servable};

use crate::{
    auth,
    csrf::{self, OriginAllowlist},
    error::ApiError,
    health, openapi,
    rate_limit::RateLimiter,
    roles, spa,
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
}

/// The `Strict-Transport-Security` value: one year, for this host only. Subdomains of the
/// public host may be other services of the operator's, which this server does not speak for.
pub const HSTS: &str = "max-age=31536000";

/// The whole application: probes, sign-out, the API document and its UI, and the SPA for the
/// rest. An unknown `/api/` path and a known path with the wrong method answer problem details.
/// Every unsafe request passes the CSRF check first, and with an `https` public URL every
/// response carries HSTS.
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
        ));
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
