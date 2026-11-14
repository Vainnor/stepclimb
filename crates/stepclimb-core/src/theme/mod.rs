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
}

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
    /// As the client sent it; it parses as a color.
    pub primary: String,
    pub ui_font: UiFont,
    pub reading_font: ReadingFont,
    pub corner_style: CornerStyle,
}

impl Brand {
    /// The inputs checked in order (`primary`, `ui_font`, `reading_font`, `corner_style`), the
    /// first that fails named in the error, as `generate.ts` checks them.
    pub fn parse(inputs: &BrandInputs) -> Result<Self, InputError> {
        color::parse_color(&inputs.primary, "primary")?;
        Ok(Self {
            primary: inputs.primary.clone(),
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
        .ok_or(InputError { field })
}

/// Why a stylesheet could not be rendered.
#[derive(Debug, thiserror::Error)]
pub enum RenderError {
    /// An input is not valid.
    #[error(transparent)]
    Input(#[from] InputError),
    /// A theme has a failing contrast pair (B21); the reports of every appearance, each with its
    /// failing pairs first.
    #[error("a generated theme fails its contrast report")]
    Contrast(Vec<ContrastReport>),
    /// The generator found no scale step for a token, which its stops rule out.
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
/// refuses a theme with any failing pair (B21) with the reports, failing pairs first.
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
        return Err(RenderError::Contrast(
            reports
                .into_iter()
                .map(ContrastReport::failing_first)
                .collect(),
        ));
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
    fn the_guard_refuses_a_theme_with_a_failing_pair_failing_pairs_first() {
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
            .take_while(|p| !p.pass)
            .map(|p| (p.foreground, p.background))
            .collect();
        assert!(failing.contains(&("fg-soft", "bg-canvas")), "{failing:?}");
        assert!(light.pairs[failing.len()..].iter().all(|p| p.pass));
        assert!(reports[1].pass && reports[2].pass);
    }
}
