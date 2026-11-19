// SPDX-License-Identifier: AGPL-3.0-or-later
import { generate } from "@stepclimb/tokens/generate";
import { contrastReport } from "@stepclimb/tokens/report";
import { screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it } from "vitest";
import { renderWithStrings, unmarkedText } from "../test/strings";
import { BrandPreview } from "./brand-preview";

const appearances = ["light", "dark", "high-contrast"] as const;

function draft(primary: string) {
  const themes = generate({ primary });
  return { themes, reports: appearances.map((a) => contrastReport(themes[a])) };
}

/**
 * A brand the generator refuses (m0/12 T9's doctored theme): light `fg-soft` moved onto the
 * canvas's lightness, so it fails 4.5:1 on `bg-canvas` and the other surfaces.
 */
function refused() {
  const themes = generate({ primary: "#007373" });
  const canvas = themes.light.colors["bg-canvas"];
  themes.light.colors["fg-soft"] = { ...canvas, l: canvas.l - 0.12 };
  return { themes, reports: appearances.map((a) => contrastReport(themes[a])) };
}

describe("BrandPreview", () => {
  it("draws the sample in each appearance with the draft's own tokens", () => {
    const { themes, reports } = draft("#500E0E");
    renderWithStrings(<BrandPreview themes={themes} reports={reports} />);
    const samples = document.querySelectorAll<HTMLElement>('[data-slot="brand-preview-sample"]');
    expect([...samples].map((s) => s.dataset.scTheme)).toEqual([...appearances]);
    for (const sample of samples) {
      const appearance = sample.dataset.appearance as (typeof appearances)[number];
      expect(sample.style.getPropertyValue("--sc-brand")).not.toBe("");
      expect(sample.style.getPropertyValue("--sc-state-progress")).toBe("var(--sc-brand)");
      // The sample is a picture: out of the tab order and the accessibility tree.
      expect(sample).toHaveAttribute("inert");
      expect(themes[appearance]).toBeDefined();
    }
    expect(samples[0]?.style.getPropertyValue("--sc-brand")).not.toBe(
      samples[1]?.style.getPropertyValue("--sc-brand"),
    );
    for (const name of ["Light", "Dark", "High contrast"]) {
      expect(screen.getByText(name, { selector: "figcaption" })).toBeInTheDocument();
    }
  });

  it("says a passing brand can be saved, and lists every pair as passing", () => {
    const { themes, reports } = draft("#007373");
    renderWithStrings(<BrandPreview themes={themes} reports={reports} />);
    const report = screen.getByRole("region", { name: "Contrast report" });
    expect(report).toHaveAttribute("data-pass", "true");
    expect(within(report).getByRole("status")).toHaveTextContent(
      "Every color pair passes in light, dark and high contrast, so these colors can be saved.",
    );
    const rows = within(screen.getByRole("grid", { name: "Light" })).getAllByRole("row");
    expect(rows.length).toBe((reports[0]?.pairs.length ?? 0) + 1);
    expect(within(report).queryByText("Fails")).toBeNull();
  });

  // Spec 16 T6: the generator's contrast failures for a brand it refuses.
  it("shows the generator's failures for a brand it refuses, failing pairs first", () => {
    const { themes, reports } = refused();
    const failing = reports[0]?.pairs.filter((p) => !p.pass) ?? [];
    expect(failing.map((p) => p.foreground)).toContain("fg-soft");
    renderWithStrings(<BrandPreview themes={themes} reports={reports} />);
    const report = screen.getByRole("region", { name: "Contrast report" });
    expect(report).toHaveAttribute("data-pass", "false");
    expect(within(report).getByRole("status")).toHaveTextContent(
      `${failing.length} color pairs are too close in contrast. Choose another brand color: these colors cannot be saved.`,
    );
    const light = screen.getByRole("tab", { name: `Light, ${failing.length} fail` });
    expect(light).toHaveAttribute("aria-selected", "true");
    const rows = within(screen.getByRole("grid", { name: "Light" }))
      .getAllByRole("row")
      .slice(1);
    for (const [index, pair] of failing.entries()) {
      const row = rows[index];
      if (!row) throw new Error("a row is missing");
      expect(row).toHaveTextContent(`${pair.foreground} on ${pair.background}`);
      expect(row).toHaveTextContent("Fails");
      expect(row).toHaveTextContent(`${pair.minimum}:1`);
    }
    expect(rows[failing.length]).toHaveTextContent("Passes");
    expect(screen.getByRole("tab", { name: "Dark" })).toHaveAttribute("aria-selected", "false");
  });

  it("cuts a ratio to two decimals, so a failing pair never reads as its minimum", () => {
    const { themes, reports } = draft("#007373");
    const [light, ...rest] = reports;
    if (!light) throw new Error("no light report");
    const pair = { foreground: "fg-soft", background: "bg-canvas", ratio: 4.4999, minimum: 4.5 };
    const doctored = { ...light, pass: false, pairs: [{ ...pair, pass: false }] };
    renderWithStrings(<BrandPreview themes={themes} reports={[doctored, ...rest]} />);
    const row = within(screen.getByRole("grid", { name: "Light" })).getAllByRole("row")[1];
    expect(row).toHaveTextContent("4.49:1");
    expect(row).toHaveTextContent("4.5:1");
  });

  it("names the brand-near-danger warning in words and moves between appearances", async () => {
    const { themes, reports } = draft("#500E0E");
    expect(reports.some((r) => r.warnings.length > 0)).toBe(true);
    renderWithStrings(<BrandPreview themes={themes} reports={reports} />);
    const report = screen.getByRole("region", { name: "Contrast report" });
    expect(within(report).getByRole("note")).toHaveTextContent(
      /close in hue to the color for errors/,
    );
    await userEvent.click(screen.getByRole("tab", { name: "High contrast" }));
    expect(screen.getByRole("grid", { name: "High contrast" })).toBeInTheDocument();
  });

  it("renders only catalog messages and the report's token names", () => {
    const { themes, reports } = refused();
    const { container } = renderWithStrings(<BrandPreview themes={themes} reports={reports} />, {
      marked: true,
    });
    expect(unmarkedText(container)).toEqual([]);
  });
});
