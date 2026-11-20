// SPDX-License-Identifier: AGPL-3.0-or-later
// @vitest-environment node
// The server's Content-Security-Policy allows inline <style> elements only by the SHA-256 hashes in
// `crates/stepclimb-server/csp-style-hashes.txt` (issue #325). React Aria adds two at run time; this test reads the
// installed React Aria, hashes every <style> it creates, and fails when the list and the library
// differ, so an upgrade that changes or adds one cannot break the page unseen.
import { createHash } from "node:crypto";
import { readdirSync, readFileSync } from "node:fs";
import { createRequire } from "node:module";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const HASHES = fileURLToPath(
  new URL("../../crates/stepclimb-server/csp-style-hashes.txt", import.meta.url),
);

/** The hashes the policy allows, as `sha256-<base64>`. */
function allowed(): string[] {
  return readFileSync(HASHES, "utf8")
    .split("\n")
    .map((line) => line.trim())
    .filter((line) => line !== "" && !line.startsWith("#"));
}

/** React Aria's built ES modules, found through the web app's own dependency. */
function reactAriaModules(): string[] {
  const fromWeb = createRequire(import.meta.url);
  const components = fromWeb.resolve("react-aria-components");
  const root = dirname(createRequire(components).resolve("react-aria/package.json"));
  const files: string[] = [];
  const walk = (dir: string) => {
    for (const entry of readdirSync(dir, { withFileTypes: true })) {
      const path = join(dir, entry.name);
      if (entry.isDirectory()) walk(path);
      else if (entry.name.endsWith(".mjs")) files.push(path);
    }
  };
  walk(join(root, "dist"));
  return files;
}

/**
 * The text of each <style> element `source` creates: a template literal assigned to
 * `textContent`, its `${name}` parts replaced with the module's `const name = '...'` strings,
 * trimmed when the code trims it.
 */
function injectedStyles(source: string): string[] {
  if (!source.includes("createElement('style')")) return [];
  const constants = new Map<string, string>();
  for (const match of source.matchAll(/const (\S+) = '([^']*)';/g)) {
    const [, name, value] = match;
    if (name !== undefined && value !== undefined) constants.set(name, value);
  }
  const styles: string[] = [];
  for (const match of source.matchAll(/\.textContent = `([^`]*)`(\.trim\(\))?/g)) {
    const template = match[1] ?? "";
    const text = template.replace(/\$\{([^}]+)\}/g, (_whole, name: string) => {
      const value = constants.get(name.trim());
      if (value === undefined) throw new Error(`no constant ${name} in a style template`);
      return value;
    });
    styles.push(match[2] === undefined ? text : text.trim());
  }
  return styles;
}

function sha256(text: string): string {
  return `sha256-${createHash("sha256").update(text, "utf8").digest("base64")}`;
}

describe("the inline styles the policy allows", () => {
  it("are exactly the <style> elements React Aria creates", () => {
    const found = new Set<string>();
    for (const file of reactAriaModules()) {
      for (const style of injectedStyles(readFileSync(file, "utf8"))) found.add(sha256(style));
    }
    // React Aria ships each module twice (public and private paths); two distinct styles remain.
    expect(found.size).toBeGreaterThan(0);
    expect([...found].sort()).toEqual([...allowed()].sort());
  });

  it("hashes a template's text as the browser sees it", () => {
    const source = [
      "const NAME = 'data-x';",
      "s = document.createElement('style');",
      // The library's template literal, written as text: `[${NAME}]` in its source.
      `s.textContent = \`\n[${"$"}{NAME}] { a: b }\n  \`.trim();`,
    ].join("\n");
    expect(injectedStyles(source)).toEqual(["[data-x] { a: b }"]);
    expect(injectedStyles("no style here")).toEqual([]);
  });
});
