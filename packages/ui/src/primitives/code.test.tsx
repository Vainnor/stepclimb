// SPDX-License-Identifier: AGPL-3.0-or-later
import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { Code } from "./code";

describe("Code", () => {
  it("renders a code element in the mono face with its data-slot", () => {
    render(<Code>auth.flow_expired</Code>);
    const el = screen.getByText("auth.flow_expired");
    expect(el.tagName).toBe("CODE");
    expect(el).toHaveAttribute("data-slot", "code");
    expect(el).toHaveClass("font-mono");
  });

  it("reads left to right in a right-to-left page", () => {
    render(
      <div dir="rtl">
        <Code>org.none</Code>
      </div>,
    );
    expect(screen.getByText("org.none")).toHaveAttribute("dir", "ltr");
  });
});
