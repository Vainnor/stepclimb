// SPDX-License-Identifier: AGPL-3.0-or-later
import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { Heading, headingSizes } from "./heading";

describe("Heading", () => {
  it.each([1, 2, 3, 4, 5, 6] as const)("renders level %i as its h element", (level) => {
    render(<Heading level={level}>Ground control</Heading>);
    const el = screen.getByRole("heading", { level, name: "Ground control" });
    expect(el.tagName).toBe(`H${level}`);
    expect(el).toHaveAttribute("data-slot", "heading");
    expect(el).toHaveClass(`text-${headingSizes[level]}`, "font-heading", "text-fg-strong");
  });

  it("keeps its level when asked for another size", () => {
    render(
      <Heading level={2} size="lg">
        Taxi instructions
      </Heading>,
    );
    const el = screen.getByRole("heading", { level: 2, name: "Taxi instructions" });
    expect(el).toHaveClass("text-lg");
    expect(el).not.toHaveClass("text-2xl");
  });

  it("gives smaller sizes to deeper levels", () => {
    const order = ["base", "lg", "xl", "2xl", "3xl", "4xl"];
    const rank = (level: 1 | 2 | 3 | 4 | 5 | 6) => order.indexOf(headingSizes[level]);
    expect(rank(1)).toBeGreaterThan(rank(2));
    expect(rank(2)).toBeGreaterThan(rank(3));
    expect(rank(3)).toBeGreaterThan(rank(4));
    expect(rank(4)).toBeGreaterThanOrEqual(rank(5));
  });
});
