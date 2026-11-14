// SPDX-License-Identifier: AGPL-3.0-or-later
//! The organization's branding under `/api/v1/admin/branding` and its stylesheet at `/theme.css`
//! (docs/specs/m0/12-tokens-and-theme-generator.md B21, B24, B25 and API).
//!
//! Reading and changing branding need `branding.manage`, which is grantable only at
//! organization scope; changing it also needs the plan's `custom_branding` feature (spec 10 Q1).
//! A change is checked, generated with `stepclimb_core::theme`, refused with 422
//! `branding.contrast_failed` when any theme has a failing contrast pair, and stored with its
//! stylesheet. `/theme.css` is public, as the sign-in screen needs it: it serves the request
//! organization's stored stylesheet, or the default brand's when it has saved none.

use std::sync::LazyLock;

use axum::{
    Json,
    extract::State,
    http::{HeaderValue, StatusCode, header},
    response::{IntoResponse, Response},
};
use serde::{Deserialize, Serialize};
use stepclimb_core::{
    OrgScope,
    branding::{self, Branding as Saved},
    feature::CustomBranding,
    perm::BrandingManage,
    theme::{self, Brand, BrandInputs as CoreInputs, ContrastReport as CoreReport, InputRule},
};
use utoipa::{
    ToResponse, ToSchema,
    openapi::{ContentBuilder, RefOr, Response as DocResponse, ResponseBuilder, schema::Ref},
};

use crate::{
    AppState,
    audit::ClientAddress,
    error::{ApiError, FieldError, PROBLEM_JSON, Problem},
    extract::ApiJson,
    openapi::problem_responses,
    request_org::{self, RequestOrg},
    require::Require,
};

/// The path of the organization's branding.
pub const BRANDING_PATH: &str = "/api/v1/admin/branding";

/// The path of the request organization's stylesheet.
pub const THEME_CSS_PATH: &str = "/theme.css";

/// `/theme.css`'s `Cache-Control`: a browser revalidates on every load, so a saved change shows
/// on the next one.
pub const THEME_CSS_CACHE_CONTROL: &str = "no-cache";

/// The codes this module produces.
pub mod code {
    /// A branding input fails validation; `errors` names it (422).
    pub const INVALID_INPUT: &str = "branding.invalid_input";
    /// A generated theme has a failing contrast pair; `report` lists every appearance's
    /// pairs, failing pairs first (422).
    pub const CONTRAST_FAILED: &str = "branding.contrast_failed";
    /// In a 422's `errors`: `primary` is not a hex color or `oklch()` the generator reads.
    pub const INVALID_COLOR: &str = "branding.invalid_color";
    /// In a 422's `errors`: `primary` is longer than 64 characters once trimmed.
    pub const TOO_LONG: &str = "branding.too_long";
    /// In a 422's `errors`: the value is not one of the input's choices.
    pub const NOT_A_CHOICE: &str = "branding.not_a_choice";
}

problem_responses! {
    BrandingWriteForbidden => "`permission.denied`: the caller does not hold `branding.manage` at organization scope. `entitlement.feature_unavailable`: the organization's plan does not include `custom_branding`. `auth.csrf_refused`: the request did not come from this site.",
}

/// A UI font the instance bundles.
#[derive(ToSchema)]
pub enum UiFont {
    #[schema(rename = "Atkinson Hyperlegible Next")]
    AtkinsonHyperlegibleNext,
    Roboto,
}

/// The reading font: the bundled serif, or the organization's UI font.
#[derive(ToSchema)]
#[schema(rename_all = "lowercase")]
pub enum ReadingFont {
    Serif,
    Sans,
}

/// The corner style: `square` sets the `xs`, `sm` and `md` radii to 0, `round` doubles them.
#[derive(ToSchema)]
#[schema(rename_all = "lowercase")]
pub enum CornerStyle {
    Square,
    Default,
    Round,
}

/// The branding inputs. The whole set is replaced; an omitted or `null` choice takes its
/// default. A value that is not a choice is a 422, not a 400.
#[derive(Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct BrandingInputs {
    /// The primary color: a CSS hex color (`#500E0E`, `#f00`) or `oklch(L C H)`, opaque.
    pub primary: String,
    /// Defaults to `Atkinson Hyperlegible Next`.
    #[serde(default)]
    #[schema(value_type = Option<UiFont>)]
    pub ui_font: Option<String>,
    /// Defaults to `serif`.
    #[serde(default)]
    #[schema(value_type = Option<ReadingFont>)]
    pub reading_font: Option<String>,
    /// Defaults to `default`.
    #[serde(default)]
    #[schema(value_type = Option<CornerStyle>)]
    pub corner_style: Option<String>,
}

/// The saved inputs, every choice made.
//
// The document's form of `stepclimb_core::theme::Brand`, which the routes serialize;
// `tests/branding.rs` checks the answers against this schema.
#[derive(ToSchema)]
#[expect(
    dead_code,
    reason = "a schema only: the routes serialize the core type"
)]
pub struct SavedInputs {
    primary: String,
    ui_font: UiFont,
    reading_font: ReadingFont,
    corner_style: CornerStyle,
}

/// An appearance every brand has.
#[derive(ToSchema)]
#[schema(rename_all = "kebab-case")]
pub enum Appearance {
    Light,
    Dark,
    HighContrast,
}

/// One measured pair: `foreground` on `background`.
#[derive(ToSchema)]
#[expect(
    dead_code,
    reason = "a schema only: the routes serialize the core type"
)]
pub struct ContrastPair {
    /// A color token, such as `fg-soft`.
    foreground: String,
    /// A color token, such as `bg-canvas`.
    background: String,
    /// The WCAG 2 contrast ratio on the displayed 8-bit sRGB colors, 1 to 21, unrounded.
    ratio: f64,
    /// 4.5 for text and 3 for other pairs; 7 and 4.5 in high contrast.
    minimum: f64,
    pass: bool,
}

/// Something to look at that does not fail the theme. `brand-near-danger`: the brand's hue is
/// within 30 degrees of `danger`'s.
#[derive(ToSchema)]
#[expect(
    dead_code,
    reason = "a schema only: the routes serialize the core type"
)]
pub struct ContrastWarning {
    code: String,
    message: String,
}

/// One appearance's contrast report.
#[derive(ToSchema)]
#[expect(
    dead_code,
    reason = "a schema only: the routes serialize the core type"
)]
pub struct ContrastReport {
    appearance: Appearance,
    /// True when every pair passes.
    pass: bool,
    pairs: Vec<ContrastPair>,
    warnings: Vec<ContrastWarning>,
}

/// The organization's branding: the saved inputs (the default brand's when it has saved none),
/// the generator version, the hash of the stylesheet `/theme.css` serves, and the contrast
/// report of its light, dark and high-contrast themes.
#[derive(Serialize, ToSchema)]
pub struct Branding {
    #[schema(value_type = SavedInputs)]
    pub inputs: Brand,
    /// The version of the theme generator that wrote the stylesheet.
    #[schema(value_type = i32)]
    pub generator: u32,
    /// The SHA-256 of the stylesheet, lowercase hex.
    pub css_hash: String,
    #[schema(value_type = Vec<ContrastReport>)]
    pub report: Vec<CoreReport>,
}

impl Branding {
    fn new(saved: &Saved) -> Self {
        Self {
            inputs: saved.brand.clone(),
            generator: saved.generator,
            css_hash: saved.css_hash.clone(),
            report: saved.report.clone(),
        }
    }
}

/// A problem details body with the contrast report: `branding.contrast_failed` carries
/// `report`, every other code leaves it out.
#[derive(Serialize, ToSchema)]
pub struct BrandingProblem {
    #[serde(flatten)]
    pub problem: Problem,
    /// On `branding.contrast_failed`, every appearance's report, failing pairs first.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schema(value_type = Option<Vec<ContrastReport>>)]
    pub report: Option<Vec<CoreReport>>,
}

/// The 422 of `PUT /api/v1/admin/branding`.
pub struct BrandingInvalid;

impl<'r> ToResponse<'r> for BrandingInvalid {
    fn response() -> (&'r str, RefOr<DocResponse>) {
        let response = ResponseBuilder::new()
            .description(
                "`branding.invalid_input`: an input fails validation; `errors` names it \
                 (`branding.invalid_color`, `branding.too_long`, `branding.not_a_choice`). `branding.contrast_failed`: \
                 a generated theme has a failing contrast pair; `report` lists every appearance's \
                 pairs, failing pairs first.",
            )
            .content(
                PROBLEM_JSON,
                ContentBuilder::new()
                    .schema(Some(Ref::from_schema_name("BrandingProblem")))
                    .build(),
            )
            .build();
        ("BrandingInvalid", response.into())
    }
}

/// An error of the branding routes: any API error, or the contrast refusal with its report.
#[derive(Debug)]
pub enum BrandingError {
    Api(ApiError),
    ContrastFailed(Vec<CoreReport>),
}

impl From<ApiError> for BrandingError {
    fn from(error: ApiError) -> Self {
        Self::Api(error)
    }
}

impl IntoResponse for BrandingError {
    fn into_response(self) -> Response {
        let reports = match self {
            Self::Api(error) => return error.into_response(),
            Self::ContrastFailed(reports) => reports,
        };
        let reports: Vec<CoreReport> = reports.into_iter().map(CoreReport::failing_first).collect();
        let error = ApiError::new(
            StatusCode::UNPROCESSABLE_ENTITY,
            code::CONTRAST_FAILED,
            "A generated theme fails its contrast report; `report` lists the failing pairs first.",
        );
        let body = BrandingProblem {
            problem: error.problem(),
            report: Some(reports),
        };
        // Strings, numbers and booleans always serialize.
        let body = serde_json::to_vec(&body).expect("a branding problem serializes");
        let content_type = [(header::CONTENT_TYPE, HeaderValue::from_static(PROBLEM_JSON))];
        (error.status(), content_type, body).into_response()
    }
}

/// The default brand's stylesheet and reports (B25).
static DEFAULT: LazyLock<Saved> = LazyLock::new(|| {
    // Invariant: the default brand passes every contrast pair; the build guard (spec 12 B22) and
    // this module's test check it.
    let rendered = theme::render(&Brand::stepclimb_default())
        .expect("the default brand renders and passes its contrast report");
    Saved::from_rendered(&rendered)
});

/// The saved branding of `scope`, as stored with its reports, or the default's. Nothing is
/// generated: what this answers always describes the stylesheet `/theme.css` serves.
async fn current(state: &AppState, scope: OrgScope) -> Result<Saved, ApiError> {
    let saved = branding::get(&state.db, scope)
        .await
        .map_err(branding_error)?;
    Ok(saved.unwrap_or_else(|| DEFAULT.clone()))
}

/// Read the organization's branding.
#[utoipa::path(
    get,
    path = "/api/v1/admin/branding",
    operation_id = "getBranding",
    tag = "branding",
    responses(
        (status = 200, description = "The organization's branding", body = Branding),
        (status = 401, response = crate::openapi::Unauthenticated),
        (status = 403, response = crate::openapi::PermissionDenied),
        (status = 404, response = crate::openapi::OrganizationNotFound),
        (status = 500, response = crate::openapi::InternalError),
        (status = 503, response = crate::openapi::NoOrganization),
    )
)]
pub async fn get_branding(
    require: Require<BrandingManage>,
    State(state): State<AppState>,
) -> Result<Json<Branding>, ApiError> {
    let saved = current(&state, require.principal().scope()).await?;
    Ok(Json(Branding::new(&saved)))
}

/// Replace the organization's branding. The server generates the light, dark and
/// high-contrast themes from the inputs and saves them only when every contrast pair passes;
/// `/theme.css` serves the new stylesheet from then on.
#[utoipa::path(
    put,
    path = "/api/v1/admin/branding",
    operation_id = "updateBranding",
    tag = "branding",
    extensions(("x-stepclimb-mutates" = json!(true))),
    request_body = BrandingInputs,
    responses(
        (status = 200, description = "The organization's branding after the change", body = Branding),
        (status = 400, response = crate::openapi::BadRequest),
        (status = 401, response = crate::openapi::Unauthenticated),
        (status = 403, response = BrandingWriteForbidden),
        (status = 404, response = crate::openapi::OrganizationNotFound),
        (status = 413, response = crate::openapi::PayloadTooLarge),
        (status = 415, response = crate::openapi::UnsupportedMediaType),
        (status = 422, response = BrandingInvalid),
        (status = 500, response = crate::openapi::InternalError),
        (status = 503, response = crate::openapi::NoOrganization),
    )
)]
pub async fn update_branding(
    _feature: Require<CustomBranding>,
    require: Require<BrandingManage>,
    client: ClientAddress,
    State(state): State<AppState>,
    ApiJson(body): ApiJson<BrandingInputs>,
) -> Result<Json<Branding>, BrandingError> {
    let brand = Brand::parse(&CoreInputs {
        primary: body.primary,
        ui_font: body.ui_font,
        reading_font: body.reading_font,
        corner_style: body.corner_style,
    })
    .map_err(invalid_input)?;
    let rendered = theme::render(&brand).map_err(render_error)?;
    let scope = require.principal().scope();
    let actor = client.actor(require.principal());
    let saved = branding::save(&state.db, scope, &rendered, state.clock.now(), actor)
        .await
        .map_err(branding_error)?;
    Ok(Json(Branding::new(&saved)))
}

/// The stylesheet of the request's organization: its saved branding's, or the default brand's.
#[utoipa::path(
    get,
    path = "/theme.css",
    operation_id = "getThemeCss",
    tag = "operations",
    responses(
        (status = 200, description = "The organization's light, dark and high-contrast themes as CSS variables, revalidated on every load (`Cache-Control: no-cache`)", body = String, content_type = "text/css"),
        (status = 404, response = crate::openapi::OrganizationNotFound),
        (status = 500, response = crate::openapi::InternalError),
        (status = 503, response = crate::openapi::NoOrganization),
    )
)]
pub async fn theme_css(
    org: RequestOrg,
    State(state): State<AppState>,
) -> Result<Response, ApiError> {
    let css = match branding::get(&state.db, org.scope()).await {
        Ok(Some(saved)) => saved.css,
        Ok(None) => DEFAULT.css.clone(),
        Err(error) => return Err(branding_error(error)),
    };
    let headers = [
        (
            header::CONTENT_TYPE,
            HeaderValue::from_static("text/css; charset=utf-8"),
        ),
        (
            header::CACHE_CONTROL,
            HeaderValue::from_static(THEME_CSS_CACHE_CONTROL),
        ),
    ];
    Ok((headers, css).into_response())
}

fn invalid_input(error: theme::InputError) -> BrandingError {
    let rule = match error.rule {
        InputRule::InvalidColor => code::INVALID_COLOR,
        InputRule::TooLong => code::TOO_LONG,
        InputRule::NotAChoice => code::NOT_A_CHOICE,
    };
    let error = ApiError::new(
        StatusCode::UNPROCESSABLE_ENTITY,
        code::INVALID_INPUT,
        "A branding input is not valid; `errors` names it.",
    )
    .with_errors(vec![FieldError::at(&[error.field], rule)]);
    BrandingError::Api(error)
}

fn render_error(error: theme::RenderError) -> BrandingError {
    match error {
        theme::RenderError::Input(error) => invalid_input(error),
        theme::RenderError::Contrast(reports) => BrandingError::ContrastFailed(reports),
        other @ theme::RenderError::NoStep(_) => BrandingError::Api(ApiError::internal(&other)),
    }
}

fn branding_error(error: branding::Error) -> ApiError {
    match error {
        branding::Error::NotFound => ApiError::new(
            StatusCode::NOT_FOUND,
            request_org::code::ORG_NOT_FOUND,
            "The request's organization cannot be determined.",
        ),
        other => ApiError::internal(&other),
    }
}

#[cfg(test)]
mod tests {
    use axum::body::to_bytes;
    use stepclimb_core::theme::BrandInputs as Inputs;

    use super::*;

    #[test]
    fn the_default_brand_renders() {
        assert_eq!(DEFAULT.brand, Brand::stepclimb_default());
        assert_eq!(DEFAULT.css_hash, branding::css_hash(&DEFAULT.css));
        assert_eq!(DEFAULT.report.len(), 3);
        assert!(DEFAULT.report.iter().all(|r| r.pass));
    }

    #[test]
    fn a_bad_input_is_422_branding_invalid_input_naming_the_field() {
        let input = |primary: &str, corner_style: Option<&str>| Inputs {
            primary: primary.into(),
            corner_style: corner_style.map(Into::into),
            ..Inputs::default()
        };
        let long = format!("oklch(0.5 0.1 {})", "1".repeat(60));
        for (inputs, field, rule) in [
            (input("dark red", None), "/primary", code::INVALID_COLOR),
            (input(&long, None), "/primary", code::TOO_LONG),
            (
                input("#500E0E", Some("pointy")),
                "/corner_style",
                code::NOT_A_CHOICE,
            ),
        ] {
            let BrandingError::Api(error) = invalid_input(Brand::parse(&inputs).unwrap_err())
            else {
                panic!("an input error is an API error");
            };
            let problem = error.problem();
            assert_eq!(
                (problem.status, problem.code.as_str()),
                (422, code::INVALID_INPUT)
            );
            assert_eq!(
                problem.errors,
                [FieldError {
                    field: field.into(),
                    code: rule.into()
                }]
            );
        }
    }

    async fn body_of(error: BrandingError) -> (StatusCode, serde_json::Value) {
        let response = error.into_response();
        let status = response.status();
        assert_eq!(response.headers()[header::CONTENT_TYPE], PROBLEM_JSON);
        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        (status, serde_json::from_slice(&body).unwrap())
    }

    #[tokio::test]
    async fn render_error_maps_a_contrast_failure_to_422_branding_contrast_failed() {
        let failing = theme::RenderError::Contrast(DEFAULT.report.clone());
        let (status, body) = body_of(render_error(failing)).await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
        assert_eq!(body["code"], code::CONTRAST_FAILED);
        assert_eq!(body["report"].as_array().map(Vec::len), Some(3));
    }

    #[tokio::test]
    async fn a_contrast_failure_answers_its_failing_pairs_first() {
        // Unsorted, as the guard gives it: fg-soft on bg-canvas (pair 9) and line-strong on
        // bg-sunken (pair 30) fail between passing pairs.
        let mut light = DEFAULT.report[0].clone();
        light.pass = false;
        light.pairs[9].pass = false;
        light.pairs[30].pass = false;
        assert!(light.pairs[0].pass);
        let reports = vec![light, DEFAULT.report[1].clone()];
        let (status, body) = body_of(BrandingError::ContrastFailed(reports)).await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
        assert_eq!(body["code"], code::CONTRAST_FAILED);
        assert_eq!(body["status"], 422);
        let pairs = body["report"][0]["pairs"].as_array().unwrap();
        let first: Vec<(&str, &str, bool)> = pairs[..3]
            .iter()
            .map(|p| {
                let name = |k: &str| p[k].as_str().unwrap();
                (
                    name("foreground"),
                    name("background"),
                    p["pass"].as_bool().unwrap(),
                )
            })
            .collect();
        assert_eq!(
            first,
            [
                ("fg-soft", "bg-canvas", false),
                ("line-strong", "bg-sunken", false),
                ("fg-strong", "bg-canvas", true),
            ]
        );
        assert_eq!(pairs.len(), DEFAULT.report[0].pairs.len());
        assert_eq!(body["report"].as_array().unwrap().len(), 2);
    }
}
