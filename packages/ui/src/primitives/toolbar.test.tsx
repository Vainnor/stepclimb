// SPDX-License-Identifier: AGPL-3.0-or-later
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it } from "vitest";
import { Button } from "./button";
import { Separator } from "./separator";
import { ToggleButton } from "./toggle-button";
import { Toolbar } from "./toolbar";

function Example() {
  return (
    <>
      <Toolbar aria-label="Formatting">
        <ToggleButton>Bold</ToggleButton>
        <ToggleButton>Italic</ToggleButton>
        <Separator orientation="vertical" />
        <Button>Insert link</Button>
      </Toolbar>
      <Button>After</Button>
    </>
  );
}

describe("Toolbar", () => {
  it("is a named toolbar with its data-slot", () => {
    render(<Example />);
    const el = screen.getByRole("toolbar", { name: "Formatting" });
    expect(el).toHaveAttribute("data-slot", "toolbar");
    expect(el).toHaveAttribute("data-orientation", "horizontal");
  });

  it("moves between items with the arrow keys and skips the separator (T12)", async () => {
    const user = userEvent.setup();
    render(<Example />);
    await user.tab();
    expect(screen.getByRole("button", { name: "Bold" })).toHaveFocus();
    await user.keyboard("{ArrowRight}");
    expect(screen.getByRole("button", { name: "Italic" })).toHaveFocus();
    await user.keyboard("{ArrowRight}");
    expect(screen.getByRole("button", { name: "Insert link" })).toHaveFocus();
    await user.keyboard("{ArrowLeft}");
    expect(screen.getByRole("button", { name: "Italic" })).toHaveFocus();
  });

  it("is one tab stop: Tab leaves it", async () => {
    const user = userEvent.setup();
    render(<Example />);
    await user.tab();
    await user.tab();
    expect(screen.getByRole("button", { name: "After" })).toHaveFocus();
  });
});
