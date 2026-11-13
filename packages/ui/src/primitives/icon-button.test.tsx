// SPDX-License-Identifier: AGPL-3.0-or-later
import { GearIcon } from "@phosphor-icons/react";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { stubMatchMedia } from "../test/media";
import { ThemeProvider } from "../theme/theme-provider";
import { IconButton, iconButton } from "./icon-button";

describe("IconButton", () => {
  beforeEach(() => stubMatchMedia(() => false));
  afterEach(() => vi.unstubAllGlobals());

  it("is a button named by its label, with a hidden icon and its data-slot", () => {
    render(<IconButton icon={GearIcon} label="Course settings" />);
    const el = screen.getByRole("button", { name: "Course settings" });
    expect(el).toHaveAttribute("data-slot", "icon-button");
    expect(el.querySelector('[data-slot="icon"]')).toHaveAttribute("aria-hidden", "true");
    expect(el).toHaveClass("aspect-square", "px-0");
  });

  it("is ghost by default and takes another intent", () => {
    render(
      <>
        <IconButton icon={GearIcon} label="Ghost" />
        <IconButton icon={GearIcon} label="Primary" intent="primary" />
      </>,
    );
    expect(screen.getByRole("button", { name: "Ghost" })).not.toHaveClass("bg-raised");
    expect(screen.getByRole("button", { name: "Primary" })).toHaveClass("bg-brand");
  });

  it("stays square at every size, whatever padding the size gives a Button", () => {
    for (const size of ["sm", "md", "lg"] as const) {
      const classes = iconButton({ size }).split(" ");
      expect(classes).toContain("px-0");
      expect(classes.filter((c) => /^px-/.test(c))).toEqual(["px-0"]);
    }
  });

  it("shows its label as a tooltip on keyboard focus", async () => {
    const user = userEvent.setup();
    render(<IconButton icon={GearIcon} label="Course settings" />);
    await user.tab();
    const tip = await screen.findByRole("tooltip");
    expect(tip).toHaveTextContent("Course settings");
    expect(tip).toHaveAttribute("data-slot", "icon-button-tooltip");
  });

  it("shows its label as a tooltip on hover", async () => {
    const user = userEvent.setup();
    render(<IconButton icon={GearIcon} label="Course settings" />);
    // React Aria shows tooltips on hover once the pointer is the input in use.
    await user.click(document.body);
    await user.hover(screen.getByRole("button"));
    const tip = await screen.findByRole("tooltip", {}, { timeout: 3000 });
    expect(tip).toHaveTextContent("Course settings");
  });

  it("draws the tooltip in the surrounding theme, though it is portaled out of it", async () => {
    const user = userEvent.setup();
    render(
      <ThemeProvider theme="dark" density="compact" brand="vzdc">
        <IconButton icon={GearIcon} label="Course settings" />
      </ThemeProvider>,
    );
    await user.tab();
    const tip = await screen.findByRole("tooltip");
    expect(tip.closest('[data-slot="theme-provider"]')).toBeNull();
    expect(tip).toHaveAttribute("data-sc-theme", "dark");
    expect(tip).toHaveAttribute("data-sc-density", "compact");
    expect(tip).toHaveAttribute("data-sc-brand", "vzdc");
  });

  it("presses on click, Enter and Space, and not when disabled", async () => {
    const user = userEvent.setup();
    const onPress = vi.fn();
    const { rerender } = render(
      <IconButton icon={GearIcon} label="Course settings" onPress={onPress} />,
    );
    const el = screen.getByRole("button");
    await user.click(el);
    el.focus();
    await user.keyboard("{Enter}");
    await user.keyboard(" ");
    expect(onPress).toHaveBeenCalledTimes(3);

    rerender(<IconButton icon={GearIcon} label="Course settings" onPress={onPress} isDisabled />);
    await user.click(el);
    expect(onPress).toHaveBeenCalledTimes(3);
  });
});
