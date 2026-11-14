// SPDX-License-Identifier: AGPL-3.0-or-later
// The contrast report (src/report.ts) against spec 12, B20 and T9: the pairs of B20's table, the
// minimums per appearance (Q10), the learning states measured through their aliases, and the
// brand-near-danger warning. Ratios are checked with colorjs.io, not the report's own function.
import Color from "colorjs.io";
import { describe, expect, it } from "vitest";
import type { Oklch } from "../src/color";
import { generate, type Theme } from "../src/generate";
import { contrastReport, describeFailures } from "../src/report";

const displayed = (c: Oklch) =>
  new Color(
    "srgb",
    new Color("oklch", [c.l, c.c, c.h])
      .to("srgb")
      .toGamut({ method: "clip" })
      .coords.map((x) => Math.round((x ?? 0) * 255) / 255) as [number, number, number],
  );
const contrast = (a: Oklch, b: Oklch) => displayed(a).contrastWCAG21(displayed(b));

const defaults = generate({ primary: "oklch(0.5 0.09 195)" });
const withColors = (theme: Theme, colors: Partial<Theme["colors"]>): Theme => ({
  ...theme,
  colors: { ...theme.colors, ...colors },
});

/** B20's table, restated: [foreground, background, "text" or "ui"]; 43 pairs. */
const b20: [string, string, "text" | "ui"][] = [
  ...["fg-strong", "fg-default", "fg-sub", "fg-soft"].flatMap((fg) =>
    ["bg-canvas", "bg-raised", "bg-sunken"].map((bg): [string, string, "text"] => [fg, bg, "text"]),
  ),
  ["on-brand", "brand", "text"],
  ["on-brand", "brand-hover", "text"],
  ["brand-fg", "brand-soft", "text"],
  ["brand-fg", "bg-canvas", "text"],
  ...["success", "warning", "danger", "info"].flatMap((s): [string, string, "text"][] => [
    [`${s}-fg`, `${s}-soft`, "text"],
    [`${s}-fg`, "bg-canvas", "text"],
  ]),
  ...[
    "brand",
    "ring",
    "line-strong",
    ...["locked", "available", "progress", "complete", "overdue", "review"].map(
      (s) => `state-${s}`,
    ),
  ].flatMap((fg): [string, string, "ui"][] => [
    [fg, "bg-canvas", "ui"],
    [fg, "bg-raised", "ui"],
  ]),
  // Owner decision (13 Nov 2026): input borders also on wells and inputs.
  ["line-strong", "bg-sunken", "ui"],
];

describe("contrastReport", () => {
  it("measures exactly B20's pairs; fg-disabled is exempt", () => {
    const report = contrastReport(defaults.light);
    const measured = report.pairs.map((p) => `${p.foreground} on ${p.background}`).sort();
    expect(measured).toEqual(b20.map(([fg, bg]) => `${fg} on ${bg}`).sort());
    expect(measured.some((p) => p.includes("fg-disabled"))).toBe(false);
  });

  it.each([
    ["light", 4.5, 3],
    ["dark", 4.5, 3],
    ["high-contrast", 7, 4.5],
  ] as const)("holds %s to %d:1 for text and %d:1 for UI (Q10)", (appearance, text, ui) => {
    const report = contrastReport(defaults[appearance]);
    expect(report.appearance).toBe(appearance);
    for (const [fg, bg, kind] of b20) {
      const pair = report.pairs.find((p) => p.foreground === fg && p.background === bg);
      expect(pair?.minimum, `${fg} on ${bg}`).toBe(kind === "text" ? text : ui);
    }
  });

  it("reports each ratio as colorjs.io measures it", () => {
    const report = contrastReport(defaults.dark);
    const fgSoft = report.pairs.find(
      (p) => p.foreground === "fg-soft" && p.background === "bg-sunken",
    );
    const expected = contrast(defaults.dark.colors["fg-soft"], defaults.dark.colors["bg-sunken"]);
    expect(Math.abs((fgSoft?.ratio ?? 0) - expected) / expected).toBeLessThan(5e-4);
  });

  it("fails a hand-built theme with fg-soft at 4.4:1 on bg-canvas on that pair only (T9)", () => {
    // bg-sunken is set to white so that fg-soft stays above 4.5:1 there.
    const canvas = defaults.light.colors["bg-canvas"];
    let l = 0.5;
    while (contrast({ l, c: 0, h: 0 }, canvas) >= 4.45) l += 0.0005;
    const fgSoft = { l, c: 0, h: 0 };
    expect(contrast(fgSoft, canvas)).toBeGreaterThanOrEqual(4.4);
    expect(contrast(fgSoft, canvas)).toBeLessThan(4.45);
    const theme = withColors(defaults.light, {
      "fg-soft": fgSoft,
      "bg-sunken": defaults.light.colors["bg-raised"],
    });
    const report = contrastReport(theme);
    expect(report.pass).toBe(false);
    const failing = report.pairs.filter((p) => !p.pass);
    expect(failing).toEqual([
      {
        foreground: "fg-soft",
        background: "bg-canvas",
        ratio: expect.closeTo(4.4, 1),
        minimum: 4.5,
        pass: false,
      },
    ]);
    expect(describeFailures(report)).toEqual([
      `fg-soft on bg-canvas: ${failing[0]?.ratio.toFixed(2)}:1, needs 4.5:1`,
    ]);
  });

  it("measures a learning state through its alias", () => {
    // warning is in no pair of its own; state-review, its alias, is.
    const pale = { l: 0.85, c: 0.1, h: 75 };
    const report = contrastReport(withColors(defaults.light, { warning: pale }));
    expect(report.pairs.filter((p) => !p.pass).map((p) => [p.foreground, p.background])).toEqual([
      ["state-review", "bg-canvas"],
      ["state-review", "bg-raised"],
    ]);
  });

  it("passes the generated default brand in every appearance", () => {
    for (const theme of Object.values(defaults)) {
      expect(describeFailures(contrastReport(theme)), theme.appearance).toEqual([]);
      expect(contrastReport(theme).pass).toBe(true);
    }
  });
});

describe("the brand-near-danger warning", () => {
  const warned = (primary: string) =>
    Object.values(generate({ primary })).map((t) => contrastReport(t).warnings.map((w) => w.code));

  it("warns when the brand's hue is within 30 degrees of danger's, in every appearance", () => {
    expect(warned("#500E0E")).toEqual([
      ["brand-near-danger"],
      ["brand-near-danger"],
      ["brand-near-danger"],
    ]);
    // danger's hue is about 27 (light) and 25 (dark); 50 is about 23 away.
    expect(warned("oklch(0.5 0.15 50)").every((w) => w.length === 1)).toBe(true);
    const report = contrastReport(generate({ primary: "#500E0E" }).light);
    expect(report.warnings[0]?.message).toMatch(/^brand's hue is \d+\.\d degrees from danger's/);
  });

  it("does not warn for a distant hue or a grey brand, and never fails the theme", () => {
    expect(warned("oklch(0.5 0.09 195)")).toEqual([[], [], []]);
    expect(warned("oklch(0.5 0.15 80)")).toEqual([[], [], []]);
    expect(warned("#808080")).toEqual([[], [], []]);
    expect(contrastReport(generate({ primary: "#500E0E" }).light).pass).toBe(true);
  });
});
