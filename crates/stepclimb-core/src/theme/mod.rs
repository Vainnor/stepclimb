// SPDX-License-Identifier: AGPL-3.0-or-later
//! The theme generator on the server (docs/specs/m0/12-tokens-and-theme-generator.md B24, Q12):
//! an organization's branding inputs in, its light, dark and high-contrast themes, their
//! contrast reports and its stylesheet out.
//!
//! It is the port of `packages/tokens/src/{color,generate,report,stylesheet}.ts`, which stays the
//! generator for the build and the browser preview. The two are held equal by the golden vectors
//! in `packages/tokens/test/golden/theme-vectors.json`: the TypeScript generator writes them,
//! and `tests/theme_golden.rs` checks this port gives the same stylesheet bytes, contrast ratios,
//! verdicts and refusals. Pure functions, no I/O.

pub mod color;
mod css;
mod generate;
mod report;

use serde::{Deserialize, Serialize};

pub use css::{stylesheet, theme_to_css};
pub use generate::{
    APPEARANCES, Appearance, COLOR_TOKENS, GeneratedThemes, RADIUS_TOKENS, Theme, generate,
};
pub use report::{
    ContrastPair, ContrastReport, ContrastWarning, Minimums, PairKind, REQUIRED_PAIRS,
    contrast_report, minimums,
};

/// The generator's version, equal to `GENERATOR_VERSION` in `packages/tokens/src/generate.ts`.
/// Stored with an organization's branding; it changes whenever the same inputs would give
/// different output.
pub const GENERATOR_VERSION: u32 = 1;

/// The branding inputs as a client sends them (spec 12, B12). Absent choices take their
/// defaults; [`Brand::parse`] checks them.
#[derive(Clone, Debug, Default, PartialEq, Eq, Deserialize)]
pub struct BrandInputs {
    pub primary: String,
    pub ui_font: Option<String>,
    pub reading_font: Option<String>,
    pub corner_style: Option<String>,
}

/// A branding input the generator cannot use. `field` is the input's name: `primary`,
/// `ui_font`, `reading_font` or `corner_style`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
#[error("the branding input {field} is not valid")]
pub struct InputError {
    pub field: &'static str,
    pub rule: InputRule,
}

/// The rule an input broke.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InputRule {
    /// `primary` is not a color [`color::parse_color`] reads.
    InvalidColor,
    /// `primary` is longer than [`MAX_PRIMARY_CHARS`] once trimmed.
    TooLong,
    /// The value is not one of the input's choices.
    NotAChoice,
}

/// The longest `primary`, in characters, once trimmed; the longest valid color needs far fewer.
pub const MAX_PRIMARY_CHARS: usize = 64;

/// The bundled UI fonts an organization chooses from (spec 12, Q8).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum UiFont {
    #[default]
    #[serde(rename = "Atkinson Hyperlegible Next")]
    AtkinsonHyperlegibleNext,
    Roboto,
}

impl UiFont {
    const ALL: [(Self, &'static str); 2] = [
        (Self::AtkinsonHyperlegibleNext, "Atkinson Hyperlegible Next"),
        (Self::Roboto, "Roboto"),
    ];

    /// The font stack `font.ui` takes.
    pub fn stack(self) -> &'static [&'static str] {
        match self {
            Self::AtkinsonHyperlegibleNext => &[
                "Atkinson Hyperlegible Next",
                "Segoe UI",
                "system-ui",
                "sans-serif",
            ],
            Self::Roboto => &["Roboto", "Segoe UI", "system-ui", "sans-serif"],
        }
    }
}

/// The reading font (spec 12, Q8): the bundled serif, or the organization's UI font.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ReadingFont {
    #[default]
    Serif,
    Sans,
}

impl ReadingFont {
    const ALL: [(Self, &'static str); 2] = [(Self::Serif, "serif"), (Self::Sans, "sans")];
}

/// `font.read` for [`ReadingFont::Serif`]: `base.json`'s reading stack.
const SERIF_STACK: &[&str] = &["Source Serif 4", "Georgia", "Times New Roman", "serif"];

/// The corner style (spec 12, Q8 and its coordinator decision): the radius of `xs`, `sm` and
/// `md`; `pill` never changes.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CornerStyle {
    Square,
    #[default]
    Default,
    Round,
}

impl CornerStyle {
    const ALL: [(Self, &'static str); 3] = [
        (Self::Square, "square"),
        (Self::Default, "default"),
        (Self::Round, "round"),
    ];

    /// `xs`, `sm` and `md` in px: `base.json`'s scale, 0 for square, doubled for round.
    pub fn radius(self) -> [u32; 3] {
        match self {
            Self::Square => [0, 0, 0],
            Self::Default => [3, 5, 8],
            Self::Round => [6, 10, 16],
        }
    }
}

/// Checked branding inputs, every choice made.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Brand {
    /// As the client sent it, without leading and trailing CSS whitespace; it parses as a color.
    pub primary: String,
    pub ui_font: UiFont,
    pub reading_font: ReadingFont,
    pub corner_style: CornerStyle,
}

impl Brand {
    /// The inputs checked in order (`primary`, `ui_font`, `reading_font`, `corner_style`), the
    /// first that fails named in the error, as `generate.ts` checks them.
    /// `primary` is trimmed of CSS whitespace and at most [`MAX_PRIMARY_CHARS`] long.
    pub fn parse(inputs: &BrandInputs) -> Result<Self, InputError> {
        let primary = inputs.primary.trim_matches(color::is_css_space);
        if primary.chars().count() > MAX_PRIMARY_CHARS {
            return Err(InputError {
                field: "primary",
                rule: InputRule::TooLong,
            });
        }
        color::parse_color(primary, "primary")?;
        Ok(Self {
            primary: primary.to_owned(),
            ui_font: choice("ui_font", inputs.ui_font.as_deref(), &UiFont::ALL)?,
            reading_font: choice(
                "reading_font",
                inputs.reading_font.as_deref(),
                &ReadingFont::ALL,
            )?,
            corner_style: choice(
                "corner_style",
                inputs.corner_style.as_deref(),
                &CornerStyle::ALL,
            )?,
        })
    }

    /// The default brand (`brands.default` in `packages/tokens/src/themes.ts`, ADR 0014), which
    /// an organization without saved branding gets (spec 12, B25).
    pub fn stepclimb_default() -> Self {
        Self {
            primary: "oklch(0.5 0.09 195)".to_owned(),
            ui_font: UiFont::default(),
            reading_font: ReadingFont::default(),
            corner_style: CornerStyle::default(),
        }
    }

    /// The `font.read` stack.
    pub fn reading_stack(&self) -> &'static [&'static str] {
        match self.reading_font {
            ReadingFont::Serif => SERIF_STACK,
            ReadingFont::Sans => self.ui_font.stack(),
        }
    }
}

/// The choice named `value`, the default when absent.
fn choice<T: Copy + Default>(
    field: &'static str,
    value: Option<&str>,
    all: &[(T, &str)],
) -> Result<T, InputError> {
    let Some(value) = value else {
        return Ok(T::default());
    };
    all.iter()
        .find(|(_, name)| *name == value)
        .map(|(choice, _)| *choice)
        .ok_or(InputError {
            field,
            rule: InputRule::NotAChoice,
        })
}

/// Why a stylesheet could not be rendered.
#[derive(Debug, thiserror::Error)]
pub enum RenderError {
    /// An input is not valid.
    #[error(transparent)]
    Input(#[from] InputError),
    /// A theme has a failing contrast pair (B21); the reports of every appearance.
    #[error("a generated theme fails its contrast report")]
    Contrast(Vec<ContrastReport>),
    /// The generator found no scale step for a token. A chroma far above
    /// [`color::MAX_CHROMA`] does this (1e150 did); at or under the cap, every input the
    /// property sweeps and the golden vectors try finds every step, which is evidence, not
    /// proof, so this stays an error rather than a panic.
    #[error("generator invariant: no scale step fits {0}")]
    NoStep(&'static str),
}

/// A brand's stylesheet whose every theme passed its contrast report. Only [`render`] makes one,
/// so nothing can store or serve a stylesheet that skipped the guard (spec 12, B21).
#[derive(Clone, Debug, PartialEq)]
pub struct Rendered {
    brand: Brand,
    css: String,
    reports: Vec<ContrastReport>,
}

impl Rendered {
    pub fn brand(&self) -> &Brand {
        &self.brand
    }

    /// The stylesheet `/theme.css` serves.
    pub fn css(&self) -> &str {
        &self.css
    }

    /// The light, dark and high-contrast reports, every one passing.
    pub fn reports(&self) -> &[ContrastReport] {
        &self.reports
    }
}

/// Generates the brand's themes, runs the contrast report on each and writes the stylesheet;
/// refuses a theme with any failing pair (B21) with the reports.
pub fn render(brand: &Brand) -> Result<Rendered, RenderError> {
    let themes = generate(brand)?;
    check(brand, &themes)
}

/// The guard of [`render`], apart so a test can give it a theme the generator never makes.
fn check(brand: &Brand, themes: &GeneratedThemes) -> Result<Rendered, RenderError> {
    let reports: Vec<ContrastReport> = APPEARANCES
        .iter()
        .map(|&a| report::contrast_report(themes.get(a)))
        .collect();
    if !reports.iter().all(|r| r.pass) {
        return Err(RenderError::Contrast(reports));
    }
    Ok(Rendered {
        brand: brand.clone(),
        css: stylesheet(themes),
        reports,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn inputs(primary: &str) -> BrandInputs {
        BrandInputs {
            primary: primary.to_owned(),
            ..BrandInputs::default()
        }
    }

    #[test]
    fn parse_fills_the_defaults_and_names_the_first_bad_input() {
        let brand = Brand::parse(&inputs("#500E0E")).unwrap();
        assert_eq!(
            (brand.ui_font, brand.reading_font, brand.corner_style),
            (
                UiFont::AtkinsonHyperlegibleNext,
                ReadingFont::Serif,
                CornerStyle::Default
            )
        );
        let bad = |change: fn(&mut BrandInputs)| {
            let mut i = inputs("#500E0E");
            change(&mut i);
            Brand::parse(&i).unwrap_err().field
        };
        assert_eq!(bad(|i| i.primary = "red".into()), "primary");
        assert_eq!(
            Brand::parse(&inputs("oklch(2 0 0)")).unwrap_err().rule,
            InputRule::InvalidColor
        );
        assert_eq!(
            Brand::parse(&inputs("#f00")).unwrap(),
            Brand {
                primary: "#f00".into(),
                ..Brand::parse(&inputs(" #f00\n")).unwrap()
            }
        );
        assert_eq!(bad(|i| i.ui_font = Some("roboto".into())), "ui_font");
        assert_eq!(
            bad(|i| i.reading_font = Some("Serif".into())),
            "reading_font"
        );
        assert_eq!(
            bad(|i| i.corner_style = Some("pointy".into())),
            "corner_style"
        );
        assert_eq!(
            bad(|i| {
                i.primary = "red".into();
                i.corner_style = Some("pointy".into());
            }),
            "primary"
        );
    }

    #[test]
    fn primary_is_trimmed_and_at_most_64_characters() {
        let padded = format!("  {}  ", "oklch(0.5 0.1 10)");
        assert_eq!(
            Brand::parse(&inputs(&padded)).unwrap().primary,
            "oklch(0.5 0.1 10)"
        );
        // Valid colors, 64 and 65 characters long once trimmed.
        let at_cap = format!("oklch(0.5 0.1 {})", "1".repeat(MAX_PRIMARY_CHARS - 15));
        assert_eq!(at_cap.chars().count(), MAX_PRIMARY_CHARS);
        assert!(Brand::parse(&inputs(&format!(" {at_cap} "))).is_ok());
        let over = format!("oklch(0.5 0.1 {})", "1".repeat(MAX_PRIMARY_CHARS - 14));
        assert!(color::parse_color(&over, "primary").is_ok());
        assert_eq!(
            Brand::parse(&inputs(&over)).unwrap_err(),
            InputError {
                field: "primary",
                rule: InputRule::TooLong
            }
        );
    }

    #[test]
    fn a_sans_reading_font_is_the_ui_font() {
        let mut brand = Brand::stepclimb_default();
        assert_eq!(brand.reading_stack(), SERIF_STACK);
        brand.ui_font = UiFont::Roboto;
        brand.reading_font = ReadingFont::Sans;
        assert_eq!(brand.reading_stack(), UiFont::Roboto.stack());
    }

    #[test]
    fn render_passes_the_default_brand() {
        let rendered = render(&Brand::stepclimb_default()).unwrap();
        assert_eq!(rendered.reports().len(), 3);
        assert!(
            rendered
                .css()
                .starts_with(":root,\n[data-sc-theme=\"light\"] {\n")
        );
    }

    #[test]
    fn the_guard_refuses_a_theme_with_a_failing_pair() {
        let brand = Brand::stepclimb_default();
        let mut themes = generate(&brand).unwrap();
        // fg-soft at the lightness of fg-disabled: about 3:1 on the light canvas.
        themes
            .light
            .set("fg-soft", color::Oklch::new(0.7, 0.008, 195.0));
        let Err(RenderError::Contrast(reports)) = check(&brand, &themes) else {
            panic!("the guard let a failing theme through");
        };
        let light = &reports[0];
        assert!(!light.pass);
        let failing: Vec<_> = light
            .pairs
            .iter()
            .filter(|p| !p.pass)
            .map(|p| (p.foreground.as_str(), p.background.as_str()))
            .collect();
        assert!(failing.contains(&("fg-soft", "bg-canvas")), "{failing:?}");
        assert!(reports[1].pass && reports[2].pass);
    }

    #[test]
    fn the_guard_refuses_a_status_tint_under_a_callouts_body_text() {
        let brand = Brand::stepclimb_default();
        let mut themes = generate(&brand).unwrap();
        // A dark warning tint as light as the dark fg-sub: fg-default on it is near 1:1.
        let tint = themes.dark.color("warning-soft").unwrap();
        themes
            .dark
            .set("warning-soft", color::Oklch::new(0.75, tint.c, tint.h));
        let Err(RenderError::Contrast(reports)) = check(&brand, &themes) else {
            panic!("the guard let a failing theme through");
        };
        let dark = &reports[1];
        assert_eq!(dark.appearance, Appearance::Dark);
        let failing: Vec<_> = dark
            .pairs
            .iter()
            .filter(|p| !p.pass)
            .map(|p| (p.foreground.as_str(), p.background.as_str()))
            .collect();
        assert!(
            failing.contains(&("fg-default", "warning-soft")),
            "{failing:?}"
        );
        assert!(reports[0].pass && reports[2].pass);
    }
}
