// SPDX-License-Identifier: AGPL-3.0-or-later
// The theme generator (src/generate.ts) against spec 12: B11 to B15, B18 and B19; T5, T6 and T8,
// and the decided Q9 (the default brand reproduces its light values). Contrast is measured with
// colorjs.io, not with the generator's own function.
import { readFileSync } from "node:fs";
import Color from "colorjs.io";
import { describe, expect, it } from "vitest";
import { type Oklch, parseColor } from "../src/color";
import {
  type BrandInputs,
  brandTokens,
  colorTokens,
  cssColor,
  generate,
  type Theme,
  themeToCss,
  themeToDtcg,
  type UiFont,
} from "../src/generate";
import { variableName } from "../src/names";
import { buildEdited, tokensDir } from "./support/build";
import { mulberry32 } from "./support/random";

/** The seed of the brand-color sweep; a failure prints the input, and this replays it. */
const SEED = 0x7e5c12;

interface DtcgColor {
  $value: { colorSpace: string; components: [number, number, number]; alpha?: number };
}
const themeFile = (name: string) =>
  JSON.parse(readFileSync(new URL(`themes/${name}.json`, tokensDir), "utf8")) as {
    color: Record<string, DtcgColor>;
    font: { ui: { $value: string[] } };
  };
const fromDtcg = ({ $value: { components, alpha } }: DtcgColor): Oklch => ({
  l: components[0],
  c: components[1],
  h: components[2],
  ...(alpha === undefined ? {} : { alpha }),
});
const ref = (c: Oklch) => new Color("oklch", [c.l, c.c, c.h]);
const contrast = (a: Oklch, b: Oklch) => ref(a).contrastWCAG21(ref(b));

/** The default brand's input: the plan's light brand value (spec 12, Q9). */
const defaultBrand: BrandInputs = { primary: "oklch(0.5 0.09 195)" };
const vzdcBrand: BrandInputs = { primary: "#500E0E", uiFont: "Roboto" };

/** Brand colors across hue, chroma and lightness, with the spec's named extremes first. */
function sweep(): string[] {
  const random = mulberry32(SEED);
  const named = ["#fffde0", "#f8f000", "#0a1030", "#000080", "#ff0000", "#808080", "#000", "#fff"];
  const swept = Array.from({ length: 300 }, () => {
    const l = (random() * 100).toFixed(2);
    const c = (random() * 0.37).toFixed(4);
    const h = (random() * 360).toFixed(2);
    return `oklch(${l}% ${c} ${h})`;
  });
  return [...named, defaultBrand.primary, vzdcBrand.primary, ...swept];
}

const allColors = (t: Theme) => colorTokens.map((token) => [token, t.colors[token]] as const);

describe("generate", () => {
  it("reproduces the default brand's light theme (Q9)", () => {
    const { light } = generate(defaultBrand);
    const file = themeFile("default-light");
    expect(Object.keys(light.colors).sort()).toEqual(Object.keys(file.color).sort());
    for (const [token, color] of allColors(light)) {
      const expected = file.color[token];
      if (!expected) throw new Error(`default-light.json has no ${token}`);
      // The plan's value as colorjs.io maps it into sRGB, the way Terrazzo writes variables.css.
      const mapped = ref(fromDtcg(expected)).toGamut({ space: "srgb", method: "css" });
      expect(ref(color).deltaE(mapped, "OK"), token).toBeLessThan(2e-4);
      expect(color.alpha, token).toBe(expected.$value.alpha);
    }
    expect(light.font.ui).toEqual(file.font.ui.$value);
  });

  it("is deterministic: the same inputs give byte-identical CSS and JSON (T5)", () => {
    for (const inputs of [defaultBrand, vzdcBrand, { primary: "#f8f000" }]) {
      const a = generate(inputs).light;
      const b = generate({ ...inputs }).light;
      expect(themeToCss(a, ":root")).toBe(themeToCss(b, ":root"));
      expect(JSON.stringify(themeToDtcg(a))).toBe(JSON.stringify(themeToDtcg(b)));
    }
  });

  it("emits only colors inside sRGB, for every brand in the sweep (T6, B13)", () => {
    for (const primary of sweep()) {
      for (const [token, color] of allColors(generate({ primary }).light)) {
        expect(ref(color).inGamut("srgb"), `${primary} ${token} ${cssColor(color)}`).toBe(true);
      }
    }
  });

  it("picks brand tokens that clear their contrast, for every brand in the sweep (B14, B15)", () => {
    for (const primary of sweep()) {
      const c = generate({ primary }).light.colors;
      const at = `${primary}: `;
      expect(contrast(c["on-brand"], c.brand), `${at}on-brand on brand`).toBeGreaterThanOrEqual(
        4.5,
      );
      expect(c["on-brand"].l, `${at}on-brand is the light end`).toBeGreaterThan(c["brand-soft"].l);
      expect(c["brand-hover"].l, `${at}hover is past brand`).toBeLessThan(c.brand.l);
      expect(contrast(c["brand-fg"], c["brand-soft"]), `${at}brand-fg`).toBeGreaterThanOrEqual(4.5);
      expect(contrast(c["brand-fg"], c["bg-canvas"]), `${at}brand-fg`).toBeGreaterThanOrEqual(4.5);
      expect(contrast(c.ring, c["bg-canvas"]), `${at}ring`).toBeGreaterThanOrEqual(3);
      expect(contrast(c.ring, c["bg-raised"]), `${at}ring`).toBeGreaterThanOrEqual(3);
      expect(c["brand-soft"].l, `${at}brand-soft is a tint`).toBeGreaterThan(c.brand.l);
    }
  });

  it("takes the passing step nearest the input's lightness for brand", () => {
    // vZDC's #500E0E is lightness 0.285: the 0.31 step, since the darkest is kept for hover.
    const vzdc = generate(vzdcBrand).light.colors;
    expect(vzdc.brand.l).toBe(0.31);
    expect(vzdc["brand-hover"].l).toBe(0.26);
    // A light yellow cannot carry white text, so brand moves down to the first step that can.
    const yellow = generate({ primary: "#f8f000" }).light.colors;
    expect(contrast(yellow["on-brand"], yellow.brand)).toBeGreaterThanOrEqual(4.5);
    expect(yellow.brand.l).toBeLessThan(0.6);
  });

  it("tints the neutrals with the brand hue at low chroma (B13)", () => {
    const primary = parseColor(vzdcBrand.primary, "primary");
    const c = generate(vzdcBrand).light.colors;
    for (const token of ["bg-canvas", "bg-sunken", "fg-strong", "fg-soft", "line"] as const) {
      expect(c[token].h, token).toBeCloseTo(primary.h, 1);
      expect(c[token].c, token).toBeGreaterThan(0);
      expect(c[token].c, token).toBeLessThanOrEqual(0.014);
    }
    const grey = generate({ primary: "#808080" }).light.colors;
    for (const token of ["bg-canvas", "fg-strong", "brand"] as const) {
      expect(grey[token], token).toMatchObject({ c: 0, h: 0 });
    }
  });

  it("keeps the default status colors and outputs no structure tokens (T8, B17, B18)", () => {
    const file = themeFile("default-light");
    for (const primary of [vzdcBrand.primary, "#f8f000", "#0a1030"]) {
      const theme = generate({ primary }).light;
      for (const s of ["success", "warning", "danger", "info"]) {
        for (const token of [s, `${s}-soft`, `${s}-fg`] as const) {
          const expected = file.color[token];
          if (!expected) throw new Error(`default-light.json has no ${token}`);
          const mapped = ref(fromDtcg(expected)).toGamut({ space: "srgb", method: "css" });
          const color = theme.colors[token as keyof Theme["colors"]];
          expect(ref(color).deltaE(mapped, "OK"), token).toBeLessThan(2e-4);
        }
      }
      const dtcg = themeToDtcg(theme) as Record<string, Record<string, unknown>>;
      expect(Object.keys(dtcg).sort()).toEqual(["color", "font"]);
      expect(Object.keys(dtcg.font ?? {})).toEqual(["ui"]);
      expect(themeToCss(theme, ":root")).not.toMatch(/--sc-(space|text|weight|control|radius)/);
    }
  });

  it("uses the chosen UI font and refuses others", () => {
    expect(generate(vzdcBrand).light.font.ui).toEqual(themeFile("vzdc-light").font.ui.$value);
    for (const uiFont of ["Comic Sans", "toString", "roboto"]) {
      expect(() => generate({ primary: "#500E0E", uiFont: uiFont as UiFont }), uiFont).toThrow(
        expect.objectContaining({ name: "ColorInputError", field: "uiFont" }),
      );
    }
  });

  it("refuses a primary color it cannot read, naming the field", () => {
    expect(() => generate({ primary: "dark red" })).toThrow(
      expect.objectContaining({ name: "ColorInputError", field: "primary" }),
    );
  });
});

describe("brandTokens", () => {
  const grey = (l: number): Oklch => ({ l, c: 0, h: 0 });
  const white = grey(1);

  it("moves brand-fg off two-past-brand when that step does not read on brand-soft", () => {
    // brand is the 0.5 step (index 4); index 6 is too light for text on the 0.97 tint, so
    // brand-fg takes the nearest step that reads, the darker of the two at distance one.
    const scale = [1, 0.97, 0.6, 0.55, 0.5, 0.45, 0.66, 0.4, 0.35, 0.3, 0.25, 0.2].map(grey);
    const t = brandTokens(scale, 0.5, white, white);
    expect(t.brand).toEqual(grey(0.5));
    expect(t["brand-hover"]).toEqual(grey(0.45));
    expect(t["brand-fg"]).toEqual(grey(0.4));
    expect(contrast(grey(0.66), grey(0.97))).toBeLessThan(4.5);
  });

  it("moves ring off brand when brand does not show on the canvas", () => {
    // A light brand under black text: it clears 4.5:1 against on-brand but not 3:1 on white.
    const scale = [0, 0.97, 0.95, 0.9, 0.8, 0.6, 0.75, 0.7, 0.85, 0.5, 0.45, 0.4].map(grey);
    const t = brandTokens(scale, 0.9, white, white);
    expect(t.brand).toEqual(grey(0.9));
    expect(t.ring).toEqual(grey(0.6));
    expect(contrast(grey(0.7), white)).toBeLessThan(3);
  });
});

describe("serialization (B19)", () => {
  it("writes CSS with variables.css's names, the learning states and the UI font", () => {
    const theme = generate(vzdcBrand).light;
    const css = themeToCss(theme, '[data-sc-brand="vzdc"]');
    expect(css.startsWith('[data-sc-brand="vzdc"] {\n')).toBe(true);
    for (const [token, color] of allColors(theme)) {
      expect(css).toContain(`  ${variableName(`color.${token}`)}: ${cssColor(color)};\n`);
    }
    expect(css).toContain("  --sc-state-progress: var(--sc-brand);\n");
    expect(css).toContain("  --sc-state-locked: var(--sc-fg-soft);\n");
    expect(css).toContain('  --sc-font-ui: "Roboto", "Segoe UI", system-ui, sans-serif;\n');
    expect(cssColor({ l: 0.22, c: 0.014, h: 195, alpha: 0.45 })).toBe(
      "oklch(22% 0.014 195 / 0.45)",
    );
  });

  it("writes DTCG JSON the token build accepts, giving the same values in variables.css", async () => {
    const theme = generate(vzdcBrand).light;
    const built = await buildEdited("themes/vzdc-light.json", () =>
      JSON.stringify(themeToDtcg(theme), null, 2),
    );
    const block = built.variables.split('[data-sc-brand="vzdc"][data-sc-theme="light"] {')[1] ?? "";
    for (const [token, color] of allColors(theme)) {
      const name = variableName(`color.${token}`);
      const value = new RegExp(`${name}: (oklch\\([^)]*\\));`).exec(block)?.[1];
      if (!value) throw new Error(`variables.css has no ${name} for vzdc light`);
      expect(ref(color).deltaE(new Color(value), "OK"), token).toBeLessThan(0.001);
    }
  });
});
