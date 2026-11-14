// SPDX-License-Identifier: AGPL-3.0-or-later
// The golden vectors (spec 12, B24; T12): inputs, and the stylesheet and contrast reports
// `generate.ts` gives for them, in test/golden/theme-vectors.json. The server's port runs the same
// file (crates/stepclimb-core/tests/theme_golden.rs, T13), so both generators give the same
// bytes for the same inputs, and both refuse the same bad inputs naming the same field.
//
// The file is the TypeScript generator's output, never written by hand. After a deliberate
// change to the generator, regenerate it and port the change to Rust:
//
//     pnpm --filter @stepclimb/tokens golden
import { createHash } from "node:crypto";
import { readFileSync, writeFileSync } from "node:fs";
import { describe, expect, it } from "vitest";
import {
  appearances,
  BrandInputError,
  type BrandInputs,
  type CornerStyle,
  GENERATOR_VERSION,
  generate,
  minimums,
  type ReadingFont,
  type UiFont,
} from "../src/generate";
import { contrastReport, requiredPairs } from "../src/report";
import { brandStylesheet } from "../src/stylesheet";
import { brands } from "../src/themes";
import { mulberry32 } from "./support/random";

const file = new URL("golden/theme-vectors.json", import.meta.url);

/** The seed of the swept vectors; changing it changes the file. */
const SEED = 0x60_1d_e7;

/** Inputs whose full stylesheet and ratios the file holds: the shipped brands and the spec's extremes. */
const named: BrandInputs[] = [
  brands.default,
  brands.vzdc,
  { primary: "#fffde0", cornerStyle: "round" },
  { primary: "#f8f000", readingFont: "sans" },
  { primary: "#0a1030", uiFont: "Roboto", readingFont: "sans", cornerStyle: "square" },
  { primary: "#000080" },
  { primary: "#FF0000" },
  { primary: "#808080" },
  { primary: "#000" },
  { primary: "#fff" },
  { primary: " oklch(70% 100% 145deg) " },
  { primary: "oklch(0.5 0.00009 120)" },
  { primary: "OKLCH(.62 .2 359.996)" },
  { primary: "oklch(0.45 0.5 -30)" },
];

/** Inputs both generators must refuse, and the field each names. */
const invalid: BrandInputs[] = [
  { primary: "dark red" },
  { primary: "#12345" },
  { primary: "#500E0E80" },
  { primary: "oklch(1.2 0.1 10)" },
  { primary: "oklch(0.5 -0.1 10)" },
  { primary: "oklch(0.5 0.1 10 / 0.5)" },
  { primary: "oklch(0.5 1e400 10)" },
  { primary: "oklch(0.5 1e150 10)" },
  { primary: "oklch(0.45 2 -30)" },
  { primary: "oklch(0.5 0.5001 10)" },
  { primary: "oklch(0.5 126% 10)" },
  { primary: "oklch(0.5 0.1 1e400)" },
  { primary: "oklch(1. 0.1 10)" },
  { primary: "rgb(1 2 3)" },
  { primary: "" },
  { primary: "#500E0E", uiFont: "Comic Sans" as UiFont },
  { primary: "#500E0E", uiFont: "toString" as UiFont },
  { primary: "#500E0E", readingFont: "mono" as ReadingFont },
  { primary: "#500E0E", cornerStyle: "pointy" as CornerStyle },
];

const fonts: UiFont[] = ["Atkinson Hyperlegible Next", "Roboto"];
const readings: ReadingFont[] = ["serif", "sans"];
const corners: CornerStyle[] = ["square", "default", "round"];

/** Brand colors across hue, chroma and lightness, as hex and as oklch(), with every other input. */
function swept(): BrandInputs[] {
  const random = mulberry32(SEED);
  const pick = <T>(list: T[]): T => list[Math.floor(random() * list.length)] as T;
  return Array.from({ length: 240 }, (_, i) => {
    const primary =
      i % 2 === 0
        ? `#${Math.floor(random() * 0x1000000)
            .toString(16)
            .padStart(6, "0")}`
        : `oklch(${(random() * 100).toFixed(2)}% ${(random() * 0.4).toFixed(4)} ${(random() * 360).toFixed(2)})`;
    return {
      primary,
      uiFont: pick(fonts),
      readingFont: pick(readings),
      cornerStyle: pick(corners),
    };
  });
}

function reports(inputs: BrandInputs) {
  const themes = generate(inputs);
  return {
    css: brandStylesheet(themes),
    reports: appearances.map((a) => contrastReport(themes[a])),
  };
}

function vectors() {
  return {
    generator: GENERATOR_VERSION,
    minimums,
    pairs: requiredPairs,
    named: named.map((inputs) => {
      const { css, reports: all } = reports(inputs);
      return {
        inputs,
        css,
        reports: all.map((r) => ({
          appearance: r.appearance,
          pass: r.pass,
          ratios: r.pairs.map((p) => p.ratio),
          warnings: r.warnings.map((w) => w.code),
        })),
      };
    }),
    swept: swept().map((inputs) => {
      const { css, reports: all } = reports(inputs);
      return {
        inputs,
        cssSha256: createHash("sha256").update(css).digest("hex"),
        pass: all.map((r) => r.pass),
        warnings: all.map((r) => r.warnings.map((w) => w.code)),
      };
    }),
    invalid: invalid.map((inputs) => {
      try {
        generate(inputs);
      } catch (error) {
        if (error instanceof BrandInputError) return { inputs, field: error.field };
        throw error;
      }
      throw new Error(`the generator accepted ${JSON.stringify(inputs)}`);
    }),
  };
}

describe("golden vectors (B24, T12)", () => {
  it("are what generate.ts gives today", () => {
    const current = vectors();
    if (process.env.UPDATE_GOLDEN === "1") {
      writeFileSync(file, `${JSON.stringify(current, null, 2)}\n`);
    }
    // Parsed, so the comparison does not depend on how Biome formats the file.
    expect(JSON.parse(readFileSync(file, "utf8"))).toEqual(JSON.parse(JSON.stringify(current)));
  });

  it("cover a swept theme that warns and one per corner style and reading font", () => {
    const { swept: all } = vectors();
    expect(all.some((v) => v.warnings.some((w) => w.length > 0))).toBe(true);
    for (const corner of corners)
      expect(all.some((v) => v.inputs.cornerStyle === corner)).toBe(true);
    for (const reading of readings)
      expect(all.some((v) => v.inputs.readingFont === reading)).toBe(true);
  });
});
