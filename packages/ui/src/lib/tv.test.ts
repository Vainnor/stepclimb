// SPDX-License-Identifier: AGPL-3.0-or-later
// @vitest-environment node
// The class merger keeps a type utility beside a color utility for every name the token theme
// defines, and still resolves real conflicts.
import { readFileSync } from "node:fs";
import { describe, expect, it } from "vitest";
import { tv } from "./tv";

const theme = readFileSync(
  new URL("../../../tokens/build/css/tailwind-theme.css", import.meta.url),
  "utf8",
);
const names = (prefix: string) =>
  Array.from(theme.matchAll(new RegExp(`^\\s*--${prefix}-([\\w-]+):`, "gm")), (m) => m[1] ?? "");

const merge = (classes: string) => tv({ base: classes })();

describe("tv", () => {
  it("reads the token theme", () => {
    expect(names("text")).toContain("caption");
    expect(names("font-weight")).toContain("heading");
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

  it("still lets a later class of the same kind win", () => {
    expect(merge("text-caption text-read")).toBe("text-read");
    expect(merge("text-fg-sub text-fg-strong")).toBe("text-fg-strong");
    expect(merge("font-ui font-read")).toBe("font-read");
    expect(merge("font-body font-heading")).toBe("font-heading");
  });
});
