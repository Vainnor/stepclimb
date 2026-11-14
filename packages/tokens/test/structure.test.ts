// SPDX-License-Identifier: AGPL-3.0-or-later
// The two halves of a page whose theme comes from the server (spec 11 B20, Q5): `/theme.css`
// (here the default brand's, build/css/theme.css) sets the colors, fonts and radii, and the
// bundle's structure.css sets everything else. Together they give every element what
// variables.css gives it, and neither declares a variable the other does, so the bundle, which
// loads after `/theme.css`, can never override an organization's theme.
import { beforeAll, describe, expect, it } from "vitest";
import { generate } from "../src/generate";
import { brandStylesheet } from "../src/stylesheet";
import { brands } from "../src/themes";
import { type Built, buildTokens } from "./support/build";
import { computed, parseRules } from "./support/cascade";

let built: Built;

beforeAll(async () => {
  built = await buildTokens();
});

const declared = (css: string) => new Set(parseRules(css).flatMap((r) => [...r.decls.keys()]));

const themes = [undefined, "light", "dark", "high-contrast"];
const densities = [undefined, "compact"];
const envs = [false, true].flatMap((dark) => [false, true].map((contrast) => ({ dark, contrast })));

describe("theme.css", () => {
  it("is the default brand's stylesheet, as the server's /theme.css serves it", () => {
    expect(built.theme).toBe(brandStylesheet(generate(brands.default)));
  });
});

describe("structure.css", () => {
  it("declares no variable a theme sets", () => {
    const theme = declared(built.theme);
    const shared = [...declared(built.structure)].filter((name) => theme.has(name));
    expect(shared).toEqual([]);
  });

  it("keeps the scales and the compact density", () => {
    const names = declared(built.structure);
    expect(names).toContain("--sc-space-4");
    expect(names).toContain("--sc-control-h");
    expect(names).toContain("--sc-radius-pill");
    expect(names).toContain("--sc-shadow-float");
    expect(built.structure).toContain('[data-sc-density="compact"] {');
  });

  it("with theme.css gives every element what variables.css gives it", () => {
    const variables = parseRules(built.variables);
    const page = [...parseRules(built.theme), ...parseRules(built.structure)];
    const names = [...declared(built.variables)];
    for (const theme of themes) {
      for (const density of densities) {
        const attrs: Record<string, string> = {};
        if (theme) attrs["data-sc-theme"] = theme;
        if (density) attrs["data-sc-density"] = density;
        const chain = [{ root: true, attrs }];
        for (const env of envs) {
          const expected = computed(variables, chain, env);
          const actual = computed(page, chain, env);
          for (const name of names) {
            const where = `${name} theme=${theme} density=${density} ${JSON.stringify(env)}`;
            expect(actual.get(name), where).toBe(expected.get(name));
          }
        }
      }
    }
  });
});
