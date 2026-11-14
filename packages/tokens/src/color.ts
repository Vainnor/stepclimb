// SPDX-License-Identifier: AGPL-3.0-or-later
// Color math for the theme generator: parsing a CSS color, OKLCH and sRGB conversion, sRGB gamut
// mapping and the WCAG 2 contrast ratio. Pure functions with no DOM or Node dependency, so the
// generator runs at build time, in the browser and as the reference for the server's port. The
// matrices and the gamut-mapping algorithm are CSS Color 4's (sections 13.2 and 18).

/** A color in OKLCH: lightness 0 to 1, chroma, hue in degrees 0 to 360, optional alpha 0 to 1. */
export interface Oklch {
  l: number;
  c: number;
  h: number;
  alpha?: number;
}

type Vec3 = [number, number, number];
type Mat3 = [Vec3, Vec3, Vec3];

/** An input the generator cannot use; `field` names the input that failed. */
export class BrandInputError extends Error {
  constructor(
    readonly field: string,
    message: string,
  ) {
    super(message);
    this.name = "BrandInputError";
  }
}

const linearSrgbToXyz: Mat3 = [
  [0.41239079926595934, 0.357584339383878, 0.1804807884018343],
  [0.21263900587151027, 0.715168678767756, 0.07219231536073371],
  [0.01933081871559182, 0.11919477979462598, 0.9505321522496607],
];
const xyzToLinearSrgb: Mat3 = [
  [3.2409699419045226, -1.537383177570094, -0.4986107602930034],
  [-0.9692436362808796, 1.8759675015077202, 0.04155505740717559],
  [0.05563007969699366, -0.20397695888897652, 1.0569715142428786],
];
const xyzToLms: Mat3 = [
  [0.819022437996703, 0.3619062600528904, -0.1288737815209879],
  [0.0329836539323885, 0.9292868615863434, 0.0361446663506424],
  [0.0481771893596242, 0.2642395317527308, 0.6335478284694309],
];
const lmsToXyz: Mat3 = [
  [1.2268798758459243, -0.5578149944602171, 0.2813910456659647],
  [-0.0405757452148008, 1.112286803280317, -0.0717110580655164],
  [-0.0763729366746601, -0.4214933324022432, 1.5869240198367816],
];
const lmsToOklab: Mat3 = [
  [0.210454268309314, 0.7936177747023054, -0.0040720430116193],
  [1.9779985324311684, -2.42859224204858, 0.450593709617411],
  [0.0259040424655478, 0.7827717124575296, -0.8086757549230774],
];
const oklabToLms: Mat3 = [
  [1, 0.3963377773761749, 0.2158037573099136],
  [1, -0.1055613458156586, -0.0638541728258133],
  [1, -0.0894841775298119, -1.2914855480194092],
];

function multiply(m: Mat3, v: Vec3): Vec3 {
  return [
    m[0][0] * v[0] + m[0][1] * v[1] + m[0][2] * v[2],
    m[1][0] * v[0] + m[1][1] * v[1] + m[1][2] * v[2],
    m[2][0] * v[0] + m[2][1] * v[1] + m[2][2] * v[2],
  ];
}

/** The sRGB transfer function: gamma-encoded channel to linear light. */
function toLinear(channel: number): number {
  const abs = Math.abs(channel);
  return abs <= 0.04045 ? channel / 12.92 : Math.sign(channel) * ((abs + 0.055) / 1.055) ** 2.4;
}

function toGamma(channel: number): number {
  const abs = Math.abs(channel);
  return abs <= 0.0031308
    ? channel * 12.92
    : Math.sign(channel) * (1.055 * abs ** (1 / 2.4) - 0.055);
}

/** Gamma-encoded sRGB channels (0 to 1 in gamut) to OKLCH. */
export function srgbToOklch(rgb: Vec3): Oklch {
  const lms = multiply(xyzToLms, multiply(linearSrgbToXyz, rgb.map(toLinear) as Vec3));
  const [l, a, b] = multiply(lmsToOklab, lms.map(Math.cbrt) as Vec3);
  const h = (Math.atan2(b, a) * 180) / Math.PI;
  return { l, c: Math.hypot(a, b), h: h < 0 ? h + 360 : h };
}

/** OKLCH to gamma-encoded sRGB channels, unclipped: a channel outside 0 to 1 is out of gamut. */
export function oklchToSrgb(color: Oklch): Vec3 {
  const rad = (color.h * Math.PI) / 180;
  const lab: Vec3 = [color.l, color.c * Math.cos(rad), color.c * Math.sin(rad)];
  const lms = multiply(oklabToLms, lab).map((x) => x ** 3) as Vec3;
  return multiply(xyzToLinearSrgb, multiply(lmsToXyz, lms)).map(toGamma) as Vec3;
}

/**
 * Whether the color is inside sRGB: every gamma-encoded channel within `epsilon` of 0 to 1. The
 * default absorbs the matrices' floating-point error only, which is far below 1e-6.
 */
export function inSrgb(color: Oklch, epsilon = 1e-6): boolean {
  return oklchToSrgb(color).every((x) => x >= -epsilon && x <= 1 + epsilon);
}

function clipToSrgb(color: Oklch): Oklch {
  const rgb = oklchToSrgb(color).map((x) => Math.min(1, Math.max(0, x))) as Vec3;
  return srgbToOklch(rgb);
}

/** Euclidean distance in OKLab (CSS Color 4's deltaEOK). */
function deltaEOK(a: Oklch, b: Oklch): number {
  const ar = (a.h * Math.PI) / 180;
  const br = (b.h * Math.PI) / 180;
  return Math.hypot(
    a.l - b.l,
    a.c * Math.cos(ar) - b.c * Math.cos(br),
    a.c * Math.sin(ar) - b.c * Math.sin(br),
  );
}

/**
 * CSS Color 4's gamut mapping into sRGB: binary search on chroma at constant lightness and hue,
 * stopping when clipping the candidate moves it less than one just-noticeable difference. Colors
 * already inside sRGB come back unchanged; lightness at or beyond either end becomes white or black.
 */
export function gamutMapSrgb(color: Oklch): Oklch {
  const alpha = color.alpha === undefined ? {} : { alpha: color.alpha };
  if (color.l >= 1) return { l: 1, c: 0, h: 0, ...alpha };
  if (color.l <= 0) return { l: 0, c: 0, h: 0, ...alpha };
  if (inSrgb(color, 0)) return { ...color };
  const jnd = 0.02;
  const epsilon = 0.0001;
  let min = 0;
  let max = color.c;
  let minInGamut = true;
  const current = { l: color.l, c: color.c, h: color.h };
  let clipped = clipToSrgb(current);
  if (deltaEOK(clipped, current) < jnd) return { ...clipped, ...alpha };
  while (max - min > epsilon) {
    current.c = (min + max) / 2;
    if (minInGamut && inSrgb(current, 0)) {
      min = current.c;
      continue;
    }
    clipped = clipToSrgb(current);
    const e = deltaEOK(clipped, current);
    if (e >= jnd) {
      max = current.c;
    } else if (jnd - e < epsilon) {
      break;
    } else {
      minInGamut = false;
      min = current.c;
    }
  }
  return { ...clipped, ...alpha };
}

/**
 * The 8-bit sRGB color a display shows for `color`: each gamma-encoded channel clipped to 0 to 1
 * and rounded to the nearest of 256 levels, returned as 0 to 1.
 */
export function toSrgb8(color: Oklch): Vec3 {
  return oklchToSrgb(color).map((x) => Math.round(Math.min(1, Math.max(0, x)) * 255) / 255) as Vec3;
}

/** WCAG 2 relative luminance of the 8-bit sRGB color displayed for `color`. */
function relativeLuminance(color: Oklch): number {
  const [r, g, b] = toSrgb8(color).map(toLinear) as Vec3;
  return 0.2126 * r + 0.7152 * g + 0.0722 * b;
}

/**
 * WCAG 2 contrast ratio between two opaque colors, 1 to 21, order-independent. Both colors are
 * measured as displayed: quantized to 8-bit sRGB first (`toSrgb8`).
 */
export function contrast(a: Oklch, b: Oklch): number {
  const la = relativeLuminance(a);
  const lb = relativeLuminance(b);
  return (Math.max(la, lb) + 0.05) / (Math.min(la, lb) + 0.05);
}

/**
 * The largest chroma an input may have. sRGB's most saturated colors are near 0.32 and P3's near
 * 0.37; a larger chroma only makes gamut mapping search longer, and a huge one leaves no scale
 * step that clears its contrast.
 */
export const maxChroma = 0.5;

/** CSS whitespace (CSS Syntax 3, after preprocessing): ASCII space, tab, LF, CR and form feed. */
const ws = "[ \\t\\n\\r\\f]";
/** A CSS `<number>` (CSS Syntax 3): `1`, `-0.5`, `.5`, `5e-1`; not `1.` or `1.e2`. */
const number = String.raw`[+-]?(?:\d+(?:\.\d+)?|\.\d+)(?:[eE][+-]?\d+)?`;
const hexPattern = new RegExp(`^${ws}*#([0-9a-f]{3}|[0-9a-f]{6})${ws}*$`, "i");
const oklchPattern = new RegExp(
  `^${ws}*oklch\\(${ws}*(${number})(%?)${ws}+(${number})(%?)${ws}+(${number})(?:deg)?${ws}*\\)${ws}*$`,
  "i",
);

/**
 * Parses an opaque CSS color. The grammar, case-insensitive, with `ws` the CSS whitespace above and
 * `number` a CSS `<number>`:
 *
 *     color = ws* (hex | oklch) ws*
 *     hex   = "#" (3 | 6) hex digits
 *     oklch = "oklch(" ws* number "%"? ws+ number "%"? ws+ number "deg"? ws* ")"
 *
 * Lightness is a number 0 to 1 or a percentage; chroma a number at least 0 or a percentage of 0.4
 * (CSS Color 4), at most `maxChroma`; hue in degrees, finite, normalized to 0 to 360. Anything
 * else, including alpha, `none` and other color functions, is a `BrandInputError` naming `field`.
 */
export function parseColor(input: string, field: string): Oklch {
  const text = input;
  const hex = hexPattern.exec(text)?.[1];
  if (hex) {
    const full = hex.length === 3 ? [...hex].map((d) => d + d).join("") : hex;
    const rgb = [0, 2, 4].map((i) => Number.parseInt(full.slice(i, i + 2), 16) / 255) as Vec3;
    return srgbToOklch(rgb);
  }
  const m = oklchPattern.exec(text);
  if (m) {
    const l = Number(m[1]) / (m[2] ? 100 : 1);
    const c = (Number(m[3]) * (m[4] ? 0.4 : 1)) / (m[4] ? 100 : 1);
    const h = ((Number(m[5]) % 360) + 360) % 360;
    // A hue too large for a double reads as Infinity, then NaN; NaN fails every comparison.
    if (!(l >= 0 && l <= 1 && c >= 0 && c <= maxChroma && Number.isFinite(h))) {
      throw new BrandInputError(
        field,
        `${field}: lightness must be 0 to 1, chroma 0 to ${maxChroma}, and hue finite`,
      );
    }
    return { l, c, h };
  }
  throw new BrandInputError(field, `${field}: expected a hex color or oklch(), got "${input}"`);
}
