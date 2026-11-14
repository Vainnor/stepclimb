// SPDX-License-Identifier: AGPL-3.0-or-later
//! The signed-in user's appearance choice, `PUT /api/v1/me/appearance`
//! (docs/specs/m0/11-app-shell.md B21, Q6).
//!
//! The choice is a theme (system, light, dark, high contrast) and a density (comfortable,
//! compact), stored on the user (`stepclimb_core::appearance`) and returned in `GET /api/v1/me`.
//! The server writes it into every page it serves as `data-sc-theme` and `data-sc-density` on
//! `<html>` (`spa`), so the page is drawn in it before any script runs. For the signed-out
//! screens, a change also sets the `__Host-sc_appearance` cookie, which the server reads when
//! the request has no session; it is a display preference on this device and authenticates
//! nothing.

use axum::{
    Json,
    extract::State,
    http::{HeaderMap, HeaderValue, header},
    response::{IntoResponse, Response},
};
use serde::{Deserialize, Serialize};
use stepclimb_core::appearance::{self, Appearance, Density, Theme};
use utoipa::ToSchema;

use crate::{
    AppState,
    cookie::{self, Presented},
    error::ApiError,
    extract::ApiJson,
    require::AuthOnly,
};

/// The path of the route.
pub const MY_APPEARANCE_PATH: &str = "/api/v1/me/appearance";

/// The cookie that carries the appearance to the signed-out screens. The `__Host-` prefix keeps
/// any other host from setting it.
pub const COOKIE_NAME: &str = "__Host-sc_appearance";

/// One year: the choice outlives any session.
const COOKIE_MAX_AGE_SECS: u32 = 31_536_000;

/// The theme a user chose. `system` follows the operating system's color and contrast settings.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "kebab-case")]
pub enum ThemeChoice {
    System,
    Light,
    Dark,
    HighContrast,
}

/// The density a user chose. `compact` sets the shorter control heights.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "lowercase")]
pub enum DensityChoice {
    Comfortable,
    Compact,
}

/// The theme and density the signed-in user's pages are drawn in.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct UserAppearance {
    pub theme: ThemeChoice,
    pub density: DensityChoice,
}

impl From<Appearance> for UserAppearance {
    fn from(appearance: Appearance) -> Self {
        let theme = match appearance.theme {
            Theme::System => ThemeChoice::System,
            Theme::Light => ThemeChoice::Light,
            Theme::Dark => ThemeChoice::Dark,
            Theme::HighContrast => ThemeChoice::HighContrast,
        };
        let density = match appearance.density {
            Density::Comfortable => DensityChoice::Comfortable,
            Density::Compact => DensityChoice::Compact,
        };
        Self { theme, density }
    }
}

impl From<UserAppearance> for Appearance {
    fn from(appearance: UserAppearance) -> Self {
        let theme = match appearance.theme {
            ThemeChoice::System => Theme::System,
            ThemeChoice::Light => Theme::Light,
            ThemeChoice::Dark => Theme::Dark,
            ThemeChoice::HighContrast => Theme::HighContrast,
        };
        let density = match appearance.density {
            DensityChoice::Comfortable => Density::Comfortable,
            DensityChoice::Compact => Density::Compact,
        };
        Self { theme, density }
    }
}

/// Set the signed-in user's theme and density. The answer also sets the `__Host-sc_appearance`
/// cookie, so this browser's signed-out screens use the same choice.
#[utoipa::path(
    put,
    path = "/api/v1/me/appearance",
    operation_id = "updateMyAppearance",
    tag = "me",
    extensions(("x-stepclimb-mutates" = json!(true))),
    request_body = UserAppearance,
    responses(
        (status = 200, description = "The appearance after the change", body = UserAppearance),
        (status = 400, response = crate::openapi::BadRequest),
        (status = 401, response = crate::openapi::Unauthenticated),
        (status = 403, response = crate::openapi::CsrfRefused),
        (status = 404, response = crate::openapi::OrganizationNotFound),
        (status = 413, response = crate::openapi::PayloadTooLarge),
        (status = 415, response = crate::openapi::UnsupportedMediaType),
        (status = 500, response = crate::openapi::InternalError),
        (status = 503, response = crate::openapi::NoOrganization),
    )
)]
pub async fn update_my_appearance(
    auth: AuthOnly,
    State(state): State<AppState>,
    ApiJson(body): ApiJson<UserAppearance>,
) -> Result<Response, ApiError> {
    let principal = auth.principal();
    let saved = appearance::set(
        &state.db,
        principal.scope(),
        principal.user_id,
        body.into(),
        state.clock.now(),
    )
    .await
    .map_err(|error| ApiError::internal(&error))?;
    let cookie = [(header::SET_COOKIE, set_cookie(saved))];
    Ok((cookie, Json(UserAppearance::from(saved))).into_response())
}

/// The cookie's value: `<theme>.<density>`, such as `dark.compact`.
fn cookie_value(appearance: Appearance) -> String {
    format!(
        "{}.{}",
        appearance.theme.as_str(),
        appearance.density.as_str()
    )
}

fn set_cookie(appearance: Appearance) -> HeaderValue {
    let cookie = format!(
        "{COOKIE_NAME}={}; Max-Age={COOKIE_MAX_AGE_SECS}; Path=/; Secure; HttpOnly; SameSite=Lax",
        cookie_value(appearance)
    );
    // Theme and density names are ASCII letters and `-`.
    HeaderValue::from_str(&cookie).expect("an appearance cookie is a valid header value")
}

/// The appearance the request's `__Host-sc_appearance` cookie names. A missing, repeated or
/// unreadable cookie is no choice: the page then follows the operating system, as before any
/// choice was made.
pub fn from_cookie(headers: &HeaderMap) -> Option<Appearance> {
    let Presented::One(value) = cookie::presented(headers, COOKIE_NAME) else {
        return None;
    };
    let (theme, density) = value.split_once('.')?;
    Some(Appearance {
        theme: Theme::parse(theme)?,
        density: Density::parse(density)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn with_cookie(cookie: &str) -> HeaderMap {
        let mut headers = HeaderMap::new();
        headers.insert(header::COOKIE, HeaderValue::from_str(cookie).unwrap());
        headers
    }

    #[test]
    fn the_cookie_round_trips_every_choice() {
        for theme in Theme::ALL {
            for density in Density::ALL {
                let appearance = Appearance { theme, density };
                let set = set_cookie(appearance);
                let pair = set.to_str().unwrap().split(';').next().unwrap().to_owned();
                assert_eq!(from_cookie(&with_cookie(&pair)), Some(appearance));
            }
        }
    }

    #[test]
    fn the_cookie_has_the_host_prefix_attributes() {
        let set = set_cookie(Appearance::default());
        assert_eq!(
            set.to_str().unwrap(),
            "__Host-sc_appearance=system.comfortable; Max-Age=31536000; Path=/; Secure; HttpOnly; SameSite=Lax"
        );
    }

    #[test]
    fn an_unreadable_or_repeated_cookie_is_no_choice() {
        for cookie in [
            "other=dark.compact",
            "__Host-sc_appearance=dark",
            "__Host-sc_appearance=dark.roomy",
            "__Host-sc_appearance=Dark.compact",
            "__Host-sc_appearance=dark.compact.x",
            "__Host-sc_appearance=dark.compact; __Host-sc_appearance=light.compact",
        ] {
            assert_eq!(from_cookie(&with_cookie(cookie)), None, "{cookie}");
        }
        assert_eq!(
            from_cookie(&with_cookie(
                "a=1; __Host-sc_appearance=high-contrast.compact"
            )),
            Some(Appearance {
                theme: Theme::HighContrast,
                density: Density::Compact
            })
        );
    }

    #[test]
    fn the_api_names_are_the_stored_names() {
        for theme in Theme::ALL {
            for density in Density::ALL {
                let appearance = Appearance { theme, density };
                let api = UserAppearance::from(appearance);
                let json = serde_json::to_value(api).unwrap();
                assert_eq!(json["theme"], theme.as_str());
                assert_eq!(json["density"], density.as_str());
                assert_eq!(Appearance::from(api), appearance);
            }
        }
    }
}
