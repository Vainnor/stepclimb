// SPDX-License-Identifier: AGPL-3.0-or-later
import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { Text, text } from "./text";

describe("Text", () => {
  it("renders a span with its data-slot at the base size and default tone", () => {
    render(<Text>Due Friday</Text>);
    const el = screen.getByText("Due Friday");
    expect(el.tagName).toBe("SPAN");
    expect(el).toHaveAttribute("data-slot", "text");
    expect(el).toHaveClass("text-base", "text-fg-default");
  });

  it("renders the element elementType names", () => {
    render(<Text elementType="p">A paragraph</Text>);
    expect(screen.getByText("A paragraph").tagName).toBe("P");
  });

  it("applies a type scale step and a tone", () => {
    render(
      <Text size="caption" tone="sub">
        Updated today
      </Text>,
    );
    const el = screen.getByText("Updated today");
    expect(el).toHaveClass("text-caption", "text-fg-sub");
    expect(el).not.toHaveClass("text-base", "text-fg-default");
  });

  it("lets className add layout without dropping the recipe", () => {
    expect(text({ className: "ms-2" })).toContain("ms-2");
    expect(text({ className: "ms-2" })).toContain("text-fg-default");
  });
});
