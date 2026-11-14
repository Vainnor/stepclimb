// SPDX-License-Identifier: AGPL-3.0-or-later
//! The generator: a brand color in, a theme of color tokens, fonts and radii per appearance out
//! (spec 12, B11 to B18). The port of `packages/tokens/src/generate.ts`; the stops, the step
//! choice and the rounding are its own, value for value.

use serde::Serialize;

use super::{
    Brand, RenderError,
    color::{IN_SRGB_EPSILON, Oklch, contrast, gamut_map_srgb, in_srgb, js_round, parse_color},
    report::minimums,
};

/// Every color token a theme carries, in the plan's order (UI tab -> Tokens).
pub const COLOR_TOKENS: [&str; 30] = [
    "bg-canvas",
    "bg-raised",
    "bg-sunken",
    "bg-overlay",
    "fg-strong",
    "fg-default",
    "fg-sub",
    "fg-soft",
    "fg-disabled",
    "line-strong",
    "line",
    "line-soft",
    "brand",
    "brand-hover",
    "brand-soft",
    "brand-fg",
    "on-brand",
    "ring",
    "success",
    "success-soft",
    "success-fg",
    "warning",
    "warning-soft",
    "warning-fg",
    "danger",
    "danger-soft",
    "danger-fg",
    "info",
    "info-soft",
    "info-fg",
];

/// The radius tokens corner style controls.
pub const RADIUS_TOKENS: [&str; 3] = ["xs", "sm", "md"];

/// The appearances every brand has (spec 12, B5, B16).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Appearance {
    Light,
    Dark,
    HighContrast,
}

/// Every appearance, in the order the stylesheet and the reports list them.
pub const APPEARANCES: [Appearance; 3] = [
    Appearance::Light,
    Appearance::Dark,
    Appearance::HighContrast,
];

/// One appearance's tokens. Colors are gamut-mapped into sRGB and rounded as they are written.
#[derive(Clone, Debug, PartialEq)]
pub struct Theme {
    pub appearance: Appearance,
    /// One color per [`COLOR_TOKENS`] entry, in its order.
    colors: Vec<(&'static str, Oklch)>,
    pub font_ui: &'static [&'static str],
    pub font_read: &'static [&'static str],
    /// `xs`, `sm` and `md` in px.
    pub radius: [u32; 3],
}

impl Theme {
    /// The color of `token`, one of [`COLOR_TOKENS`].
    pub fn color(&self, token: &str) -> Option<Oklch> {
        self.colors
            .iter()
            .find(|(t, _)| *t == token)
            .map(|(_, c)| *c)
    }

    /// Every color token with its color, in [`COLOR_TOKENS`] order.
    pub fn colors(&self) -> &[(&'static str, Oklch)] {
        &self.colors
    }

    /// Replaces the color of `token`, for tests that need a theme the generator never makes.
    #[cfg(test)]
    pub(crate) fn set(&mut self, token: &str, color: Oklch) {
        if let Some(entry) = self.colors.iter_mut().find(|(t, _)| *t == token) {
            entry.1 = color;
        }
    }
}

/// A brand's three themes.
#[derive(Clone, Debug, PartialEq)]
pub struct GeneratedThemes {
    pub light: Theme,
    pub dark: Theme,
    pub high_contrast: Theme,
}

impl GeneratedThemes {
    pub fn get(&self, appearance: Appearance) -> &Theme {
        match appearance {
            Appearance::Light => &self.light,
            Appearance::Dark => &self.dark,
            Appearance::HighContrast => &self.high_contrast,
        }
    }
}

/// One scale step: lightness, and chroma for an input of [`REFERENCE_CHROMA`].
type Stop = (f64, f64);

/// The chroma the stops are written for: the default brand's, `oklch(0.50 0.09 195)`.
const REFERENCE_CHROMA: f64 = 0.09;

/// How one appearance is drawn from a brand color (`Recipe` in `generate.ts`).
struct Recipe {
    /// From `on-brand`'s end: step 0 is `on-brand`, step 1 `brand-soft`.
    brand_stops: &'static [Stop],
    /// The lightness `brand` aims at; `None` for the input's own.
    brand_target: Option<f64>,
    /// How many steps past `brand` `brand-fg` aims at.
    fg_offset: usize,
    neutral_stops: &'static [Stop],
    /// Each neutral token's step.
    neutral_steps: &'static [(&'static str, usize)],
    /// The modal scrim: a neutral step at this opacity.
    overlay: (usize, f64),
    /// Status colors (B17): token, lightness, chroma, hue.
    status: &'static [(&'static str, f64, f64, f64)],
}

const LIGHT: Recipe = Recipe {
    brand_stops: &[
        (0.99, 0.005),
        (0.95, 0.025),
        (0.9, 0.04),
        (0.83, 0.055),
        (0.75, 0.07),
        (0.67, 0.08),
        (0.58, 0.087),
        (0.5, 0.09),
        (0.45, 0.09),
        (0.4, 0.08),
        (0.31, 0.075),
        (0.26, 0.068),
    ],
    brand_target: None,
    fg_offset: 2,
    neutral_stops: &[
        (1.0, 0.0),
        (0.985, 0.004),
        (0.955, 0.007),
        (0.93, 0.006),
        (0.885, 0.009),
        (0.7, 0.008),
        (0.633, 0.012),
        (0.533, 0.01),
        (0.46, 0.012),
        (0.31, 0.013),
        (0.22, 0.014),
    ],
    neutral_steps: &[
        ("bg-raised", 0),
        ("bg-canvas", 1),
        ("bg-sunken", 2),
        ("line-soft", 3),
        ("line", 4),
        ("fg-disabled", 5),
        ("line-strong", 6),
        ("fg-soft", 7),
        ("fg-sub", 8),
        ("fg-default", 9),
        ("fg-strong", 10),
    ],
    overlay: (10, 0.45),
    status: &[
        ("success", 0.55, 0.12, 150.0),
        ("success-soft", 0.95, 0.03, 150.0),
        ("success-fg", 0.42, 0.1, 150.0),
        ("warning", 0.663, 0.14, 75.0),
        ("warning-soft", 0.96, 0.04, 85.0),
        ("warning-fg", 0.47, 0.1, 65.0),
        ("danger", 0.55, 0.18, 27.0),
        ("danger-soft", 0.95, 0.03, 25.0),
        ("danger-fg", 0.47, 0.16, 27.0),
        ("info", 0.55, 0.13, 250.0),
        ("info-soft", 0.95, 0.03, 250.0),
        ("info-fg", 0.45, 0.13, 250.0),
    ],
};

const DARK: Recipe = Recipe {
    brand_stops: &[
        (0.18, 0.02),
        (0.28, 0.04),
        (0.34, 0.05),
        (0.42, 0.06),
        (0.5, 0.07),
        (0.58, 0.08),
        (0.67, 0.09),
        (0.76, 0.1),
        (0.82, 0.09),
        (0.84, 0.08),
        (0.9, 0.06),
        (0.95, 0.03),
    ],
    brand_target: Some(0.76),
    fg_offset: 2,
    neutral_stops: &[
        (0.1, 0.008),
        (0.145, 0.008),
        (0.17, 0.008),
        (0.21, 0.01),
        (0.26, 0.01),
        (0.31, 0.011),
        (0.45, 0.01),
        (0.507, 0.012),
        (0.64, 0.01),
        (0.75, 0.01),
        (0.89, 0.008),
        (0.96, 0.005),
    ],
    neutral_steps: &[
        ("bg-sunken", 1),
        ("bg-canvas", 2),
        ("bg-raised", 3),
        ("line-soft", 4),
        ("line", 5),
        ("fg-disabled", 6),
        ("line-strong", 7),
        ("fg-soft", 8),
        ("fg-sub", 9),
        ("fg-default", 10),
        ("fg-strong", 11),
    ],
    overlay: (0, 0.6),
    status: &[
        ("success", 0.74, 0.13, 150.0),
        ("success-soft", 0.27, 0.04, 150.0),
        ("success-fg", 0.82, 0.11, 150.0),
        ("warning", 0.8, 0.13, 80.0),
        ("warning-soft", 0.29, 0.05, 80.0),
        ("warning-fg", 0.86, 0.11, 80.0),
        ("danger", 0.7, 0.16, 25.0),
        ("danger-soft", 0.28, 0.05, 25.0),
        ("danger-fg", 0.82, 0.11, 25.0),
        ("info", 0.74, 0.11, 250.0),
        ("info-soft", 0.28, 0.05, 250.0),
        ("info-fg", 0.84, 0.08, 250.0),
    ],
};

const HIGH_CONTRAST: Recipe = Recipe {
    brand_stops: &[
        (1.0, 0.0),
        (0.95, 0.03),
        (0.88, 0.05),
        (0.78, 0.08),
        (0.68, 0.1),
        (0.58, 0.11),
        (0.5, 0.12),
        (0.44, 0.12),
        (0.38, 0.12),
        (0.32, 0.1),
        (0.26, 0.09),
        (0.2, 0.07),
    ],
    brand_target: Some(0.38),
    fg_offset: 1,
    neutral_stops: &[
        (1.0, 0.0),
        (0.96, 0.0),
        (0.55, 0.0),
        (0.5, 0.0),
        (0.38, 0.0),
        (0.35, 0.0),
        (0.3, 0.0),
        (0.25, 0.0),
        (0.18, 0.0),
        (0.13, 0.0),
        (0.0, 0.0),
    ],
    neutral_steps: &[
        ("bg-canvas", 0),
        ("bg-raised", 0),
        ("bg-sunken", 1),
        ("fg-disabled", 2),
        ("line-soft", 3),
        ("fg-soft", 4),
        ("line", 5),
        ("fg-sub", 6),
        ("line-strong", 7),
        ("fg-default", 8),
        ("fg-strong", 9),
    ],
    overlay: (10, 0.6),
    status: &[
        ("success", 0.42, 0.12, 150.0),
        ("success-soft", 0.95, 0.03, 150.0),
        ("success-fg", 0.36, 0.1, 150.0),
        ("warning", 0.5, 0.11, 70.0),
        ("warning-soft", 0.96, 0.04, 85.0),
        ("warning-fg", 0.38, 0.08, 65.0),
        ("danger", 0.44, 0.17, 27.0),
        ("danger-soft", 0.95, 0.03, 25.0),
        ("danger-fg", 0.38, 0.15, 27.0),
        ("info", 0.42, 0.13, 250.0),
        ("info-soft", 0.95, 0.03, 250.0),
        ("info-fg", 0.36, 0.12, 250.0),
    ],
};

fn recipe(appearance: Appearance) -> &'static Recipe {
    match appearance {
        Appearance::Light => &LIGHT,
        Appearance::Dark => &DARK,
        Appearance::HighContrast => &HIGH_CONTRAST,
    }
}

/// `x` rounded to `digits` decimals as `generate.ts`'s `round` does.
fn round(x: f64, digits: i32) -> f64 {
    let scale = 10_f64.powi(digits);
    js_round(x * scale) / scale
}

/// The color as it is written (`finalize` in `generate.ts`): gamut-mapped into sRGB, lightness
/// and chroma rounded to four decimals and hue to two (360 becomes 0), chroma stepped down by
/// 0.0001 while rounding left it outside sRGB, hue 0 when chroma is 0.
pub(crate) fn finalize(color: Oklch) -> Oklch {
    let mapped = gamut_map_srgb(color);
    let mut out = Oklch::new(round(mapped.l, 4), round(mapped.c, 4), round(mapped.h, 2));
    if out.h >= 360.0 {
        out.h = 0.0;
    }
    while out.c > 0.0 && !in_srgb(out, IN_SRGB_EPSILON) {
        out.c = round(out.c - 0.0001, 4);
    }
    if out.c == 0.0 {
        out.h = 0.0;
    }
    if let Some(alpha) = color.alpha.filter(|&a| a < 1.0) {
        out.alpha = Some(round(alpha, 3));
    }
    out
}

fn scale(stops: &[Stop], hue: f64, chroma_factor: f64) -> Vec<Oklch> {
    stops
        .iter()
        .map(|&(l, c)| finalize(Oklch::new(l, c * chroma_factor, hue)))
        .collect()
}

/// The candidate with the smallest `distance`, the later on a tie.
fn nearest(
    candidates: &[usize],
    distance: impl Fn(usize) -> f64,
    token: &'static str,
) -> Result<usize, RenderError> {
    let mut best: Option<usize> = None;
    for &i in candidates {
        if best.is_none_or(|b| distance(i) <= distance(b)) {
            best = Some(i);
        }
    }
    best.ok_or(RenderError::NoStep(token))
}

/// The minimums a brand step choice must clear, and where `brand-fg` aims.
struct BrandRule {
    text: f64,
    ui: f64,
    fg_offset: usize,
}

/// The brand tokens from a scale ordered from `on-brand`'s end (`brandTokens` in `generate.ts`).
fn brand_tokens(
    scale: &[Oklch],
    target: f64,
    canvas: Oklch,
    raised: Oklch,
    rule: &BrandRule,
) -> Result<[(&'static str, Oklch); 6], RenderError> {
    let on_brand = scale[0];
    let soft = scale[1];
    let all: Vec<usize> = (0..scale.len()).collect();
    let brandable: Vec<usize> = all[..all.len() - 1]
        .iter()
        .copied()
        .filter(|&i| contrast(scale[i], on_brand) >= rule.text)
        .collect();
    let brand = nearest(&brandable, |i| (scale[i].l - target).abs(), "brand")?;
    let fg_ok: Vec<usize> = all
        .iter()
        .copied()
        .filter(|&i| {
            contrast(scale[i], soft) >= rule.text && contrast(scale[i], canvas) >= rule.text
        })
        .collect();
    let ring_ok: Vec<usize> = all
        .iter()
        .copied()
        .filter(|&i| contrast(scale[i], canvas) >= rule.ui && contrast(scale[i], raised) >= rule.ui)
        .collect();
    let index_distance = |aim: usize| move |i: usize| (i as f64 - aim as f64).abs();
    let fg = nearest(&fg_ok, index_distance(brand + rule.fg_offset), "brand-fg")?;
    let ring = nearest(&ring_ok, index_distance(brand), "ring")?;
    Ok([
        ("brand", scale[brand]),
        ("brand-hover", scale[brand + 1]),
        ("brand-soft", soft),
        ("brand-fg", scale[fg]),
        ("on-brand", on_brand),
        ("ring", scale[ring]),
    ])
}

fn theme(appearance: Appearance, primary: Oklch, brand: &Brand) -> Result<Theme, RenderError> {
    let recipe = recipe(appearance);
    // A grey's hue is floating-point noise; below the written chroma precision it is 0.
    let hue = if primary.c < 0.0001 { 0.0 } else { primary.h };
    let neutrals = scale(
        recipe.neutral_stops,
        hue,
        (primary.c / REFERENCE_CHROMA).min(1.0),
    );
    let brand_scale = scale(recipe.brand_stops, hue, primary.c / REFERENCE_CHROMA);
    let mut colors: Vec<(&'static str, Oklch)> = recipe
        .neutral_steps
        .iter()
        .map(|&(token, i)| (token, neutrals[i]))
        .collect();
    let neutral = |token: &str| colors.iter().find(|(t, _)| *t == token).map(|(_, c)| *c);
    let (canvas, raised) = neutral("bg-canvas")
        .zip(neutral("bg-raised"))
        .ok_or(RenderError::NoStep("bg-canvas"))?;
    let (step, alpha) = recipe.overlay;
    colors.push((
        "bg-overlay",
        Oklch {
            alpha: Some(alpha),
            ..neutrals[step]
        },
    ));
    let min = minimums(appearance);
    let rule = BrandRule {
        text: min.text,
        ui: min.ui,
        fg_offset: recipe.fg_offset,
    };
    let target = recipe.brand_target.unwrap_or(primary.l);
    colors.extend(brand_tokens(&brand_scale, target, canvas, raised, &rule)?);
    colors.extend(
        recipe
            .status
            .iter()
            .map(|&(token, l, c, h)| (token, finalize(Oklch::new(l, c, h)))),
    );
    let ordered = COLOR_TOKENS
        .iter()
        .map(|&token| {
            let color = colors.iter().find(|(t, _)| *t == token).map(|(_, c)| *c);
            color.map(|c| (token, c)).ok_or(RenderError::NoStep(token))
        })
        .collect::<Result<_, _>>()?;
    Ok(Theme {
        appearance,
        colors: ordered,
        font_ui: brand.ui_font.stack(),
        font_read: brand.reading_stack(),
        radius: brand.corner_style.radius(),
    })
}

/// Generates a brand's light, dark and high-contrast themes from the same inputs (B16).
pub fn generate(brand: &Brand) -> Result<GeneratedThemes, RenderError> {
    let primary = parse_color(&brand.primary, "primary")?;
    Ok(GeneratedThemes {
        light: theme(Appearance::Light, primary, brand)?,
        dark: theme(Appearance::Dark, primary, brand)?,
        high_contrast: theme(Appearance::HighContrast, primary, brand)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finalize_wraps_a_hue_that_rounds_to_360_and_drops_the_hue_of_no_chroma() {
        assert_eq!(
            finalize(Oklch::new(0.5, 0.05, 359.996)),
            Oklch::new(0.5, 0.05, 0.0)
        );
        assert_eq!(
            finalize(Oklch::new(0.5, 0.05, 359.994)),
            Oklch::new(0.5, 0.05, 359.99)
        );
        assert_eq!(
            finalize(Oklch::new(0.5, 0.00004, 120.0)),
            Oklch::new(0.5, 0.0, 0.0)
        );
    }

    #[test]
    fn the_default_brand_gives_the_plans_brand_tokens() {
        let themes = generate(&Brand::stepclimb_default()).unwrap();
        assert_eq!(
            themes.light.color("brand"),
            Some(Oklch::new(0.5022, 0.0856, 195.13))
        );
        assert_eq!(
            themes.light.color("on-brand"),
            Some(Oklch::new(0.99, 0.005, 195.0))
        );
        let overlay = themes.light.color("bg-overlay").unwrap();
        assert_eq!(overlay.alpha, Some(0.45));
        for appearance in APPEARANCES {
            let theme = themes.get(appearance);
            let tokens: Vec<_> = theme.colors().iter().map(|(t, _)| *t).collect();
            assert_eq!(tokens, COLOR_TOKENS);
        }
    }

    #[test]
    fn nearest_takes_the_later_candidate_on_a_tie_and_fails_with_none() {
        assert_eq!(
            nearest(&[1, 3], |i| (i as f64 - 2.0).abs(), "x").unwrap(),
            3
        );
        assert!(matches!(
            nearest(&[], |_| 0.0, "ring"),
            Err(RenderError::NoStep("ring"))
        ));
    }
}
