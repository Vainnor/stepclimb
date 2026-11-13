// SPDX-License-Identifier: AGPL-3.0-or-later
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import { ToggleButton, ToggleButtonGroup } from "./toggle-button";

describe("ToggleButton", () => {
  it("is a pressed-state button with its data-slot", async () => {
    const user = userEvent.setup();
    render(<ToggleButton>Show answers</ToggleButton>);
    const el = screen.getByRole("button", { name: "Show answers" });
    expect(el).toHaveAttribute("data-slot", "toggle-button");
    expect(el).toHaveAttribute("aria-pressed", "false");
    await user.click(el);
    expect(el).toHaveAttribute("aria-pressed", "true");
    expect(el).toHaveAttribute("data-selected", "true");
    expect(el).toHaveClass("data-[selected]:bg-brand-soft");
  });

  it("frames a selected ghost button, not only tints it", () => {
    render(
      <ToggleButton intent="ghost" defaultSelected>
        Show answers
      </ToggleButton>,
    );
    const el = screen.getByRole("button", { name: "Show answers" });
    expect(el).toHaveClass(
      "border",
      "border-transparent",
      "data-[selected]:border-brand",
      "data-[selected]:inset-ring",
      "data-[selected]:inset-ring-brand",
    );
  });

  it("toggles with Enter and Space", async () => {
    const user = userEvent.setup();
    render(<ToggleButton defaultSelected>Bold</ToggleButton>);
    const el = screen.getByRole("button", { name: "Bold" });
    el.focus();
    await user.keyboard("{Enter}");
    expect(el).toHaveAttribute("aria-pressed", "false");
    await user.keyboard(" ");
    expect(el).toHaveAttribute("aria-pressed", "true");
  });

  it("works controlled", async () => {
    const user = userEvent.setup();
    const onChange = vi.fn();
    render(
      <ToggleButton isSelected={false} onChange={onChange}>
        Bold
      </ToggleButton>,
    );
    const el = screen.getByRole("button", { name: "Bold" });
    await user.click(el);
    expect(onChange).toHaveBeenCalledWith(true);
    expect(el).toHaveAttribute("aria-pressed", "false");
  });
});

function Group(props: Partial<Parameters<typeof ToggleButtonGroup>[0]>) {
  return (
    <ToggleButtonGroup aria-label="View" {...props}>
      <ToggleButton id="list">List</ToggleButton>
      <ToggleButton id="board">Board</ToggleButton>
      <ToggleButton id="calendar">Calendar</ToggleButton>
    </ToggleButtonGroup>
  );
}

describe("ToggleButtonGroup", () => {
  it("selects one at a time by default, with its data-slot", async () => {
    const user = userEvent.setup();
    render(<Group defaultSelectedKeys={["list"]} />);
    expect(screen.getByRole("radiogroup", { name: "View" })).toHaveAttribute(
      "data-slot",
      "toggle-button-group",
    );
    await user.click(screen.getByRole("radio", { name: "Board" }));
    expect(screen.getByRole("radio", { name: "Board" })).toBeChecked();
    expect(screen.getByRole("radio", { name: "List" })).not.toBeChecked();
  });

  it("moves between its buttons with the arrow keys", async () => {
    const user = userEvent.setup();
    render(<Group defaultSelectedKeys={["list"]} />);
    await user.tab();
    expect(screen.getByRole("radio", { name: "List" })).toHaveFocus();
    await user.keyboard("{ArrowRight}");
    expect(screen.getByRole("radio", { name: "Board" })).toHaveFocus();
  });

  it("works controlled with multiple selection", async () => {
    const user = userEvent.setup();
    const onSelectionChange = vi.fn();
    render(
      <Group
        selectionMode="multiple"
        selectedKeys={["list"]}
        onSelectionChange={onSelectionChange}
      />,
    );
    await user.click(screen.getByRole("button", { name: "Board" }));
    expect([...(onSelectionChange.mock.calls[0]?.[0] ?? [])]).toEqual(["list", "board"]);
    expect(screen.getByRole("button", { name: "Board" })).toHaveAttribute("aria-pressed", "false");
  });
});
