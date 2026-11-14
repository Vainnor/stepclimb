// SPDX-License-Identifier: AGPL-3.0-or-later
//! The contrast report (spec 12, B20): every pair of tokens a theme must keep apart, measured
//! with the WCAG 2 contrast ratio on the displayed 8-bit sRGB colors. The port of
//! `packages/tokens/src/report.ts`; the pairs are its `requiredPairs`, in its order.

use serde::{Deserialize, Serialize};

use super::{
    Appearance, Theme,
    color::{Oklch, contrast},
    css::js_to_fixed,
};

/// Text needs the text minimum, everything else the UI minimum.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum PairKind {
    Text,
    Ui,
}

/// An appearance's contrast minimums.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Minimums {
    pub text: f64,
    pub ui: f64,
}

/// WCAG 2.2 AA (4.5:1 text, 3:1 UI) in light and dark; in high contrast 7:1 and 4.5:1 (Q10).
pub fn minimums(appearance: Appearance) -> Minimums {
    match appearance {
        Appearance::Light | Appearance::Dark => Minimums { text: 4.5, ui: 3.0 },
        Appearance::HighContrast => Minimums { text: 7.0, ui: 4.5 },
    }
}

/// Each learning state and the color token it aliases (`src/tokens/learning.json`, Q3).
pub(crate) const LEARNING_ALIASES: [(&str, &str); 6] = [
    ("state-locked", "fg-soft"),
    ("state-available", "fg-sub"),
    ("state-progress", "brand"),
    ("state-complete", "success"),
    ("state-overdue", "danger"),
    ("state-review", "warning"),
];

use PairKind::{Text, Ui};

/// B20's table, in `report.ts`'s order: foreground, background, kind. `fg-disabled` is absent:
/// WCAG exempts disabled controls.
pub const REQUIRED_PAIRS: [(&str, &str, PairKind); 43] = [
    ("fg-strong", "bg-canvas", Text),
    ("fg-strong", "bg-raised", Text),
    ("fg-strong", "bg-sunken", Text),
    ("fg-default", "bg-canvas", Text),
    ("fg-default", "bg-raised", Text),
    ("fg-default", "bg-sunken", Text),
    ("fg-sub", "bg-canvas", Text),
    ("fg-sub", "bg-raised", Text),
    ("fg-sub", "bg-sunken", Text),
    ("fg-soft", "bg-canvas", Text),
    ("fg-soft", "bg-raised", Text),
    ("fg-soft", "bg-sunken", Text),
    ("on-brand", "brand", Text),
    ("on-brand", "brand-hover", Text),
    ("brand-fg", "brand-soft", Text),
    ("brand-fg", "bg-canvas", Text),
    ("success-fg", "success-soft", Text),
    ("success-fg", "bg-canvas", Text),
    ("warning-fg", "warning-soft", Text),
    ("warning-fg", "bg-canvas", Text),
    ("danger-fg", "danger-soft", Text),
    ("danger-fg", "bg-canvas", Text),
    ("info-fg", "info-soft", Text),
    ("info-fg", "bg-canvas", Text),
    ("brand", "bg-canvas", Ui),
    ("brand", "bg-raised", Ui),
    ("ring", "bg-canvas", Ui),
    ("ring", "bg-raised", Ui),
    ("line-strong", "bg-canvas", Ui),
    ("line-strong", "bg-raised", Ui),
    // Input borders sit on wells and inputs too (owner decision, spec 12 B20).
    ("line-strong", "bg-sunken", Ui),
    ("state-locked", "bg-canvas", Ui),
    ("state-locked", "bg-raised", Ui),
    ("state-available", "bg-canvas", Ui),
    ("state-available", "bg-raised", Ui),
    ("state-progress", "bg-canvas", Ui),
    ("state-progress", "bg-raised", Ui),
    ("state-complete", "bg-canvas", Ui),
    ("state-complete", "bg-raised", Ui),
    ("state-overdue", "bg-canvas", Ui),
    ("state-overdue", "bg-raised", Ui),
    ("state-review", "bg-canvas", Ui),
    ("state-review", "bg-raised", Ui),
];

/// One measured pair: `foreground` on `background`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ContrastPair {
    pub foreground: String,
    pub background: String,
    /// The WCAG 2 contrast ratio, 1 to 21, unrounded.
    pub ratio: f64,
    pub minimum: f64,
    pub pass: bool,
}

/// Something a person should look at that does not fail the theme.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContrastWarning {
    /// `brand-near-danger`.
    pub code: String,
    pub message: String,
}

/// One appearance's report.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ContrastReport {
    pub appearance: Appearance,
    /// True when every pair passes; warnings never fail a theme.
    pub pass: bool,
    pub pairs: Vec<ContrastPair>,
    pub warnings: Vec<ContrastWarning>,
}

impl ContrastReport {
    /// The report with its failing pairs first, each group in [`REQUIRED_PAIRS`] order.
    pub fn failing_first(mut self) -> Self {
        self.pairs.sort_by_key(|p| p.pass);
        self
    }
}

fn color_of(theme: &Theme, token: &str) -> Oklch {
    let target = LEARNING_ALIASES
        .iter()
        .find(|(state, _)| *state == token)
        .map_or(token, |(_, target)| target);
    // Invariant: every token of REQUIRED_PAIRS, after its alias, is in COLOR_TOKENS, and every
    // theme carries every color token; the unit test below walks the table.
    theme
        .color(target)
        .expect("every required pair names a color token")
}

/// Hue difference in degrees, 0 to 180.
fn hue_distance(a: f64, b: f64) -> f64 {
    let d = (a - b).abs() % 360.0;
    if d > 180.0 { 360.0 - d } else { d }
}

/// Below this hue difference, and at or above [`NEAR_DANGER_CHROMA`], the brand reads as the
/// danger color.
const NEAR_DANGER_HUE: f64 = 30.0;
const NEAR_DANGER_CHROMA: f64 = 0.04;

fn warnings(theme: &Theme) -> Vec<ContrastWarning> {
    let brand = color_of(theme, "brand");
    let danger = color_of(theme, "danger");
    let distance = hue_distance(brand.h, danger.h);
    if brand.c < NEAR_DANGER_CHROMA || distance >= NEAR_DANGER_HUE {
        return Vec::new();
    }
    vec![ContrastWarning {
        code: "brand-near-danger".to_owned(),
        message: format!(
            "brand's hue is {} degrees from danger's; in-progress and overdue learning states may look alike",
            js_to_fixed(distance, 1)
        ),
    }]
}

/// Measures every required pair of `theme` against its appearance's minimums.
pub fn contrast_report(theme: &Theme) -> ContrastReport {
    let min = minimums(theme.appearance);
    let pairs: Vec<ContrastPair> = REQUIRED_PAIRS
        .iter()
        .map(|&(foreground, background, kind)| {
            let ratio = contrast(color_of(theme, foreground), color_of(theme, background));
            let minimum = match kind {
                Text => min.text,
                Ui => min.ui,
            };
            ContrastPair {
                foreground: foreground.to_owned(),
                background: background.to_owned(),
                ratio,
                minimum,
                pass: ratio >= minimum,
            }
        })
        .collect();
    ContrastReport {
        appearance: theme.appearance,
        pass: pairs.iter().all(|p| p.pass),
        pairs,
        warnings: warnings(theme),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::{APPEARANCES, Brand, COLOR_TOKENS, generate};

    #[test]
    fn every_required_pair_names_a_color_token_after_its_alias() {
        for (fg, bg, _) in REQUIRED_PAIRS {
            for token in [fg, bg] {
                let target = LEARNING_ALIASES
                    .iter()
                    .find(|(s, _)| *s == token)
                    .map_or(token, |(_, t)| t);
                assert!(COLOR_TOKENS.contains(&target), "{token}");
            }
        }
    }

    #[test]
    fn the_default_brand_passes_and_vzdc_warns_near_danger() {
        let default = generate(&Brand::stepclimb_default()).unwrap();
        for a in APPEARANCES {
            let report = contrast_report(default.get(a));
            assert!(report.pass, "{a:?}");
            assert!(report.warnings.is_empty());
        }
        let vzdc = Brand {
            primary: "#500E0E".into(),
            ..Brand::stepclimb_default()
        };
        let light = contrast_report(&generate(&vzdc).unwrap().light);
        assert_eq!(light.warnings.len(), 1);
        assert_eq!(light.warnings[0].code, "brand-near-danger");
    }

    #[test]
    fn hue_distance_wraps_around() {
        assert_eq!(hue_distance(350.0, 10.0), 20.0);
        assert_eq!(hue_distance(10.0, 190.0), 180.0);
    }
}
