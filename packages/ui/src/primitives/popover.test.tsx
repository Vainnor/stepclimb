// SPDX-License-Identifier: AGPL-3.0-or-later
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { stubMatchMedia } from "../test/media";
import { ThemeProvider } from "../theme/theme-provider";
import { Button } from "./button";
import { Heading } from "./heading";
import { Popover, PopoverDialog, PopoverTrigger } from "./popover";

function Example(props: { isOpen?: boolean; onOpenChange?: (open: boolean) => void }) {
  return (
    <PopoverTrigger {...props}>
      <Button>Details</Button>
      <Popover>
        <PopoverDialog>
          <Heading slot="title" level={2} size="base">
            Pass mark
          </Heading>
          <p>80 percent, best of three attempts.</p>
          <Button>Close</Button>
        </PopoverDialog>
      </Popover>
    </PopoverTrigger>
  );
}

describe("Popover", () => {
  beforeEach(() => stubMatchMedia(() => false));
  afterEach(() => vi.unstubAllGlobals());

  it("opens from its trigger as a dialog named by its heading, with its data-slots", async () => {
    const user = userEvent.setup();
    render(<Example />);
    await user.click(screen.getByRole("button", { name: "Details" }));
    const dialog = screen.getByRole("dialog", { name: "Pass mark" });
    expect(dialog).toHaveAttribute("data-slot", "popover-dialog");
    expect(dialog.closest('[data-slot="popover"]')).toHaveClass("shadow-float", "bg-raised");
  });

  it("moves focus in, and closes on Escape with focus back on the trigger", async () => {
    const user = userEvent.setup();
    render(<Example />);
    const trigger = screen.getByRole("button", { name: "Details" });
    await user.click(trigger);
    expect(screen.getByRole("dialog")).toContainElement(document.activeElement as HTMLElement);
    await user.keyboard("{Escape}");
    expect(screen.queryByRole("dialog")).toBeNull();
    await waitFor(() => expect(trigger).toHaveFocus());
  });

  it("works controlled", async () => {
    const user = userEvent.setup();
    const onOpenChange = vi.fn();
    const { rerender } = render(<Example isOpen={false} onOpenChange={onOpenChange} />);
    await user.click(screen.getByRole("button", { name: "Details" }));
    expect(onOpenChange).toHaveBeenCalledWith(true);
    expect(screen.queryByRole("dialog")).toBeNull();
    rerender(<Example isOpen onOpenChange={onOpenChange} />);
    expect(screen.getByRole("dialog")).toBeInTheDocument();
  });

  it("draws in the surrounding theme, though it is portaled out of it", async () => {
    const user = userEvent.setup();
    render(
      <ThemeProvider theme="high-contrast" density="compact">
        <Example />
      </ThemeProvider>,
    );
    await user.click(screen.getByRole("button", { name: "Details" }));
    const popover = screen.getByRole("dialog").closest('[data-slot="popover"]');
    expect(popover?.closest('[data-slot="theme-provider"]')).toBeNull();
    expect(popover).toHaveAttribute("data-sc-theme", "high-contrast");
    expect(popover).toHaveAttribute("data-sc-density", "compact");
    expect(popover).not.toHaveAttribute("data-sc-brand");
  });
});
