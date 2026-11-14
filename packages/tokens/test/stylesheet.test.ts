// SPDX-License-Identifier: AGPL-3.0-or-later
// An organization's stylesheet (src/stylesheet.ts) against spec 12, B19: for the default brand
// it gives every element the values variables.css gives it, in every combination of the page's
// theme attribute and the OS settings, so the server's `/theme.css` and variables.css are
// interchangeable.
import { beforeAll, describe, expect, it } from "vitest";
import { generate } from "../src/generate";
import { brandStylesheet } from "../src/stylesheet";
import { brands } from "../src/themes";
import { buildTokens } from "./support/build";
import { computed, parseRules, type Rule } from "./support/cascade";

let variables: Rule[];
const stylesheet = brandStylesheet(generate(brands.default));
const own = parseRules(stylesheet);

beforeAll(async () => {
  variables = parseRules((await buildTokens()).variables);
});

const names = [...new Set(own.flatMap((r) => [...r.decls.keys()]))];
const themes = [undefined, "light", "dark", "high-contrast"];
const envs = [false, true].flatMap((dark) => [false, true].map((contrast) => ({ dark, contrast })));

describe("brandStylesheet (B19)", () => {
  it("declares every color, the learning states, both fonts and the radii", () => {
    expect(names).toContain("--sc-bg-canvas");
    expect(names).toContain("--sc-state-progress");
    expect(names).toContain("--sc-font-read");
    expect(names).toContain("--sc-radius-md");
    expect(names).not.toContain("--sc-radius-pill");
    expect(names.every((n) => n.startsWith("--sc-"))).toBe(true);
  });

  it.each(themes)("matches variables.css for the default brand with data-sc-theme=%s", (theme) => {
    const attrs: Record<string, string> = theme ? { "data-sc-theme": theme } : {};
    const el = { root: true, attrs };
    for (const env of envs) {
      const expected = computed(variables, [el], env);
      const actual = computed(own, [el], env);
      for (const name of names) {
        expect(actual.get(name), `${name} ${JSON.stringify(env)}`).toBe(expected.get(name));
      }
    }
  });

  it("applies to an inner element that sets a theme, as variables.css does", () => {
    const chain = [{ root: true }, { attrs: { "data-sc-theme": "dark" } }];
    for (const env of envs) {
      const expected = computed(variables, chain, env);
      const actual = computed(own, chain, env);
      for (const name of names) expect(actual.get(name), name).toBe(expected.get(name));
    }
  });
});
