// SPDX-License-Identifier: AGPL-3.0-or-later
// The theme generator (src/generate.ts) against spec 12: B11 to B17, B18 and B19; T5 to T8, and
// the decided Q9 and Q11 (the default brand reproduces its themes in every appearance). Contrast
// is measured with colorjs.io, not with the generator's own function.
import Color from "colorjs.io";
import { describe, expect, it } from "vitest";
import { type Oklch, parseColor } from "../src/color";
import {
  type Appearance,
  appearances,
  type BrandInputs,
  brandTokens,
  colorTokens,
  cssColor,
  finalize,
  generate,
  type Theme,
  themeToCss,
  themeToDtcg,
  type UiFont,
} from "../src/generate";
import { variableName } from "../src/names";
import { contrastReport } from "../src/report";
import { brandThemes } from "../src/themes";
import { buildTokens, tokensDir } from "./support/build";
import { defaultThemes } from "./support/default-themes";
import { mulberry32 } from "./support/random";

/** The seed of the brand-color sweep; a failure prints the input, and this replays it. */
const SEED = 0x7e5c12;

type Value = (typeof defaultThemes)["light"]["brand"];
const fromValue = ([l, c, h, alpha]: Value): Oklch => ({
  l,
  c,
  h,
  ...(alpha === undefined ? {} : { alpha }),
});
const ref = (c: Oklch) => new Color("oklch", [c.l, c.c, c.h]);
/** The 8-bit sRGB color a display shows, computed with colorjs.io: clip, then 256 levels. */
const displayed = (c: Oklch) =>
  new Color(
    "srgb",
    ref(c)
      .to("srgb")
      .toGamut({ method: "clip" })
      .coords.map((x) => Math.round((x ?? 0) * 255) / 255) as [number, number, number],
  );
const contrast = (a: Oklch, b: Oklch) => displayed(a).contrastWCAG21(displayed(b));

/** The default brand's input: the plan's light brand value (spec 12, Q9). */
const defaultBrand: BrandInputs = { primary: "oklch(0.5 0.09 195)" };
const vzdcBrand: BrandInputs = { primary: "#500E0E", uiFont: "Roboto" };

/** The vZDC example's light rule as the generator writes it; the server's port must match it. */
const VZDC_RULE = `[data-sc-brand="vzdc"] {
  --sc-bg-canvas: oklch(98.5% 0.004 25.87);
  --sc-bg-raised: oklch(100% 0 0);
  --sc-bg-sunken: oklch(95.5% 0.007 25.87);
  --sc-bg-overlay: oklch(22% 0.014 25.87 / 0.45);
  --sc-fg-strong: oklch(22% 0.014 25.87);
  --sc-fg-default: oklch(31% 0.013 25.87);
  --sc-fg-sub: oklch(46% 0.012 25.87);
  --sc-fg-soft: oklch(53.3% 0.01 25.87);
  --sc-fg-disabled: oklch(70% 0.008 25.87);
  --sc-line-strong: oklch(63.3% 0.012 25.87);
  --sc-line: oklch(88.5% 0.009 25.87);
  --sc-line-soft: oklch(93% 0.006 25.87);
  --sc-brand: oklch(31% 0.0808 25.87);
  --sc-brand-hover: oklch(26% 0.0732 25.87);
  --sc-brand-soft: oklch(94.89% 0.0254 26.32);
  --sc-brand-fg: oklch(26% 0.0732 25.87);
  --sc-on-brand: oklch(98.97% 0.0049 26.6);
  --sc-ring: oklch(31% 0.0808 25.87);
  --sc-success: oklch(55% 0.12 150);
  --sc-success-soft: oklch(95% 0.03 150);
  --sc-success-fg: oklch(42% 0.1 150);
  --sc-warning: oklch(66.3% 0.1398 74.97);
  --sc-warning-soft: oklch(96% 0.04 85);
  --sc-warning-fg: oklch(47% 0.1 65);
  --sc-danger: oklch(55% 0.18 27);
  --sc-danger-soft: oklch(94.72% 0.0263 25.97);
  --sc-danger-fg: oklch(47% 0.16 27);
  --sc-info: oklch(55% 0.13 250);
  --sc-info-soft: oklch(94.89% 0.0263 244.03);
  --sc-info-fg: oklch(45.06% 0.1292 250.5);
  --sc-state-locked: var(--sc-fg-soft);
  --sc-state-available: var(--sc-fg-sub);
  --sc-state-progress: var(--sc-brand);
  --sc-state-complete: var(--sc-success);
  --sc-state-overdue: var(--sc-danger);
  --sc-state-review: var(--sc-warning);
  --sc-font-ui: "Roboto", "Segoe UI", system-ui, sans-serif;
  --sc-font-read: "Source Serif 4", "Georgia", "Times New Roman", serif;
  --sc-radius-xs: 3px;
  --sc-radius-sm: 5px;
  --sc-radius-md: 8px;
}
`;

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

/** The learning states' targets (spec 12, Q3), restated for the test. */
const stateTargets: Record<string, string> = {
  "state-locked": "fg-soft",
  "state-available": "fg-sub",
  "state-progress": "brand",
  "state-complete": "success",
  "state-overdue": "danger",
  "state-review": "warning",
};
const resolve = (t: Theme, token: string): Oklch =>
  t.colors[(stateTargets[token] ?? token) as keyof Theme["colors"]];

const allColors = (t: Theme) => colorTokens.map((token) => [token, t.colors[token]] as const);

describe("generate", () => {
  it.each(appearances)("reproduces the default brand's %s theme (Q9, Q11)", (appearance) => {
    const theme = generate(defaultBrand)[appearance];
    const expected = defaultThemes[appearance];
    expect(theme.appearance).toBe(appearance);
    expect(Object.keys(theme.colors).sort()).toEqual(Object.keys(expected).sort());
    for (const [token, color] of allColors(theme)) {
      const value = fromValue(expected[token]);
      // The value as colorjs.io maps it into sRGB (CSS Color 4).
      const mapped = ref(value).toGamut({ space: "srgb", method: "css" });
      expect(ref(color).deltaE(mapped, "OK"), token).toBeLessThan(2e-4);
      expect(color.alpha, token).toBe(value.alpha);
    }
    expect(theme.font.ui).toEqual([
      "Atkinson Hyperlegible Next",
      "Segoe UI",
      "system-ui",
      "sans-serif",
    ]);
  });

  it("is deterministic: the same inputs give byte-identical CSS and JSON (T5)", () => {
    for (const inputs of [defaultBrand, vzdcBrand, { primary: "#f8f000" }]) {
      for (const appearance of appearances) {
        const a = generate(inputs)[appearance];
        const b = generate({ ...inputs })[appearance];
        expect(themeToCss(a, ":root")).toBe(themeToCss(b, ":root"));
        expect(JSON.stringify(themeToDtcg(a))).toBe(JSON.stringify(themeToDtcg(b)));
      }
    }
  });

  it("emits only colors inside sRGB, for every brand in the sweep (T6, B13)", () => {
    for (const primary of sweep()) {
      const themes = generate({ primary });
      for (const appearance of appearances) {
        for (const [token, color] of allColors(themes[appearance])) {
          const at = `${primary} ${appearance} ${token} ${cssColor(color)}`;
          expect(ref(color).inGamut("srgb"), at).toBe(true);
        }
      }
    }
  });

  /** The contrast minimums per appearance (spec 12, B20 and Q10), restated for the test. */
  const minimum: Record<Appearance, { text: number; ui: number }> = {
    light: { text: 4.5, ui: 3 },
    dark: { text: 4.5, ui: 3 },
    "high-contrast": { text: 7, ui: 4.5 },
  };

  it.each(appearances)(
    "picks %s brand tokens that clear their contrast, for every brand in the sweep (B14, B15)",
    (appearance) => {
      const { text, ui } = minimum[appearance];
      // on-brand is the light end in light and high contrast, the dark end in dark (B14).
      const lightEnd = appearance !== "dark";
      for (const primary of sweep()) {
        const c = generate({ primary })[appearance].colors;
        const at = `${primary} ${appearance}: `;
        expect(contrast(c["on-brand"], c.brand), `${at}on-brand on brand`).toBeGreaterThanOrEqual(
          text,
        );
        expect(c["on-brand"].l > c.brand.l, `${at}on-brand's end`).toBe(lightEnd);
        expect(c["brand-hover"].l < c.brand.l, `${at}hover is past brand`).toBe(lightEnd);
        expect(contrast(c["brand-fg"], c["brand-soft"]), `${at}brand-fg`).toBeGreaterThanOrEqual(
          text,
        );
        expect(contrast(c["brand-fg"], c["bg-canvas"]), `${at}brand-fg`).toBeGreaterThanOrEqual(
          text,
        );
        expect(contrast(c.ring, c["bg-canvas"]), `${at}ring`).toBeGreaterThanOrEqual(ui);
        expect(contrast(c.ring, c["bg-raised"]), `${at}ring`).toBeGreaterThanOrEqual(ui);
        expect(
          Math.abs(c["brand-soft"].l - c["bg-canvas"].l) < Math.abs(c.brand.l - c["bg-canvas"].l),
          `${at}brand-soft is a tint near the canvas`,
        ).toBe(true);
      }
    },
  );

  it("passes its contrast report in every appearance, for every brand in the sweep (T7)", () => {
    for (const primary of sweep()) {
      const themes = generate({ primary });
      for (const appearance of appearances) {
        const report = contrastReport(themes[appearance]);
        const failing = report.pairs.filter((p) => !p.pass);
        expect(failing, `${primary} ${appearance}`).toEqual([]);
        // colorjs.io measures every pair as passing too, at the report's ratio within 0.05%,
        // so the report does not grade itself.
        for (const p of report.pairs) {
          const at = `${primary} ${appearance} ${p.foreground} on ${p.background}`;
          const measured = contrast(
            resolve(themes[appearance], p.foreground),
            resolve(themes[appearance], p.background),
          );
          expect(measured, at).toBeGreaterThanOrEqual(p.minimum);
          expect(Math.abs(p.ratio - measured) / measured, at).toBeLessThan(5e-4);
          expect(p.minimum).toBe(
            minimum[appearance][/^fg-|-fg$|^on-brand$/.test(p.foreground) ? "text" : "ui"],
          );
        }
      }
    }
  });

  it("derives dark and high contrast from the input's hue, not from a fixed palette (B16)", () => {
    const vzdc = generate(vzdcBrand);
    const teal = generate(defaultBrand);
    for (const appearance of ["dark", "high-contrast"] as const) {
      // Gamut mapping moves a hue by at most a degree here.
      expect(Math.abs(vzdc[appearance].colors.brand.h - 25.87), appearance).toBeLessThan(1);
      expect(Math.abs(teal[appearance].colors.brand.h - 195), appearance).toBeLessThan(1);
    }
    expect(vzdc.dark.colors["bg-canvas"].h).toBeCloseTo(25.87, 1);
    // High contrast's neutrals are grey for every brand.
    expect(vzdc["high-contrast"].colors["bg-sunken"]).toEqual({ l: 0.96, c: 0, h: 0 });
  });

  it("clears 4.5:1 on the displayed 8-bit colors where the exact ones barely pass", () => {
    // From the seeded sweep: before contrast was measured on 8-bit sRGB, this input's brand
    // cleared 4.5:1 only before quantization.
    const c = generate({ primary: "oklch(0.9083 0.2527 23.46)" }).light.colors;
    expect(contrast(c["on-brand"], c.brand)).toBeGreaterThanOrEqual(4.5);
    expect(c.brand).toEqual({ l: 0.5122, c: 0.2081, h: 25.98 });
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
    const statuses = ["success", "warning", "danger", "info"].flatMap(
      (st) => [st, `${st}-soft`, `${st}-fg`] as const,
    ) as (keyof Theme["colors"])[];
    const defaults = generate(defaultBrand);
    for (const primary of [vzdcBrand.primary, "#f8f000", "#0a1030"]) {
      const themes = generate({ primary });
      for (const appearance of appearances) {
        const theme = themes[appearance];
        for (const token of statuses) {
          expect(theme.colors[token], `${appearance} ${token}`).toEqual(
            defaults[appearance].colors[token],
          );
        }
        const dtcg = themeToDtcg(theme) as Record<string, Record<string, unknown>>;
        expect(Object.keys(dtcg).sort()).toEqual(["color", "font", "radius"]);
        expect(Object.keys(dtcg.font ?? {})).toEqual(["ui", "read"]);
        expect(Object.keys(dtcg.radius ?? {})).toEqual(["xs", "sm", "md"]);
        expect(themeToCss(theme, ":root")).not.toMatch(
          /--sc-(space|text|weight|control|leading|shadow|motion|radius-pill)/,
        );
      }
    }
  });

  it("uses the chosen UI font in every appearance and refuses others", () => {
    const roboto = ["Roboto", "Segoe UI", "system-ui", "sans-serif"];
    for (const appearance of appearances) {
      expect(generate(vzdcBrand)[appearance].font.ui).toEqual(roboto);
    }
    for (const uiFont of ["Comic Sans", "toString", "roboto"]) {
      expect(() => generate({ primary: "#500E0E", uiFont: uiFont as UiFont }), uiFont).toThrow(
        expect.objectContaining({ name: "BrandInputError", field: "uiFont" }),
      );
    }
  });

  it("refuses a primary color it cannot read, naming the field", () => {
    expect(() => generate({ primary: "dark red" })).toThrow(
      expect.objectContaining({ name: "BrandInputError", field: "primary" }),
    );
  });

  it("refuses a chroma above 0.5 and a hue too large to be finite, quickly", () => {
    const start = performance.now();
    for (const primary of [
      "oklch(0.5 0.5001 120)",
      "oklch(0.5 126% 120)",
      "oklch(0.5 2 120)",
      "oklch(0.5 1e100 120)",
      "oklch(0.5 1e150 120)",
      "oklch(0.5 1e400 120)",
      "oklch(0.5 0.1 1e400)",
      "oklch(0.5 0.1 -1e400)",
    ]) {
      expect(() => generate({ primary }), primary).toThrow(
        expect.objectContaining({ name: "BrandInputError", field: "primary" }),
      );
    }
    // A refused input never reaches gamut mapping, so the whole list takes well under a second.
    expect(performance.now() - start).toBeLessThan(1000);
  });

  it("generates a passing theme at the chroma cap in every appearance", () => {
    for (const hue of [0, 30, 145, 250, 330]) {
      const themes = generate({ primary: `oklch(0.6 0.5 ${hue})` });
      for (const appearance of appearances) {
        expect(contrastReport(themes[appearance]).pass, `${hue} ${appearance}`).toBe(true);
      }
    }
  });

  it("takes the reading font and corner style, in every appearance (Q8)", () => {
    const roboto = ["Roboto", "Segoe UI", "system-ui", "sans-serif"];
    const serif = ["Source Serif 4", "Georgia", "Times New Roman", "serif"];
    const cases: [BrandInputs, readonly string[], Record<string, number>][] = [
      [{ primary: "#500E0E" }, serif, { xs: 3, sm: 5, md: 8 }],
      [
        { primary: "#500E0E", uiFont: "Roboto", readingFont: "sans" },
        roboto,
        { xs: 3, sm: 5, md: 8 },
      ],
      [
        { primary: "#500E0E", readingFont: "serif", cornerStyle: "square" },
        serif,
        { xs: 0, sm: 0, md: 0 },
      ],
      [{ primary: "#500E0E", cornerStyle: "round" }, serif, { xs: 6, sm: 10, md: 16 }],
    ];
    for (const [inputs, read, radius] of cases) {
      for (const appearance of appearances) {
        const theme = generate(inputs)[appearance];
        expect(theme.font.read, JSON.stringify(inputs)).toEqual(read);
        expect(theme.radius, JSON.stringify(inputs)).toEqual(radius);
      }
    }
  });

  it("refuses a reading font or corner style that is not a choice, naming the field", () => {
    const bad: [Partial<BrandInputs>, string][] = [
      [{ readingFont: "mono" as never }, "readingFont"],
      [{ readingFont: "toString" as never }, "readingFont"],
      [{ cornerStyle: "pointy" as never }, "cornerStyle"],
      [{ cornerStyle: "hasOwnProperty" as never }, "cornerStyle"],
    ];
    for (const [inputs, field] of bad) {
      expect(() => generate({ primary: "#500E0E", ...inputs }), field).toThrow(
        expect.objectContaining({ name: "BrandInputError", field }),
      );
    }
  });
});

describe("brandTokens", () => {
  const grey = (l: number): Oklch => ({ l, c: 0, h: 0 });
  // Text steps on a light tint: brand is the 0.5 step (index 4), brand-fg aims at index 6 (0.4).
  const textScale = (soft: number) =>
    [1, soft, 0.6, 0.55, 0.5, 0.45, 0.4, 0.3, 0.25, 0.2, 0.15, 0.1].map(grey);
  // A light brand under black text, brand the 0.9 step (index 3); index 5 (0.6) is nearest that
  // could ring it, index 6 (0.45) the next.
  const ringScale = [0, 0.97, 0.95, 0.9, 0.8, 0.6, 0.45, 0.7, 0.85, 0.5, 0.75, 0.4].map(grey);

  it("rejects a brand-fg step that reads on the canvas but not on brand-soft", () => {
    expect(contrast(grey(0.4), grey(1))).toBeGreaterThanOrEqual(4.5);
    expect(contrast(grey(0.4), grey(0.75))).toBeLessThan(4.5);
    const t = brandTokens(textScale(0.75), 0.5, grey(1), grey(1));
    expect(t.brand).toEqual(grey(0.5));
    expect(t["brand-hover"]).toEqual(grey(0.45));
    expect(t["brand-fg"]).toEqual(grey(0.3));
  });

  it("rejects a brand-fg step that reads on brand-soft but not on the canvas", () => {
    expect(contrast(grey(0.4), grey(0.97))).toBeGreaterThanOrEqual(4.5);
    const t = brandTokens(textScale(0.97), 0.5, grey(0.75), grey(1));
    expect(t["brand-fg"]).toEqual(grey(0.3));
  });

  it("takes two past brand for brand-fg when it reads on both", () => {
    expect(brandTokens(textScale(0.97), 0.5, grey(1), grey(1))["brand-fg"]).toEqual(grey(0.4));
  });

  it("rejects a ring step that shows on the canvas but not on bg-raised", () => {
    expect(contrast(grey(0.6), grey(1))).toBeGreaterThanOrEqual(3);
    expect(contrast(grey(0.6), grey(0.85))).toBeLessThan(3);
    expect(contrast(grey(0.45), grey(0.85))).toBeGreaterThanOrEqual(3);
    const t = brandTokens(ringScale, 0.9, grey(1), grey(0.85));
    expect(t.brand).toEqual(grey(0.9));
    expect(t.ring).toEqual(grey(0.45));
  });

  it("rejects a ring step that shows on bg-raised but not on the canvas", () => {
    expect(brandTokens(ringScale, 0.9, grey(0.85), grey(1)).ring).toEqual(grey(0.45));
  });

  it("takes the nearest step to brand that shows on both for ring", () => {
    expect(brandTokens(ringScale, 0.9, grey(1), grey(1)).ring).toEqual(grey(0.6));
  });
});

describe("exact output", () => {
  it("writes the default brand's brand tokens byte for byte", () => {
    const css = themeToCss(generate(defaultBrand).light, ":root");
    for (const line of [
      "--sc-brand: oklch(50.22% 0.0856 195.13);",
      "--sc-brand-hover: oklch(45.62% 0.0777 195.43);",
      "--sc-brand-soft: oklch(95% 0.025 195);",
      "--sc-brand-fg: oklch(40.55% 0.0691 195.43);",
      "--sc-on-brand: oklch(99% 0.005 195);",
      "--sc-ring: oklch(50.22% 0.0856 195.13);",
      "--sc-bg-overlay: oklch(22% 0.014 195 / 0.45);",
    ]) {
      expect(css).toContain(`  ${line}\n`);
    }
  });

  it("writes the vZDC example's rule byte for byte", () => {
    expect(themeToCss(generate(vzdcBrand).light, '[data-sc-brand="vzdc"]')).toBe(VZDC_RULE);
  });

  it("writes a grey brand with hue 0 everywhere it has no chroma", () => {
    const css = themeToCss(generate({ primary: "#808080" }).light, ".grey");
    for (const line of [
      "--sc-bg-canvas: oklch(98.5% 0 0);",
      "--sc-fg-strong: oklch(22% 0 0);",
      "--sc-brand: oklch(50% 0 0);",
      "--sc-brand-hover: oklch(45% 0 0);",
      "--sc-brand-fg: oklch(40% 0 0);",
      "--sc-on-brand: oklch(99% 0 0);",
    ]) {
      expect(css).toContain(`  ${line}\n`);
    }
  });

  it("wraps a hue that rounds to 360 to 0", () => {
    expect(finalize({ l: 0.5, c: 0.05, h: 359.996 })).toEqual({ l: 0.5, c: 0.05, h: 0 });
    expect(finalize({ l: 0.5, c: 0.05, h: 359.994 })).toEqual({ l: 0.5, c: 0.05, h: 359.99 });
  });

  it("gives hue 0 to a color whose chroma rounds to 0, whatever the input's chroma", () => {
    // Only finalize's guard applies: the input is not grey, the step's chroma rounds away.
    expect(finalize({ l: 0.5, c: 0.00004, h: 120 })).toEqual({ l: 0.5, c: 0, h: 0 });
    // Only the input guard applies: below 0.0001 the input's hue is dropped even though the
    // brand step keeps a written chroma of 0.0001.
    expect(generate({ primary: "oklch(0.5 0.00009 120)" }).light.colors.brand).toEqual({
      l: 0.5,
      c: 0.0001,
      h: 0,
    });
    // Neither: at 0.00011 the input has a hue and keeps it.
    expect(generate({ primary: "oklch(0.5 0.00011 120)" }).light.colors.brand).toEqual({
      l: 0.5,
      c: 0.0001,
      h: 120,
    });
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
    // A brand the shipped themes do not use, put in vZDC's light context.
    for (const cornerStyle of ["square", "round"] as const) {
      const inputs: BrandInputs = { primary: "#f8f000", uiFont: "Roboto", cornerStyle };
      const theme = generate({ ...inputs, readingFont: "sans" }).light;
      const themes = brandThemes();
      themes.set("vzdc-light", theme);
      const built = await buildTokens(tokensDir, themes);
      const block =
        (built.variables.split('[data-sc-brand="vzdc"][data-sc-theme="light"] {')[1] ?? "").split(
          "}",
        )[0] ?? "";
      for (const [token, color] of allColors(theme)) {
        const name = variableName(`color.${token}`);
        const value = new RegExp(`${name}: (oklch\\([^)]*\\));`).exec(block)?.[1];
        if (!value) throw new Error(`variables.css has no ${name} for vzdc light`);
        expect(ref(color).deltaE(new Color(value), "OK"), token).toBeLessThan(0.001);
      }
      // The fonts and radii as the build writes them, line for line.
      const own = themeToCss(theme, "x").split("\n");
      for (const name of ["font-ui", "font-read", "radius-xs", "radius-sm", "radius-md"]) {
        const line = own.find((l) => l.startsWith(`  --sc-${name}:`))?.trim();
        expect(block, `${cornerStyle} ${name}`).toContain(`${line}\n`);
      }
    }
  });
});
