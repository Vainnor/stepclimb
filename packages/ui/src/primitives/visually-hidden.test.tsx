// SPDX-License-Identifier: AGPL-3.0-or-later
import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { VisuallyHidden } from "./visually-hidden";

describe("VisuallyHidden", () => {
  it("keeps its content for screen readers while drawing nothing", () => {
    render(
      <button type="button">
        <svg aria-hidden="true" />
        <VisuallyHidden>Close</VisuallyHidden>
      </button>,
    );
    expect(screen.getByRole("button", { name: "Close" })).toBeInTheDocument();
    const el = screen.getByText("Close");
    expect(el.tagName).toBe("SPAN");
    expect(el).toHaveAttribute("data-slot", "visually-hidden");
    expect(el.style.position).toBe("absolute");
    expect(el.style.width).toBe("1px");
    expect(el.style.overflow).toBe("hidden");
  });

  it("renders the element elementType names, and a part's own data-slot", () => {
    render(
      <VisuallyHidden elementType="div" data-slot="link-external">
        external
      </VisuallyHidden>,
    );
    const el = screen.getByText("external");
    expect(el.tagName).toBe("DIV");
    expect(el).toHaveAttribute("data-slot", "link-external");
  });
});
