// SPDX-License-Identifier: AGPL-3.0-or-later
import { screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import { renderWithStrings } from "../test/strings";
import { Checkbox, CheckboxGroup } from "./checkbox";

const mark = (name: string) =>
  screen
    .getByRole("checkbox", { name })
    .closest('[data-slot="checkbox-button"]')
    ?.querySelector('[data-slot="checkbox-box"] [data-slot="icon"]');

describe("Checkbox", () => {
  it("toggles on press and Space, uncontrolled, with its data-slots", async () => {
    const user = userEvent.setup();
    const onChange = vi.fn();
    renderWithStrings(
      <Checkbox description="Learners see it in their list." onChange={onChange}>
        Published
      </Checkbox>,
    );
    const box = screen.getByRole("checkbox", { name: "Published" });
    expect(box.closest('[data-slot="checkbox"]')).not.toBeNull();
    expect(box).toHaveAccessibleDescription("Learners see it in their list.");
    expect(mark("Published")).toBeFalsy();
    await user.click(screen.getByText("Published"));
    expect(box).toBeChecked();
    expect(mark("Published")).toBeTruthy();
    await user.keyboard(" ");
    expect(box).not.toBeChecked();
    expect(onChange.mock.calls).toEqual([[true], [false]]);
  });

  it("works controlled", async () => {
    const user = userEvent.setup();
    const onChange = vi.fn();
    const { rerender } = renderWithStrings(
      <Checkbox isSelected={false} onChange={onChange}>
        Published
      </Checkbox>,
    );
    await user.click(screen.getByText("Published"));
    expect(onChange).toHaveBeenCalledWith(true);
    expect(screen.getByRole("checkbox")).not.toBeChecked();
    rerender(
      <Checkbox isSelected onChange={onChange}>
        Published
      </Checkbox>,
    );
    expect(screen.getByRole("checkbox")).toBeChecked();
  });

  it("draws the indeterminate state with its own mark", () => {
    renderWithStrings(<Checkbox isIndeterminate>All modules</Checkbox>);
    const box = screen.getByRole("checkbox", { name: "All modules" }) as HTMLInputElement;
    expect(box.indeterminate).toBe(true);
    expect(box.closest('[data-slot="checkbox-button"]')).toHaveAttribute("data-indeterminate");
    expect(mark("All modules")).toBeTruthy();
  });

  it("without a label, is named by aria-label and is a 44 px target on touch", () => {
    renderWithStrings(<Checkbox aria-label="Select Ground control" />);
    const box = screen.getByRole("checkbox", { name: "Select Ground control" });
    expect(box.closest('[data-slot="checkbox-button"]')).toHaveClass(
      "pointer-coarse:min-w-(--sc-control-touch)",
      "pointer-coarse:min-h-(--sc-control-touch)",
    );
    expect(
      box.closest('[data-slot="checkbox-button"]')?.querySelector('[data-slot="checkbox-label"]'),
    ).toBeNull();
  });

  it("shows its error when invalid", () => {
    renderWithStrings(
      <Checkbox isInvalid errorMessage="Accept the honor code to start.">
        I accept the honor code
      </Checkbox>,
    );
    expect(screen.getByText("Accept the honor code to start.")).toBeVisible();
  });
});

function Group(props: { value?: string[]; onChange?: (v: string[]) => void }) {
  return (
    <CheckboxGroup label="Notify me about" description="By email." {...props}>
      <Checkbox value="grades">Grades</Checkbox>
      <Checkbox value="deadlines">Deadlines</Checkbox>
    </CheckboxGroup>
  );
}

describe("CheckboxGroup", () => {
  it("is a labeled group holding the checked values, uncontrolled and controlled", async () => {
    const user = userEvent.setup();
    const onChange = vi.fn();
    const { rerender } = renderWithStrings(<Group onChange={onChange} />);
    const group = screen.getByRole("group", { name: "Notify me about" });
    expect(group).toHaveAttribute("data-slot", "checkbox-group");
    await user.click(screen.getByText("Deadlines"));
    expect(onChange).toHaveBeenLastCalledWith(["deadlines"]);
    rerender(<Group value={["grades"]} onChange={onChange} />);
    expect(screen.getByRole("checkbox", { name: "Grades" })).toBeChecked();
    expect(screen.getByRole("checkbox", { name: "Deadlines" })).not.toBeChecked();
  });
});
