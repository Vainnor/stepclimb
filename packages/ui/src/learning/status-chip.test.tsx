// SPDX-License-Identifier: AGPL-3.0-or-later
import { screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { renderWithStrings, unmarkedText } from "../test/strings";
import { learningStates, StatusChip } from "./status-chip";

describe("StatusChip", () => {
  it("shows each state's label from the catalog, with its data-slots", () => {
    const labels = {
      locked: "Locked",
      available: "Available",
      progress: "In progress",
      complete: "Complete",
      overdue: "Overdue",
      review: "Needs review",
    } as const;
    for (const state of learningStates) {
      const { container, unmount } = renderWithStrings(<StatusChip state={state} />);
      const chip = container.querySelector('[data-slot="status-chip"]');
      expect(chip).toHaveAttribute("data-state", state);
      expect(screen.getByText(labels[state])).toHaveAttribute("data-slot", "status-chip-label");
      unmount();
    }
  });

  it("draws each state with its own token on the icon", () => {
    for (const state of learningStates) {
      const { container, unmount } = renderWithStrings(<StatusChip state={state} />);
      expect(container.querySelector('[data-slot="icon"]')).toHaveClass(`text-state-${state}`);
      unmount();
    }
  });

  it("tells the states apart without color: a different icon and label each (T2)", () => {
    const seen = learningStates.map((state) => {
      const { container, unmount } = renderWithStrings(<StatusChip state={state} />);
      const icon = container.querySelector('[data-slot="icon"]');
      // The icon's drawing, without the class that carries its color.
      const drawing = icon?.innerHTML ?? "";
      const label = container.textContent ?? "";
      unmount();
      return { drawing, label };
    });
    expect(new Set(seen.map((s) => s.drawing)).size).toBe(learningStates.length);
    expect(new Set(seen.map((s) => s.label)).size).toBe(learningStates.length);
    expect(seen.every((s) => s.drawing.length > 0)).toBe(true);
  });

  it("draws complete in the fill weight and available in the regular one", () => {
    const { container: complete } = renderWithStrings(<StatusChip state="complete" />);
    const { container: available } = renderWithStrings(<StatusChip state="available" />);
    const completeIcon = complete.querySelector('[data-slot="icon"]')?.innerHTML;
    const availableIcon = available.querySelector('[data-slot="icon"]')?.innerHTML;
    expect(completeIcon).not.toEqual(availableIcon);
  });

  it("keeps a hidden label for screen readers", () => {
    renderWithStrings(<StatusChip state="overdue" labelHidden />);
    const label = screen.getByText("Overdue");
    expect(label).toHaveAttribute("data-slot", "status-chip-label");
    // React Aria's VisuallyHidden clips the label to 1 px instead of removing it.
    expect(label.style.clip).not.toBe("");
  });

  it("renders only catalog messages", () => {
    const { container } = renderWithStrings(<StatusChip state="review" />, { marked: true });
    expect(unmarkedText(container)).toEqual([]);
  });
});
