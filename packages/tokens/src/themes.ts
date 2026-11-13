// SPDX-License-Identifier: AGPL-3.0-or-later
// The shipped themes (spec 12, B22, B23): the default brand and the vZDC example, each generated
// in every appearance from its brand inputs, and the build guard that refuses a theme with a
// failing contrast pair.
import { appearances, type BrandInputs, generate, type Theme } from "./generate";
import { type ContrastReport, contrastReport, describeFailures } from "./report";

/**
 * The brands the build generates. `default` is Stepclimb's (ADR 0014: the plan's light brand
 * value); `vzdc` is the plan's example organization brand, which Storybook shows.
 */
export const brands = {
  default: { primary: "oklch(0.5 0.09 195)" },
  vzdc: { primary: "#500E0E", uiFont: "Roboto" },
} as const satisfies Record<string, BrandInputs>;

/** Every shipped theme by its resolver context name, `<brand>-<appearance>`, in build order. */
export function brandThemes(): Map<string, Theme> {
  const themes = new Map<string, Theme>();
  for (const [brand, inputs] of Object.entries(brands)) {
    const generated = generate(inputs);
    for (const appearance of appearances) {
      themes.set(`${brand}-${appearance}`, generated[appearance]);
    }
  }
  return themes;
}

/** A shipped theme fails its contrast report; the message lists every failing pair. */
export class ThemeContrastError extends Error {
  constructor(readonly reports: Map<string, ContrastReport>) {
    const lines = [...reports].flatMap(([name, report]) =>
      describeFailures(report).map((line) => `  ${name}: ${line}`),
    );
    super(`theme contrast: ${lines.length} failing pair(s)\n${lines.join("\n")}`);
    this.name = "ThemeContrastError";
  }
}

/** Throws `ThemeContrastError` when any theme has a failing pair (B22). */
export function checkThemes(themes: Map<string, Theme>): void {
  const failing = new Map<string, ContrastReport>();
  for (const [name, theme] of themes) {
    const report = contrastReport(theme);
    if (!report.pass) failing.set(name, report);
  }
  if (failing.size > 0) throw new ThemeContrastError(failing);
}
