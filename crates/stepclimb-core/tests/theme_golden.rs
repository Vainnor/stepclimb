// SPDX-License-Identifier: AGPL-3.0-or-later
//! The golden vectors against the server's generator (docs/specs/m0/12-tokens-and-theme-generator.md
//! B24, T13). `packages/tokens/test/golden/theme-vectors.json` is what the TypeScript generator
//! gives for each input, and its own test (T12) fails when the file is stale; this test fails when
//! the Rust port gives anything else: another stylesheet byte, another contrast ratio than within
//! 1e-9, another verdict or warning, or another refusal.

use serde::Deserialize;
use serde_json::Value;
use sha2::{Digest, Sha256};
use stepclimb_core::theme::{
    self, APPEARANCES, Brand, BrandInputs, GENERATOR_VERSION, REQUIRED_PAIRS, minimums,
};

const VECTORS: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../packages/tokens/test/golden/theme-vectors.json"
);

#[derive(Deserialize)]
struct File {
    generator: u32,
    minimums: Value,
    pairs: Vec<(String, String, String)>,
    named: Vec<Named>,
    swept: Vec<Swept>,
    invalid: Vec<Invalid>,
}

/// `BrandInputs` as `generate.ts` names them.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Inputs {
    primary: String,
    ui_font: Option<String>,
    reading_font: Option<String>,
    corner_style: Option<String>,
}

impl From<&Inputs> for BrandInputs {
    fn from(i: &Inputs) -> Self {
        Self {
            primary: i.primary.clone(),
            ui_font: i.ui_font.clone(),
            reading_font: i.reading_font.clone(),
            corner_style: i.corner_style.clone(),
        }
    }
}

#[derive(Deserialize)]
struct Named {
    inputs: Inputs,
    css: String,
    reports: Vec<Report>,
}

#[derive(Deserialize)]
struct Report {
    appearance: String,
    pass: bool,
    ratios: Vec<f64>,
    warnings: Vec<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Swept {
    inputs: Inputs,
    css_sha256: String,
    pass: Vec<bool>,
    warnings: Vec<Vec<String>>,
}

#[derive(Deserialize)]
struct Invalid {
    inputs: Inputs,
    field: String,
}

fn vectors() -> File {
    let text = std::fs::read_to_string(VECTORS).expect("the golden vectors exist");
    serde_json::from_str(&text).expect("the golden vectors parse")
}

/// The stylesheet and reports the port gives for `inputs`, whatever their verdict.
fn port(inputs: &Inputs) -> (String, Vec<theme::ContrastReport>) {
    let brand = Brand::parse(&inputs.into()).expect("valid inputs");
    let themes = theme::generate(&brand).expect("the generator finds every step");
    let reports = APPEARANCES
        .iter()
        .map(|&a| theme::contrast_report(themes.get(a)))
        .collect();
    (theme::stylesheet(&themes), reports)
}

fn appearance_name(report: &theme::ContrastReport) -> String {
    serde_json::to_value(report.appearance)
        .expect("an appearance serializes")
        .as_str()
        .expect("as a string")
        .to_owned()
}

#[test]
fn the_version_pairs_and_minimums_are_generate_ts_s() {
    let file = vectors();
    assert_eq!(file.generator, GENERATOR_VERSION);
    let pairs: Vec<(String, String, String)> = REQUIRED_PAIRS
        .iter()
        .map(|(fg, bg, kind)| {
            let kind = serde_json::to_value(kind).expect("a kind serializes");
            (
                (*fg).to_owned(),
                (*bg).to_owned(),
                kind.as_str().expect("a string").to_owned(),
            )
        })
        .collect();
    assert_eq!(pairs, file.pairs);
    for appearance in APPEARANCES {
        let name = serde_json::to_value(appearance).expect("serializes");
        let m = &file.minimums[name.as_str().expect("a string")];
        let ours = minimums(appearance);
        assert_eq!(
            (m["text"].as_f64(), m["ui"].as_f64()),
            (Some(ours.text), Some(ours.ui))
        );
    }
}

#[test]
fn t13_named_vectors_give_the_same_stylesheet_ratios_and_verdicts() {
    let file = vectors();
    assert!(!file.named.is_empty());
    for vector in &file.named {
        let at = &vector.inputs.primary;
        let (css, reports) = port(&vector.inputs);
        assert_eq!(css, vector.css, "{at}: the stylesheet differs");
        assert_eq!(reports.len(), vector.reports.len(), "{at}");
        for (ours, theirs) in reports.iter().zip(&vector.reports) {
            assert_eq!(appearance_name(ours), theirs.appearance, "{at}");
            assert_eq!(ours.pass, theirs.pass, "{at} {}", theirs.appearance);
            let codes: Vec<&str> = ours.warnings.iter().map(|w| w.code.as_str()).collect();
            assert_eq!(codes, theirs.warnings, "{at} {}", theirs.appearance);
            assert_eq!(ours.pairs.len(), theirs.ratios.len(), "{at}");
            for (pair, ratio) in ours.pairs.iter().zip(&theirs.ratios) {
                assert!(
                    (pair.ratio - ratio).abs() <= 1e-9,
                    "{at} {} {} on {}: {} against {ratio}",
                    theirs.appearance,
                    pair.foreground,
                    pair.background,
                    pair.ratio
                );
            }
        }
    }
}

#[test]
fn t13_swept_vectors_give_the_same_stylesheet_and_verdicts() {
    let file = vectors();
    assert!(file.swept.len() >= 200, "{}", file.swept.len());
    for vector in &file.swept {
        let at = &vector.inputs.primary;
        let (css, reports) = port(&vector.inputs);
        let hash: String = Sha256::digest(css.as_bytes())
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect();
        assert_eq!(
            hash, vector.css_sha256,
            "{at}: the stylesheet differs:\n{css}"
        );
        let pass: Vec<bool> = reports.iter().map(|r| r.pass).collect();
        assert_eq!(pass, vector.pass, "{at}");
        let warnings: Vec<Vec<String>> = reports
            .iter()
            .map(|r| r.warnings.iter().map(|w| w.code.clone()).collect())
            .collect();
        assert_eq!(warnings, vector.warnings, "{at}");
    }
}

#[test]
fn t13_invalid_vectors_are_refused_naming_the_same_field() {
    let file = vectors();
    assert!(!file.invalid.is_empty());
    for vector in &file.invalid {
        let field = match vector.field.as_str() {
            "uiFont" => "ui_font",
            "readingFont" => "reading_font",
            "cornerStyle" => "corner_style",
            other => other,
        };
        let error = Brand::parse(&(&vector.inputs).into())
            .expect_err(&format!("{:?} is refused", vector.inputs.primary));
        assert_eq!(error.field, field, "{:?}", vector.inputs.primary);
    }
}
