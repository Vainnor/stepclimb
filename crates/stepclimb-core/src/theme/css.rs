// SPDX-License-Identifier: AGPL-3.0-or-later
//! Themes as CSS with `variables.css`'s names: the port of `cssColor` and `themeToCss` in
//! `packages/tokens/src/generate.ts` and of `brandStylesheet` in `src/stylesheet.ts`, byte for
//! byte.

use super::{GeneratedThemes, RADIUS_TOKENS, Theme, color::Oklch, report::LEARNING_ALIASES};

/// JavaScript's `Number.prototype.toFixed` for a number at least 0: the exact decimal value
/// rounded to `digits` decimals, a tie rounded up. Rust's own formatting rounds a tie to even.
pub(crate) fn js_to_fixed(x: f64, digits: usize) -> String {
    // `-0` and `0` both print as `0`.
    let x = x.abs();
    // Every double below 2^53 has an exact decimal expansion of at most 1074 fractional digits.
    let exact = format!("{x:.1100}");
    let (whole, fraction) = exact.split_once('.').unwrap_or((&exact, ""));
    let mut kept: Vec<u8> = whole.bytes().chain(fraction.bytes().take(digits)).collect();
    if fraction.as_bytes().get(digits).is_some_and(|&d| d >= b'5') {
        let mut i = kept.len();
        loop {
            if i == 0 {
                kept.insert(0, b'1');
                break;
            }
            i -= 1;
            if kept[i] == b'9' {
                kept[i] = b'0';
            } else {
                kept[i] += 1;
                break;
            }
        }
    }
    let split = kept.len() - digits;
    let (whole, fraction) = kept.split_at(split);
    let whole = String::from_utf8_lossy(whole);
    if digits == 0 {
        whole.into_owned()
    } else {
        format!("{whole}.{}", String::from_utf8_lossy(fraction))
    }
}

/// `toFixed`, then trailing zeros and a bare point removed: `50.20` -> `50.2`, `195.00` -> `195`.
fn fixed(x: f64, digits: usize) -> String {
    let text = js_to_fixed(x, digits);
    if !text.contains('.') {
        return text;
    }
    text.trim_end_matches('0').trim_end_matches('.').to_owned()
}

/// `oklch(50.22% 0.0857 195.13)`, with ` / 0.45` when the color is translucent.
fn css_color(color: Oklch) -> String {
    let alpha = color
        .alpha
        .map_or_else(String::new, |a| format!(" / {}", fixed(a, 3)));
    format!(
        "oklch({}% {} {}{alpha})",
        fixed(color.l * 100.0, 2),
        fixed(color.c, 4),
        fixed(color.h, 2)
    )
}

const GENERIC_FAMILIES: [&str; 13] = [
    "serif",
    "sans-serif",
    "monospace",
    "cursive",
    "fantasy",
    "system-ui",
    "ui-serif",
    "ui-sans-serif",
    "ui-monospace",
    "ui-rounded",
    "math",
    "emoji",
    "fangsong",
];

fn css_font_stack(stack: &[&str]) -> String {
    stack
        .iter()
        .map(|f| {
            if GENERIC_FAMILIES.contains(f) {
                (*f).to_owned()
            } else {
                format!("\"{f}\"")
            }
        })
        .collect::<Vec<_>>()
        .join(", ")
}

/// The theme as one CSS rule under `selector`: every color token, the learning states as `var()`
/// of their targets, both fonts and the radii. `indent` goes before every line.
pub fn theme_to_css(theme: &Theme, selector: &str, indent: &str) -> String {
    let mut lines: Vec<String> = theme
        .colors()
        .iter()
        .map(|(token, color)| format!("--sc-{token}: {};", css_color(*color)))
        .collect();
    lines.extend(
        LEARNING_ALIASES
            .iter()
            .map(|(state, target)| format!("--sc-{state}: var(--sc-{target});")),
    );
    lines.push(format!("--sc-font-ui: {};", css_font_stack(theme.font_ui)));
    lines.push(format!(
        "--sc-font-read: {};",
        css_font_stack(theme.font_read)
    ));
    lines.extend(
        RADIUS_TOKENS
            .iter()
            .zip(theme.radius)
            .map(|(token, px)| format!("--sc-radius-{token}: {px}px;")),
    );
    let body: Vec<String> = lines.iter().map(|l| format!("{indent}  {l}")).collect();
    format!("{indent}{selector} {{\n{}\n{indent}}}\n", body.join("\n"))
}

/// The OS dark setting applies only when the page chose neither light nor high contrast.
const OS_DARK: &str = r#":not([data-sc-theme="light"]):not([data-sc-theme="high-contrast"])"#;

/// The OS contrast setting selects high contrast when the page chose neither light nor dark.
const OS_CONTRAST: &str = r#":not([data-sc-theme="light"]):not([data-sc-theme="dark"])"#;

/// The brand's stylesheet in `variables.css`'s order (`brandStylesheet` in `stylesheet.ts`):
/// light on `:root` and by attribute, dark by attribute then by OS setting, high contrast by
/// attribute then by OS setting.
pub fn stylesheet(themes: &GeneratedThemes) -> String {
    let media = |query: &str, selector: &str, theme: &Theme| {
        format!(
            "@media {query} {{\n{}}}\n",
            theme_to_css(theme, selector, "  ")
        )
    };
    [
        theme_to_css(&themes.light, ":root,\n[data-sc-theme=\"light\"]", ""),
        theme_to_css(&themes.dark, "[data-sc-theme=\"dark\"]", ""),
        media(
            "(prefers-color-scheme: dark)",
            &format!(":root{OS_DARK}"),
            &themes.dark,
        ),
        theme_to_css(
            &themes.high_contrast,
            "[data-sc-theme=\"high-contrast\"]",
            "",
        ),
        media(
            "(prefers-contrast: more)",
            &format!(":root{OS_CONTRAST}"),
            &themes.high_contrast,
        ),
    ]
    .join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn to_fixed_rounds_an_exact_tie_up_as_javascript_does() {
        assert_eq!(js_to_fixed(0.25, 1), "0.3");
        assert_eq!(js_to_fixed(0.75, 1), "0.8");
        assert_eq!(js_to_fixed(9.995, 2), "9.99"); // 9.995 is below the tie as a double.
        assert_eq!(js_to_fixed(9.96, 1), "10.0");
        assert_eq!(js_to_fixed(-0.0, 2), "0.00");
        assert_eq!(js_to_fixed(195.0, 2), "195.00");
    }

    #[test]
    fn colors_print_as_generate_ts_prints_them() {
        assert_eq!(
            css_color(Oklch {
                alpha: Some(0.45),
                ..Oklch::new(0.22, 0.014, 195.0)
            }),
            "oklch(22% 0.014 195 / 0.45)"
        );
        assert_eq!(css_color(Oklch::new(1.0, 0.0, 0.0)), "oklch(100% 0 0)");
        assert_eq!(
            css_color(Oklch::new(0.5022, 0.0856, 195.13)),
            "oklch(50.22% 0.0856 195.13)"
        );
    }

    #[test]
    fn font_stacks_quote_families_and_not_generic_names() {
        assert_eq!(
            css_font_stack(&["Roboto", "Segoe UI", "system-ui", "sans-serif"]),
            "\"Roboto\", \"Segoe UI\", system-ui, sans-serif"
        );
    }
}
