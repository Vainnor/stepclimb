// SPDX-License-Identifier: AGPL-3.0-or-later
// The docs tables: build/docs/tokens.json, the data Storybook's Tokens page reads, and
// build/docs/tokens.md, the same tables as Markdown. One row per token with its name, CSS
// variable, Tailwind utility, description and value in each theme of the default brand. The
// values are the ones the CSS build wrote for variables.css, so the tables cannot drift from it.
import { readFileSync } from "node:fs";
import type { Plugin, TokenNormalized, TokenTransformed } from "@terrazzo/parser";
import { tailwindKey, variableName } from "./names";

export const appearances = ["light", "dark", "high-contrast"] as const;
export type Appearance = (typeof appearances)[number];

export interface DocsToken {
  /** The DTCG id: `color.bg-canvas`. */
  id: string;
  /** The plan's name: `bg-canvas`, `radius-sm`. */
  name: string;
  variable: string;
  /** The DTCG `$type`: `color`, `dimension`, `shadow`... */
  type: string;
  /** The usual utility (`bg-canvas`); every utility of the token's Tailwind namespace works. */
  utility: string;
  description: string;
  /** The token this one aliases (`fg-soft` for `state-locked`); its value is the target's. */
  alias: string | null;
  /** The value variables.css gives in each theme; colors as declared for sRGB displays. */
  values: Record<Appearance, string>;
  /** The value under `[data-sc-density="compact"]`, for the tokens density changes. */
  compact: string | null;
}

export interface DocsGroup {
  title: string;
  tokens: DocsToken[];
}

export interface DocsTables {
  brand: "default";
  groups: DocsGroup[];
}

const status = ["success", "warning", "danger", "info"].flatMap((s) => [s, `${s}-soft`, `${s}-fg`]);
const color = (names: string[]) => names.map((n) => `color.${n}`);
const scale = (group: string, names: string[]) => names.map((n) => `${group}.${n}`);

/** The groups of the plan's Tokens tables (UI tab -> Tokens), in the plan's order. */
export const docsGroups: { title: string; ids: string[] }[] = [
  { title: "Surfaces", ids: color(["bg-canvas", "bg-raised", "bg-sunken", "bg-overlay"]) },
  { title: "Text", ids: color(["fg-strong", "fg-default", "fg-sub", "fg-soft", "fg-disabled"]) },
  { title: "Lines", ids: color(["line-strong", "line", "line-soft"]) },
  {
    title: "Brand",
    ids: color(["brand", "brand-hover", "brand-soft", "brand-fg", "on-brand", "ring"]),
  },
  { title: "Status", ids: color(status) },
  {
    title: "Learning",
    ids: color(
      ["locked", "available", "progress", "complete", "overdue", "review"].map((s) => `state-${s}`),
    ),
  },
  {
    title: "Type",
    ids: [
      ...scale("text", ["xs", "caption", "sm", "base", "lg", "xl", "2xl", "3xl", "4xl", "read"]),
      "leading.read",
    ],
  },
  { title: "Weight", ids: scale("weight", ["body", "control", "heading"]) },
  {
    title: "Space",
    ids: scale("space", ["0-5", "1", "1-5", "2", "3", "4", "5", "6", "8", "10", "14", "18"]),
  },
  { title: "Radius", ids: scale("radius", ["xs", "sm", "md", "pill"]) },
  { title: "Control height", ids: scale("control", ["h", "h-sm", "touch"]) },
  { title: "Elevation", ids: ["shadow.float"] },
  { title: "Motion", ids: [...scale("motion", ["fast", "base", "slow"]), "ease.out-expo"] },
  { title: "Fonts", ids: scale("font", ["ui", "read", "mono"]) },
];

/** The utility prefix shown for each Tailwind namespace other than color. */
const utilityPrefixes: Record<string, string> = {
  spacing: "p",
  radius: "rounded",
  text: "text",
  leading: "leading",
  "font-weight": "font",
  font: "font",
  ease: "ease",
  shadow: "shadow",
};

/** A color's usual utility follows its role: text colors on `text-`, lines on `border-`. */
function colorPrefix(name: string): string {
  if (name.startsWith("fg-") || name.endsWith("-fg") || name.startsWith("on-")) return "text";
  if (name.startsWith("state-")) return "text";
  if (name.startsWith("line")) return "border";
  if (name === "ring") return "ring";
  return "bg";
}

/**
 * `color.bg-canvas` -> `bg-canvas`, `color.fg-sub` -> `text-fg-sub`, `motion.fast` ->
 * `duration-fast`. Control heights have no theme key; components read their variables directly.
 */
export function tailwindUtility(id: string): string {
  if (id.startsWith("motion.")) return `duration-${id.slice("motion.".length)}`;
  if (id === "control.touch") return `min-h-(${variableName(id)})`;
  if (id.startsWith("control.")) return `h-(${variableName(id)})`;
  const key = tailwindKey(id);
  if (!key) throw new Error(`docs tables: no Tailwind utility for ${id}`);
  const prefix =
    key.namespace === "color"
      ? colorPrefix(id.slice("color.".length))
      : utilityPrefixes[key.namespace];
  if (!prefix) throw new Error(`docs tables: no utility prefix for the ${key.namespace} namespace`);
  return `${prefix}-${key.name}`;
}

/** What the tables need of each token. */
export interface TokenFacts {
  type: string;
  description: string;
  aliasOf: string | null;
}

/** The CSS each permutation declares, by token id: the three themes and the compact density. */
export type CssValues = Record<Appearance | "compact", Map<string, string>>;

function resolvedValue(
  id: string,
  appearance: Appearance,
  facts: Map<string, TokenFacts>,
  css: CssValues,
): string {
  const target = facts.get(id)?.aliasOf;
  if (target) return resolvedValue(target, appearance, facts, css);
  // A theme declares only what changes between themes; everything else is the :root value.
  const value = css[appearance].get(id) ?? css.light.get(id);
  if (value === undefined) throw new Error(`docs tables: variables.css has no value for ${id}`);
  return value;
}

function docsToken(id: string, facts: Map<string, TokenFacts>, css: CssValues): DocsToken {
  const variable = variableName(id);
  const aliasOf = facts.get(id)?.aliasOf ?? null;
  const values = Object.fromEntries(
    appearances.map((a) => [a, resolvedValue(id, a, facts, css)]),
  ) as Record<Appearance, string>;
  return {
    id,
    name: variable.slice("--sc-".length),
    variable,
    type: facts.get(id)?.type ?? "",
    utility: tailwindUtility(id),
    description: facts.get(id)?.description ?? "",
    alias: aliasOf ? variableName(aliasOf).slice("--sc-".length) : null,
    values,
    compact: css.compact.get(id) ?? null,
  };
}

/**
 * Builds the tables. Every token is in a group and every listed id is a token, or the build
 * fails, so a new token cannot be left out of the docs.
 */
export function buildDocsTables(facts: Map<string, TokenFacts>, css: CssValues): DocsTables {
  const listed = docsGroups.flatMap((g) => g.ids);
  for (const id of listed) {
    if (!facts.has(id))
      throw new Error(`docs tables: ${id} is listed in src/docs.ts but is not a token`);
  }
  for (const id of facts.keys()) {
    if (!listed.includes(id)) throw new Error(`docs tables: ${id} is in no group of src/docs.ts`);
  }
  return {
    brand: "default",
    groups: docsGroups.map((g) => ({
      title: g.title,
      tokens: g.ids.map((id) => docsToken(id, facts, css)),
    })),
  };
}

const cell = (text: string) => text.replaceAll("|", "\\|");
const code = (text: string | null) => (text ? `\`${cell(text)}\`` : "");

/** The tables as Markdown, one section per group. */
export function docsMarkdown(tables: DocsTables): string {
  const sections = tables.groups.map((group) => {
    const compact = group.tokens.some((t) => t.compact !== null);
    const head = [
      "Token",
      "CSS variable",
      "Tailwind",
      "Description",
      "Light",
      "Dark",
      "High contrast",
    ];
    if (compact) head.push("Compact");
    const rows = group.tokens.map((t) => {
      const values = appearances.map((a) => code(t.values[a]));
      const description = t.alias ? `${t.description} (alias of \`${t.alias}\`)` : t.description;
      const row = [code(t.name), code(t.variable), code(t.utility), cell(description), ...values];
      if (compact) row.push(code(t.compact));
      return `| ${row.join(" | ")} |`;
    });
    const rule = `|${head.map(() => " --- ").join("|")}|`;
    return [`## ${group.title}`, "", `| ${head.join(" | ")} |`, rule, ...rows].join("\n");
  });
  const intro =
    "Generated by packages/tokens (src/docs.ts) from the token JSON. Do not edit. Values are the default brand's, as variables.css declares them; colors are the sRGB values.";
  return `# Stepclimb UI tokens\n\n${intro}\n\n${sections.join("\n\n")}\n`;
}

/** The value variables.css declares: a color's sRGB form, which is the only one it writes. */
function baseValue(t: TokenTransformed): string {
  if (typeof t.value === "string") return t.value;
  const srgb = t.value.srgb;
  if (srgb === undefined)
    throw new Error(`docs tables: ${t.id} has a value the tables cannot show`);
  return srgb;
}

const inputs: Record<Appearance | "compact", Record<string, string>> = {
  light: { theme: "default-light" },
  dark: { theme: "default-dark" },
  "high-contrast": { theme: "default-high-contrast" },
  compact: { theme: "default-light", density: "compact" },
};

/**
 * Each token's description has one source. A color's is in `descriptions.json`, since color
 * tokens are written once per theme file; a `$description` a theme file still carries must match
 * it. Any other token's is its `$description` in the token JSON, or the map's entry when the JSON
 * has none.
 */
export function tokenFacts(
  tokens: Record<string, TokenNormalized>,
  descriptions: Record<string, string>,
): Map<string, TokenFacts> {
  for (const id of Object.keys(descriptions)) {
    if (!tokens[id])
      throw new Error(`docs tables: descriptions.json describes ${id}, which is not a token`);
  }
  return new Map(
    Object.values(tokens).map((t) => {
      const mapped = descriptions[t.id];
      if (t.id.startsWith("color.") && mapped === undefined) {
        throw new Error(`docs tables: ${t.id} has no description in descriptions.json`);
      }
      if (t.id.startsWith("color.") && t.$description !== undefined && t.$description !== mapped) {
        throw new Error(
          `docs tables: ${t.id} is described differently in its JSON and in descriptions.json`,
        );
      }
      if (!t.id.startsWith("color.") && mapped !== undefined && t.$description !== undefined) {
        throw new Error(
          `docs tables: ${t.id} is described both in its JSON and in descriptions.json`,
        );
      }
      const description = mapped ?? t.$description ?? "";
      return [t.id, { type: t.$type, description, aliasOf: t.aliasOf ?? null }];
    }),
  );
}

function readDescriptions(file: URL): Record<string, string> {
  const json = JSON.parse(readFileSync(file, "utf8")) as { descriptions?: Record<string, string> };
  if (!json.descriptions) throw new Error(`docs tables: ${file.pathname} has no descriptions`);
  return json.descriptions;
}

/**
 * The Terrazzo plugin; it runs after the CSS plugin and reads that plugin's values.
 * `descriptions` is the URL of `descriptions.json`.
 */
export function docsTables(descriptions: URL): Plugin {
  return {
    name: "stepclimb-docs-tables",
    async build({ resolver, getTransforms, outputFile }) {
      const css = Object.fromEntries(
        Object.entries(inputs).map(([key, input]) => [
          key,
          new Map(getTransforms({ format: "css", input }).map((t) => [t.id, baseValue(t)])),
        ]),
      ) as CssValues;
      const facts = tokenFacts(resolver.apply(inputs.light), readDescriptions(descriptions));
      const tables = buildDocsTables(facts, css);
      outputFile("docs/tokens.json", `${JSON.stringify(tables, null, 2)}\n`);
      outputFile("docs/tokens.md", docsMarkdown(tables));
    },
  };
}
