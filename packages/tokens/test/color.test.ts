// SPDX-License-Identifier: AGPL-3.0-or-later
// The generator's color math (src/color.ts) against colorjs.io, an implementation of the same CSS
// Color 4 conversions and gamut mapping we did not write, and against published WCAG ratios.
import Color from "colorjs.io";
import { describe, expect, it } from "vitest";
import {
  ColorInputError,
  contrastRatio,
  gamutMapSrgb,
  inSrgb,
  type Oklch,
  oklchToSrgb,
  parseColor,
  srgbToOklch,
} from "../src/color";
import { mulberry32 } from "./support/random";

const SEED = 0x5c0101;
const reference = (c: Oklch) => new Color("oklch", [c.l, c.c, c.h]);

describe("parseColor", () => {
  it("reads six- and three-digit hex", () => {
    const vzdc = parseColor("#500E0E", "primary");
    const expected = new Color("#500E0E").to("oklch").coords;
    expect(vzdc.l).toBeCloseTo(expected[0] ?? 0, 9);
    expect(vzdc.c).toBeCloseTo(expected[1] ?? 0, 9);
    expect(vzdc.h).toBeCloseTo(expected[2] ?? 0, 6);
    expect(parseColor("#fff", "primary").l).toBeCloseTo(1, 9);
    expect(parseColor("#5e0", "primary")).toEqual(parseColor("#55ee00", "primary"));
  });

  it("reads oklch() with numbers, percentages and deg", () => {
    expect(parseColor("oklch(0.5 0.09 195)", "primary")).toEqual({ l: 0.5, c: 0.09, h: 195 });
    const pct = parseColor("oklch(50% 22.5% 195deg)", "primary");
    expect(pct.l).toBeCloseTo(0.5, 12);
    expect(pct.c).toBeCloseTo(0.09, 12);
    expect(parseColor("oklch(0.5 0.1 -30)", "primary").h).toBe(330);
  });

  it.each([
    "red",
    "#12345",
    "#500E0E80",
    "rgb(80 14 14)",
    "oklch(1.2 0.1 20)",
    "oklch(0.5 -0.1 20)",
    "oklch(0.5 0.1 20 / 0.5)",
    "",
  ])("refuses %j, naming the field", (input) => {
    expect(() => parseColor(input, "primary")).toThrow(ColorInputError);
    try {
      parseColor(input, "primary");
    } catch (e) {
      expect((e as ColorInputError).field).toBe("primary");
    }
  });
});

describe("conversion", () => {
  it("sRGB to OKLCH and back matches colorjs.io over a seeded sweep", () => {
    const random = mulberry32(SEED);
    for (let n = 0; n < 500; n++) {
      const rgb: [number, number, number] = [random(), random(), random()];
      const ours = srgbToOklch(rgb);
      const [l, c, h] = new Color("srgb", rgb).to("oklch").coords;
      expect(ours.l).toBeCloseTo(l ?? 0, 9);
      expect(ours.c).toBeCloseTo(c ?? 0, 9);
      if ((c ?? 0) > 1e-4)
        expect(Math.abs(((ours.h - (h ?? 0) + 540) % 360) - 180)).toBeLessThan(1e-6);
      const back = oklchToSrgb(ours);
      for (const i of [0, 1, 2]) expect(back[i]).toBeCloseTo(rgb[i] ?? 0, 9);
    }
  });
});

describe("gamutMapSrgb", () => {
  it("leaves an sRGB color unchanged", () => {
    const c = { l: 0.985, c: 0.004, h: 195 };
    expect(gamutMapSrgb(c)).toEqual(c);
  });

  it("maps lightness at either end to white and black", () => {
    expect(gamutMapSrgb({ l: 1.1, c: 0.2, h: 30 })).toEqual({ l: 1, c: 0, h: 0 });
    expect(gamutMapSrgb({ l: 0, c: 0.2, h: 30, alpha: 0.5 })).toEqual({
      l: 0,
      c: 0,
      h: 0,
      alpha: 0.5,
    });
  });

  it("matches colorjs.io's CSS Color 4 mapping over a seeded sweep", () => {
    const random = mulberry32(SEED + 1);
    let outside = 0;
    for (let n = 0; n < 500; n++) {
      const color = { l: 0.02 + random() * 0.96, c: random() * 0.4, h: random() * 360 };
      if (!inSrgb(color, 0)) outside++;
      const ours = gamutMapSrgb(color);
      const theirs = reference(color).toGamut({ space: "srgb", method: "css" }).to("oklab");
      const d = new Color("oklch", [ours.l, ours.c, ours.h]).deltaE(theirs, "OK");
      expect(d, JSON.stringify(color)).toBeLessThan(1e-6);
      expect(reference(ours).inGamut("srgb")).toBe(true);
    }
    expect(outside).toBeGreaterThan(250);
  });
});

describe("contrastRatio", () => {
  const hex = (h: string) => parseColor(h, "test");

  it("gives the published WCAG ratios", () => {
    expect(contrastRatio(hex("#000"), hex("#fff"))).toBeCloseTo(21, 6);
    expect(contrastRatio(hex("#fff"), hex("#fff"))).toBeCloseTo(1, 9);
    // #767676 on white is the lightest grey that passes 4.5:1, at 4.54:1.
    expect(contrastRatio(hex("#767676"), hex("#fff"))).toBeCloseTo(4.54, 2);
    expect(contrastRatio(hex("#777777"), hex("#fff"))).toBeLessThan(4.5);
  });

  it("is order-independent and matches colorjs.io over a seeded sweep", () => {
    const random = mulberry32(SEED + 2);
    for (let n = 0; n < 300; n++) {
      const a = srgbToOklch([random(), random(), random()]);
      const b = srgbToOklch([random(), random(), random()]);
      expect(contrastRatio(a, b)).toBe(contrastRatio(b, a));
      // colorjs.io weights luminance by the unrounded sRGB-to-XYZ row; WCAG's coefficients are
      // that row rounded to four decimals, which moves the ratio by under 0.05%.
      const theirs = reference(a).contrastWCAG21(reference(b));
      expect(Math.abs(contrastRatio(a, b) / theirs - 1)).toBeLessThan(5e-4);
    }
  });
});
