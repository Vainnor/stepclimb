// SPDX-License-Identifier: AGPL-3.0-or-later
// A small model of the CSS cascade for the shape of CSS the build writes: rules whose selectors
// are compounds of `:root`, `[attr="value"]` and `:not([attr="value"])`, inside optional
// `@media` blocks. It answers "what is --sc-x on this element", which is what the theme and
// density selectors exist to decide, so the tests check outcomes rather than selector strings.

export interface Rule {
  media: string[];
  selectors: string[];
  decls: Map<string, string>;
  order: number;
}

/** Parses the generated CSS into flat rules, keeping the `@media` conditions around each. */
export function parseRules(css: string): Rule[] {
  const src = css.replace(/\/\*[\s\S]*?\*\//g, "");
  const rules: Rule[] = [];
  let i = 0;
  const block = (media: string[]) => {
    while (i < src.length) {
      const open = src.indexOf("{", i);
      const close = src.indexOf("}", i);
      if (open === -1 || (close !== -1 && close < open)) {
        i = close === -1 ? src.length : close + 1;
        return;
      }
      const prelude = src.slice(i, open).trim();
      i = open + 1;
      if (prelude.startsWith("@media")) {
        block([...media, prelude.replace(/^@media\s*/, "")]);
      } else if (prelude.startsWith("@")) {
        i = src.indexOf("}", i) + 1;
      } else {
        const end = src.indexOf("}", i);
        const decls = new Map<string, string>();
        for (const d of src.slice(i, end).split(";")) {
          const colon = d.indexOf(":");
          if (colon > 0) decls.set(d.slice(0, colon).trim(), d.slice(colon + 1).trim());
        }
        const selectors = prelude.split(",").map((s) => s.trim());
        rules.push({ media, selectors, decls, order: rules.length });
        i = end + 1;
      }
    }
  };
  block([]);
  return rules;
}

export interface Element {
  root?: boolean;
  attrs?: Record<string, string>;
}

export interface Env {
  /** The OS setting `prefers-color-scheme: dark`. */
  dark: boolean;
}

const part = /:root|:not\(\[([\w-]+)="([^"]*)"\]\)|\[([\w-]+)="([^"]*)"\]/gy;

/** The selector's specificity (every part counts one class) when it matches, else -1. */
function match(selector: string, el: Element): number {
  part.lastIndex = 0;
  let count = 0;
  let m: RegExpExecArray | null = part.exec(selector);
  while (m) {
    count++;
    const [whole, notName, notValue, name, value] = m;
    if (whole === ":root" && !el.root) return -1;
    if (notName && el.attrs?.[notName] === notValue) return -1;
    if (name && el.attrs?.[name] !== value) return -1;
    if (part.lastIndex === selector.length) return count;
    m = part.exec(selector);
  }
  throw new Error(`selector outside the model: ${selector}`);
}

function mediaMatches(condition: string, env: Env): boolean {
  if (condition === "(prefers-color-scheme: dark)") return env.dark;
  // The model is an sRGB display: wide-gamut overrides do not apply.
  if (/^\(color-gamut: /.test(condition)) return false;
  throw new Error(`media query outside the model: ${condition}`);
}

/**
 * The custom properties computed on the last element of `chain` (root first). Each element
 * inherits its parent's values; a `var()` resolves on the element that declares it, as CSS does.
 */
export function computed(rules: Rule[], chain: Element[], env: Env): Map<string, string> {
  let inherited = new Map<string, string>();
  for (const el of chain) {
    const winners = new Map<string, { spec: number; order: number; value: string }>();
    for (const rule of rules) {
      if (!rule.media.every((c) => mediaMatches(c, env))) continue;
      const spec = Math.max(...rule.selectors.map((s) => match(s, el)));
      if (spec < 0) continue;
      for (const [prop, value] of rule.decls) {
        const prev = winners.get(prop);
        if (!prev || spec > prev.spec || (spec === prev.spec && rule.order > prev.order)) {
          winners.set(prop, { spec, order: rule.order, value });
        }
      }
    }
    const own = new Map(inherited);
    for (const [prop, { value }] of winners) own.set(prop, value);
    const resolve = (value: string, depth: number): string =>
      value.replace(/var\((--[\w-]+)\)/g, (_, name: string) => {
        const target = own.get(name);
        if (target === undefined || depth > 10) throw new Error(`unresolved ${name}`);
        return resolve(target, depth + 1);
      });
    for (const prop of winners.keys()) own.set(prop, resolve(own.get(prop) ?? "", 0));
    inherited = own;
  }
  return inherited;
}
