// SPDX-License-Identifier: AGPL-3.0-or-later
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { Button as AriaButton } from "react-aria-components";
import { describe, expect, it, vi } from "vitest";
import { Tooltip, TooltipTrigger } from "./tooltip";

describe("Tooltip", () => {
  it("opens on keyboard focus, describes its trigger, and closes on Escape", async () => {
    const user = userEvent.setup();
    render(
      <TooltipTrigger>
        <AriaButton>Due 14 Nov</AriaButton>
        <Tooltip>Thursday, 14 November, 23:59 UTC</Tooltip>
      </TooltipTrigger>,
    );
    await user.tab();
    const tip = await screen.findByRole("tooltip");
    expect(tip).toHaveAttribute("data-slot", "tooltip");
    expect(screen.getByRole("button")).toHaveAccessibleDescription(
      "Thursday, 14 November, 23:59 UTC",
    );
    await user.keyboard("{Escape}");
    expect(screen.queryByRole("tooltip")).toBeNull();
  });

  it("works controlled", () => {
    const onOpenChange = vi.fn();
    const { rerender } = render(
      <TooltipTrigger isOpen={false} onOpenChange={onOpenChange}>
        <AriaButton>Due</AriaButton>
        <Tooltip>Thursday</Tooltip>
      </TooltipTrigger>,
    );
    expect(screen.queryByRole("tooltip")).toBeNull();
    rerender(
      <TooltipTrigger isOpen onOpenChange={onOpenChange}>
        <AriaButton>Due</AriaButton>
        <Tooltip>Thursday</Tooltip>
      </TooltipTrigger>,
    );
    expect(screen.getByRole("tooltip")).toHaveTextContent("Thursday");
  });

  it("works uncontrolled from defaultOpen", () => {
    render(
      <TooltipTrigger defaultOpen>
        <AriaButton>Due</AriaButton>
        <Tooltip className="max-w-48">Thursday</Tooltip>
      </TooltipTrigger>,
    );
    expect(screen.getByRole("tooltip")).toHaveClass("max-w-48", "bg-fg-strong");
  });
});
