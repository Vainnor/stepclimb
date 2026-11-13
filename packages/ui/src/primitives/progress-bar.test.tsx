// SPDX-License-Identifier: AGPL-3.0-or-later
import { screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { renderWithStrings, unmarkedText } from "../test/strings";
import { fractionOf, ProgressBar } from "./progress-bar";

describe("fractionOf", () => {
  it.each([
    [40, 0, 100, 0.4],
    [3, 0, 7, 3 / 7],
    [-5, 0, 100, 0],
    [150, 0, 100, 1],
    [5, 5, 5, 0],
  ])("%d in %d to %d is %d", (value, min, max, expected) => {
    expect(fractionOf(value, min, max)).toBeCloseTo(expected);
  });
});

describe("ProgressBar", () => {
  it("is a named progressbar with its value as a percentage from the catalog", () => {
    renderWithStrings(<ProgressBar label="Ground control" value={40} />);
    const el = screen.getByRole("progressbar", { name: "Ground control" });
    expect(el).toHaveAttribute("data-slot", "progress-bar");
    expect(el).toHaveAttribute("aria-valuenow", "40");
    expect(el).toHaveAttribute("aria-valuetext", "40%");
    expect(el.querySelector('[data-slot="progress-bar-value"]')).toHaveTextContent("40%");
    expect(el.querySelector('[data-slot="progress-bar-fill"]')).toHaveStyle({ width: "40%" });
    expect(el.querySelector('[data-slot="progress-bar-fill"]')).toHaveClass("bg-brand");
  });

  it("measures within its own range", () => {
    renderWithStrings(<ProgressBar label="Modules" value={3} minValue={0} maxValue={4} />);
    const el = screen.getByRole("progressbar", { name: "Modules" });
    expect(el).toHaveAttribute("aria-valuetext", "75%");
    expect(el.querySelector('[data-slot="progress-bar-fill"]')).toHaveStyle({ width: "75%" });
  });

  it("takes a value label of its own", () => {
    renderWithStrings(<ProgressBar label="Modules" value={3} maxValue={7} valueLabel="3 of 7" />);
    expect(screen.getByRole("progressbar")).toHaveAttribute("aria-valuetext", "3 of 7");
  });

  it("takes its value label from the strings catalog (T2)", () => {
    const { container } = renderWithStrings(<ProgressBar label="«caller»" value={40} />, {
      marked: true,
    });
    expect(screen.getByRole("progressbar")).toHaveAttribute(
      "aria-valuetext",
      "«progressBar.value»",
    );
    expect(unmarkedText(container)).toEqual([]);
  });
});
