// SPDX-License-Identifier: AGPL-3.0-or-later
//! The built web SPA (`web/dist`), embedded into the binary.
//!
//! Release builds embed the files at compile time; debug builds read them from `web/dist` when
//! a request comes in, so `just web-build` updates a running debug server. A path that names a
//! file is served from the build; any other path outside `/api/` gets `index.html`, so the
//! client router handles it. When the web app has not been built, those requests get a 503
//! naming the command that builds it. A path under `/api/` that no route matched answers 404
//! `request.not_found` as problem details.
//!
//! `index.html` is written for each request (docs/specs/m0/11-app-shell.md B20, B21, Q4, Q6):
//! its `/theme.css` link carries the request organization's stylesheet hash as `?v=`, which
//! `/theme.css` caches for a year, and `<html>` carries the appearance as `data-sc-theme` and
//! `data-sc-density`: the signed-in user's choice, or for a request without a session the
//! `__Host-sc_appearance` cookie's. Without a theme chosen, the page takes the organization's
//! default appearance (docs/specs/m0/12-tokens-and-theme-generator.md B27). The page is therefore
//! drawn in its theme before any script runs, without an inline script. It is `private, no-cache`
//! and varies by cookie. Its `theme-color` meta carries the organization's light theme `brand`
//! color, as the web app manifest does (docs/specs/m1/15-installable-web-app-and-offline-reading.md
//! B1, B2).

use axum::{
    body::Body,
    extract::{FromRequestParts, Request, State},
    http::{HeaderValue, StatusCode, header, request::Parts},
    response::{IntoResponse, Response},
};
use rust_embed::RustEmbed;
use stepclimb_core::appearance::{self, Appearance, Density, Theme};

use crate::{
    AppState,
    appearance::from_cookie,
    branding::{self, THEME_CSS_PATH, THEME_CSS_VERSION_PARAM},
    csrf,
    error::ApiError,
    manifest::light_colors,
    request_org::RequestOrg,
    session::Authenticated,
};

#[derive(RustEmbed)]
#[folder = "../../web/dist/"]
#[allow_missing = true]
struct WebDist;

/// Vite fingerprints everything under `assets/`, so those files never change at a given name.
const IMMUTABLE: &str = "public, max-age=31536000, immutable";
const REVALIDATE: &str = "no-cache";
/// `index.html` is written for its request: no shared cache may keep it.
const PAGE_CACHE: &str = "private, no-cache";

const INDEX: &str = "index.html";

/// The fallback handler: a file from the web build, or `index.html` for a client route.
pub async fn serve(State(state): State<AppState>, request: Request) -> Response {
    let (mut parts, _) = request.into_parts();
    let path = parts.uri.path().trim_start_matches('/').to_owned();
    if path == "api" || path.starts_with("api/") {
        return ApiError::route_not_found().into_response();
    }
    if !path.is_empty() && path != INDEX {
        if let Some(response) = file(&path) {
            return response;
        }
        if names_a_file(&path) {
            return StatusCode::NOT_FOUND.into_response();
        }
    }
    let Some(index) = WebDist::get(INDEX) else {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            "The web app is not built into this server. Run `just web-build`, then restart it.",
        )
            .into_response();
    };
    let template = String::from_utf8_lossy(&index.data);
    let (theme_hash, default_theme, theme_color) = theme(&state, &mut parts).await;
    let page = Page {
        theme_hash,
        theme_color,
        appearance: page_appearance(&state, &mut parts).await,
        default_theme,
    };
    page_response(render_index(&template, &page))
}

/// What `index.html` is written with for one request.
#[derive(Debug, Default)]
struct Page {
    /// The hash of the stylesheet `/theme.css` serves the request's organization; `None` leaves
    /// the link unfingerprinted.
    theme_hash: Option<String>,
    appearance: Appearance,
    /// The organization's default appearance, for an `appearance` without a theme.
    default_theme: Theme,
    /// The organization's light theme `brand` color as `#rrggbb`; `None` leaves the template's.
    theme_color: Option<String>,
}

/// The request organization's stylesheet hash, default appearance and `theme-color`. Without
/// them the link stays `/theme.css`, which answers the same stylesheet revalidated on every load,
/// or the same refusal, a page without a chosen theme follows the operating system, and the
/// `theme-color` stays the default brand's from the template.
async fn theme(state: &AppState, parts: &mut Parts) -> (Option<String>, Theme, Option<String>) {
    let org = RequestOrg::from_request_parts(parts, state).await;
    match branding::served(state, org).await {
        Ok(served) => {
            let color = match light_colors(&served.brand) {
                Ok(colors) => Some(colors.brand),
                Err(error) => {
                    tracing::error!(%error, "index.html: no theme-color");
                    None
                }
            };
            (Some(served.css_hash), served.default_appearance, color)
        }
        Err(error) => {
            if error.status().is_server_error() {
                tracing::error!(code = error.code(), "index.html: no theme hash");
            }
            (None, Theme::System, None)
        }
    }
}

/// The signed-in user's appearance; without a session, the cookie's; without either, none
/// chosen. A server error while finding the session or the user's choice is logged and the page
/// drawn from the cookie, or as if none were chosen: the app's `/me` request then shows the
/// failure itself. A page request carrying a bearer credential is drawn from the cookie without
/// authenticating it, so a page load never looks up an API key or records its use.
async fn page_appearance(state: &AppState, parts: &mut Parts) -> Appearance {
    if csrf::carries_bearer(&parts.headers) {
        return from_cookie(&parts.headers).unwrap_or_default();
    }
    let auth = match Authenticated::from_request_parts(parts, state).await {
        Ok(auth) => auth,
        Err(rejection) => {
            let error = rejection.error();
            if error.status().is_server_error() {
                tracing::error!(code = error.code(), "index.html: the request's session");
            }
            return from_cookie(&parts.headers).unwrap_or_default();
        }
    };
    match appearance::get(&state.db, auth.scope(), auth.user_id).await {
        Ok(appearance) => appearance,
        Err(error) => {
            tracing::error!(%error, "index.html: the user's appearance");
            Appearance::default()
        }
    }
}

/// A SHA-256 as the branding record stores it: 64 lowercase hex digits.
pub(crate) fn is_css_hash(hash: &str) -> bool {
    hash.len() == 64 && hash.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}

/// The start of the `theme-color` meta's value in `index.html`, which a `#rrggbb` follows.
const THEME_COLOR_META: &str = r#"<meta name="theme-color" content=""#;

/// `#rrggbb` in lowercase hex.
fn is_hex_color(color: &str) -> bool {
    color.len() == 7
        && color.starts_with('#')
        && color[1..]
            .bytes()
            .all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}

/// `html` with the `theme-color` meta's `#rrggbb` replaced by `color`; unchanged when `color` is
/// not a lowercase `#rrggbb` or the template's value is not one.
fn with_theme_color(html: String, color: &str) -> String {
    let Some(start) = html
        .find(THEME_COLOR_META)
        .map(|i| i + THEME_COLOR_META.len())
    else {
        return html;
    };
    let current = html.get(start..start + 7).unwrap_or_default();
    if !is_hex_color(color) || !is_hex_color(current) {
        return html;
    }
    let mut html = html;
    html.replace_range(start..start + 7, color);
    html
}

/// `index.html` with the fingerprinted theme link, the organization's `theme-color` and the
/// appearance attributes on `<html>`. A stored hash that is not a SHA-256 leaves the link
/// unfingerprinted, and a color that is not `#rrggbb` leaves the template's, so nothing but hex
/// digits reaches either attribute; the appearance values are fixed names. None needs escaping.
fn render_index(template: &str, page: &Page) -> String {
    let mut html = template.to_owned();
    if let Some(color) = page.theme_color.as_deref() {
        html = with_theme_color(html, color);
    }
    if let Some(hash) = page.theme_hash.as_deref().filter(|hash| is_css_hash(hash)) {
        let link = format!("href=\"{THEME_CSS_PATH}\"");
        let fingerprinted = format!("href=\"{THEME_CSS_PATH}?{THEME_CSS_VERSION_PARAM}={hash}\"");
        html = html.replacen(&link, &fingerprinted, 1);
    }
    let mut attributes = String::new();
    let theme = page.appearance.theme_or(page.default_theme);
    if theme != Theme::System {
        attributes.push_str(&format!(" data-sc-theme=\"{}\"", theme.as_str()));
    }
    if page.appearance.density != Density::Comfortable {
        attributes.push_str(&format!(
            " data-sc-density=\"{}\"",
            page.appearance.density.as_str()
        ));
    }
    html.replacen("<html", &format!("<html{attributes}"), 1)
}

fn page_response(html: String) -> Response {
    let mut response = Response::new(Body::from(html));
    let headers = response.headers_mut();
    headers.insert(header::CONTENT_TYPE, HeaderValue::from_static("text/html"));
    headers.insert(header::CACHE_CONTROL, HeaderValue::from_static(PAGE_CACHE));
    headers.insert(header::VARY, HeaderValue::from_static("Cookie"));
    headers.insert(
        header::X_CONTENT_TYPE_OPTIONS,
        HeaderValue::from_static("nosniff"),
    );
    response
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
    use super::*;

    const TEMPLATE: &str = "<!doctype html>\n<html lang=\"en\">\n  <head>\n    <link rel=\"stylesheet\" href=\"/theme.css\" />\n    <script type=\"module\" src=\"/assets/index.js\"></script>\n  </head>\n</html>\n";

    const HASH: &str = "dfea0dc5cea5d1d6f5b7ac255b46b3bf2b4357aa9804d1c882bba7537b286910";

    #[test]
    fn the_page_links_the_fingerprinted_theme() {
        let page = Page {
            theme_hash: Some(HASH.into()),
            ..Page::default()
        };
        let html = render_index(TEMPLATE, &page);
        assert!(html.contains(r#"<html lang="en">"#), "{html}");
        assert!(
            html.contains(&format!(r#"href="/theme.css?v={HASH}""#)),
            "{html}"
        );
        assert!(!html.contains(r#"href="/theme.css""#), "{html}");
    }

    #[test]
    fn a_hash_that_is_not_a_sha256_leaves_the_link_plain() {
        let upper = HASH.to_uppercase();
        let quoted = format!("{}\"", &HASH[..63]);
        for hash in [
            "ab12",
            &HASH[..63],
            &format!("{HASH}0"),
            upper.as_str(),
            quoted.as_str(),
            "",
        ] {
            let page = Page {
                theme_hash: Some(hash.into()),
                ..Page::default()
            };
            assert_eq!(render_index(TEMPLATE, &page), TEMPLATE, "{hash:?}");
        }
    }

    #[test]
    fn without_a_hash_the_link_is_unchanged() {
        let html = render_index(TEMPLATE, &Page::default());
        assert_eq!(html, TEMPLATE);
    }

    #[test]
    fn the_page_carries_the_chosen_theme_and_density() {
        for (theme, density, expected) in [
            (
                Some(Theme::Dark),
                Density::Comfortable,
                r#"<html data-sc-theme="dark" lang="en">"#,
            ),
            (
                Some(Theme::HighContrast),
                Density::Compact,
                r#"<html data-sc-theme="high-contrast" data-sc-density="compact" lang="en">"#,
            ),
            (
                Some(Theme::System),
                Density::Compact,
                r#"<html data-sc-density="compact" lang="en">"#,
            ),
            (None, Density::Comfortable, r#"<html lang="en">"#),
            (
                Some(Theme::Light),
                Density::Comfortable,
                r#"<html data-sc-theme="light" lang="en">"#,
            ),
        ] {
            let page = Page {
                appearance: Appearance { theme, density },
                ..Page::default()
            };
            assert!(
                render_index(TEMPLATE, &page).contains(expected),
                "{theme:?} {density:?}"
            );
        }
    }

    #[test]
    fn without_a_chosen_theme_the_page_takes_the_organizations_default() {
        for (theme, default, expected) in [
            (
                None,
                Theme::Dark,
                r#"<html data-sc-theme="dark" lang="en">"#,
            ),
            (
                None,
                Theme::HighContrast,
                r#"<html data-sc-theme="high-contrast" lang="en">"#,
            ),
            (None, Theme::System, r#"<html lang="en">"#),
            // The user's own choice wins, "Match my device" included.
            (Some(Theme::System), Theme::Dark, r#"<html lang="en">"#),
            (
                Some(Theme::Light),
                Theme::Dark,
                r#"<html data-sc-theme="light" lang="en">"#,
            ),
        ] {
            let page = Page {
                appearance: Appearance {
                    theme,
                    density: Density::Comfortable,
                },
                default_theme: default,
                ..Page::default()
            };
            assert!(
                render_index(TEMPLATE, &page).contains(expected),
                "{theme:?} {default:?}"
            );
        }
    }

    const META: &str = "<head>\n    <meta name=\"theme-color\" content=\"#007a78\" />\n</head>";

    #[test]
    fn the_page_carries_the_organizations_theme_color() {
        let page = Page {
            theme_color: Some("#500e0e".into()),
            ..Page::default()
        };
        let html = render_index(META, &page);
        assert!(
            html.contains(r##"<meta name="theme-color" content="#500e0e" />"##),
            "{html}"
        );
        assert_eq!(render_index(META, &Page::default()), META);
    }

    #[test]
    fn a_theme_color_that_is_not_hex_leaves_the_templates() {
        for color in [
            "#500E0E",
            "#500e0",
            "#500e0e0",
            "red",
            "\"><script>",
            "#50\"e0e",
            "",
        ] {
            let page = Page {
                theme_color: Some(color.into()),
                ..Page::default()
            };
            assert_eq!(render_index(META, &page), META, "{color:?}");
        }
    }

    /// `web/index.html`'s `theme-color` is the default brand's, which a page keeps when the
    /// organization's cannot be read, and from which the web build draws its default icons.
    #[test]
    fn the_templates_theme_color_is_the_default_brands() {
        let template = include_str!("../../../web/index.html");
        let start = template
            .find(THEME_COLOR_META)
            .expect("web/index.html has a theme-color meta")
            + THEME_COLOR_META.len();
        let default = light_colors(&stepclimb_core::theme::Brand::stepclimb_default())
            .expect("the default brand renders");
        assert_eq!(&template[start..start + 7], default.brand);
    }

    #[test]
    fn a_segment_with_a_dot_names_a_file() {
        assert!(names_a_file("assets/index-abc123.js"));
        assert!(names_a_file("favicon.ico"));
        assert!(!names_a_file("courses/42"));
        assert!(!names_a_file("c/s1/m/2"));
    }
}
