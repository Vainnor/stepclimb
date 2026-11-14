// SPDX-License-Identifier: AGPL-3.0-or-later
// The docs tables (spec 12, B10 and T11). They are checked against what the other build outputs
// actually do: the variables variables.css declares, the values the cascade model computes from
// it in each theme, and the CSS Tailwind generates for each utility the tables name.
import { compile } from "tailwindcss";
import { beforeAll, describe, expect, it } from "vitest";
import type { DocsTables, DocsToken } from "../src/docs";
import { type Built, buildEdited, buildTokens } from "./support/build";
import { computed, type Element, parseRules } from "./support/cascade";

let built: Built;
let tables: DocsTables;
let rows: DocsToken[];

/** The CSS property each token's utility should set: the plan's role for each token. */
const byGroup: Record<string, string> = {
  Surfaces: "background-color",
  Text: "color",
  Lines: "border-color",
  Learning: "color",
  Type: "font-size",
  Weight: "font-weight",
  "Control height": "height",
  Space: "padding",
  Radius: "border-radius",
  Elevation: "--tw-shadow",
  Motion: "transition-duration",
  Fonts: "font-family",
};
const byName: Record<string, string> = {
  "brand-fg": "color",
  "on-brand": "color",
  ring: "--tw-ring-color",
  "control-touch": "min-height",
  "leading-read": "line-height",
  "ease-out-expo": "transition-timing-function",
  ...Object.fromEntries(["success", "warning", "danger", "info"].map((s) => [`${s}-fg`, "color"])),
};
function intendedProperty(group: string, name: string): string {
  const property =
    byName[name] ?? (group === "Brand" || group === "Status" ? "background-color" : byGroup[group]);
  if (!property) throw new Error(`no intended property for ${name}`);
  return property;
}

/** A Markdown row's cells, without the code backticks; empty cells are null. */
function cells(line: string): (string | null)[] {
  return line
    .slice(2, -2)
    .split(" | ")
    .map((c) => (c === "" ? null : c.replace(/^`(.*)`$/, "$1").replaceAll("\\|", "|")));
}

beforeAll(async () => {
  built = await buildTokens();
  tables = JSON.parse(built.docs) as DocsTables;
  rows = tables.groups.flatMap((g) => g.tokens);
});

describe("the docs tables (T11)", () => {
  it("list every variable in variables.css once, and no other", () => {
    const declared = new Set(
      [...built.variables.matchAll(/^\s*(--sc-[\w-]+):/gm)].map((m) => m[1]),
    );
    const listed = rows.map((r) => r.variable);
    expect(new Set(listed).size).toBe(listed.length);
    expect([...listed].sort()).toEqual([...declared].sort());
  });

  it("have the plan's groups, in the plan's order", () => {
    expect(tables.groups.map((g) => g.title)).toEqual([
      "Surfaces",
      "Text",
      "Lines",
      "Brand",
      "Status",
      "Learning",
      "Type",
      "Weight",
      "Space",
      "Radius",
      "Control height",
      "Elevation",
      "Motion",
      "Fonts",
    ]);
  });

  it("give each value as variables.css resolves it in each theme of the default brand", () => {
    const rules = parseRules(built.variables);
    const root = (attrs: Record<string, string>): Element => ({ root: true, attrs });
    const theme = {
      light: root({}),
      dark: root({ "data-sc-theme": "dark" }),
      "high-contrast": root({ "data-sc-theme": "high-contrast" }),
    };
    for (const [appearance, el] of Object.entries(theme)) {
      const values = computed(rules, [el], { dark: false });
      for (const row of rows) {
        const shown = row.values[appearance as keyof typeof theme];
        expect(shown, `${row.name} in ${appearance}`).toBe(values.get(row.variable));
      }
    }
    const compact = computed(rules, [root({}), { attrs: { "data-sc-density": "compact" } }], {
      dark: false,
    });
    for (const row of rows) {
      const changed = compact.get(row.variable) !== row.values.light;
      expect(row.compact, row.name).toBe(changed ? compact.get(row.variable) : null);
    }
  });

  it("name the alias target of each learning state", () => {
    const states = tables.groups.find((g) => g.title === "Learning")?.tokens ?? [];
    expect(Object.fromEntries(states.map((t) => [t.name, t.alias]))).toEqual({
      "state-locked": "fg-soft",
      "state-available": "fg-sub",
      "state-progress": "brand",
      "state-complete": "success",
      "state-overdue": "danger",
      "state-review": "warning",
    });
  });

  it("name Tailwind utilities that set the property each token is for", async () => {
    const compiler = await compile(`${built.tailwind}\n@tailwind utilities;`);
    for (const group of tables.groups) {
      for (const row of group.tokens) {
        const utility = row.utility;
        const css = compiler.build([utility]);
        const selector = `.${utility.replace(/[()]/g, "\\$&")} {`;
        expect(css, utility).toContain(selector);
        const rule = css.slice(css.indexOf(selector));
        const declarations = rule
          .slice(rule.indexOf("{") + 1, rule.indexOf("}"))
          .split("\n")
          .map((d) => d.trim());
        expect(declarations, `${utility} for ${row.name}`).toContain(
          `${intendedProperty(group.title, row.name)}: var(${row.variable});`,
        );
      }
    }
  });

  it("give each token its DTCG type", () => {
    const types = Object.fromEntries(rows.map((r) => [r.name, r.type]));
    expect(types).toMatchObject({
      "bg-canvas": "color",
      "state-locked": "color",
      "space-1": "dimension",
      "control-h": "dimension",
      "leading-read": "number",
      "weight-body": "fontWeight",
      "shadow-float": "shadow",
      "motion-fast": "duration",
      "ease-out-expo": "cubicBezier",
      "font-ui": "fontFamily",
    });
    for (const row of rows) expect(row.type, row.name).not.toBe("");
  });

  it("describe every color token", () => {
    for (const row of rows.filter((r) => r.id.startsWith("color."))) {
      expect(row.description, row.name).not.toBe("");
    }
  });

  it("write the same tables as Markdown, row for row", () => {
    const md = built.docsMarkdown;
    for (const group of tables.groups) expect(md).toContain(`## ${group.title}\n`);
    const mdRows = md.split("\n").filter((l) => l.startsWith("| `"));
    expect(mdRows).toHaveLength(rows.length);
    rows.forEach((row, i) => {
      const [name, variable, utility, description, light, dark, contrast, compact] = cells(
        mdRows[i] ?? "",
      );
      const alias = row.alias ? ` (alias of \`${row.alias}\`)` : "";
      expect({ name, variable, utility, description, light, dark, contrast }, row.name).toEqual({
        name: row.name,
        variable: row.variable,
        utility: row.utility,
        description: row.description === "" && !alias ? null : `${row.description}${alias}`,
        light: row.values.light,
        dark: row.values.dark,
        contrast: row.values["high-contrast"],
      });
      expect(compact ?? null, row.name).toBe(row.compact);
    });
  });
});

type Json = { [key: string]: Json | string | number };
/** The object at `path` inside `json`. */
const at = (json: Json, ...path: string[]): Json =>
  path.reduce((j, key) => {
    const next = j[key];
    if (typeof next !== "object") throw new Error(`no object at ${path.join(".")}`);
    return next;
  }, json);
const editJson = (edit: (json: Json) => void) => (text: string) => {
  const json = JSON.parse(text) as Json;
  edit(json);
  return JSON.stringify(json);
};

describe("the docs build", () => {
  it("fails when a token belongs to no group", async () => {
    await expect(
      buildEdited(
        "base.json",
        editJson((json) => {
          at(json, "radius").xl = { $value: { value: 12, unit: "px" } };
        }),
      ),
    ).rejects.toThrow("docs tables: radius.xl is in no group of src/docs.ts");
  });

  it("fails when a group lists an id that is not a token", async () => {
    await expect(
      buildEdited(
        "base.json",
        editJson((json) => {
          // Not xs, sm or md: the generated themes carry those, so they stay tokens.
          delete at(json, "radius").pill;
        }),
      ),
    ).rejects.toThrow("docs tables: radius.pill is listed in src/docs.ts but is not a token");
  });

  it("fails when a color has no description in descriptions.json", async () => {
    await expect(
      buildEdited(
        "descriptions.json",
        editJson((json) => {
          delete at(json, "descriptions")["color.ring"];
        }),
      ),
    ).rejects.toThrow("docs tables: color.ring has no description in descriptions.json");
  });

  it("fails when descriptions.json describes an id that is not a token", async () => {
    await expect(
      buildEdited(
        "descriptions.json",
        editJson((json) => {
          at(json, "descriptions")["color.nope"] = "Nothing";
        }),
      ),
    ).rejects.toThrow("docs tables: descriptions.json describes color.nope, which is not a token");
  });

  it("fails when a color's JSON describes it differently from descriptions.json", async () => {
    await expect(
      buildEdited(
        "learning.json",
        editJson((json) => {
          at(json, "color", "state-locked").$description = "Something else";
        }),
      ),
    ).rejects.toThrow(
      "docs tables: color.state-locked is described differently in its JSON and in descriptions.json",
    );
  });

  it("fails when a token is described in its JSON and in descriptions.json", async () => {
    await expect(
      buildEdited(
        "base.json",
        editJson((json) => {
          at(json, "space", "1").$description = "4px";
        }),
      ),
    ).rejects.toThrow(
      "docs tables: space.1 is described both in its JSON and in descriptions.json",
    );
  });
});
