// SPDX-License-Identifier: AGPL-3.0-or-later
// The lint checks of docs/specs/m0/13-base-ui-components.md B18 to B20 against their fixtures in
// bin/fixtures/ui-lint (T7 to T9). A fixture is checked at a path inside the repository that it
// stands in for, so the same file can be in scope in packages/ui and out of scope in
// packages/tokens. Run by `just web-lint`: `node --test bin/check-ui-lint.test.mjs`.
import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { after, before, describe, it } from "node:test";
import { fileURLToPath } from "node:url";
import { checkSource, tailwindPalette } from "./check-ui-lint.mjs";

const root = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const fixture = (name) => readFileSync(join(root, "bin/fixtures/ui-lint", name), "utf8");

/** The scanner's findings for a fixture checked at `path`, as `line rule` strings. */
function scan(name, path) {
  return checkSource(path, fixture(name)).map((f) => `${f.line} ${f.rule}`);
}

describe("raw colors (B18, T7)", () => {
  it("fails #fff in a TSX string", () => {
    assert.deepEqual(scan("hex-string.tsx", "packages/ui/src/a.tsx"), ["2 raw-color"]);
  });
  it("fails rgb(0 0 0) in CSS", () => {
    assert.deepEqual(scan("rgb.css", "packages/ui/src/a.css"), ["3 raw-color"]);
  });
  it("fails bg-[#000], once", () => {
    const found = checkSource("web/src/a.tsx", fixture("arbitrary-color.tsx"));
    assert.deepEqual(
      found.map((f) => [f.line, f.rule, f.text]),
      [[2, "raw-color", "bg-[#000]"]],
    );
  });
  it("fails text-blue-500 from the removed palette", () => {
    assert.deepEqual(scan("palette.tsx", "web/src/a.tsx"), ["2 palette"]);
  });
  it("fails oklch() in packages/ui and passes it in packages/tokens", () => {
    assert.deepEqual(scan("oklch.css", "packages/ui/src/a.css"), ["3 raw-color"]);
    assert.deepEqual(scan("oklch.css", "packages/tokens/src/a.css"), []);
  });
  it("fails a named color in a CSS value, not a token or a keyword", () => {
    assert.deepEqual(scan("named-color.css", "web/src/a.css"), ["3 raw-color"]);
  });
  it("fails a raw color in a custom property, also in @theme", () => {
    assert.deepEqual(
      checkSource("web/src/a.css", fixture("custom-property.css")).map((f) => [f.line, f.text]),
      [
        [3, "#ff0000"],
        [4, "white"],
        [8, "oklch("],
      ],
    );
  });
  it("fails a color word only where it styles: in a style or className value", () => {
    assert.deepEqual(scan("color-words.tsx", "web/src/a.tsx"), ["4 raw-color"]);
  });
  it("fails a named color in a Tailwind arbitrary property", () => {
    const found = checkSource("web/src/a.tsx", fixture("arbitrary-property.tsx"));
    assert.deepEqual(
      found.map((f) => [f.rule, f.text]),
      [
        ["raw-color", "[color:red]"],
        ["logical-properties", "[margin-left:4px]"],
      ],
    );
  });
  it("names the file, the line, the rule and the fix", () => {
    const [f] = checkSource("packages/ui/src/a.tsx", fixture("hex-string.tsx"));
    assert.equal(f.file, "packages/ui/src/a.tsx");
    assert.match(f.message, /`#fff` is a raw color; use a token utility such as `bg-brand`/);
  });
});

describe("the removed palette", () => {
  it("is read from the pinned tailwindcss/theme.css, newer families included", () => {
    const { shaded, single } = tailwindPalette();
    for (const name of ["blue", "slate", "mauve", "olive", "mist", "taupe"]) {
      assert.ok(shaded.includes(name), name);
    }
    assert.deepEqual(single, ["black", "white"]);
  });
  it("fails a class from any of its families", () => {
    assert.deepEqual(
      checkSource("web/src/a.tsx", fixture("palette-new.tsx")).map((f) => f.text),
      ["text-mauve-500", "bg-taupe-950/50", "border-olive-100", "ring-mist-300", "text-white"],
    );
  });
});

describe("logical properties (Q2)", () => {
  it("fails physical-direction utilities", () => {
    assert.deepEqual(
      checkSource("web/src/a.tsx", fixture("physical.tsx")).map((f) => f.text),
      ["ml-2", "pr-4", "left-0", "text-right", "rounded-l-md"],
    );
  });
  it("fails physical directions in a React style object", () => {
    assert.deepEqual(
      checkSource("web/src/a.tsx", fixture("style-object.tsx")).map((f) => [f.line, f.text]),
      [
        [5, "marginLeft: 4"],
        [6, "left: 0"],
        [7, "paddingRight: 2"],
        [8, 'textAlign: "left"'],
        [9, "borderTopLeftRadius: 2"],
      ],
    );
  });
  it("fails physical-direction CSS", () => {
    assert.deepEqual(scan("physical.css", "packages/ui/src/a.css"), [
      "3 logical-properties",
      "4 logical-properties",
      "5 logical-properties",
    ]);
  });
});

describe("emoji (B19, T8)", () => {
  it("flags an emoji in JSX in packages/ui and web/src", () => {
    assert.deepEqual(scan("emoji.tsx", "packages/ui/src/a.tsx"), ["2 emoji"]);
    assert.deepEqual(scan("emoji.tsx", "web/src/a.tsx"), ["2 emoji"]);
  });
});

describe("what passes", () => {
  it("passes comments, identifiers, tokens, logical utilities and text symbols", () => {
    assert.deepEqual(scan("clean.tsx", "packages/ui/src/a.tsx"), []);
    assert.deepEqual(scan("clean.css", "packages/ui/src/a.css"), []);
  });
  it("skips files that are not TS, TSX or CSS", () => {
    assert.deepEqual(scan("hex-string.tsx", "docs/a.md"), []);
  });
});

/**
 * Biome over fixtures with the repository's lint configuration: biome.json with its plugin paths
 * made absolute, in a temporary tree outside git where each fixture sits at its stand-in path.
 */
describe("Biome checks (B19, B20, T8, T9)", () => {
  const cases = {
    "packages/ui/src/icon-lucide.tsx": "icon-lucide.tsx",
    "web/src/icon-phosphor.tsx": "icon-phosphor.tsx",
    "web/src/classname-color.tsx": "classname-color.tsx",
    "web/src/classname-ring-offset.tsx": "classname-ring-offset.tsx",
    "web/src/namespace-import.tsx": "namespace-import.tsx",
    "web/src/classname-layout.tsx": "classname-layout.tsx",
    "web/src/classname-suppressed.tsx": "classname-suppressed.tsx",
    "packages/ui/src/patterns/classname-relative.tsx": "classname-relative.tsx",
    "web/src/routes/classname-relative.tsx": "classname-relative.tsx",
  };
  let dir;
  const biome = (args) =>
    spawnSync(join(root, "node_modules/.bin/biome"), ["lint", ...args], {
      cwd: dir,
      encoding: "utf8",
    });
  let byFile;

  before(() => {
    dir = mkdtempSync(join(tmpdir(), "stepclimb-ui-lint-"));
    const config = JSON.parse(readFileSync(join(root, "biome.json"), "utf8"));
    delete config.$schema;
    config.vcs = { enabled: false };
    config.files = { includes: ["**"] };
    config.plugins = config.plugins.map((p) => resolve(root, p));
    writeFileSync(join(dir, "biome.json"), JSON.stringify(config));
    for (const [path, name] of Object.entries(cases)) {
      mkdirSync(dirname(join(dir, path)), { recursive: true });
      writeFileSync(join(dir, path), fixture(name));
    }
    const run = biome(["--reporter=json", ...Object.keys(cases)]);
    const report = JSON.parse(run.stdout);
    byFile = {};
    for (const d of report.diagnostics) {
      const file = d.location.path;
      byFile[file] ??= [];
      byFile[file].push(`${d.location.start.line} ${d.severity} ${d.category}`);
    }
  });
  after(() => rmSync(dir, { recursive: true, force: true }));

  it("fails an import from lucide-react", () => {
    assert.deepEqual(byFile["packages/ui/src/icon-lucide.tsx"], [
      "2 error lint/style/noRestrictedImports",
    ]);
  });
  it("accepts @phosphor-icons/react", () => {
    assert.equal(byFile["web/src/icon-phosphor.tsx"], undefined);
  });
  it("warns on a color utility in a Stepclimb UI component's className", () => {
    assert.deepEqual(byFile["web/src/classname-color.tsx"], ["4 warning plugin"]);
  });
  it("warns on ring-offset colors", () => {
    assert.deepEqual(byFile["web/src/classname-ring-offset.tsx"], ["4 warning plugin"]);
  });
  it("refuses a namespace import of @stepclimb/ui, which the className rule cannot follow", () => {
    assert.deepEqual(byFile["web/src/namespace-import.tsx"], ["2 error plugin"]);
  });
  it("passes a layout utility, and colors on a plain element", () => {
    assert.equal(byFile["web/src/classname-layout.tsx"], undefined);
  });
  it("honors the suppression comment", () => {
    assert.equal(byFile["web/src/classname-suppressed.tsx"], undefined);
  });
  it("treats a relative import as Stepclimb UI inside packages/ui only", () => {
    assert.deepEqual(byFile["packages/ui/src/patterns/classname-relative.tsx"], [
      "4 warning plugin",
    ]);
    assert.equal(byFile["web/src/routes/classname-relative.tsx"], undefined);
  });
  it("fails the gate on the warning, as `just web-lint` runs with --error-on-warnings (Q3)", () => {
    const layout = biome(["--error-on-warnings", "web/src/classname-layout.tsx"]);
    assert.equal(layout.status, 0, layout.stdout + layout.stderr);
    const run = biome(["--error-on-warnings", "web/src/classname-color.tsx"]);
    assert.notEqual(run.status, 0, run.stdout + run.stderr);
  });
});

/**
 * The className plugin names the token color families in its pattern (GritQL has no way to read
 * the token build). Every Tailwind color the tokens define must start with one of them, and every
 * family must still exist, so a token rename cannot leave the plugin blind.
 */
describe("the className plugin's color families (B20)", () => {
  const keywords = ["inherit", "current", "transparent", "black", "white"];
  const grit = readFileSync(join(root, "packages/ui/lint/classname-styling.grit"), "utf8");
  const listed = /\|caret\|accent\)-\(\?:([a-z|-]+)\)/.exec(grit);
  const families = (listed?.[1] ?? "").split("|").filter((f) => !keywords.includes(f));
  const theme = readFileSync(join(root, "packages/tokens/build/css/tailwind-theme.css"), "utf8");
  const colors = [...theme.matchAll(/--color-([a-z0-9-]+):/g)].map((m) => m[1]);
  const covers = (family, color) => color === family || color.startsWith(`${family}-`);

  it("reads both lists", () => {
    assert.ok(families.length > 0, "no families found in the plugin");
    assert.ok(colors.length > 0, "no colors in the token build");
  });
  it("covers every token color", () => {
    assert.deepEqual(
      colors.filter((c) => !families.some((f) => covers(f, c))),
      [],
    );
  });
  it("names no family the tokens lack", () => {
    assert.deepEqual(
      families.filter((f) => !colors.some((c) => covers(f, c))),
      [],
    );
  });
});
