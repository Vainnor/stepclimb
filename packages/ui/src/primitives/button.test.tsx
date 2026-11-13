// SPDX-License-Identifier: AGPL-3.0-or-later
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import { Button, button } from "./button";

describe("Button", () => {
  it("renders a native button with its data-slot", () => {
    render(<Button>Start quiz</Button>);
    const el = screen.getByRole("button", { name: "Start quiz" });
    expect(el.tagName).toBe("BUTTON");
    expect(el).toHaveAttribute("data-slot", "button");
  });

  it("is secondary unless primary is asked for", () => {
    render(<Button>Preview</Button>);
    const el = screen.getByRole("button", { name: "Preview" });
    expect(el).toHaveClass("bg-raised", "border-line-strong");
    expect(el).not.toHaveClass("bg-brand");
  });

  it("applies the intent and size recipes", () => {
    render(
      <Button intent="primary" size="sm">
        Continue
      </Button>,
    );
    const el = screen.getByRole("button", { name: "Continue" });
    expect(el).toHaveClass("bg-brand", "text-on-brand", "h-(--sc-control-h-sm)");
  });

  it("presses on click, Enter and Space", async () => {
    const user = userEvent.setup();
    const onPress = vi.fn();
    render(<Button onPress={onPress}>Save</Button>);
    const el = screen.getByRole("button", { name: "Save" });

    await user.click(el);
    el.focus();
    await user.keyboard("{Enter}");
    await user.keyboard(" ");

    expect(onPress).toHaveBeenCalledTimes(3);
  });

  it("marks keyboard focus with data-focus-visible, which the ring is styled from", async () => {
    const user = userEvent.setup();
    render(<Button>Skip</Button>);
    await user.tab();
    expect(screen.getByRole("button", { name: "Skip" })).toHaveAttribute("data-focus-visible");
  });

  it("does not press when disabled", async () => {
    const user = userEvent.setup();
    const onPress = vi.fn();
    render(
      <Button isDisabled onPress={onPress}>
        Delete attempt
      </Button>,
    );
    const el = screen.getByRole("button", { name: "Delete attempt" });

    await user.click(el);

    expect(onPress).not.toHaveBeenCalled();
    expect(el).toBeDisabled();
    expect(el).toHaveAttribute("data-disabled");
  });

  it("lets className add layout without dropping the recipe", () => {
    expect(button({ className: "ms-auto" })).toContain("ms-auto");
    expect(button({ className: "ms-auto" })).toContain("bg-raised");
  });
});
