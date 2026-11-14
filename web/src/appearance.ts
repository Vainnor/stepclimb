// SPDX-License-Identifier: AGPL-3.0-or-later
// The user's appearance choice (docs/specs/m0/11-app-shell.md B21, Q6): the theme and density
// stored on the server, which writes them into the page as `data-sc-theme` and `data-sc-density`
// on `<html>` before any script runs. The app sets the same attributes when the choice changes
// and when `/me` loads, so a page the server did not write (the Vite dev server, the Tauri shell)
// follows it too.
import type { components } from "@stepclimb/api-client";
import { api, unwrap } from "./api";

export type UserAppearance = components["schemas"]["UserAppearance"];
export type ThemeChoice = UserAppearance["theme"];
export type DensityChoice = UserAppearance["density"];

export const themeChoices: readonly ThemeChoice[] = ["system", "light", "dark", "high-contrast"];
export const densityChoices: readonly DensityChoice[] = ["comfortable", "compact"];

/**
 * Sets the page's theme and density attributes. `system` and `comfortable` remove them, so the
 * OS settings select the theme and the default control heights apply.
 */
export function applyAppearance(
  { theme, density }: UserAppearance,
  root: HTMLElement = document.documentElement,
): void {
  if (theme === "system") root.removeAttribute("data-sc-theme");
  else root.setAttribute("data-sc-theme", theme);
  if (density === "comfortable") root.removeAttribute("data-sc-density");
  else root.setAttribute("data-sc-density", density);
}

/** Stores the choice on the server, which answers it as saved. */
export async function saveAppearance(appearance: UserAppearance): Promise<UserAppearance> {
  return unwrap(await api.PUT("/api/v1/me/appearance", { body: appearance }));
}
