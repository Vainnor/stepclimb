// SPDX-License-Identifier: AGPL-3.0-or-later
//! The built web SPA (`web/dist`), embedded into the binary.
//!
//! Release builds embed the files at compile time; debug builds read them from `web/dist` when
//! a request comes in, so `just web-build` updates a running debug server. A path that names a
//! file is served from the build; any other path outside `/api/` gets `index.html`, so the
//! client router handles it. When the web app has not been built, those requests get a 503
//! naming the command that builds it.

use axum::{
    body::Body,
    http::{HeaderValue, StatusCode, Uri, header},
    response::{IntoResponse, Response},
};
use rust_embed::RustEmbed;

#[derive(RustEmbed)]
#[folder = "../../web/dist/"]
#[allow_missing = true]
struct WebDist;

/// Vite fingerprints everything under `assets/`, so those files never change at a given name.
const IMMUTABLE: &str = "public, max-age=31536000, immutable";
const REVALIDATE: &str = "no-cache";

/// The fallback handler: a file from the web build, or `index.html` for a client route.
pub async fn serve(uri: Uri) -> Response {
    let path = uri.path().trim_start_matches('/');
    if path == "api" || path.starts_with("api/") {
        return StatusCode::NOT_FOUND.into_response();
    }
    if !path.is_empty() {
        if let Some(response) = file(path) {
            return response;
        }
        if names_a_file(path) {
            return StatusCode::NOT_FOUND.into_response();
        }
    }
    file("index.html").unwrap_or_else(|| {
        (
            StatusCode::SERVICE_UNAVAILABLE,
            "The web app is not built into this server. Run `just web-build`, then restart it.",
        )
            .into_response()
    })
}

fn file(path: &str) -> Option<Response> {
    let asset = WebDist::get(path)?;
    let mime = asset.metadata.mimetype();
    let cache = if path.starts_with("assets/") {
        IMMUTABLE
    } else {
        REVALIDATE
    };
    let mut response = Response::new(Body::from(asset.data.into_owned()));
    let headers = response.headers_mut();
    if let Ok(value) = HeaderValue::from_str(mime) {
        headers.insert(header::CONTENT_TYPE, value);
    }
    headers.insert(header::CACHE_CONTROL, HeaderValue::from_static(cache));
    headers.insert(
        header::X_CONTENT_TYPE_OPTIONS,
        HeaderValue::from_static("nosniff"),
    );
    Some(response)
}

/// A last path segment with an extension names a file, which never falls back to the SPA.
fn names_a_file(path: &str) -> bool {
    path.rsplit('/')
        .next()
        .is_some_and(|segment| segment.contains('.'))
}

#[cfg(test)]
mod tests {
    use super::names_a_file;

    #[test]
    fn a_segment_with_a_dot_names_a_file() {
        assert!(names_a_file("assets/index-abc123.js"));
        assert!(names_a_file("favicon.ico"));
        assert!(!names_a_file("courses/42"));
        assert!(!names_a_file("c/s1/m/2"));
    }
}
