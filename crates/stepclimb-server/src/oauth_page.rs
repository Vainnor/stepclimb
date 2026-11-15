// SPDX-License-Identifier: AGPL-3.0-or-later
//! The pages the OAuth 2.1 authorization endpoint draws itself (docs/specs/m0/08-sessions-and-csrf.md
//! B31, Q10): the confirmation page and the error page.
//!
//! Both are small HTML documents the server writes, since the endpoint is a browser navigation
//! the app opens in the system browser and the error page must show without the web app. Every
//! value from the organization or the user is HTML-escaped. Each page answers `Cache-Control:
//! no-store`, `X-Content-Type-Options: nosniff`, `Referrer-Policy: no-referrer`, and a
//! `Content-Security-Policy` that loads nothing and refuses framing (with `X-Frame-Options:
//! DENY` for older browsers), so another site cannot overlay the confirmation buttons. The
//! policy leaves `form-action` out on purpose: browsers apply it to the redirect after the form
//! is sent, and that redirect goes to the app's own scheme or loopback address.

use axum::{
    http::{HeaderValue, StatusCode, header},
    response::{IntoResponse, Response},
};

use crate::oauth_authorize::AUTHORIZE_PATH;

/// The page's `Content-Security-Policy`.
pub const PAGE_CSP: &str = "default-src 'none'; frame-ancestors 'none'; base-uri 'none'";

/// The headers every page and every redirect of the endpoint carries.
pub(crate) fn protect(response: &mut Response) {
    let headers = response.headers_mut();
    headers.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    headers.insert(
        header::REFERRER_POLICY,
        HeaderValue::from_static("no-referrer"),
    );
    headers.insert(
        header::X_CONTENT_TYPE_OPTIONS,
        HeaderValue::from_static("nosniff"),
    );
    headers.insert(header::X_FRAME_OPTIONS, HeaderValue::from_static("DENY"));
    headers.insert(
        header::CONTENT_SECURITY_POLICY,
        HeaderValue::from_static(PAGE_CSP),
    );
}

fn page(status: StatusCode, title: &str, body: &str) -> Response {
    let html = format!(
        "<!doctype html>\n<html lang=\"en\">\n<head>\n<meta charset=\"utf-8\">\n\
         <meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\n\
         <title>{title}</title>\n</head>\n<body>\n<main>\n{body}</main>\n</body>\n</html>\n",
        title = escape(title),
    );
    let mut response = (
        status,
        [(
            header::CONTENT_TYPE,
            HeaderValue::from_static("text/html; charset=utf-8"),
        )],
        html,
    )
        .into_response();
    protect(&mut response);
    response
}

/// The confirmation page: `app` asks to sign in as `user` at `organization`. Its form posts
/// the answer back to the endpoint; the request itself waits in the sign-in flow whose cookie
/// the caller adds.
pub(crate) fn confirmation(app: &str, user: &str, organization: &str) -> Response {
    let (app, user, organization) = (escape(app), escape(user), escape(organization));
    let body = format!(
        "<h1>Sign in to {app}?</h1>\n\
         <p>{app} on this device asks to use <strong>{organization}</strong> as \
         <strong>{user}</strong>.</p>\n\
         <p>Allow it only if you started signing in from the app just now.</p>\n\
         <form method=\"post\" action=\"{AUTHORIZE_PATH}\">\n\
         <button type=\"submit\" name=\"decision\" value=\"allow\">Allow</button>\n\
         <button type=\"submit\" name=\"decision\" value=\"deny\">Cancel</button>\n\
         </form>\n"
    );
    page(StatusCode::OK, "Sign in to the app", &body)
}

/// The error page for `code`, with its status and message. Nothing is redirected.
pub(crate) fn refused(code: &'static str) -> Response {
    let (status, message) = refusal(code);
    let body = format!(
        "<h1>The app could not sign in</h1>\n<p>{}</p>\n<p>Error code: <code>{}</code></p>\n",
        escape(message),
        escape(code),
    );
    page(status, "The app could not sign in", &body)
}

/// The status and the message of each code [`refused`] shows.
fn refusal(code: &'static str) -> (StatusCode, &'static str) {
    use crate::{flow::code as flow, session::code as session};
    use stepclimb_auth::oauth_server::authorize::code as oauth;
    match code {
        oauth::OAUTH_CLIENT_UNKNOWN => (
            StatusCode::BAD_REQUEST,
            "The app that sent you here is not one this server knows.",
        ),
        oauth::OAUTH_REDIRECT_URI_MISMATCH => (
            StatusCode::BAD_REQUEST,
            "The app asked to return to an address it has not registered.",
        ),
        flow::AUTH_FLOW_INVALID | flow::AUTH_FLOW_EXPIRED => (
            StatusCode::BAD_REQUEST,
            "This request was already answered, has expired, or belongs to another account. \
             Return to the app and sign in again.",
        ),
        session::AUTH_UNAUTHENTICATED => (
            StatusCode::UNAUTHORIZED,
            "You are no longer signed in. Return to the app and sign in again.",
        ),
        _ => (
            StatusCode::BAD_REQUEST,
            "The request could not be read. Return to the app and sign in again.",
        ),
    }
}

/// `text` with `&`, `<`, `>`, `"` and `'` written as character references.
pub(crate) fn escape(text: &str) -> String {
    let mut escaped = String::with_capacity(text.len());
    for character in text.chars() {
        match character {
            '&' => escaped.push_str("&amp;"),
            '<' => escaped.push_str("&lt;"),
            '>' => escaped.push_str("&gt;"),
            '"' => escaped.push_str("&quot;"),
            '\'' => escaped.push_str("&#39;"),
            other => escaped.push(other),
        }
    }
    escaped
}

#[cfg(test)]
mod tests {
    use http_body_util::BodyExt;

    use super::*;

    async fn text(response: Response) -> String {
        let bytes = response.into_body().collect().await.unwrap().to_bytes();
        String::from_utf8(bytes.to_vec()).unwrap()
    }

    #[test]
    fn markup_is_escaped() {
        assert_eq!(
            escape(r#"<a href="x">&'</a>"#),
            "&lt;a href=&quot;x&quot;&gt;&amp;&#39;&lt;/a&gt;"
        );
    }

    #[tokio::test]
    async fn the_confirmation_page_names_the_app_the_user_and_the_organization_escaped() {
        let response = confirmation("the app", "Sam <sam@example.test>", "Org & <b>Co</b>");
        assert_eq!(response.status(), StatusCode::OK);
        let html = text(response).await;
        assert!(
            html.contains("as <strong>Sam &lt;sam@example.test&gt;</strong>"),
            "{html}"
        );
        assert!(
            html.contains("<strong>Org &amp; &lt;b&gt;Co&lt;/b&gt;</strong>"),
            "{html}"
        );
        assert!(html.contains("<form method=\"post\" action=\"/oauth/authorize\">"));
        assert!(html.contains("value=\"allow\"") && html.contains("value=\"deny\""));
    }

    #[test]
    fn every_page_refuses_framing_and_caching() {
        for response in [confirmation("a", "b", "c"), refused("oauth.client_unknown")] {
            let headers = response.headers();
            assert_eq!(headers[header::CACHE_CONTROL], "no-store");
            assert_eq!(headers[header::X_FRAME_OPTIONS], "DENY");
            assert_eq!(headers[header::REFERRER_POLICY], "no-referrer");
            assert_eq!(headers[header::CONTENT_SECURITY_POLICY], PAGE_CSP);
            assert!(PAGE_CSP.contains("frame-ancestors 'none'"));
            assert!(!PAGE_CSP.contains("form-action"));
            assert_eq!(headers[header::CONTENT_TYPE], "text/html; charset=utf-8");
        }
    }

    #[test]
    fn each_refusal_has_its_status() {
        assert_eq!(
            refused("oauth.client_unknown").status(),
            StatusCode::BAD_REQUEST
        );
        assert_eq!(
            refused("auth.unauthenticated").status(),
            StatusCode::UNAUTHORIZED
        );
        assert_eq!(
            refused("auth.flow_expired").status(),
            StatusCode::BAD_REQUEST
        );
    }
}
