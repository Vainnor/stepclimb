// SPDX-License-Identifier: AGPL-3.0-or-later
import { screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { renderWithStrings, unmarkedText } from "../test/strings";
import { Meter } from "./meter";

describe("Meter", () => {
  it("is a named meter with its value as a percentage from the catalog, in a neutral fill", () => {
    renderWithStrings(<Meter label="Storage used" value={720} maxValue={1000} />);
    const el = screen.getByRole("meter", { name: "Storage used" });
    expect(el).toHaveAttribute("data-slot", "meter");
    expect(el).toHaveAttribute("aria-valuetext", "72%");
    expect(el.querySelector('[data-slot="meter-value"]')).toHaveTextContent("72%");
    const fill = el.querySelector('[data-slot="meter-fill"]');
    expect(fill).toHaveStyle({ width: "72%" });
    expect(fill).toHaveClass("bg-fg-sub");
    expect(fill).not.toHaveClass("bg-brand");
  });

  it("takes its value label from the strings catalog (T2)", () => {
    const { container } = renderWithStrings(<Meter label="«caller»" value={10} />, {
      marked: true,
    });
    expect(screen.getByRole("meter")).toHaveAttribute("aria-valuetext", "«meter.value»");
    expect(unmarkedText(container)).toEqual([]);
  });
});
