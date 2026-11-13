// SPDX-License-Identifier: AGPL-3.0-or-later
import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { Separator } from "./separator";

describe("Separator", () => {
  it("is a horizontal hr in the line token with its data-slot", () => {
    render(<Separator />);
    const el = screen.getByRole("separator");
    expect(el.tagName).toBe("HR");
    expect(el).toHaveAttribute("data-slot", "separator");
    expect(el).toHaveClass("border-t", "border-line", "w-full");
  });

  it("is a vertical separator with its orientation exposed", () => {
    render(<Separator orientation="vertical" />);
    const el = screen.getByRole("separator");
    expect(el).toHaveAttribute("aria-orientation", "vertical");
    expect(el).toHaveClass("border-s", "self-stretch");
  });

  it("uses line-soft for row separators", () => {
    render(<Separator tone="soft" />);
    const el = screen.getByRole("separator");
    expect(el).toHaveClass("border-line-soft");
    expect(el).not.toHaveClass("border-line");
  });
});
