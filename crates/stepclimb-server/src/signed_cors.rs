// SPDX-License-Identifier: AGPL-3.0-or-later
//! Cross-origin reads of signed URLs (docs/specs/m1/08-uploaded-video.md B18a): `/media/` and
//! `/files/`.
//!
//! The web app plays media from its own origin and needs none of this. A player on another origin
//! (the Tauri app's bundle) fetches playlists and segments with `fetch`, which it can read only
//! with CORS. So a request to either prefix whose `Origin` is in the server's allowlist (the
//! public URL's origin and `allowed_origins`, as the CSRF check reads them) gets
//! `Access-Control-Allow-Origin` echoing it and `Access-Control-Expose-Headers` naming
//! `Content-Range`, `Content-Length` and `Accept-Ranges`; an `OPTIONS` preflight for `GET` or
//! `HEAD`, asking at most for the `Range` header, is answered 204 here with the allowed method
//! and header. Every answer of the two prefixes carries `Vary: Origin`. No answer allows
//! credentials: the token in the path is the credential. Any other origin, or a preflight for
//! another method or header, gets no CORS header, so the browser keeps the answer from the page.

use axum::{
    extract::{Request, State},
    http::{HeaderMap, HeaderValue, Method, StatusCode, header},
    middleware::Next,
    response::{IntoResponse, Response},
};

use crate::csrf::OriginAllowlist;

/// The path prefixes whose answers this layer decorates.
const PREFIXES: [&str; 2] = ["/media/", "/files/"];

/// How long a browser may keep a preflight's answer: 10 minutes.
const MAX_AGE: &str = "600";

/// The layer's function: answer a preflight to the signed prefixes, decorate their answers, and
/// pass every other request through untouched.
pub async fn signed_cors(
    State(allowlist): State<OriginAllowlist>,
    request: Request,
    next: Next,
) -> Response {
    let path = request.uri().path();
    if !PREFIXES.iter().any(|prefix| path.starts_with(prefix)) {
        return next.run(request).await;
    }
    let origin = allowed_origin(request.headers(), &allowlist);
    if request.method() == Method::OPTIONS {
        return preflight(request.headers(), origin);
    }
    let mut response = next.run(request).await;
    let headers = response.headers_mut();
    headers.append(header::VARY, HeaderValue::from_static("origin"));
    if let Some(origin) = origin {
        headers.insert(header::ACCESS_CONTROL_ALLOW_ORIGIN, origin);
        headers.insert(
            header::ACCESS_CONTROL_EXPOSE_HEADERS,
            HeaderValue::from_static("content-range, content-length, accept-ranges"),
        );
    }
    response
}

/// The request's `Origin` when the allowlist holds it.
fn allowed_origin(headers: &HeaderMap, allowlist: &OriginAllowlist) -> Option<HeaderValue> {
    let origin = headers.get(header::ORIGIN)?;
    allowlist
        .contains(origin.as_bytes())
        .then(|| origin.clone())
}

/// 204 for a preflight. It allows the request only for an allowed `origin`, a `GET` or `HEAD`,
/// and no header but `Range`.
fn preflight(headers: &HeaderMap, origin: Option<HeaderValue>) -> Response {
    let mut answer = HeaderMap::new();
    answer.insert(header::VARY, HeaderValue::from_static("origin"));
    let method_ok = headers
        .get(header::ACCESS_CONTROL_REQUEST_METHOD)
        .is_some_and(|m| m == "GET" || m == "HEAD");
    let headers_ok = headers
        .get_all(header::ACCESS_CONTROL_REQUEST_HEADERS)
        .iter()
        .filter_map(|value| value.to_str().ok())
        .flat_map(|value| value.split(','))
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .all(|name| name.eq_ignore_ascii_case("range"));
    if let (Some(origin), true, true) = (origin, method_ok, headers_ok) {
        answer.insert(header::ACCESS_CONTROL_ALLOW_ORIGIN, origin);
        answer.insert(
            header::ACCESS_CONTROL_ALLOW_METHODS,
            HeaderValue::from_static("GET, HEAD"),
        );
        answer.insert(
            header::ACCESS_CONTROL_ALLOW_HEADERS,
            HeaderValue::from_static("range"),
        );
        answer.insert(
            header::ACCESS_CONTROL_MAX_AGE,
            HeaderValue::from_static(MAX_AGE),
        );
    }
    (StatusCode::NO_CONTENT, answer).into_response()
}
