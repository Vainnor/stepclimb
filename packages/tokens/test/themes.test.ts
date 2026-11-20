// SPDX-License-Identifier: AGPL-3.0-or-later
// The shipped themes come from the generator (spec 12, B22, B23; T10): the build writes exactly
// the generator's colors for each brand and appearance, and refuses to build when a theme has a
// failing contrast pair.
import { beforeAll, describe, expect, it } from "vitest";
import { appearances, colorTokens, cssColor, generate, type Theme } from "../src/generate";
import { contrastReport } from "../src/report";
import { brands, brandThemes, checkThemes, ThemeContrastError } from "../src/themes";
import { buildTokens, tokensDir } from "./support/build";
import { computed, type Element, parseRules, type Rule } from "./support/cascade";

let rules: Rule[];

beforeAll(async () => {
  rules = parseRules((await buildTokens()).variables);
});

const withColor = (theme: Theme, token: keyof Theme["colors"], l: number): Theme => ({
  ...theme,
  colors: { ...theme.colors, [token]: { ...theme.colors[token], l } },
});

describe("the shipped themes (B23)", () => {
  it("are the default and vZDC brands in every appearance", () => {
    expect([...brandThemes().keys()]).toEqual(
      ["default", "vzdc"].flatMap((b) => appearances.map((a) => `${b}-${a}`)),
    );
  });

  const elements: Record<string, (appearance: string) => Element> = {
    default: (a) => ({ root: true, attrs: { "data-sc-theme": a } }),
    vzdc: (a) => ({ root: true, attrs: { "data-sc-brand": "vzdc", "data-sc-theme": a } }),
  };

  it.each(Object.keys(brands))("put the generator's %s colors in variables.css", (brand) => {
    const generated = generate(brands[brand as keyof typeof brands]);
    for (const appearance of appearances) {
      const el = elements[brand]?.(appearance);
      if (!el) throw new Error(`no element for ${brand}`);
      for (const dark of [false, true]) {
        const values = computed(rules, [el], { dark, contrast: !dark });
        for (const token of colorTokens) {
          expect(values.get(`--sc-${token}`), `${brand} ${appearance} ${token}`).toBe(
            cssColor(generated[appearance].colors[token]),
          );
        }
      }
    }
  });

  it("write no color-gamut overrides, since every generated color is inside sRGB", async () => {
    expect((await buildTokens()).variables).not.toContain("color-gamut");
  });
});

describe("callout body text on the status tints (B20)", () => {
  it.each([...brandThemes()])(
    "keeps fg-default at its text minimum on every tint in %s",
    (_, theme) => {
      const report = contrastReport(theme);
      for (const status of ["success", "warning", "danger", "info"]) {
        const pair = report.pairs.find(
          (p) => p.foreground === "fg-default" && p.background === `${status}-soft`,
        );
        expect(pair, status).toBeDefined();
        expect(pair?.pass, `${status}: ${pair?.ratio}`).toBe(true);
      }
    },
  );
});

describe("the build guard (B22, T10)", () => {
  it("passes the shipped themes", () => {
    expect(() => checkThemes(brandThemes())).not.toThrow();
  });

  it("fails the build when a shipped theme has a failing pair, naming it", async () => {
    const themes = brandThemes();
    const light = themes.get("vzdc-light");
    if (!light) throw new Error("no vzdc-light");
    // fg-soft at the lightness of fg-disabled, about 3:1 on the canvas.
    themes.set("vzdc-light", withColor(light, "fg-soft", 0.7));
    const build = buildTokens(tokensDir, themes);
    await expect(build).rejects.toThrow(ThemeContrastError);
    await expect(build).rejects.toThrow(
      /vzdc-light: fg-soft on bg-canvas: \d\.\d\d:1, needs 4\.5:1/,
    );
  });

  it("fails the build when a status tint leaves a callout's body text below 4.5:1", async () => {
    const themes = brandThemes();
    const dark = themes.get("default-dark");
    if (!dark) throw new Error("no default-dark");
    // A warning tint as light as the dark theme's fg-sub: fg-default on it is near 1:1.
    themes.set("default-dark", withColor(dark, "warning-soft", 0.75));
    await expect(buildTokens(tokensDir, themes)).rejects.toThrow(
      /default-dark: fg-default on warning-soft: \d\.\d\d:1, needs 4\.5:1/,
    );
  });

  it("measures high contrast at its own minimums", () => {
    const themes = brandThemes();
    const hc = themes.get("default-high-contrast");
    if (!hc) throw new Error("no default-high-contrast");
    // fg-soft at 0.5 clears 4.5:1 on white but not 7:1.
    themes.set("default-high-contrast", withColor(hc, "fg-soft", 0.5));
    expect(() => checkThemes(themes)).toThrow(
      /default-high-contrast: fg-soft on bg-canvas: \d\.\d\d:1, needs 7:1/,
    );
  });

  it("fails when the resolver's theme contexts are not the generated themes", async () => {
    const themes = brandThemes();
    themes.delete("vzdc-dark");
    await expect(buildTokens(tokensDir, themes)).rejects.toThrow(
      /the theme contexts \(.*vzdc-dark.*\) are not the generated themes/,
    );
  });
});
