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
    accounts, admission_settings, admission_webhook, appearance, auth, branding, courses,
    csrf::{self, OriginAllowlist},
    discovery, enrollment,
    error::ApiError,
    health, identities, items, local_sign_in, manifest, me, modules, my_passkeys, oauth_authorize,
    oauth_token, openapi, organization, passkey_enrollment, passkey_sign_in, password,
    password_hasher::PasswordHasher,
    provider_admin, provider_callback, provider_sign_in,
    providers::Providers,
    rate_limit::RateLimiter,
    role_assignments, roles, service_accounts, spa, totp, totp_enrollment, users,
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
    /// The argon2id hasher every password check and new password goes through, at most four
    /// at once.
    pub passwords: PasswordHasher,
    /// Where course files are stored; `None` when `storage_url` is not set, and a handler
    /// taking [`crate::storage::Storage`] then answers 503 `storage.not_configured`.
    pub storage: Option<crate::storage::Storage>,
}

/// The `Strict-Transport-Security` value: one year, for this host only. Subdomains of the
/// public host may be other services of the operator's, which this server does not speak for.
pub const HSTS: &str = "max-age=31536000";

/// The `Cache-Control` value of every `/api/v1` response: API answers carry personal data, which
/// neither a shared cache nor the browser's may keep.
pub const API_CACHE_CONTROL: &str = "no-store";

/// A router of `routes`, each at its path: the one place routes of [`routes`] are mounted.
fn mount(
    routes: impl IntoIterator<Item = (&'static str, MethodRouter<AppState>)>,
) -> Router<AppState> {
    let mut app = Router::new();
    for (path, handler) in routes {
        app = app.route(path, handler);
    }
    app
}

/// The whole application: probes, sign-out, the signed-in user, the API document and its UI,
/// and the SPA for the rest. An unknown `/api/` path and a known path with the wrong method
/// answer problem details. Every unsafe request passes the CSRF check first, every `/api/v1`
/// response carries `Cache-Control: no-store`, and with an `https` public URL every response
/// carries HSTS.
pub fn router(state: AppState) -> Router {
    let allowlist = OriginAllowlist::new(&state.public_url, &state.allowed_origins);
    let https = state.public_url.is_https();
    let app = mount(routes())
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
        .layer(TraceLayer::new_for_http().make_span_with(request_span))
        .with_state(state)
}

/// The routes of a `worker` process (docs/specs/m1/01-job-queue-and-outbox.md B13): `/healthz`
/// and `/readyz`, so a container health check works unchanged, and 404 for every other path. As
/// in [`router`], a wrong method answers the problem-details 405 and an `https` public URL adds
/// HSTS.
pub fn probes_router(state: AppState) -> Router {
    let probes = routes()
        .into_iter()
        .filter(|(path, _)| PROBE_PATHS.contains(path));
    let https = state.public_url.is_https();
    let app = mount(probes).method_not_allowed_fallback(method_not_allowed);
    with_hsts(app, https)
        .layer(TraceLayer::new_for_http().make_span_with(request_span))
        .with_state(state)
}

/// The paths of [`routes`] a `worker` process serves.
const PROBE_PATHS: [&str; 2] = ["/healthz", "/readyz"];

/// The span every request is traced in: its method, path and HTTP version. The query is left
/// out, since a provider's callback carries its authorization `code` and `state` there and no
/// credential may reach a log line (spec 08 B4).
fn request_span(request: &axum::http::Request<axum::body::Body>) -> tracing::Span {
    tracing::debug_span!(
        "request",
        method = %request.method(),
        path = %request.uri().path(),
        version = ?request.version(),
    )
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
        (discovery::DISCOVERY_PATH, get(discovery::get_discovery)),
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
            password::ENROLLMENT_PASSWORD_PATH,
            post(password::enroll_password),
        ),
        (
            password::PASSWORD_SIGN_IN_PATH,
            post(password::sign_in_with_password),
        ),
        (totp::TOTP_SIGN_IN_PATH, post(totp::sign_in_with_totp)),
        (
            totp_enrollment::TOTP_ENROLLMENT_PATH,
            post(totp_enrollment::enroll_totp_at_sign_in),
        ),
        (
            provider_sign_in::START_PATH,
            get(provider_sign_in::start_provider_sign_in),
        ),
        (
            provider_callback::CALLBACK_PATH,
            get(provider_callback::finish_provider_sign_in),
        ),
        (
            oauth_authorize::AUTHORIZE_PATH,
            get(oauth_authorize::authorize_app).post(oauth_authorize::answer_app_authorization),
        ),
        (
            oauth_token::TOKEN_PATH,
            post(oauth_token::request_app_token),
        ),
        (
            oauth_token::REVOKE_PATH,
            post(oauth_token::revoke_app_token),
        ),
        (branding::THEME_CSS_PATH, get(branding::theme_css)),
        (manifest::MANIFEST_PATH, get(manifest::get_web_manifest)),
        (me::ME_PATH, get(me::get_me)),
        (
            appearance::MY_APPEARANCE_PATH,
            put(appearance::update_my_appearance),
        ),
        (
            password::MY_PASSWORD_PATH,
            put(password::change_my_password),
        ),
        (totp::MY_TOTP_PATH, post(totp::start_my_totp)),
        (totp::MY_TOTP_CONFIRM_PATH, post(totp::confirm_my_totp)),
        (totp::MY_TOTP_DISABLE_PATH, post(totp::disable_my_totp)),
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
        (
            courses::COURSES_PATH,
            get(courses::list_courses).post(courses::create_course),
        ),
        (
            "/api/v1/courses/{course_id}",
            get(courses::get_course).patch(courses::update_course),
        ),
        (
            "/api/v1/courses/{course_id}/status",
            post(courses::set_course_status),
        ),
        (
            "/api/v1/courses/{course_id}/move",
            post(courses::move_course),
        ),
        (
            "/api/v1/courses/{course_id}/modules",
            post(modules::create_module),
        ),
        (
            "/api/v1/courses/{course_id}/modules/order",
            put(modules::reorder_modules),
        ),
        (
            "/api/v1/modules/{module_id}",
            patch(modules::update_module).delete(modules::delete_module),
        ),
        (
            "/api/v1/modules/{module_id}/items",
            post(items::create_item),
        ),
        (
            "/api/v1/modules/{module_id}/items/order",
            put(items::reorder_items),
        ),
        (
            "/api/v1/items/{item_id}",
            patch(items::update_item).delete(items::delete_item),
        ),
        ("/api/v1/items/{item_id}/move", post(items::move_item)),
        (
            "/api/v1/courses/{course_id}/copy",
            post(courses::copy_course),
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
        (totp::USER_TOTP_PATH, delete(totp::remove_user_totp)),
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
            service_accounts::SERVICE_ACCOUNTS_PATH,
            get(service_accounts::list_service_accounts)
                .post(service_accounts::create_service_account),
        ),
        (
            "/api/v1/admin/service-accounts/{service_account_id}",
            delete(service_accounts::delete_service_account),
        ),
        (
            branding::BRANDING_PATH,
            get(branding::get_branding).put(branding::update_branding),
        ),
        (
            local_sign_in::LOCAL_SIGN_IN_PATH,
            get(local_sign_in::get_local_sign_in).put(local_sign_in::update_local_sign_in),
        ),
        (
            admission_settings::ADMISSION_PATH,
            get(admission_settings::get_admission_settings)
                .put(admission_settings::update_admission_settings),
        ),
        (
            admission_webhook::WEBHOOK_PATH,
            get(admission_webhook::get_admission_webhook)
                .put(admission_webhook::update_admission_webhook)
                .delete(admission_webhook::delete_admission_webhook),
        ),
        (
            admission_webhook::ROTATE_SECRET_PATH,
            post(admission_webhook::rotate_admission_webhook_secret),
        ),
        (
            provider_admin::PROVIDERS_PATH,
            get(provider_admin::list_providers).post(provider_admin::create_provider),
        ),
        (
            "/api/v1/admin/providers/{provider_id}",
            get(provider_admin::get_provider)
                .patch(provider_admin::update_provider)
                .delete(provider_admin::delete_provider),
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
