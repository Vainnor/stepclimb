// SPDX-License-Identifier: AGPL-3.0-or-later
import { CheckCircleIcon } from "@phosphor-icons/react";
import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { Icon, icon } from "./icon";

describe("Icon", () => {
  it("draws the Phosphor icon as an svg with its data-slot, hidden when decorative", () => {
    const { container } = render(<Icon icon={CheckCircleIcon} />);
    const svg = container.querySelector("svg");
    expect(svg).toHaveAttribute("data-slot", "icon");
    expect(svg).toHaveAttribute("aria-hidden", "true");
    expect(svg).not.toHaveAttribute("role");
    expect(screen.queryByRole("img")).toBeNull();
  });

  it("is an image named by its label when it carries meaning on its own", () => {
    render(<Icon icon={CheckCircleIcon} label="Complete" />);
    const img = screen.getByRole("img", { name: "Complete" });
    expect(img).not.toHaveAttribute("aria-hidden");
  });

  it("draws available in the regular weight and complete in the fill weight", () => {
    // Phosphor draws each weight with different paths; compare against the icon drawn directly.
    const draw = (weight: "regular" | "fill") =>
      render(<CheckCircleIcon weight={weight} />)
        .container.querySelector("path")
        ?.getAttribute("d");
    const regular = draw("regular");
    const fill = draw("fill");
    expect(regular).not.toBe(fill);

    const pathOf = (el: HTMLElement) => el.querySelector("path")?.getAttribute("d");
    expect(pathOf(render(<Icon icon={CheckCircleIcon} />).container)).toBe(regular);
    expect(pathOf(render(<Icon icon={CheckCircleIcon} state="available" />).container)).toBe(
      regular,
    );
    expect(pathOf(render(<Icon icon={CheckCircleIcon} state="complete" />).container)).toBe(fill);
  });

  it("sizes from the token scale, or the surrounding text by default", () => {
    expect(icon()).toContain("size-[1em]");
    expect(icon({ size: "sm" })).toContain("size-4");
    expect(icon({ size: "lg" })).toContain("size-6");
    const { container } = render(<Icon icon={CheckCircleIcon} size="md" className="ms-1" />);
    expect(container.querySelector("svg")).toHaveClass("size-5", "ms-1");
  });

  it("is never a tab stop", () => {
    const { container } = render(<Icon icon={CheckCircleIcon} label="Complete" />);
    expect(container.querySelector("svg")).toHaveAttribute("focusable", "false");
  });
});
