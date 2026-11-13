// SPDX-License-Identifier: AGPL-3.0-or-later
import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { Kbd } from "./kbd";

describe("Kbd", () => {
  it("renders a kbd element in the mono face with its data-slot", () => {
    render(<Kbd>Ctrl K</Kbd>);
    const el = screen.getByText("Ctrl K");
    expect(el.tagName).toBe("KBD");
    expect(el).toHaveAttribute("data-slot", "kbd");
    expect(el).toHaveClass("font-mono");
  });

  it("reads left to right in a right-to-left page", () => {
    render(
      <div dir="rtl">
        <Kbd>Ctrl K</Kbd>
      </div>,
    );
    expect(screen.getByText("Ctrl K")).toHaveAttribute("dir", "ltr");
  });
});
