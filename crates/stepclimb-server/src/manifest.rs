// SPDX-License-Identifier: AGPL-3.0-or-later
//! `GET /manifest.webmanifest`, the web app manifest of the request's organization
//! (docs/specs/m1/15-installable-web-app-and-offline-reading.md B1).
//!
//! The manifest is public, as a browser reads it before anyone signs in. It names the app after
//! the organization (`short_name` cut to 12 characters), opens it at `/` in its own window, takes
//! `theme_color` and `background_color` from the organization's light theme (`brand` and
//! `bg-canvas`, as `/theme.css` draws them) and lists Stepclimb's default icons, which the web
//! build writes under `/icons/`. The answer is `Cache-Control: no-cache`: a rename or a branding
//! change shows on the next read.

use axum::{
    Json,
    extract::State,
    http::{HeaderValue, StatusCode, header},
    response::{IntoResponse, Response},
};
use serde::Serialize;
use stepclimb_core::{
    organizations,
    theme::{self, Brand, RenderError, color},
};
use utoipa::ToSchema;

use crate::{
    AppState, branding,
    error::ApiError,
    request_org::{self, RequestOrg},
};

/// The path of the manifest.
pub const MANIFEST_PATH: &str = "/manifest.webmanifest";

/// The manifest's media type (W3C Web Application Manifest, IANA registration).
pub const MANIFEST_CONTENT_TYPE: &str = "application/manifest+json";

/// The manifest's `Cache-Control`: revalidated on every use.
pub const MANIFEST_CACHE_CONTROL: &str = "no-cache";

/// The longest `short_name`, in characters (Unicode scalar values).
pub const SHORT_NAME_MAX: usize = 12;

/// The default icons the web build writes (`web/vite.config.ts`, `appIcons`).
const ICONS: [(&str, &str, Option<&str>); 3] = [
    ("/icons/icon-192.png", "192x192", None),
    ("/icons/icon-512.png", "512x512", None),
    ("/icons/icon-maskable-512.png", "512x512", Some("maskable")),
];

/// The organization's web app manifest.
#[derive(Debug, Serialize, ToSchema)]
pub struct WebManifest {
    /// The organization's name.
    pub name: String,
    /// The organization's name cut to 12 characters, trailing spaces removed.
    pub short_name: String,
    /// Always `/`.
    pub start_url: String,
    /// Always `/`.
    pub scope: String,
    /// Always `standalone`.
    pub display: String,
    /// The organization's light theme `brand` color, as `#rrggbb`.
    pub theme_color: String,
    /// The organization's light theme `bg-canvas` color, as `#rrggbb`.
    pub background_color: String,
    /// Stepclimb's default icons: 192 and 512 px, and a maskable 512 px.
    pub icons: Vec<ManifestIcon>,
}

/// One icon of the manifest.
#[derive(Debug, Serialize, ToSchema)]
pub struct ManifestIcon {
    /// The icon's path.
    pub src: String,
    /// `<width>x<height>` in px.
    pub sizes: String,
    /// Always `image/png`.
    #[serde(rename = "type")]
    pub mime: String,
    /// `maskable` for the icon drawn to be cropped to the platform's shape; absent otherwise.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schema(required = false, nullable = false)]
    pub purpose: Option<String>,
}

/// The two colors of a brand's light theme the manifest and the page's `theme-color` use, as
/// `#rrggbb`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LightColors {
    /// `brand`.
    pub brand: String,
    /// `bg-canvas`.
    pub canvas: String,
}

/// `brand`'s light theme `brand` and `bg-canvas` colors as the 8-bit sRGB a display shows.
pub fn light_colors(brand: &Brand) -> Result<LightColors, RenderError> {
    let themes = theme::generate(brand)?;
    let hex = |token: &'static str| {
        themes
            .light
            .color(token)
            .map(|c| to_hex(color::to_srgb8(c)))
            .ok_or(RenderError::NoStep(token))
    };
    Ok(LightColors {
        brand: hex("brand")?,
        canvas: hex("bg-canvas")?,
    })
}

/// `#rrggbb` for a color whose channels are 0 to 1 on 256 levels.
fn to_hex(rgb: [f64; 3]) -> String {
    let [r, g, b] = rgb.map(|x| (x * 255.0).round().clamp(0.0, 255.0) as u8);
    format!("#{r:02x}{g:02x}{b:02x}")
}

/// `name` cut to [`SHORT_NAME_MAX`] characters, trailing whitespace removed.
pub fn short_name(name: &str) -> String {
    let cut: String = name.chars().take(SHORT_NAME_MAX).collect();
    cut.trim_end().to_owned()
}

impl WebManifest {
    fn new(name: String, colors: LightColors) -> Self {
        let icons = ICONS
            .iter()
            .map(|(src, sizes, purpose)| ManifestIcon {
                src: (*src).to_owned(),
                sizes: (*sizes).to_owned(),
                mime: "image/png".to_owned(),
                purpose: purpose.map(str::to_owned),
            })
            .collect();
        Self {
            short_name: short_name(&name),
            name,
            start_url: "/".to_owned(),
            scope: "/".to_owned(),
            display: "standalone".to_owned(),
            theme_color: colors.brand,
            background_color: colors.canvas,
            icons,
        }
    }
}

/// The web app manifest of the request's organization: its name, its light theme's colors and
/// Stepclimb's default icons.
#[utoipa::path(
    get,
    path = "/manifest.webmanifest",
    operation_id = "getWebManifest",
    tag = "operations",
    responses(
        (
            status = 200,
            description = "The organization's web app manifest. `Cache-Control: no-cache`.",
            body = WebManifest,
            content_type = "application/manifest+json",
            headers(("Cache-Control" = String, description = "`no-cache`"))
        ),
        (status = 404, response = crate::openapi::OrganizationNotFound),
        (status = 500, response = crate::openapi::InternalError),
        (status = 503, response = crate::openapi::NoOrganization),
    )
)]
pub async fn get_web_manifest(
    org: RequestOrg,
    State(state): State<AppState>,
) -> Result<Response, ApiError> {
    let scope = org.scope();
    let name = match organizations::get(&state.db, scope).await {
        Ok(organization) => organization.name,
        Err(organizations::Error::NotFound) => return Err(org_not_found()),
        Err(error) => return Err(ApiError::internal(&error)),
    };
    let served = branding::served(&state, Ok(org)).await?;
    let colors = light_colors(&served.brand).map_err(|error| ApiError::internal(&error))?;
    let headers = [
        (
            header::CONTENT_TYPE,
            HeaderValue::from_static(MANIFEST_CONTENT_TYPE),
        ),
        (
            header::CACHE_CONTROL,
            HeaderValue::from_static(MANIFEST_CACHE_CONTROL),
        ),
    ];
    Ok((headers, Json(WebManifest::new(name, colors))).into_response())
}

fn org_not_found() -> ApiError {
    ApiError::new(
        StatusCode::NOT_FOUND,
        request_org::code::ORG_NOT_FOUND,
        "The request's organization cannot be determined.",
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_short_name_is_cut_to_twelve_characters() {
        for (name, expected) in [
            ("Example", "Example"),
            ("Example Training", "Example Trai"),
            ("Twelve chars", "Twelve chars"),
            // Cut at the twelfth character, then the trailing space goes.
            ("Example Org Two", "Example Org"),
            ("Écoles réunies du Nord", "Écoles réuni"),
            ("", ""),
        ] {
            assert_eq!(short_name(name), expected, "{name:?}");
        }
        assert_eq!(short_name("Écoles réunies du Nord").chars().count(), 12);
    }

    #[test]
    fn the_default_brand_gives_its_light_colors() {
        let colors = light_colors(&Brand::stepclimb_default()).unwrap();
        for value in [&colors.brand, &colors.canvas] {
            assert_eq!(value.len(), 7, "{value}");
            assert!(value.starts_with('#'), "{value}");
            assert!(
                value[1..]
                    .bytes()
                    .all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f')),
                "{value}"
            );
        }
        assert_ne!(colors.brand, colors.canvas);
    }

    #[test]
    fn hex_rounds_each_channel() {
        assert_eq!(to_hex([0.0, 0.5, 1.0]), "#0080ff");
        assert_eq!(to_hex([1.0 / 255.0, 254.0 / 255.0, 0.0]), "#01fe00");
    }

    #[test]
    fn the_manifest_lists_the_default_icons() {
        let colors = LightColors {
            brand: "#007a78".into(),
            canvas: "#fbfbfb".into(),
        };
        let manifest = WebManifest::new("Example Training".into(), colors);
        let json = serde_json::to_value(&manifest).unwrap();
        assert_eq!(
            json["icons"],
            serde_json::json!([
                { "src": "/icons/icon-192.png", "sizes": "192x192", "type": "image/png" },
                { "src": "/icons/icon-512.png", "sizes": "512x512", "type": "image/png" },
                {
                    "src": "/icons/icon-maskable-512.png",
                    "sizes": "512x512",
                    "type": "image/png",
                    "purpose": "maskable"
                },
            ])
        );
        assert_eq!(json["start_url"], "/");
        assert_eq!(json["scope"], "/");
        assert_eq!(json["display"], "standalone");
        assert_eq!(json["short_name"], "Example Trai");
        assert_eq!(json["theme_color"], "#007a78");
        assert_eq!(json["background_color"], "#fbfbfb");
    }
}
