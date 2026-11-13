// SPDX-License-Identifier: AGPL-3.0-or-later
// @vitest-environment node
// The class merger knows every name the token build defines: a type utility stays beside a color
// utility, and of two utilities of the same kind the later one wins.
import { readFileSync } from "node:fs";
import { describe, expect, it } from "vitest";
import { tv } from "./tv";

const theme = readFileSync(
  new URL("../../../tokens/build/css/tailwind-theme.css", import.meta.url),
  "utf8",
);
/** The names the build defines for a theme namespace (`--text-*`) or as `@utility <prefix>-*`. */
const names = (prefix: string) => [
  ...Array.from(theme.matchAll(new RegExp(`^\\s*--${prefix}-([\\w-]+):`, "gm")), (m) => m[1] ?? ""),
  ...Array.from(
    theme.matchAll(new RegExp(`^@utility ${prefix}-([\\w-]+)`, "gm")),
    (m) => m[1] ?? "",
  ),
];

const merge = (classes: string) => tv({ base: classes })();

/** Each token name's utility, with the class written for it. */
const groups: [namespace: string, utility: string, other: string][] = [
  ["text", "text", "text-xs"],
  ["font-weight", "font", "font-normal"],
  ["font", "font", "font-sans"],
  ["leading", "leading", "leading-none"],
  ["radius", "rounded", "rounded-none"],
  ["shadow", "shadow", "shadow-none"],
  ["ease", "ease", "ease-linear"],
  ["duration", "duration", "duration-0"],
];

describe("tv", () => {
  it.each(groups)("reads --%s-* names from the token build", (namespace) => {
    expect(names(namespace).length).toBeGreaterThan(0);
  });

  describe.each(groups)("%s", (namespace, utility, other) => {
    it.each(names(namespace))(`a later ${utility}-%s replaces an earlier one`, (name) => {
      expect(merge(`${other} ${utility}-${name}`)).toBe(`${utility}-${name}`);
    });
  });

  it.each(names("text"))("keeps text-%s beside a text color", (size) => {
    expect(merge(`text-${size} text-fg-sub`)).toBe(`text-${size} text-fg-sub`);
  });

  it.each(names("font-weight"))("keeps font-%s beside a font family", (weight) => {
    expect(merge(`font-ui font-${weight}`)).toBe(`font-ui font-${weight}`);
  });

  it.each(names("leading"))("keeps leading-%s beside a text size", (leading) => {
    expect(merge(`text-read leading-${leading}`)).toBe(`text-read leading-${leading}`);
  });

  it("keeps a text color beside a size and a family beside a weight", () => {
    expect(merge("text-fg-sub text-fg-strong")).toBe("text-fg-strong");
    expect(merge("font-ui font-read")).toBe("font-read");
  });
});
