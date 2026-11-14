// SPDX-License-Identifier: AGPL-3.0-or-later
//! Color math for the theme generator: parsing a CSS color, OKLCH and sRGB conversion, sRGB gamut
//! mapping and the WCAG 2 contrast ratio. The port of `packages/tokens/src/color.ts`, operation
//! for operation, so both give the same doubles; the golden vectors hold them equal (spec 12,
//! B24). The matrices and the gamut-mapping algorithm are CSS Color 4's (sections 13.2 and 18).

use std::f64::consts::PI;

use super::{InputError, InputRule};

/// A color in OKLCH: lightness 0 to 1, chroma, hue in degrees 0 to 360, optional alpha 0 to 1.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Oklch {
    pub l: f64,
    pub c: f64,
    pub h: f64,
    pub alpha: Option<f64>,
}

impl Oklch {
    /// An opaque color.
    pub const fn new(l: f64, c: f64, h: f64) -> Self {
        Self {
            l,
            c,
            h,
            alpha: None,
        }
    }
}

type Vec3 = [f64; 3];
type Mat3 = [Vec3; 3];

const LINEAR_SRGB_TO_XYZ: Mat3 = [
    [0.41239079926595934, 0.357584339383878, 0.1804807884018343],
    [0.21263900587151027, 0.715168678767756, 0.07219231536073371],
    [0.01933081871559182, 0.11919477979462598, 0.9505321522496607],
];
const XYZ_TO_LINEAR_SRGB: Mat3 = [
    [3.2409699419045226, -1.537383177570094, -0.4986107602930034],
    [-0.9692436362808796, 1.8759675015077202, 0.04155505740717559],
    [
        0.05563007969699366,
        -0.20397695888897652,
        1.0569715142428786,
    ],
];
const XYZ_TO_LMS: Mat3 = [
    [0.819022437996703, 0.3619062600528904, -0.1288737815209879],
    [0.0329836539323885, 0.9292868615863434, 0.0361446663506424],
    [0.0481771893596242, 0.2642395317527308, 0.6335478284694309],
];
const LMS_TO_XYZ: Mat3 = [
    [1.2268798758459243, -0.5578149944602171, 0.2813910456659647],
    [-0.0405757452148008, 1.112286803280317, -0.0717110580655164],
    [-0.0763729366746601, -0.4214933324022432, 1.5869240198367816],
];
const LMS_TO_OKLAB: Mat3 = [
    [0.210454268309314, 0.7936177747023054, -0.0040720430116193],
    [1.9779985324311684, -2.42859224204858, 0.450593709617411],
    [0.0259040424655478, 0.7827717124575296, -0.8086757549230774],
];
const OKLAB_TO_LMS: Mat3 = [
    [1.0, 0.3963377773761749, 0.2158037573099136],
    [1.0, -0.1055613458156586, -0.0638541728258133],
    [1.0, -0.0894841775298119, -1.2914855480194092],
];

fn multiply(m: &Mat3, v: Vec3) -> Vec3 {
    [
        m[0][0] * v[0] + m[0][1] * v[1] + m[0][2] * v[2],
        m[1][0] * v[0] + m[1][1] * v[1] + m[1][2] * v[2],
        m[2][0] * v[0] + m[2][1] * v[1] + m[2][2] * v[2],
    ]
}

/// JavaScript's `Math.sign` for a non-zero, non-NaN number.
fn sign(x: f64) -> f64 {
    if x < 0.0 { -1.0 } else { 1.0 }
}

/// JavaScript's `Math.hypot` as V8 computes it: the values scaled by the largest, summed with
/// Kahan compensation, then scaled back. It differs from C's `hypot` in the last bit.
pub(crate) fn js_hypot(values: &[f64]) -> f64 {
    let max = values.iter().fold(0.0_f64, |max, v| max.max(v.abs()));
    if max == 0.0 {
        return 0.0;
    }
    let mut sum = 0.0;
    let mut compensation = 0.0;
    for v in values {
        let n = v.abs() / max;
        let summand = n * n - compensation;
        let preliminary = sum + summand;
        compensation = (preliminary - sum) - summand;
        sum = preliminary;
    }
    sum.sqrt() * max
}

/// JavaScript's `Math.round`: the nearest integer, ties toward positive infinity.
pub(crate) fn js_round(x: f64) -> f64 {
    let floor = x.floor();
    if x - floor >= 0.5 { floor + 1.0 } else { floor }
}

/// The sRGB transfer function: gamma-encoded channel to linear light.
fn to_linear(channel: f64) -> f64 {
    let abs = channel.abs();
    if abs <= 0.04045 {
        channel / 12.92
    } else {
        sign(channel) * ((abs + 0.055) / 1.055).powf(2.4)
    }
}

fn to_gamma(channel: f64) -> f64 {
    let abs = channel.abs();
    if abs <= 0.0031308 {
        channel * 12.92
    } else {
        sign(channel) * (1.055 * abs.powf(1.0 / 2.4) - 0.055)
    }
}

/// Gamma-encoded sRGB channels (0 to 1 in gamut) to OKLCH.
pub fn srgb_to_oklch(rgb: Vec3) -> Oklch {
    let lms = multiply(
        &XYZ_TO_LMS,
        multiply(&LINEAR_SRGB_TO_XYZ, rgb.map(to_linear)),
    );
    let [l, a, b] = multiply(&LMS_TO_OKLAB, lms.map(f64::cbrt));
    let h = (b.atan2(a) * 180.0) / PI;
    Oklch::new(l, js_hypot(&[a, b]), if h < 0.0 { h + 360.0 } else { h })
}

/// OKLCH to gamma-encoded sRGB channels, unclipped: a channel outside 0 to 1 is out of gamut.
pub fn oklch_to_srgb(color: Oklch) -> Vec3 {
    let rad = (color.h * PI) / 180.0;
    let lab = [color.l, color.c * rad.cos(), color.c * rad.sin()];
    let lms = multiply(&OKLAB_TO_LMS, lab).map(|x| x.powf(3.0));
    multiply(&XYZ_TO_LINEAR_SRGB, multiply(&LMS_TO_XYZ, lms)).map(to_gamma)
}

/// Whether the color is inside sRGB: every gamma-encoded channel within `epsilon` of 0 to 1.
/// [`IN_SRGB_EPSILON`] absorbs the matrices' floating-point error only.
pub fn in_srgb(color: Oklch, epsilon: f64) -> bool {
    oklch_to_srgb(color)
        .iter()
        .all(|&x| x >= -epsilon && x <= 1.0 + epsilon)
}

/// The default tolerance of [`in_srgb`], far above the matrices' floating-point error.
pub const IN_SRGB_EPSILON: f64 = 1e-6;

fn clip_to_srgb(color: Oklch) -> Oklch {
    srgb_to_oklch(oklch_to_srgb(color).map(|x| x.clamp(0.0, 1.0)))
}

/// Euclidean distance in OKLab (CSS Color 4's deltaEOK).
fn delta_eok(a: Oklch, b: Oklch) -> f64 {
    let ar = (a.h * PI) / 180.0;
    let br = (b.h * PI) / 180.0;
    js_hypot(&[
        a.l - b.l,
        a.c * ar.cos() - b.c * br.cos(),
        a.c * ar.sin() - b.c * br.sin(),
    ])
}

/// CSS Color 4's gamut mapping into sRGB: binary search on chroma at constant lightness and hue,
/// stopping when clipping the candidate moves it less than one just-noticeable difference.
/// Colors already inside sRGB come back unchanged; lightness at or beyond either end becomes
/// white or black.
pub fn gamut_map_srgb(color: Oklch) -> Oklch {
    let alpha = color.alpha;
    if color.l >= 1.0 {
        return Oklch {
            alpha,
            ..Oklch::new(1.0, 0.0, 0.0)
        };
    }
    if color.l <= 0.0 {
        return Oklch {
            alpha,
            ..Oklch::new(0.0, 0.0, 0.0)
        };
    }
    if in_srgb(color, 0.0) {
        return color;
    }
    const JND: f64 = 0.02;
    const EPSILON: f64 = 0.0001;
    let mut min = 0.0;
    let mut max = color.c;
    let mut min_in_gamut = true;
    let mut current = Oklch::new(color.l, color.c, color.h);
    let mut clipped = clip_to_srgb(current);
    if delta_eok(clipped, current) < JND {
        return Oklch { alpha, ..clipped };
    }
    while max - min > EPSILON {
        current.c = (min + max) / 2.0;
        if min_in_gamut && in_srgb(current, 0.0) {
            min = current.c;
            continue;
        }
        clipped = clip_to_srgb(current);
        let e = delta_eok(clipped, current);
        if e >= JND {
            max = current.c;
        } else if JND - e < EPSILON {
            break;
        } else {
            min_in_gamut = false;
            min = current.c;
        }
    }
    Oklch { alpha, ..clipped }
}

/// The 8-bit sRGB color a display shows for `color`: each gamma-encoded channel clipped to 0 to
/// 1 and rounded to the nearest of 256 levels, returned as 0 to 1.
pub fn to_srgb8(color: Oklch) -> Vec3 {
    oklch_to_srgb(color).map(|x| js_round(x.clamp(0.0, 1.0) * 255.0) / 255.0)
}

/// WCAG 2 relative luminance of the 8-bit sRGB color displayed for `color`.
fn relative_luminance(color: Oklch) -> f64 {
    let [r, g, b] = to_srgb8(color).map(to_linear);
    0.2126 * r + 0.7152 * g + 0.0722 * b
}

/// WCAG 2 contrast ratio between two opaque colors, 1 to 21, order-independent. Both colors are
/// measured as displayed: quantized to 8-bit sRGB first ([`to_srgb8`]).
pub fn contrast(a: Oklch, b: Oklch) -> f64 {
    let la = relative_luminance(a);
    let lb = relative_luminance(b);
    (la.max(lb) + 0.05) / (la.min(lb) + 0.05)
}

/// The largest chroma an input may have (`maxChroma` in `color.ts`). sRGB's most saturated
/// colors are near 0.32 and P3's near 0.37; a larger chroma only makes gamut mapping search
/// longer, and a huge one leaves no scale step that clears its contrast.
pub const MAX_CHROMA: f64 = 0.5;

/// CSS whitespace (CSS Syntax 3, after preprocessing): ASCII space, tab, LF, CR and form feed.
pub(crate) fn is_css_space(c: char) -> bool {
    matches!(c, ' ' | '\t' | '\n' | '\r' | '\x0c')
}

/// Parses an opaque CSS color. The grammar, case-insensitive, with `ws` CSS whitespace and
/// `number` a CSS `<number>` (`1`, `-0.5`, `.5`, `5e-1`; not `1.` or `1.e2`):
///
/// ```text
/// color = ws* (hex | oklch) ws*
/// hex   = "#" (3 | 6) hex digits
/// oklch = "oklch(" ws* number "%"? ws+ number "%"? ws+ number "deg"? ws* ")"
/// ```
///
/// Lightness is a number 0 to 1 or a percentage; chroma a number at least 0 or a percentage of
/// 0.4 (CSS Color 4), at most [`MAX_CHROMA`]; hue in degrees, finite, normalized to 0 to 360.
/// Anything else, including alpha, `none` and other color functions, is an [`InputError`]
/// naming `field`.
pub fn parse_color(input: &str, field: &'static str) -> Result<Oklch, InputError> {
    let error = || InputError {
        field,
        rule: InputRule::InvalidColor,
    };
    let text = input.trim_matches(is_css_space);
    if let Some(hex) = text.strip_prefix('#') {
        return parse_hex(hex).ok_or_else(error);
    }
    let (l, c, h) = parse_oklch(text).ok_or_else(error)?;
    let h = ((h % 360.0) + 360.0) % 360.0;
    // A hue too large for a double reads as infinity, then NaN; NaN fails every comparison.
    if !((0.0..=1.0).contains(&l) && (0.0..=MAX_CHROMA).contains(&c) && h.is_finite()) {
        return Err(error());
    }
    Ok(Oklch::new(l, c, h))
}

fn parse_hex(hex: &str) -> Option<Oklch> {
    if !(hex.len() == 3 || hex.len() == 6) || !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    let full: String = if hex.len() == 3 {
        hex.chars().flat_map(|d| [d, d]).collect()
    } else {
        hex.to_owned()
    };
    let channel = |i: usize| u8::from_str_radix(&full[i..i + 2], 16).map(|v| f64::from(v) / 255.0);
    Some(srgb_to_oklch([
        channel(0).ok()?,
        channel(2).ok()?,
        channel(4).ok()?,
    ]))
}

/// The three components of `oklch(...)` as given: lightness 0 to 1 and chroma, each with its
/// percentage applied, and hue in degrees, not yet normalized.
fn parse_oklch(text: &str) -> Option<(f64, f64, f64)> {
    let prefix = text.get(..6)?;
    if !prefix.eq_ignore_ascii_case("oklch(") {
        return None;
    }
    let inner = text[6..].strip_suffix(')')?;
    let mut parts = inner.split(is_css_space).filter(|p| !p.is_empty());
    let (l, l_percent) = number_with_suffix(parts.next()?, "%")?;
    let (c, c_percent) = number_with_suffix(parts.next()?, "%")?;
    let (h, _) = number_with_suffix(parts.next()?, "deg")?;
    if parts.next().is_some() {
        return None;
    }
    let l = l / if l_percent { 100.0 } else { 1.0 };
    let c = (c * if c_percent { 0.4 } else { 1.0 }) / if c_percent { 100.0 } else { 1.0 };
    Some((l, c, h))
}

/// A CSS `<number>` with an optional, case-insensitive `suffix`, and whether the suffix was there.
fn number_with_suffix(part: &str, suffix: &str) -> Option<(f64, bool)> {
    let split = part.len().checked_sub(suffix.len());
    let (number, has_suffix) = match split.and_then(|at| part.get(at..).map(|s| (at, s))) {
        Some((at, end)) if end.eq_ignore_ascii_case(suffix) => (&part[..at], true),
        _ => (part, false),
    };
    is_css_number(number)
        .then(|| number.parse().ok())
        .flatten()
        .map(|n| (n, has_suffix))
}

/// Whether `text` is a CSS `<number>`: `[+-]? (digits ("." digits)? | "." digits) ([eE] [+-]? digits)?`.
fn is_css_number(text: &str) -> bool {
    let digits = |s: &str| -> usize { s.bytes().take_while(u8::is_ascii_digit).count() };
    let rest = text.strip_prefix(['+', '-']).unwrap_or(text);
    let whole = digits(rest);
    let mut rest = &rest[whole..];
    if let Some(after) = rest.strip_prefix('.') {
        let fraction = digits(after);
        if fraction == 0 {
            return false;
        }
        rest = &after[fraction..];
    } else if whole == 0 {
        return false;
    }
    if let Some(after) = rest.strip_prefix(['e', 'E']) {
        let after = after.strip_prefix(['+', '-']).unwrap_or(after);
        let exponent = digits(after);
        return exponent > 0 && exponent == after.len();
    }
    rest.is_empty()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn js_round_rounds_half_up_and_keeps_the_largest_double_below_a_half() {
        assert_eq!(js_round(2.5), 3.0);
        assert_eq!(js_round(-2.5), -2.0);
        assert_eq!(js_round(0.499_999_999_999_999_94), 0.0);
        assert_eq!(js_round(1.4), 1.0);
    }

    #[test]
    fn js_hypot_is_zero_for_zeros_and_exact_for_a_right_triangle() {
        assert_eq!(js_hypot(&[0.0, 0.0]), 0.0);
        assert_eq!(js_hypot(&[3.0, -4.0]), 5.0);
    }

    #[test]
    fn white_and_black_have_full_and_no_lightness() {
        let white = srgb_to_oklch([1.0, 1.0, 1.0]);
        assert!((white.l - 1.0).abs() < 1e-9 && white.c < 1e-6, "{white:?}");
        assert_eq!(srgb_to_oklch([0.0, 0.0, 0.0]).l, 0.0);
        assert!((contrast(white, Oklch::new(0.0, 0.0, 0.0)) - 21.0).abs() < 1e-12);
    }

    #[test]
    fn gamut_mapping_brings_a_color_inside_srgb_and_keeps_one_inside() {
        let vivid = Oklch::new(0.7, 0.4, 145.0);
        assert!(!in_srgb(vivid, 0.0));
        assert!(in_srgb(gamut_map_srgb(vivid), IN_SRGB_EPSILON));
        let grey = Oklch::new(0.5, 0.0, 0.0);
        assert_eq!(gamut_map_srgb(grey), grey);
    }

    #[test]
    fn parses_hex_and_oklch_in_every_form_the_grammar_takes() {
        let red = parse_color("#f00", "primary").unwrap();
        assert_eq!(red, parse_color(" #FF0000\n", "primary").unwrap());
        let percent = parse_color("OKLCH(50% 25% 400deg)", "primary").unwrap();
        assert_eq!((percent.l, percent.c), (0.5, 0.1));
        assert!((percent.h - 40.0).abs() < 1e-9);
        assert_eq!(parse_color("oklch(0.5 0.5 10)", "primary").unwrap().c, 0.5);
        assert_eq!(parse_color("oklch(0.5 125% 10)", "primary").unwrap().c, 0.5);
        let short = parse_color("oklch(.5 +1e-1 -30)", "primary").unwrap();
        assert_eq!((short.l, short.c, short.h), (0.5, 0.1, 330.0));
    }

    #[test]
    fn refuses_what_the_grammar_does_not_take_naming_the_field() {
        for bad in [
            "",
            "dark red",
            "#12345",
            "#500E0E80",
            "#ggg",
            "oklch(1.2 0.1 10)",
            "oklch(0.5 -0.1 10)",
            "oklch(0.5 0.1 10 / 0.5)",
            "oklch(0.5 1e400 10)",
            "oklch(0.5 0.5001 10)",
            "oklch(0.5 126% 10)",
            "oklch(0.5 1e150 10)",
            "oklch(0.5 0.1 1e400)",
            "oklch(1. 0.1 10)",
            "oklch(0.5 0.1)",
            "oklch(0.5 0.1 10",
            "oklch( 0.5 0.1 10 )x",
            "oklch(0.5 0.1 10deg%)",
            "oklch(0.5% % 10)",
            "oklch(nan 0.1 10)",
            "oklch(inf 0.1 10)",
            "rgb(1 2 3)",
        ] {
            assert_eq!(
                parse_color(bad, "primary"),
                Err(InputError {
                    field: "primary",
                    rule: InputRule::InvalidColor
                }),
                "{bad:?}"
            );
        }
    }
}
