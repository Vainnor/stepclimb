// SPDX-License-Identifier: AGPL-3.0-or-later
// The contrast report (spec 12, B20): every pair of tokens a theme must keep apart, measured with
// the WCAG 2 contrast ratio on the displayed 8-bit sRGB colors. The build refuses a theme with a
// failing pair (B22) and the server will refuse to save one (B21). No DOM or Node dependency, so
// the branding screen's preview runs the same function.
import { contrast, type Oklch } from "./color";
import {
  type Appearance,
  type ColorToken,
  learningAliases,
  minimums,
  type Theme,
} from "./generate";

export { minimums } from "./generate";

/** One measured pair: `foreground` on `background`. */
export interface ContrastPair {
  foreground: string;
  background: string;
  /** The WCAG 2 contrast ratio, 1 to 21, unrounded. */
  ratio: number;
  minimum: number;
  pass: boolean;
}

/** Something a person should look at that does not fail the theme. */
export interface ContrastWarning {
  code: "brand-near-danger";
  message: string;
}

export interface ContrastReport {
  appearance: Appearance;
  /** True when every pair passes; warnings never fail a theme. */
  pass: boolean;
  pairs: ContrastPair[];
  warnings: ContrastWarning[];
}

type Kind = "text" | "ui";
type Pair = readonly [foreground: string, background: string, kind: Kind];

const surfaces = ["bg-canvas", "bg-raised", "bg-sunken"] as const;
const statuses = ["success", "warning", "danger", "info"] as const;
const onCanvasAndRaised = (fg: string, kind: Kind): Pair[] => [
  [fg, "bg-canvas", kind],
  [fg, "bg-raised", kind],
];

/** B20's table, in its order. `fg-disabled` is absent: WCAG exempts disabled controls. */
export const requiredPairs: readonly Pair[] = [
  ...(["fg-strong", "fg-default", "fg-sub", "fg-soft"] as const).flatMap((fg) =>
    surfaces.map((bg): Pair => [fg, bg, "text"]),
  ),
  ["on-brand", "brand", "text"],
  ["on-brand", "brand-hover", "text"],
  ["brand-fg", "brand-soft", "text"],
  ["brand-fg", "bg-canvas", "text"],
  ...statuses.flatMap((s): Pair[] => [
    [`${s}-fg`, `${s}-soft`, "text"],
    [`${s}-fg`, "bg-canvas", "text"],
  ]),
  ...(["brand", "ring", "line-strong"] as const).flatMap((t) => onCanvasAndRaised(t, "ui")),
  ...learningAliases.flatMap(([state]) => onCanvasAndRaised(state, "ui")),
];

const aliasOf = new Map<string, string>(learningAliases);

function colorOf(theme: Theme, token: string): Oklch {
  const target = (aliasOf.get(token) ?? token) as ColorToken;
  const color = theme.colors[target];
  if (!color) throw new Error(`contrast report: the theme has no ${target}`);
  return color;
}

/** Hue difference in degrees, 0 to 180. */
function hueDistance(a: number, b: number): number {
  const d = Math.abs(a - b) % 360;
  return d > 180 ? 360 - d : d;
}

/**
 * Below this hue difference, and above `nearDangerChroma`, the brand reads as the danger color:
 * a learner may take an in-progress module (drawn in the brand) for an overdue one.
 */
const nearDangerHue = 30;
/** A brand greyer than this has no hue a reader would confuse with danger's. */
const nearDangerChroma = 0.04;

function warnings(theme: Theme): ContrastWarning[] {
  const brand = theme.colors.brand;
  const danger = theme.colors.danger;
  const distance = hueDistance(brand.h, danger.h);
  if (brand.c < nearDangerChroma || distance >= nearDangerHue) return [];
  return [
    {
      code: "brand-near-danger",
      message: `brand's hue is ${distance.toFixed(1)} degrees from danger's; in-progress and overdue learning states may look alike`,
    },
  ];
}

/** Measures every required pair of `theme` against its appearance's minimums. */
export function contrastReport(theme: Theme): ContrastReport {
  const min = minimums[theme.appearance];
  const pairs = requiredPairs.map(([foreground, background, kind]) => {
    const ratio = contrast(colorOf(theme, foreground), colorOf(theme, background));
    const minimum = min[kind];
    return { foreground, background, ratio, minimum, pass: ratio >= minimum };
  });
  return {
    appearance: theme.appearance,
    pass: pairs.every((p) => p.pass),
    pairs,
    warnings: warnings(theme),
  };
}

/** The failing pairs as lines: `fg-soft on bg-canvas: 4.40:1, needs 4.5:1`. */
export function describeFailures(report: ContrastReport): string[] {
  return report.pairs
    .filter((p) => !p.pass)
    .map(
      (p) => `${p.foreground} on ${p.background}: ${p.ratio.toFixed(2)}:1, needs ${p.minimum}:1`,
    );
}
