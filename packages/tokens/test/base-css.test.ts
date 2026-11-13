// SPDX-License-Identifier: AGPL-3.0-or-later
// packages/ui's base.css picks `color-scheme` (form controls, scrollbars) with the same theme
// selectors as variables.css. It is checked here because the cascade model lives here: the
// color scheme must follow the theme the tokens resolve to. Only base.css's rules that declare
// `color-scheme` enter the model; its other rules (focus, touch, motion, forced colors) use
// selectors and media queries the model does not cover, and apps/ui-docs/visual checks them in
// a browser.
import { readFileSync } from "node:fs";
import { beforeAll, describe, expect, it } from "vitest";
import { buildTokens } from "./support/build";
import { computed, type Element, parseRules, type Rule } from "./support/cascade";

const baseCss = readFileSync(new URL("../../ui/src/styles/base.css", import.meta.url), "utf8");

let rules: Rule[];
const scheme = (chain: Element[], dark: boolean) =>
  computed(rules, chain, { dark }).get("color-scheme");
const root = (theme?: string): Element => ({
  root: true,
  attrs: theme ? { "data-sc-theme": theme } : {},
});
const inner = (theme: string): Element => ({ attrs: { "data-sc-theme": theme } });

beforeAll(async () => {
  const schemes = parseRules(baseCss).filter((rule) => rule.decls.has("color-scheme"));
  rules = [...parseRules((await buildTokens()).variables), ...schemes].map((rule, order) => ({
    ...rule,
    order,
  }));
});

describe("base.css color-scheme", () => {
  it.each([
    [undefined, false, "light"],
    [undefined, true, "dark"],
    ["light", true, "light"],
    ["dark", false, "dark"],
    ["high-contrast", true, "light"],
    ["high-contrast", false, "light"],
  ])("is right for the page theme %s with OS dark %s", (theme, dark, expected) => {
    expect(scheme([root(theme)], dark)).toBe(expected);
  });

  it.each([
    ["dark", "light", "light"],
    ["dark", "high-contrast", "light"],
    ["light", "dark", "dark"],
    ["high-contrast", "dark", "dark"],
  ])("inside a %s page, follows a %s subtree", (page, sub, expected) => {
    expect(scheme([root(page), inner(sub)], false)).toBe(expected);
    expect(scheme([root(page), inner(sub)], true)).toBe(expected);
  });

  it("follows the OS dark setting into a subtree with no theme", () => {
    expect(scheme([root(), { attrs: {} }], true)).toBe("dark");
  });
});
