// SPDX-License-Identifier: AGPL-3.0-or-later
// The theme generator: an organization's brand color in, a theme of color tokens and the UI font
// out (spec 12, B11 to B15, B18, B19). It builds a 12-step OKLCH brand scale and a 12-step
// neutral scale at the brand's hue, gamut-maps every color into sRGB, and picks the brand tokens
// by their WCAG contrast. No DOM or Node dependency: it runs at build time, in the browser and is
// the reference the server's port is held equal to.
import { BrandInputError, contrast, gamutMapSrgb, inSrgb, type Oklch, parseColor } from "./color";
import { variableName } from "./names";
import learning from "./tokens/learning.json";

export type { Oklch } from "./color";
export { BrandInputError } from "./color";

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

/** The appearances every brand has (spec 12, B5, B16). */
export const appearances = ["light", "dark", "high-contrast"] as const;
export type Appearance = (typeof appearances)[number];

/** One appearance's tokens. Colors are gamut-mapped into sRGB and rounded as they are written. */
export interface Theme {
  appearance: Appearance;
  colors: Record<ColorToken, Oklch>;
  font: { ui: readonly string[] };
}

export type GeneratedThemes = Record<Appearance, Theme>;

/**
 * The contrast minimums (spec 12, B20): WCAG 2.2 AA (4.5:1 text, 3:1 UI) in light and dark; in
 * high contrast 7:1 text (AAA) and 4.5:1 UI (Q10).
 */
export const minimums: Record<Appearance, { text: number; ui: number }> = {
  light: { text: 4.5, ui: 3 },
  dark: { text: 4.5, ui: 3 },
  "high-contrast": { text: 7, ui: 4.5 },
};

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

type NeutralToken =
  | "bg-canvas"
  | "bg-raised"
  | "bg-sunken"
  | "line-soft"
  | "line"
  | "line-strong"
  | "fg-disabled"
  | "fg-soft"
  | "fg-sub"
  | "fg-default"
  | "fg-strong";

/** How one appearance is drawn from a brand color. */
interface Recipe {
  /**
   * The brand scale, from `on-brand`'s end: step 0 is `on-brand`, step 1 `brand-soft`, and
   * `brand-hover` is the step after `brand`.
   */
  brandStops: readonly Stop[];
  /** The lightness `brand` aims at, given the input's. */
  brandTarget: (inputL: number) => number;
  /** How many steps past `brand` `brand-fg` aims at. */
  fgOffset: number;
  /** The neutral scale, and each neutral token's step in it. */
  neutralStops: readonly Stop[];
  neutralSteps: Record<NeutralToken, number>;
  /** The modal scrim: a neutral step at this opacity. */
  overlay: { step: number; alpha: number };
  /** Status colors do not follow the brand (B17): `oklch()` lightness, chroma, hue. */
  status: Record<string, [number, number, number]>;
}

/**
 * Light. The brand stops are fitted so the default brand reproduces the plan's light values
 * (spec 12, Q9): on-brand 0.99, brand-soft 0.95, brand 0.50, brand-hover 0.45, brand-fg 0.40;
 * `brand` is the passing step nearest the input's lightness. The neutrals are the plan's, except
 * `line-strong` (0.78 in the plan) and `fg-soft` (0.55), darkened by the least that clears B20's
 * minimums for every brand hue (coordinator decision, spec 12).
 */
const light: Recipe = {
  brandStops: [
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
  ],
  brandTarget: (inputL) => inputL,
  fgOffset: 2,
  neutralStops: [
    { l: 1, c: 0 },
    { l: 0.985, c: 0.004 },
    { l: 0.955, c: 0.007 },
    { l: 0.93, c: 0.006 },
    { l: 0.885, c: 0.009 },
    { l: 0.7, c: 0.008 },
    { l: 0.655, c: 0.012 },
    { l: 0.62, c: 0.009 },
    { l: 0.533, c: 0.01 },
    { l: 0.46, c: 0.012 },
    { l: 0.31, c: 0.013 },
    { l: 0.22, c: 0.014 },
  ],
  neutralSteps: {
    "bg-raised": 0,
    "bg-canvas": 1,
    "bg-sunken": 2,
    "line-soft": 3,
    line: 4,
    "fg-disabled": 5,
    "line-strong": 6,
    "fg-soft": 8,
    "fg-sub": 9,
    "fg-default": 10,
    "fg-strong": 11,
  },
  overlay: { step: 11, alpha: 0.45 },
  status: {
    success: [0.55, 0.12, 150],
    "success-soft": [0.95, 0.03, 150],
    "success-fg": [0.42, 0.1, 150],
    // The plan's 0.72, darkened by the least that clears 3:1 on bg-canvas and bg-raised.
    warning: [0.663, 0.14, 75],
    "warning-soft": [0.96, 0.04, 85],
    "warning-fg": [0.47, 0.1, 65],
    danger: [0.55, 0.18, 27],
    "danger-soft": [0.95, 0.03, 25],
    "danger-fg": [0.47, 0.16, 27],
    info: [0.55, 0.13, 250],
    "info-soft": [0.95, 0.03, 250],
    "info-fg": [0.45, 0.13, 250],
  },
};

/**
 * Dark. The brand scale runs from the dark end, where `on-brand` is; `brand` aims at the plan's
 * dark value, 0.76, whatever the input's lightness, since on a dark canvas the brand must be light.
 * The neutrals are the default dark theme's, with `line-strong` (0.42) lightened by the least that
 * clears 3:1 on bg-canvas and bg-raised.
 */
const dark: Recipe = {
  brandStops: [
    { l: 0.18, c: 0.02 },
    { l: 0.28, c: 0.04 },
    { l: 0.34, c: 0.05 },
    { l: 0.42, c: 0.06 },
    { l: 0.5, c: 0.07 },
    { l: 0.58, c: 0.08 },
    { l: 0.67, c: 0.09 },
    { l: 0.76, c: 0.1 },
    { l: 0.82, c: 0.09 },
    { l: 0.84, c: 0.08 },
    { l: 0.9, c: 0.06 },
    { l: 0.95, c: 0.03 },
  ],
  brandTarget: () => 0.76,
  fgOffset: 2,
  neutralStops: [
    { l: 0.1, c: 0.008 },
    { l: 0.145, c: 0.008 },
    { l: 0.17, c: 0.008 },
    { l: 0.21, c: 0.01 },
    { l: 0.26, c: 0.01 },
    { l: 0.31, c: 0.011 },
    { l: 0.45, c: 0.01 },
    { l: 0.507, c: 0.012 },
    { l: 0.64, c: 0.01 },
    { l: 0.75, c: 0.01 },
    { l: 0.89, c: 0.008 },
    { l: 0.96, c: 0.005 },
  ],
  neutralSteps: {
    "bg-sunken": 1,
    "bg-canvas": 2,
    "bg-raised": 3,
    "line-soft": 4,
    line: 5,
    "fg-disabled": 6,
    "line-strong": 7,
    "fg-soft": 8,
    "fg-sub": 9,
    "fg-default": 10,
    "fg-strong": 11,
  },
  overlay: { step: 0, alpha: 0.6 },
  status: {
    success: [0.74, 0.13, 150],
    "success-soft": [0.27, 0.04, 150],
    "success-fg": [0.82, 0.11, 150],
    warning: [0.8, 0.13, 80],
    "warning-soft": [0.29, 0.05, 80],
    "warning-fg": [0.86, 0.11, 80],
    danger: [0.7, 0.16, 25],
    "danger-soft": [0.28, 0.05, 25],
    "danger-fg": [0.82, 0.11, 25],
    info: [0.74, 0.11, 250],
    "info-soft": [0.28, 0.05, 250],
    "info-fg": [0.84, 0.08, 250],
  },
};

/**
 * High contrast (spec 12, Q10: 7:1 text, 4.5:1 UI). A white canvas and grey neutrals with no
 * brand tint; `brand` aims at the plan's high-contrast value, 0.38, and `brand-fg` at the step
 * after it.
 */
const highContrast: Recipe = {
  brandStops: [
    { l: 1, c: 0 },
    { l: 0.95, c: 0.03 },
    { l: 0.88, c: 0.05 },
    { l: 0.78, c: 0.08 },
    { l: 0.68, c: 0.1 },
    { l: 0.58, c: 0.11 },
    { l: 0.5, c: 0.12 },
    { l: 0.44, c: 0.12 },
    { l: 0.38, c: 0.12 },
    { l: 0.32, c: 0.1 },
    { l: 0.26, c: 0.09 },
    { l: 0.2, c: 0.07 },
  ],
  brandTarget: () => 0.38,
  fgOffset: 1,
  neutralStops: [
    { l: 1, c: 0 },
    { l: 0.96, c: 0 },
    { l: 0.55, c: 0 },
    { l: 0.5, c: 0 },
    { l: 0.38, c: 0 },
    { l: 0.35, c: 0 },
    { l: 0.3, c: 0 },
    { l: 0.25, c: 0 },
    { l: 0.18, c: 0 },
    { l: 0.13, c: 0 },
    { l: 0.06, c: 0 },
    { l: 0, c: 0 },
  ],
  neutralSteps: {
    "bg-canvas": 0,
    "bg-raised": 0,
    "bg-sunken": 1,
    "fg-disabled": 2,
    "line-soft": 3,
    "fg-soft": 4,
    line: 5,
    "fg-sub": 6,
    "line-strong": 7,
    "fg-default": 8,
    "fg-strong": 9,
  },
  overlay: { step: 11, alpha: 0.6 },
  status: {
    success: [0.42, 0.12, 150],
    "success-soft": [0.95, 0.03, 150],
    "success-fg": [0.36, 0.1, 150],
    warning: [0.5, 0.11, 70],
    "warning-soft": [0.96, 0.04, 85],
    "warning-fg": [0.38, 0.08, 65],
    danger: [0.44, 0.17, 27],
    "danger-soft": [0.95, 0.03, 25],
    "danger-fg": [0.38, 0.15, 27],
    info: [0.42, 0.13, 250],
    "info-soft": [0.95, 0.03, 250],
    "info-fg": [0.36, 0.12, 250],
  },
};

const recipes: Record<Appearance, Recipe> = { light, dark, "high-contrast": highContrast };

/** Rounds `x` to `digits` decimals, half away from zero for positive `x`. */
function round(x: number, digits: number): number {
  const scale = 10 ** digits;
  return Math.round(x * scale) / scale;
}

/**
 * The color as it is written: gamut-mapped into sRGB, then lightness and chroma rounded to four
 * decimals and hue to two (a hue that rounds to 360 becomes 0). Chroma then steps down by 0.0001 while
 * rounding has left the color outside sRGB, so every written color is inside it, within 1e-6 per
 * gamma-encoded channel. A color whose chroma rounds to 0 gets hue 0.
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
 * The candidate with the smallest `distance`, the later (higher index) on a tie. Throws when there
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

/** The minimums a brand step choice must clear, and where `brand-fg` aims. */
export interface BrandRule {
  text: number;
  ui: number;
  fgOffset: number;
}

const lightRule: BrandRule = { ...minimums.light, fgOffset: 2 };

/**
 * The brand tokens from a brand scale ordered from `on-brand`'s end (spec 12, B14, B15). `brand`
 * is the step nearest `target` lightness that clears the text minimum against `on-brand`, step 0;
 * the last step is kept for `brand-hover`, one step past `brand`. `brand-fg` is the step nearest
 * `fgOffset` past `brand` that clears the text minimum on `brand-soft` (step 1) and `bg-canvas`;
 * `ring` the step nearest `brand` that clears the UI minimum on `bg-canvas` and `bg-raised`.
 * Exported for its unit tests, which give it scales the generator's stops never produce.
 */
export function brandTokens(
  brandScale: Oklch[],
  target: number,
  canvas: Oklch,
  raised: Oklch,
  rule: BrandRule = lightRule,
) {
  const onBrand = brandScale[0] as Oklch;
  const soft = brandScale[1] as Oklch;
  const step = (i: number) => brandScale[i] as Oklch;
  const all = indices(brandScale.length);
  const brandable = all.slice(0, -1).filter((i) => contrast(step(i), onBrand) >= rule.text);
  const brand = nearest(brandable, (i) => Math.abs(step(i).l - target), "brand");
  const fgOk = all.filter(
    (i) => contrast(step(i), soft) >= rule.text && contrast(step(i), canvas) >= rule.text,
  );
  const ringOk = all.filter(
    (i) => contrast(step(i), canvas) >= rule.ui && contrast(step(i), raised) >= rule.ui,
  );
  return {
    brand: step(brand),
    "brand-hover": step(brand + 1),
    "brand-soft": soft,
    "brand-fg": step(nearest(fgOk, (i) => Math.abs(i - (brand + rule.fgOffset)), "brand-fg")),
    "on-brand": onBrand,
    ring: step(nearest(ringOk, (i) => Math.abs(i - brand), "ring")),
  };
}

function theme(appearance: Appearance, primary: Oklch, font: readonly string[]): Theme {
  const recipe = recipes[appearance];
  // A grey's hue is floating-point noise; below the written chroma precision it is treated as 0.
  const hue = primary.c < 0.0001 ? 0 : primary.h;
  const neutrals = scale(recipe.neutralStops, hue, Math.min(1, primary.c / referenceChroma));
  const brandScale = scale(recipe.brandStops, hue, primary.c / referenceChroma);
  const neutral = Object.fromEntries(
    Object.entries(recipe.neutralSteps).map(([token, i]) => [token, neutrals[i] as Oklch]),
  ) as Record<NeutralToken, Oklch>;
  const status = Object.fromEntries(
    Object.entries(recipe.status).map(([token, [l, c, h]]) => [token, finalize({ l, c, h })]),
  );
  const colors = {
    ...neutral,
    "bg-overlay": { ...(neutrals[recipe.overlay.step] as Oklch), alpha: recipe.overlay.alpha },
    ...brandTokens(
      brandScale,
      recipe.brandTarget(primary.l),
      neutral["bg-canvas"],
      neutral["bg-raised"],
      { ...minimums[appearance], fgOffset: recipe.fgOffset },
    ),
    ...status,
  } as Record<ColorToken, Oklch>;
  return {
    appearance,
    colors: Object.fromEntries(colorTokens.map((t) => [t, colors[t]])) as Theme["colors"],
    font: { ui: font },
  };
}

/**
 * Generates a brand's light, dark and high-contrast themes from the same inputs (B16). Throws
 * `BrandInputError` on a bad input.
 */
export function generate(inputs: BrandInputs): GeneratedThemes {
  const primary = parseColor(inputs.primary, "primary");
  const name = inputs.uiFont ?? "Atkinson Hyperlegible Next";
  if (!Object.hasOwn(uiFonts, name)) {
    throw new BrandInputError("uiFont", `uiFont: "${name}" is not a bundled font`);
  }
  const font = uiFonts[name];
  return {
    light: theme("light", primary, font),
    dark: theme("dark", primary, font),
    "high-contrast": theme("high-contrast", primary, font),
  };
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
export const learningAliases = Object.entries(learning.color).map(([token, { $value }]) => {
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
