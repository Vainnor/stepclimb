// SPDX-License-Identifier: AGPL-3.0-or-later
// An organization's whole theme as one stylesheet (spec 12, B19, B24): its light, dark and
// high-contrast themes under the selectors variables.css gives the default brand, so the
// server's `/theme.css` and variables.css are interchangeable. The server's port writes the same
// bytes (the golden vectors, test/golden.test.ts). No DOM or Node dependency.
import type { GeneratedThemes } from "./generate";
import { themeToCss } from "./generate";

/**
 * The OS dark setting applies only when the page chose neither light nor high contrast; without
 * the second `:not`, this selector's higher specificity would beat `[data-sc-theme="high-contrast"]`.
 */
export const osDark = ':not([data-sc-theme="light"]):not([data-sc-theme="high-contrast"])';

/**
 * The OS contrast setting selects high contrast when the page chose neither light nor dark (spec
 * 12, Q5). It comes after the OS dark rule with the same specificity, so with both OS settings on
 * high contrast wins, as ThemeProvider decides.
 */
export const osContrast = ':not([data-sc-theme="light"]):not([data-sc-theme="dark"])';

/**
 * The brand's stylesheet, in variables.css's order: light on `:root` and by attribute, dark by
 * attribute then by OS setting, high contrast by attribute then by OS setting.
 */
export function brandStylesheet(themes: GeneratedThemes): string {
  const media = (query: string, selector: string, theme: GeneratedThemes[keyof GeneratedThemes]) =>
    `@media ${query} {\n${themeToCss(theme, selector, "  ")}}\n`;
  return [
    themeToCss(themes.light, ':root,\n[data-sc-theme="light"]'),
    themeToCss(themes.dark, '[data-sc-theme="dark"]'),
    media("(prefers-color-scheme: dark)", `:root${osDark}`, themes.dark),
    themeToCss(themes["high-contrast"], '[data-sc-theme="high-contrast"]'),
    media("(prefers-contrast: more)", `:root${osContrast}`, themes["high-contrast"]),
  ].join("\n");
}
