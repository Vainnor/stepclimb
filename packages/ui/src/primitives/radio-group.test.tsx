// SPDX-License-Identifier: AGPL-3.0-or-later
import { screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import { renderWithStrings } from "../test/strings";
import { Radio, RadioGroup } from "./radio-group";

function Example(props: { value?: string; defaultValue?: string; onChange?: (v: string) => void }) {
  return (
    <RadioGroup label="Feedback" description="What learners see after submitting." {...props}>
      <Radio value="full" description="Answers explained.">
        Full
      </Radio>
      <Radio value="result">Result only</Radio>
      <Radio value="none">None</Radio>
    </RadioGroup>
  );
}

describe("RadioGroup", () => {
  it("is a labeled radio group with its data-slots", () => {
    renderWithStrings(<Example />);
    const group = screen.getByRole("radiogroup", { name: "Feedback" });
    expect(group).toHaveAttribute("data-slot", "radio-group");
    expect(
      screen.getByRole("radio", { name: "Full" }).closest('[data-slot="radio"]'),
    ).not.toBeNull();
    expect(screen.getByRole("radio", { name: "Full" })).toHaveAccessibleDescription(
      /^Answers explained\./,
    );
  });

  it("moves the choice with the arrow keys, uncontrolled", async () => {
    const user = userEvent.setup();
    const onChange = vi.fn();
    renderWithStrings(<Example defaultValue="full" onChange={onChange} />);
    await user.tab();
    expect(screen.getByRole("radio", { name: "Full" })).toHaveFocus();
    await user.keyboard("{ArrowDown}");
    expect(screen.getByRole("radio", { name: "Result only" })).toBeChecked();
    expect(onChange).toHaveBeenCalledWith("result");
    const dot = screen
      .getByRole("radio", { name: "Result only" })
      .closest('[data-slot="radio-button"]')
      ?.querySelector('[data-slot="radio-dot"]');
    expect(dot).toBeTruthy();
  });

  it("works controlled", async () => {
    const user = userEvent.setup();
    const onChange = vi.fn();
    const { rerender } = renderWithStrings(<Example value="full" onChange={onChange} />);
    await user.click(screen.getByText("None"));
    expect(onChange).toHaveBeenCalledWith("none");
    expect(screen.getByRole("radio", { name: "Full" })).toBeChecked();
    rerender(<Example value="none" onChange={onChange} />);
    expect(screen.getByRole("radio", { name: "None" })).toBeChecked();
  });
});
