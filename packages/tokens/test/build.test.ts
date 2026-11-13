// SPDX-License-Identifier: AGPL-3.0-or-later
// The built theme files (run `pnpm build` first; `just web-build` does). They are what every
// consumer imports, so the test reads the output rather than the token JSON.
import { readFileSync } from "node:fs";
import { describe, expect, it } from "vitest";

const read = (file: string) =>
  readFileSync(new URL(`../build/css/${file}`, import.meta.url), "utf8");

describe("variables.css", () => {
  const css = read("variables.css");

  it("has the default light theme on :root", () => {
    expect(css).toMatch(/:root,\n\[data-sc-theme="light"\] \{/);
    expect(css).toContain("--sc-bg-canvas: oklch(98.5% 0.004 195);");
  });

  it("has the default dark theme by attribute and by OS setting", () => {
    expect(css).toContain('[data-sc-theme="dark"] {');
    expect(css).toMatch(
      /@media \(prefers-color-scheme: dark\) \{\n {2}:root:not\(\[data-sc-theme="light"\]\)/,
    );
  });

  it("has the vZDC example brand in light and dark, with its font", () => {
    expect(css).toContain('[data-sc-brand="vzdc"][data-sc-theme="light"] {');
    expect(css).toContain('[data-sc-brand="vzdc"][data-sc-theme="dark"] {');
    expect(css).toContain('--sc-font-ui: "Roboto"');
    // vZDC's brand in light mode is the deep red the plan gives as #500E0E.
    expect(css).toContain("--sc-brand: oklch(31% 0.1 25);");
  });

  it("names every variable with the sc- prefix", () => {
    const names = [...css.matchAll(/^\s*(--[\w-]+):/gm)].map((m) => m[1]);
    expect(names.length).toBeGreaterThan(50);
    for (const name of names) expect(name).toMatch(/^--sc-/);
  });
});

describe("tailwind-theme.css", () => {
  const css = read("tailwind-theme.css");

  it("removes Tailwind's default palette", () => {
    expect(css).toContain("--color-*: initial;");
  });

  it("maps utilities onto the theme variables", () => {
    expect(css).toContain("--color-canvas: var(--sc-bg-canvas);");
    expect(css).toContain("--color-brand: var(--sc-brand);");
    expect(css).toContain("--radius-sm: var(--sc-radius-sm);");
    expect(css).toContain("--font-ui: var(--sc-font-ui);");
  });

  it("gives each motion token a duration utility, which Tailwind 4's theme cannot", () => {
    expect(css).toMatch(
      /@utility duration-fast \{\n {2}transition-duration: var\(--sc-motion-fast\);/,
    );
  });
});
