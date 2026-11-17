// SPDX-License-Identifier: AGPL-3.0-or-later
// A small model of the CSS cascade for the shape of CSS the build and base.css write: rules
// whose selectors are compounds of `html`, `:root`, `[attr="value"]` and
// `:not([attr="value"])`, inside optional `@media` and `@layer` blocks. `@keyframes` blocks
// style no element and are skipped. It answers "what is this property on this element", which
// is what the theme and density selectors exist to decide, so the tests check outcomes rather
// than selector strings.

export interface Rule {
  media: string[];
  /** Inside an `@layer`: any unlayered declaration beats it, whatever the specificity. */
  layered: boolean;
  selectors: string[];
  decls: Map<string, string>;
  order: number;
}

function declarations(body: string): Map<string, string> {
  const decls = new Map<string, string>();
  for (const d of body.split(";")) {
    const colon = d.indexOf(":");
    if (colon > 0) decls.set(d.slice(0, colon).trim(), d.slice(colon + 1).trim());
  }
  return decls;
}

/** Parses the CSS into flat rules, keeping the `@media` conditions and `@layer` around each. */
export function parseRules(css: string): Rule[] {
  const src = css.replace(/\/\*[\s\S]*?\*\//g, "");
  const rules: Rule[] = [];
  let i = 0;
  const atRule = (prelude: string, media: string[], layered: boolean) => {
    if (prelude.startsWith("@media")) block([...media, prelude.replace(/^@media\s*/, "")], layered);
    else if (prelude.startsWith("@layer")) block(media, true);
    else if (prelude.startsWith("@keyframes")) skip();
    else throw new Error(`at-rule outside the model: ${prelude}`);
  };
  /** Moves past the block just opened, with every block nested in it. */
  const skip = () => {
    for (let depth = 1; depth > 0 && i < src.length; i++) {
      if (src[i] === "{") depth++;
      else if (src[i] === "}") depth--;
    }
  };
  const block = (media: string[], layered: boolean) => {
    while (i < src.length) {
      const open = src.indexOf("{", i);
      const close = src.indexOf("}", i);
      if (open === -1 || (close !== -1 && close < open)) {
        i = close === -1 ? src.length : close + 1;
        return;
      }
      const prelude = src.slice(i, open).trim();
      i = open + 1;
      if (prelude.startsWith("@")) {
        atRule(prelude, media, layered);
        continue;
      }
      const end = src.indexOf("}", i);
      const selectors = prelude.split(",").map((s) => s.trim());
      rules.push({ media, layered, selectors, decls: declarations(src.slice(i, end)), order: 0 });
      i = end + 1;
    }
  };
  block([], false);
  rules.forEach((r, n) => {
    r.order = n;
  });
  return rules;
}

export interface Element {
  root?: boolean;
  attrs?: Record<string, string>;
}

export interface Env {
  /** The OS setting `prefers-color-scheme: dark`. */
  dark: boolean;
  /** The OS setting `prefers-contrast: more`; off when absent. */
  contrast?: boolean;
  /** The display's gamut; `srgb` when absent. */
  gamut?: "srgb" | "p3" | "rec2020";
}

const part = /html|:root|:not\(\[([\w-]+)="([^"]*)"\]\)|\[([\w-]+)="([^"]*)"\]/gy;

function partMatches(m: RegExpExecArray, el: Element): boolean {
  const [whole, notName, notValue, name, value] = m;
  if (whole === ":root" || whole === "html") return el.root === true;
  if (notName) return el.attrs?.[notName] !== notValue;
  return name !== undefined && el.attrs?.[name] === value;
}

/**
 * The selector's specificity when it matches, else -1. Classes, attributes and pseudo-classes
 * count 100, `html` counts 1. A pseudo-element (`::selection`) never matches an element.
 */
function match(selector: string, el: Element): number {
  if (selector.startsWith("::")) return -1;
  part.lastIndex = 0;
  let spec = 0;
  let matched = true;
  for (let m = part.exec(selector); m; m = part.exec(selector)) {
    spec += m[0] === "html" ? 1 : 100;
    matched &&= partMatches(m, el);
    if (part.lastIndex === selector.length) return matched ? spec : -1;
  }
  throw new Error(`selector outside the model: ${selector}`);
}

const gamuts = ["srgb", "p3", "rec2020"];

function mediaMatches(condition: string, env: Env): boolean {
  if (condition === "(prefers-color-scheme: dark)") return env.dark;
  if (condition === "(prefers-contrast: more)") return env.contrast === true;
  const gamut = /^\(color-gamut: (\w+)\)$/.exec(condition)?.[1];
  if (gamut) return gamuts.indexOf(env.gamut ?? "srgb") >= gamuts.indexOf(gamut);
  throw new Error(`media query outside the model: ${condition}`);
}

interface Winner {
  rank: [number, number, number];
  value: string;
}

const beats = (a: Winner["rank"], b: Winner["rank"]) =>
  a[0] !== b[0] ? a[0] > b[0] : a[1] !== b[1] ? a[1] > b[1] : a[2] > b[2];

/** The declarations that win on one element: unlayered first, then specificity, then order. */
function winners(rules: Rule[], el: Element, env: Env): Map<string, string> {
  const won = new Map<string, Winner>();
  for (const rule of rules) {
    if (!rule.media.every((c) => mediaMatches(c, env))) continue;
    const spec = Math.max(...rule.selectors.map((s) => match(s, el)));
    if (spec < 0) continue;
    const rank: Winner["rank"] = [rule.layered ? 0 : 1, spec, rule.order];
    for (const [prop, value] of rule.decls) {
      const prev = won.get(prop);
      if (!prev || beats(rank, prev.rank)) won.set(prop, { rank, value });
    }
  }
  return new Map([...won].map(([prop, w]) => [prop, w.value]));
}

/** Resolves `var()` references against the element's own values, as CSS does. */
function resolveVars(own: Map<string, string>, value: string, depth = 0): string {
  return value.replace(/var\((--[\w-]+)\)/g, (_, name: string) => {
    const target = own.get(name);
    if (target === undefined || depth > 10) throw new Error(`unresolved ${name}`);
    return resolveVars(own, target, depth + 1);
  });
}

/**
 * The properties computed on the last element of `chain` (root first). Every modeled property
 * inherits (custom properties and `color-scheme` do); a `var()` resolves on the element that
 * declares it.
 */
export function computed(rules: Rule[], chain: Element[], env: Env): Map<string, string> {
  let inherited = new Map<string, string>();
  for (const el of chain) {
    const declared = winners(rules, el, env);
    const own = new Map([...inherited, ...declared]);
    for (const prop of declared.keys()) own.set(prop, resolveVars(own, own.get(prop) ?? ""));
    inherited = own;
  }
  return inherited;
}
