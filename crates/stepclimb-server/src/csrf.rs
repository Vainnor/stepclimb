// SPDX-License-Identifier: AGPL-3.0-or-later
//! The CSRF check on every unsafe request (docs/specs/m0/08-sessions-and-csrf.md B12 to B16,
//! Q6).
//!
//! [`refuse_cross_site`] runs as a layer around the whole router, so it runs before any route,
//! fallback or extractor. A request with a method other than GET, HEAD or OPTIONS passes when:
//!
//! - it carries an `Authorization: Bearer` header, a credential a browser never attaches on its
//!   own (B16); the session extractor then ignores the session cookie;
//! - or its one `Sec-Fetch-Site` header is `same-origin` (B14);
//! - or it has no `Sec-Fetch-Site` and its one `Origin` header is the public origin or one of
//!   the allowed origins (B14, Q6).
//!
//! Anything else answers 403 `auth.csrf_refused`, including a request with neither header (Q6:
//! every browser that runs the web app sends at least one of them on a POST). Other
//! `Authorization` schemes do not skip the check: a browser resends `Basic` credentials on its
//! own, for example to a site behind a proxy that asks for them.

use std::sync::Arc;

use axum::{
    extract::{Request, State},
    http::{HeaderMap, Method, StatusCode, header},
    middleware::Next,
    response::{IntoResponse, Response},
};

use crate::{error::ApiError, web_origin::WebOrigin};

/// The codes this module produces.
pub mod code {
    /// An unsafe request that is not same-origin and carries no bearer credential (403).
    pub const AUTH_CSRF_REFUSED: &str = "auth.csrf_refused";
}

/// The origins an unsafe cookie request may come from: the public origin, then the allowed ones.
#[derive(Clone, Debug)]
pub struct OriginAllowlist(Arc<[WebOrigin]>);

impl OriginAllowlist {
    pub fn new(public: &WebOrigin, allowed: &[WebOrigin]) -> Self {
        Self(std::iter::once(public).chain(allowed).cloned().collect())
    }

    fn contains(&self, origin: &[u8]) -> bool {
        self.0
            .iter()
            .any(|allowed| allowed.as_str().as_bytes() == origin)
    }
}

/// The layer's function: let a safe, bearer or same-origin request through, refuse the rest.
pub async fn refuse_cross_site(
    State(allowlist): State<OriginAllowlist>,
    request: Request,
    next: Next,
) -> Response {
    if is_safe(request.method())
        || carries_bearer(request.headers())
        || is_same_origin(request.headers(), &allowlist)
    {
        next.run(request).await
    } else {
        refused().into_response()
    }
}

fn refused() -> ApiError {
    ApiError::new(
        StatusCode::FORBIDDEN,
        code::AUTH_CSRF_REFUSED,
        "The request did not come from this site, so it was refused.",
    )
}

/// GET, HEAD and OPTIONS change no state but `last_seen_at` and sign-in flows (B12).
fn is_safe(method: &Method) -> bool {
    matches!(*method, Method::GET | Method::HEAD | Method::OPTIONS)
}

/// Whether an `Authorization` header uses the `Bearer` scheme (B16). The scheme is matched
/// without regard to case (RFC 9110 section 11.1).
pub fn carries_bearer(headers: &HeaderMap) -> bool {
    headers.get_all(header::AUTHORIZATION).iter().any(|value| {
        let bytes = value.as_bytes();
        let scheme = bytes.split(|&byte| byte == b' ').next().unwrap_or(bytes);
        scheme.eq_ignore_ascii_case(b"bearer")
    })
}

/// Fetch Metadata first; `Origin` only when the browser sent no `Sec-Fetch-Site` (B14, Q6). A
/// header sent more than once is not trusted to mean either value.
fn is_same_origin(headers: &HeaderMap, allowlist: &OriginAllowlist) -> bool {
    let mut fetch_site = headers.get_all("sec-fetch-site").iter();
    match (fetch_site.next(), fetch_site.next()) {
        (Some(value), None) => return value.as_bytes() == b"same-origin",
        (Some(_), Some(_)) => return false,
        (None, _) => {}
    }
    let mut origin = headers.get_all(header::ORIGIN).iter();
    match (origin.next(), origin.next()) {
        (Some(value), None) => allowlist.contains(value.as_bytes()),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use axum::http::HeaderValue;

    use super::*;

    fn allowlist() -> OriginAllowlist {
        OriginAllowlist::new(
            &WebOrigin::parse("https://lms.example.org").unwrap(),
            &[WebOrigin::parse("https://app.example.org").unwrap()],
        )
    }

    fn headers(pairs: &[(&str, &str)]) -> HeaderMap {
        let mut headers = HeaderMap::new();
        for (name, value) in pairs {
            headers.append(
                axum::http::HeaderName::from_bytes(name.as_bytes()).unwrap(),
                HeaderValue::from_str(value).unwrap(),
            );
        }
        headers
    }

    #[test]
    fn only_same_origin_fetch_metadata_passes() {
        let allowlist = allowlist();
        assert!(is_same_origin(
            &headers(&[("sec-fetch-site", "same-origin")]),
            &allowlist
        ));
        for value in ["same-site", "cross-site", "none", "SAME-ORIGIN", ""] {
            let request = headers(&[("sec-fetch-site", value)]);
            assert!(!is_same_origin(&request, &allowlist), "{value:?}");
        }
    }

    #[test]
    fn fetch_metadata_wins_over_an_allowed_origin() {
        let request = headers(&[
            ("sec-fetch-site", "cross-site"),
            ("origin", "https://lms.example.org"),
        ]);
        assert!(!is_same_origin(&request, &allowlist()));
    }

    #[test]
    fn without_fetch_metadata_the_origin_must_be_allowed() {
        let allowlist = allowlist();
        for origin in ["https://lms.example.org", "https://app.example.org"] {
            assert!(
                is_same_origin(&headers(&[("origin", origin)]), &allowlist),
                "{origin}"
            );
        }
        for origin in [
            "https://evil.example",
            "http://lms.example.org",
            "https://lms.example.org:8443",
            "https://lms.example.org/",
            "https://LMS.example.org",
            "null",
        ] {
            assert!(
                !is_same_origin(&headers(&[("origin", origin)]), &allowlist),
                "{origin}"
            );
        }
    }

    #[test]
    fn neither_header_is_refused() {
        assert!(!is_same_origin(&HeaderMap::new(), &allowlist()));
    }

    #[test]
    fn a_repeated_header_is_refused() {
        let allowlist = allowlist();
        let fetch = headers(&[
            ("sec-fetch-site", "same-origin"),
            ("sec-fetch-site", "same-origin"),
        ]);
        assert!(!is_same_origin(&fetch, &allowlist));
        let origin = headers(&[
            ("origin", "https://lms.example.org"),
            ("origin", "https://lms.example.org"),
        ]);
        assert!(!is_same_origin(&origin, &allowlist));
    }

    #[test]
    fn only_the_bearer_scheme_counts_as_a_bearer_credential() {
        for value in ["Bearer sc_x", "bearer abc", "BEARER abc", "Bearer"] {
            assert!(
                carries_bearer(&headers(&[("authorization", value)])),
                "{value}"
            );
        }
        for value in ["Basic dTpw", "Bearerx abc", "Token abc", ""] {
            assert!(
                !carries_bearer(&headers(&[("authorization", value)])),
                "{value}"
            );
        }
        assert!(!carries_bearer(&HeaderMap::new()));
    }

    #[test]
    fn only_get_head_and_options_are_safe() {
        for method in [Method::GET, Method::HEAD, Method::OPTIONS] {
            assert!(is_safe(&method), "{method}");
        }
        for method in [
            Method::POST,
            Method::PUT,
            Method::PATCH,
            Method::DELETE,
            Method::TRACE,
            Method::CONNECT,
            Method::from_bytes(b"PROPFIND").unwrap(),
        ] {
            assert!(!is_safe(&method), "{method}");
        }
    }
}
