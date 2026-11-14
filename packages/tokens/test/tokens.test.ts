// SPDX-License-Identifier: AGPL-3.0-or-later
// The token set against the plan (UI tab -> Tokens) and spec 12 (B1 to B9, T1 to T4). The CSS
// is built in memory with the package's own Terrazzo config, and the theme, density and alias
// behavior is read through a model of the cascade (support/cascade.ts).
import { beforeAll, describe, expect, it } from "vitest";
import { type Built, buildEdited, buildTokens } from "./support/build";
import { computed, type Element, type Env, parseRules, type Rule } from "./support/cascade";

/** The plan's Tokens table, group by group. */
const planTokens = {
  surfaces: ["bg-canvas", "bg-raised", "bg-sunken", "bg-overlay"],
  text: ["fg-strong", "fg-default", "fg-sub", "fg-soft", "fg-disabled"],
  lines: ["line-strong", "line", "line-soft"],
  brand: ["brand", "brand-hover", "brand-soft", "brand-fg", "on-brand", "ring"],
  status: ["success", "warning", "danger", "info"].flatMap((s) => [s, `${s}-soft`, `${s}-fg`]),
  learning: ["locked", "available", "progress", "complete", "overdue", "review"].map(
    (s) => `state-${s}`,
  ),
};
const allTokens = Object.values(planTokens).flat();

/** Spec 12, Q3 (decided): each learning state's alias target. */
const stateTargets: Record<string, string> = {
  "state-locked": "fg-soft",
  "state-available": "fg-sub",
  "state-progress": "brand",
  "state-complete": "success",
  "state-overdue": "danger",
  "state-review": "warning",
};

const root = (attrs: Record<string, string> = {}): Element => ({ root: true, attrs });
const vzdc = { "data-sc-brand": "vzdc" };

/** Every way a page selects a theme: the attribute, or the OS setting when none is set. */
const contexts: Record<string, { el: Element; dark: boolean }> = {
  "default light": { el: root(), dark: false },
  "default dark (attribute)": { el: root({ "data-sc-theme": "dark" }), dark: false },
  "default dark (OS)": { el: root(), dark: true },
  "default high contrast": { el: root({ "data-sc-theme": "high-contrast" }), dark: false },
  "vzdc light": { el: root(vzdc), dark: false },
  "vzdc dark (attribute)": { el: root({ ...vzdc, "data-sc-theme": "dark" }), dark: false },
  "vzdc dark (OS)": { el: root(vzdc), dark: true },
  "vzdc high contrast": {
    el: root({ ...vzdc, "data-sc-theme": "high-contrast" }),
    dark: false,
  },
};

let built: Built;
let rules: Rule[];
const at = (el: Element, dark = false, under: Element[] = [], gamut: Env["gamut"] = "srgb") =>
  computed(rules, [...under, el], { dark, gamut });
const inContext = (name: string) => {
  const c = contexts[name];
  if (!c) throw new Error(`no context ${name}`);
  return at(c.el, c.dark);
};
const colors = (values: Map<string, string>) =>
  Object.fromEntries(allTokens.map((t) => [t, values.get(`--sc-${t}`)]));

beforeAll(async () => {
  built = await buildTokens();
  rules = parseRules(built.variables);
});

describe("the plan's token set (T1)", () => {
  it.each(Object.keys(contexts))("resolves every token in %s", (name) => {
    const values = inContext(name);
    for (const token of allTokens) {
      expect(values.get(`--sc-${token}`), token).toMatch(/^oklch\(/);
    }
  });

  it("declares every color token in every theme block, so no theme inherits another's", () => {
    const themeBlocks = rules.filter(
      (r) => !r.media.some((m) => m.startsWith("(color-gamut")) && r.decls.has("--sc-brand"),
    );
    // :root, then light, dark, OS dark, high contrast and OS high contrast for each brand.
    expect(themeBlocks).toHaveLength(11);
    for (const block of themeBlocks) {
      for (const token of allTokens) expect(block.decls.has(`--sc-${token}`), token).toBe(true);
    }
  });

  it("gives every token a Tailwind color utility; surfaces drop their bg- prefix", () => {
    for (const token of allTokens) {
      const utility = token.replace(/^bg-/, "");
      expect(built.tailwind).toContain(`--color-${utility}: var(--sc-${token});`);
    }
  });

  it("gives each appearance its own values", () => {
    const light = colors(inContext("default light"));
    const dark = colors(inContext("default dark (attribute)"));
    const contrast = colors(inContext("default high contrast"));
    expect(dark["bg-canvas"]).not.toBe(light["bg-canvas"]);
    expect(contrast.brand).not.toBe(light.brand);
    expect(contrast.line).not.toBe(light.line);
  });
});

describe("learning-state aliases (B2, T2)", () => {
  it.each(Object.keys(contexts))("resolve to their decided targets in %s", (name) => {
    const values = inContext(name);
    for (const [state, target] of Object.entries(stateTargets)) {
      expect(values.get(`--sc-${state}`), state).toBe(values.get(`--sc-${target}`));
    }
  });

  it("are written as aliases in the CSS, not copied values", () => {
    expect(built.variables).toContain("--sc-state-progress: var(--sc-brand);");
  });

  it("follow a theme set on an inner element", () => {
    const inner = at({ attrs: { "data-sc-theme": "dark" } }, false, [root()]);
    expect(inner.get("--sc-state-progress")).toBe(
      inContext("default dark (attribute)").get("--sc-brand"),
    );
  });

  it("retarget in every theme, and change nothing else, from one edit", async () => {
    const edited = parseRules(
      (
        await buildEdited("learning.json", (json) =>
          json.replace('"{color.warning}"', '"{color.info}"'),
        )
      ).variables,
    );
    for (const [name, { el, dark }] of Object.entries(contexts)) {
      const before = computed(rules, [el], { dark });
      const after = computed(edited, [el], { dark });
      expect(after.get("--sc-state-review"), name).toBe(before.get("--sc-info"));
      expect(after.get("--sc-state-review"), name).not.toBe(before.get("--sc-state-review"));
      after.delete("--sc-state-review");
      before.delete("--sc-state-review");
      expect(after, name).toEqual(before);
    }
  });
});

describe("density (B4, T3)", () => {
  const compact = { "data-sc-density": "compact" };

  it("is comfortable with no attribute", () => {
    const values = at(root());
    expect(values.get("--sc-control-h")).toBe("2.5rem");
    expect(values.get("--sc-control-h-sm")).toBe("2rem");
  });

  it("sets the compact heights and leaves the touch minimum alone", () => {
    const values = at(root(compact));
    expect(values.get("--sc-control-h")).toBe("2rem");
    expect(values.get("--sc-control-h-sm")).toBe("1.625rem");
    expect(values.get("--sc-control-touch")).toBe(at(root()).get("--sc-control-touch"));
    expect(values.get("--sc-control-touch")).toBe("2.75rem");
  });

  it("applies to one view inside a comfortable page, in any theme, without touching color", () => {
    const page = root({ "data-sc-theme": "dark" });
    const view = at({ attrs: compact }, false, [page]);
    const outside = at(page);
    expect(view.get("--sc-control-h")).toBe("2rem");
    expect(outside.get("--sc-control-h")).toBe("2.5rem");
    expect(colors(view)).toEqual(colors(outside));
  });

  it("survives a light theme set inside a compact view", () => {
    const view = { attrs: compact };
    const inner = at({ attrs: { "data-sc-theme": "light" } }, false, [
      root({ "data-sc-theme": "dark" }),
      view,
    ]);
    expect(inner.get("--sc-control-h")).toBe("2rem");
    expect(inner.get("--sc-control-h-sm")).toBe("1.625rem");
    expect(colors(inner)).toEqual(colors(inContext("default light")));
  });

  it.each(["dark", "high-contrast"])("survives a %s theme set inside a compact view", (theme) => {
    const inner = at({ attrs: { "data-sc-theme": theme } }, false, [root(), { attrs: compact }]);
    expect(inner.get("--sc-control-h")).toBe("2rem");
  });
});

describe("theme selection (B5, B8, T4)", () => {
  it("has a high-contrast theme for each brand", () => {
    const base = colors(inContext("default high contrast"));
    const brand = inContext("vzdc high contrast");
    expect(base.brand).not.toBe(colors(inContext("default light")).brand);
    expect(colors(brand).brand).not.toBe(base.brand);
    expect(brand.get("--sc-font-ui")).toMatch(/^"Roboto"/);
  });

  it("gives vZDC high contrast its own values, not vZDC light's", () => {
    const light = colors(inContext("vzdc light"));
    const contrast = colors(inContext("vzdc high contrast"));
    expect(contrast.line).not.toBe(light.line);
    expect(contrast["fg-soft"]).not.toBe(light["fg-soft"]);
  });

  const brands = [
    ["default", {}],
    ["vzdc", vzdc],
  ] as const;

  it.each(brands)("lets %s light win over the OS dark setting", (_, brand) => {
    const chosen = at(root({ ...brand, "data-sc-theme": "light" }), true);
    expect(chosen).toEqual(at(root(brand), false));
  });

  it.each(brands)("lets %s high contrast win over the OS dark setting", (_, brand) => {
    const el = root({ ...brand, "data-sc-theme": "high-contrast" });
    expect(at(el, true)).toEqual(at(el, false));
    expect(at(el, true)).not.toEqual(at(root(brand), false));
  });

  it.each(brands)("follows the OS dark setting only when no theme is chosen, %s", (_, brand) => {
    const os = at(root(brand), true);
    expect(os).toEqual(at(root({ ...brand, "data-sc-theme": "dark" }), false));
    expect(os).not.toEqual(at(root(brand), false));
  });

  it.each(brands)(
    "lets the OS contrast setting select %s high contrast when no theme is chosen (Q5)",
    (_, brand) => {
      const chosen = at(root({ ...brand, "data-sc-theme": "high-contrast" }));
      const os = (dark: boolean, attrs: Record<string, string> = {}) =>
        computed(rules, [root({ ...brand, ...attrs })], { dark, contrast: true });
      expect(os(false)).toEqual(chosen);
      // High contrast wins over the OS dark setting, as ThemeProvider decides.
      expect(os(true)).toEqual(chosen);
      expect(os(true, { "data-sc-theme": "high-contrast" })).toEqual(chosen);
      // A chosen light or dark theme wins over the OS contrast setting.
      expect(os(true, { "data-sc-theme": "light" })).toEqual(at(root(brand)));
      expect(os(false, { "data-sc-theme": "dark" })).toEqual(
        at(root({ ...brand, "data-sc-theme": "dark" })),
      );
    },
  );

  it("keeps the OS contrast setting on the root, so an inner theme still wins", () => {
    const inner = computed(rules, [root(), { attrs: { "data-sc-theme": "dark" } }], {
      dark: false,
      contrast: true,
    });
    expect(colors(inner)).toEqual(colors(inContext("default dark (attribute)")));
  });

  it("gives a wide-gamut display the same values, since no theme has color-gamut overrides", () => {
    expect(built.variables).not.toContain("color-gamut");
    for (const gamut of ["p3", "rec2020"] as const) {
      expect(at(root(vzdc), false, [], gamut)).toEqual(at(root(vzdc), false));
    }
  });

  it("writes the themes in resolution order: light, dark, OS dark, high contrast, OS high contrast, then each further brand", () => {
    const order = [
      ":root {",
      '[data-sc-theme="light"] {',
      '[data-sc-theme="dark"] {',
      "@media (prefers-color-scheme: dark) {\n  :root:not(",
      '[data-sc-theme="high-contrast"] {',
      "@media (prefers-contrast: more) {\n  :root:not(",
      '[data-sc-brand="vzdc"],\n[data-sc-brand="vzdc"][data-sc-theme="light"] {',
      '[data-sc-brand="vzdc"][data-sc-theme="dark"] {',
      '@media (prefers-color-scheme: dark) {\n  :root[data-sc-brand="vzdc"]:not(',
      '[data-sc-brand="vzdc"][data-sc-theme="high-contrast"] {',
      '@media (prefers-contrast: more) {\n  :root[data-sc-brand="vzdc"]:not(',
      '[data-sc-density="compact"] {',
    ].map((s) => built.variables.indexOf(s));
    expect(order.every((i) => i >= 0)).toBe(true);
    expect([...order].sort((a, b) => a - b)).toEqual(order);
  });
});

describe("scales (B3, B6)", () => {
  const rem = (px: number) => `${px / 16}rem`;

  it("match the plan's type, space and radius scales", () => {
    const values = at(root());
    const text = ["xs", "caption", "sm", "base", "lg", "xl", "2xl", "3xl", "4xl"];
    expect(text.map((t) => values.get(`--sc-text-${t}`))).toEqual(
      [12, 13, 14, 16, 18, 20, 24, 30, 38].map(rem),
    );
    const space = ["0-5", "1", "1-5", "2", "3", "4", "5", "6", "8", "10", "14", "18"];
    expect(space.map((s) => values.get(`--sc-space-${s}`))).toEqual(
      [2, 4, 6, 8, 12, 16, 20, 24, 32, 40, 56, 72].map(rem),
    );
    expect(["xs", "sm", "md"].map((r) => values.get(`--sc-radius-${r}`))).toEqual([
      "3px",
      "5px",
      "8px",
    ]);
    expect(["body", "control", "heading"].map((w) => values.get(`--sc-weight-${w}`))).toEqual([
      "400",
      "500",
      "700",
    ]);
    expect(["fast", "base", "slow"].map((m) => values.get(`--sc-motion-${m}`))).toEqual([
      "120ms",
      "180ms",
      "320ms",
    ]);
  });

  it("have reading text at 18px and 1.7, inside the plan's 17-18px and 1.65-1.7", () => {
    const values = at(root());
    expect(values.get("--sc-text-read")).toBe(rem(18));
    expect(values.get("--sc-leading-read")).toBe("1.7");
    expect(built.tailwind).toContain("--text-read: var(--sc-text-read);");
    expect(built.tailwind).toContain("--leading-read: var(--sc-leading-read);");
  });

  it("have one shadow, for floating layers", () => {
    expect(at(root()).get("--sc-shadow-float")).toMatch(/^0px 4px 16px oklch\(/);
    expect(built.tailwind).toContain("--shadow-float: var(--sc-shadow-float);");
    expect(built.tailwind.match(/--shadow-/g)).toHaveLength(1);
  });

  it("make the overlay scrim translucent in every theme", () => {
    for (const name of Object.keys(contexts)) {
      expect(inContext(name).get("--sc-bg-overlay"), name).toMatch(/ \/ 0\.\d+\)$/);
    }
  });
});
