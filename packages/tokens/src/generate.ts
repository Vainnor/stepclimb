// SPDX-License-Identifier: AGPL-3.0-or-later
// The theme generator: an organization's brand color in, a theme of color tokens and the UI font
// out (spec 12, B11 to B15, B18, B19). It builds a 12-step OKLCH brand scale and a 12-step
// neutral scale at the brand's hue, gamut-maps every color into sRGB, and picks the brand tokens
// by their WCAG contrast. No DOM or Node dependency: it runs at build time, in the browser and is
// the reference the server's port is held equal to. Only the light appearance is generated here.
import {
  ColorInputError,
  contrastRatio,
  gamutMapSrgb,
  inSrgb,
  type Oklch,
  parseColor,
} from "./color";
import { variableName } from "./names";
import learning from "./tokens/learning.json";

export type { Oklch } from "./color";
export { ColorInputError } from "./color";

/** The bundled UI fonts an organization chooses from (spec 12, Q8). */
export const uiFonts = {
  "Atkinson Hyperlegible Next": [
    "Atkinson Hyperlegible Next",
    "Segoe UI",
    "system-ui",
    "sans-serif",
  ],
  Roboto: ["Roboto", "Segoe UI", "system-ui", "sans-serif"],
} as const satisfies Record<string, readonly string[]>;
export type UiFont = keyof typeof uiFonts;

/** The branding inputs that shape tokens (spec 12, B12). */
export interface BrandInputs {
  /** The primary color: a CSS hex color, or `oklch(L C H)`. */
  primary: string;
  /** Defaults to Atkinson Hyperlegible Next. */
  uiFont?: UiFont;
}

/** Every color token a theme carries, in the plan's order (UI tab -> Tokens). */
export const colorTokens = [
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
  ...(["success", "warning", "danger", "info"] as const).flatMap(
    (s) => [s, `${s}-soft`, `${s}-fg`] as const,
  ),
] as const;
export type ColorToken = (typeof colorTokens)[number];

/** One appearance's tokens. Colors are gamut-mapped into sRGB and rounded as they are written. */
export interface Theme {
  colors: Record<ColorToken, Oklch>;
  font: { ui: readonly string[] };
}

export interface GeneratedThemes {
  light: Theme;
}

/** One step of a scale: its lightness, and its chroma for an input of `referenceChroma`. */
interface Stop {
  l: number;
  c: number;
}

/**
 * The chroma the stops are written for: the default brand's, `oklch(0.50 0.09 195)`. Brand stops
 * scale with the input's chroma; neutral stops scale down for a greyer input and never up.
 */
const referenceChroma = 0.09;

/**
 * The light brand scale, lightest first. The stops are fitted so the default brand reproduces the
 * plan's light values (spec 12, Q9): on-brand 0.99, brand-soft 0.95, brand 0.50, brand-hover
 * 0.45, brand-fg 0.40.
 */
const lightBrandStops: readonly Stop[] = [
  { l: 0.99, c: 0.005 },
  { l: 0.95, c: 0.025 },
  { l: 0.9, c: 0.04 },
  { l: 0.83, c: 0.055 },
  { l: 0.75, c: 0.07 },
  { l: 0.67, c: 0.08 },
  { l: 0.58, c: 0.087 },
  { l: 0.5, c: 0.09 },
  { l: 0.45, c: 0.09 },
  { l: 0.4, c: 0.08 },
  { l: 0.31, c: 0.075 },
  { l: 0.26, c: 0.068 },
];

/** The light neutral scale, lightest first, fitted to the default brand's light neutrals. */
const lightNeutralStops: readonly Stop[] = [
  { l: 1, c: 0 },
  { l: 0.985, c: 0.004 },
  { l: 0.955, c: 0.007 },
  { l: 0.93, c: 0.006 },
  { l: 0.885, c: 0.009 },
  { l: 0.78, c: 0.012 },
  { l: 0.7, c: 0.008 },
  { l: 0.62, c: 0.009 },
  { l: 0.55, c: 0.01 },
  { l: 0.46, c: 0.012 },
  { l: 0.31, c: 0.013 },
  { l: 0.22, c: 0.014 },
];

/** The neutral tokens' steps in the light neutral scale. */
const lightNeutralSteps = {
  "bg-raised": 0,
  "bg-canvas": 1,
  "bg-sunken": 2,
  "line-soft": 3,
  line: 4,
  "line-strong": 5,
  "fg-disabled": 6,
  "fg-soft": 8,
  "fg-sub": 9,
  "fg-default": 10,
  "fg-strong": 11,
} as const;

/** The modal scrim: the strongest text color at this opacity. */
const lightOverlayAlpha = 0.45;

/**
 * Status colors do not follow the brand (spec 12, B17): the default theme's values, as `oklch()`
 * lightness, chroma and hue before gamut mapping.
 */
const lightStatus: Record<string, [number, number, number]> = {
  success: [0.55, 0.12, 150],
  "success-soft": [0.95, 0.03, 150],
  "success-fg": [0.42, 0.1, 150],
  warning: [0.72, 0.14, 75],
  "warning-soft": [0.96, 0.04, 85],
  "warning-fg": [0.47, 0.1, 65],
  danger: [0.55, 0.18, 27],
  "danger-soft": [0.95, 0.03, 25],
  "danger-fg": [0.47, 0.16, 27],
  info: [0.55, 0.13, 250],
  "info-soft": [0.95, 0.03, 250],
  "info-fg": [0.45, 0.13, 250],
};

const textContrast = 4.5;
const uiContrast = 3;

/** Rounds `x` to `digits` decimals, half away from zero for positive `x`. */
function round(x: number, digits: number): number {
  const scale = 10 ** digits;
  return Math.round(x * scale) / scale;
}

/**
 * The color as it is written: gamut-mapped into sRGB, then lightness and chroma rounded to four
 * decimals and hue to two. Chroma then steps down by 0.0001 while rounding has left the color
 * outside sRGB, so every written color is inside it. Achromatic colors get hue 0.
 */
export function finalize(color: Oklch): Oklch {
  const mapped = gamutMapSrgb(color);
  const out: Oklch = { l: round(mapped.l, 4), c: round(mapped.c, 4), h: round(mapped.h, 2) };
  if (out.h >= 360) out.h = 0;
  while (out.c > 0 && !inSrgb(out)) out.c = round(out.c - 0.0001, 4);
  if (out.c === 0) out.h = 0;
  if (color.alpha !== undefined && color.alpha < 1) out.alpha = round(color.alpha, 3);
  return out;
}

function scale(stops: readonly Stop[], hue: number, chromaFactor: number): Oklch[] {
  return stops.map((s) => finalize({ l: s.l, c: s.c * chromaFactor, h: hue }));
}

/**
 * The candidate with the smallest `distance`, the darker (higher index) on a tie. Throws when there
 * is none: the stops give every input a candidate, which the property tests check.
 */
function nearest(candidates: number[], distance: (i: number) => number, token: string): number {
  let best: number | undefined;
  for (const i of candidates) {
    if (best === undefined || distance(i) <= distance(best)) best = i;
  }
  if (best === undefined) throw new Error(`generator invariant: no scale step fits ${token}`);
  return best;
}

const indices = (n: number) => Array.from({ length: n }, (_, i) => i);

/**
 * The brand tokens from the brand scale (spec 12, B14, B15). `brand` is the step nearest the
 * input's lightness that clears 4.5:1 against `on-brand`, the scale's light end; the darkest step
 * is kept for `brand-hover`, one step past `brand`. `brand-fg` is the step nearest two past `brand`
 * that clears 4.5:1 on `brand-soft` and `bg-canvas`; `ring` the step nearest `brand` that clears
 * 3:1 on `bg-canvas` and `bg-raised`. Exported for its unit tests, which give it scales the
 * generator's stops never produce.
 */
export function brandTokens(brandScale: Oklch[], inputL: number, canvas: Oklch, raised: Oklch) {
  const onBrand = brandScale[0] as Oklch;
  const soft = brandScale[1] as Oklch;
  const step = (i: number) => brandScale[i] as Oklch;
  const all = indices(brandScale.length);
  const brandable = all.slice(0, -1).filter((i) => contrastRatio(step(i), onBrand) >= textContrast);
  const brand = nearest(brandable, (i) => Math.abs(step(i).l - inputL), "brand");
  const fgOk = all.filter(
    (i) =>
      contrastRatio(step(i), soft) >= textContrast &&
      contrastRatio(step(i), canvas) >= textContrast,
  );
  const ringOk = all.filter(
    (i) =>
      contrastRatio(step(i), canvas) >= uiContrast && contrastRatio(step(i), raised) >= uiContrast,
  );
  return {
    brand: step(brand),
    "brand-hover": step(brand + 1),
    "brand-soft": soft,
    "brand-fg": step(nearest(fgOk, (i) => Math.abs(i - (brand + 2)), "brand-fg")),
    "on-brand": onBrand,
    ring: step(nearest(ringOk, (i) => Math.abs(i - brand), "ring")),
  };
}

function lightTheme(primary: Oklch, font: readonly string[]): Theme {
  // A grey's hue is floating-point noise; below the written chroma precision it is treated as 0.
  const hue = primary.c < 0.0001 ? 0 : primary.h;
  const neutrals = scale(lightNeutralStops, hue, Math.min(1, primary.c / referenceChroma));
  const brandScale = scale(lightBrandStops, hue, primary.c / referenceChroma);
  const neutral = Object.fromEntries(
    Object.entries(lightNeutralSteps).map(([token, i]) => [token, neutrals[i] as Oklch]),
  ) as Record<keyof typeof lightNeutralSteps, Oklch>;
  const status = Object.fromEntries(
    Object.entries(lightStatus).map(([token, [l, c, h]]) => [token, finalize({ l, c, h })]),
  );
  const colors = {
    ...neutral,
    "bg-overlay": { ...neutral["fg-strong"], alpha: lightOverlayAlpha },
    ...brandTokens(brandScale, primary.l, neutral["bg-canvas"], neutral["bg-raised"]),
    ...status,
  } as Record<ColorToken, Oklch>;
  return {
    colors: Object.fromEntries(colorTokens.map((t) => [t, colors[t]])) as Theme["colors"],
    font: { ui: font },
  };
}

/** Generates the themes for a set of branding inputs. Throws `ColorInputError` on a bad input. */
export function generate(inputs: BrandInputs): GeneratedThemes {
  const primary = parseColor(inputs.primary, "primary");
  const name = inputs.uiFont ?? "Atkinson Hyperlegible Next";
  if (!Object.hasOwn(uiFonts, name)) {
    throw new ColorInputError("uiFont", `uiFont: "${name}" is not a bundled font`);
  }
  return { light: lightTheme(primary, uiFonts[name]) };
}

/** The theme as DTCG 2025.10 JSON, the shape of `src/tokens/themes/*.json`. */
export function themeToDtcg(theme: Theme): object {
  const color = Object.fromEntries(
    colorTokens.map((t) => {
      const { l, c, h, alpha } = theme.colors[t];
      const value = {
        colorSpace: "oklch",
        components: [l, c, h],
        ...(alpha === undefined ? {} : { alpha }),
      };
      return [t, { $type: "color", $value: value }];
    }),
  );
  return { color, font: { ui: { $type: "fontFamily", $value: [...theme.font.ui] } } };
}

/** Trims a fixed-point number's trailing zeros: `50.20` -> `50.2`, `195.00` -> `195`. */
function fixed(x: number, digits: number): string {
  return x.toFixed(digits).replace(/\.?0+$/, "");
}

/** `oklch(50.22% 0.0857 195.13)`, with ` / 0.45` when the color is translucent. */
export function cssColor({ l, c, h, alpha }: Oklch): string {
  const a = alpha === undefined ? "" : ` / ${fixed(alpha, 3)}`;
  return `oklch(${fixed(l * 100, 2)}% ${fixed(c, 4)} ${fixed(h, 2)}${a})`;
}

const genericFamilies = new Set([
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
]);

function cssFontStack(stack: readonly string[]): string {
  return stack.map((f) => (genericFamilies.has(f) ? f : `"${f}"`)).join(", ");
}

/** The learning states (`src/tokens/learning.json`): state token -> the color token it aliases. */
const learningAliases = Object.entries(learning.color).map(([token, { $value }]) => {
  const target = /^\{color\.([a-z-]+)\}$/.exec($value)?.[1];
  if (!target) throw new Error(`learning.json: ${token} is not an alias of a color token`);
  return [token, target] as const;
});

/**
 * The theme as one CSS rule under `selector`, with variables.css's names: every color token, the
 * learning states as `var()` of their targets (so they resolve to this theme's colors), and the UI
 * font.
 */
export function themeToCss(theme: Theme, selector: string): string {
  const lines = [
    ...colorTokens.map((t) => `${variableName(`color.${t}`)}: ${cssColor(theme.colors[t])};`),
    ...learningAliases.map(
      ([state, target]) =>
        `${variableName(`color.${state}`)}: var(${variableName(`color.${target}`)});`,
    ),
    `${variableName("font.ui")}: ${cssFontStack(theme.font.ui)};`,
  ];
  return `${selector} {\n${lines.map((l) => `  ${l}`).join("\n")}\n}\n`;
}
