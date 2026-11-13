// SPDX-License-Identifier: AGPL-3.0-or-later
import { screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import { renderWithStrings } from "../test/strings";
import { Switch } from "./switch";

describe("Switch", () => {
  it("toggles on press and Space, uncontrolled, with its data-slots", async () => {
    const user = userEvent.setup();
    const onChange = vi.fn();
    renderWithStrings(
      <Switch description="Sends a weekly summary." onChange={onChange}>
        Email digest
      </Switch>,
    );
    const toggle = screen.getByRole("switch", { name: "Email digest" });
    expect(toggle.closest('[data-slot="switch"]')).not.toBeNull();
    expect(toggle).toHaveAccessibleDescription("Sends a weekly summary.");
    await user.click(screen.getByText("Email digest"));
    expect(toggle).toBeChecked();
    expect(toggle.closest('[data-slot="switch-button"]')).toHaveAttribute("data-selected");
    await user.keyboard(" ");
    expect(toggle).not.toBeChecked();
    expect(onChange.mock.calls).toEqual([[true], [false]]);
  });

  it("works controlled", async () => {
    const user = userEvent.setup();
    const onChange = vi.fn();
    const { rerender } = renderWithStrings(
      <Switch isSelected={false} onChange={onChange}>
        Email digest
      </Switch>,
    );
    await user.click(screen.getByText("Email digest"));
    expect(onChange).toHaveBeenCalledWith(true);
    expect(screen.getByRole("switch")).not.toBeChecked();
    rerender(
      <Switch isSelected onChange={onChange}>
        Email digest
      </Switch>,
    );
    expect(screen.getByRole("switch")).toBeChecked();
  });
});
