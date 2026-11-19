// SPDX-License-Identifier: AGPL-3.0-or-later
// The brand the branding screen is editing, built in the browser with the theme generator the
// server's port is held equal to (docs/specs/m0/12-tokens-and-theme-generator.md B11, B21, B24):
// its light, dark and high-contrast themes and their contrast reports, so the screen shows what a
// save would give before Save is pressed. The server checks the same again when it saves.
import { gamutMapSrgb, parseColor, toSrgb8 } from "@stepclimb/tokens/color";
import {
  appearances,
  BrandInputError,
  type GeneratedThemes,
  generate,
} from "@stepclimb/tokens/generate";
import { type ContrastReport, contrastReport } from "@stepclimb/tokens/report";
import type { SavedInputs } from "./branding";

export type Draft =
  | { ok: true; themes: GeneratedThemes; reports: ContrastReport[]; pass: boolean }
  | { ok: false };

/** The draft brand of `inputs`, or `{ ok: false }` when the generator refuses an input. */
export function brandDraft(inputs: SavedInputs): Draft {
  try {
    const themes = generate({
      primary: inputs.primary,
      uiFont: inputs.ui_font,
      readingFont: inputs.reading_font,
      cornerStyle: inputs.corner_style,
    });
    const reports = appearances.map((appearance) => contrastReport(themes[appearance]));
    return { ok: true, themes, reports, pass: reports.every((report) => report.pass) };
  } catch (error) {
    if (error instanceof BrandInputError) return { ok: false };
    throw error;
  }
}

/**
 * `primary` as `#RRGGBB` for the color field, which takes hex only; `oklch()` (as the default
 * brand is written) is mapped into sRGB first. Undefined for a value the generator refuses.
 */
export function hexOf(primary: string): string | undefined {
  try {
    const rgb = toSrgb8(gamutMapSrgb(parseColor(primary, "primary")));
    return `#${rgb
      .map((channel) =>
        Math.round(channel * 255)
          .toString(16)
          .padStart(2, "0"),
      )
      .join("")}`;
  } catch (error) {
    if (error instanceof BrandInputError) return undefined;
    throw error;
  }
}
