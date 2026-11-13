// SPDX-License-Identifier: AGPL-3.0-or-later
import { screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, describe, expect, it, vi } from "vitest";
import { WIDE_QUERY } from "../lib/breakpoints";
import { stubMatchMedia } from "../test/media";
import { renderWithStrings } from "../test/strings";
import { ComboBox, ComboBoxItem } from "./combo-box";

function wide(isWide: boolean) {
  stubMatchMedia((query) => (query === WIDE_QUERY ? isWide : false));
}

const facilities = [
  { id: "ground", name: "Ground control" },
  { id: "tower", name: "Local control" },
  { id: "approach", name: "Approach control" },
];

function Example(props: {
  isLoading?: boolean;
  onChange?: (key: unknown) => void;
  value?: string | null;
}) {
  return (
    <ComboBox label="Position" placeholder="Type a position" {...props}>
      {facilities.map((f) => (
        <ComboBoxItem key={f.id} id={f.id}>
          {f.name}
        </ComboBoxItem>
      ))}
    </ComboBox>
  );
}

describe("ComboBox", () => {
  afterEach(() => vi.unstubAllGlobals());

  it("filters as the person types and picks with the keyboard at 768 px and wider", async () => {
    wide(true);
    const user = userEvent.setup();
    const onChange = vi.fn();
    renderWithStrings(<Example onChange={onChange} />);
    const input = screen.getByRole("combobox", { name: "Position" });
    expect(input).toHaveAttribute("data-slot", "combo-box-input");
    expect(input.closest('[data-slot="combo-box-group"]')).not.toBeNull();
    await user.type(input, "loc");
    const list = screen.getByRole("listbox");
    expect(list.closest('[data-slot="list-box-popover"]')).not.toBeNull();
    expect(screen.getAllByRole("option").map((o) => o.textContent)).toEqual(["Local control"]);
    await user.keyboard("{ArrowDown}{Enter}");
    expect(onChange).toHaveBeenCalledWith("tower");
    expect(input).toHaveValue("Local control");
  });

  it("names its button from the strings catalog", () => {
    wide(true);
    renderWithStrings(<Example />);
    expect(screen.getByRole("button", { name: /Show suggestions/ })).toHaveAttribute(
      "data-slot",
      "combo-box-button",
    );
  });

  it("says nothing matched, or that it is loading", async () => {
    wide(true);
    const user = userEvent.setup();
    const { rerender } = renderWithStrings(<Example />);
    await user.type(screen.getByRole("combobox"), "zzz");
    expect(screen.getByText("No matches")).toHaveAttribute("data-slot", "combo-box-empty");
    rerender(<Example isLoading />);
    expect(screen.getByRole("listbox")).toHaveTextContent("Loading");
    // The spinner waits 300 ms, like Button's, so a fast answer never flashes it.
    expect(document.querySelector('[data-slot="combo-box-loading"]')).toBeNull();
    // The open list hides the rest of the page from screen readers, the spinner too.
    await waitFor(() =>
      expect(document.querySelector('[data-slot="combo-box-loading"]')).toHaveAttribute(
        "aria-label",
        "Loading",
      ),
    );
  });

  it("under 768 px opens a bottom sheet holding the input and the options (Q10)", async () => {
    wide(false);
    const user = userEvent.setup();
    const onChange = vi.fn();
    renderWithStrings(<Example onChange={onChange} />);
    // Named by the label and the text it shows (WCAG 2.5.3).
    const trigger = screen.getByRole("button", { name: "Position Type a position" });
    expect(trigger).toHaveAttribute("data-slot", "combo-box-sheet-trigger");
    expect(trigger).toHaveTextContent("Type a position");
    expect(screen.queryByRole("combobox")).toBeNull();
    await user.click(trigger);
    const input = screen.getByRole("combobox", { name: "Position" });
    expect(input.closest('[data-slot="list-box-sheet"]')).not.toBeNull();
    expect(screen.getByRole("listbox").closest('[data-slot="list-box-sheet"]')).not.toBeNull();
    expect(input).toHaveFocus();
    await user.keyboard("appr");
    expect(screen.getAllByRole("option").map((o) => o.textContent)).toEqual(["Approach control"]);
    await user.keyboard("{ArrowDown}{Enter}");
    expect(onChange).toHaveBeenCalledWith("approach");
    expect(screen.queryByRole("listbox")).toBeNull();
    expect(trigger).toHaveAccessibleName("Position Approach control");
  });

  it("works controlled", async () => {
    wide(true);
    const user = userEvent.setup();
    const onChange = vi.fn();
    const { rerender } = renderWithStrings(<Example value="ground" onChange={onChange} />);
    expect(screen.getByRole("combobox")).toHaveValue("Ground control");
    await user.click(screen.getByRole("button", { name: /Show suggestions/ }));
    await user.click(screen.getByRole("option", { name: "Local control" }));
    expect(onChange).toHaveBeenCalledWith("tower");
    rerender(<Example value="tower" onChange={onChange} />);
    expect(screen.getByRole("combobox")).toHaveValue("Local control");
  });

  describe("under 768 px", () => {
    it("puts the field's description, error and invalid state on the trigger", () => {
      wide(false);
      renderWithStrings(
        <ComboBox
          label="Position"
          description="Where you train."
          errorMessage="Choose a position."
          isInvalid
          isRequired
        >
          <ComboBoxItem id="gnd">Ground control</ComboBoxItem>
        </ComboBox>,
      );
      const trigger = screen.getByRole("button", { name: /^Position/ });
      expect(trigger).toHaveAttribute("aria-invalid", "true");
      // aria-required is not allowed on a button; the hidden input carries `required`.
      expect(trigger).not.toHaveAttribute("aria-required");
      expect(document.querySelector('[data-slot="combo-box-form-input"]')).toBeRequired();
      expect(trigger).toHaveAccessibleDescription("Where you train. Choose a position.");
    });

    it("moves focus to the trigger when its label is pressed", async () => {
      wide(false);
      const user = userEvent.setup();
      renderWithStrings(<Example />);
      await user.click(screen.getByText("Position"));
      expect(screen.getByRole("button", { name: /^Position/ })).toHaveFocus();
    });

    it("submits its text under its name in a form", () => {
      wide(false);
      const { container } = renderWithStrings(
        <form>
          <ComboBox label="Position" name="position" allowsCustomValue defaultInputValue="Oceanic">
            <ComboBoxItem id="gnd">Ground control</ComboBoxItem>
          </ComboBox>
        </form>,
      );
      const form = container.querySelector("form") as HTMLFormElement;
      expect(new FormData(form).get("position")).toBe("Oceanic");
    });

    it("blocks the form while required and empty, and focuses the trigger", async () => {
      wide(false);
      const onSubmit = vi.fn((e: React.FormEvent) => e.preventDefault());
      const { container } = renderWithStrings(
        <form onSubmit={onSubmit}>
          <ComboBox label="Position" name="position" isRequired>
            <ComboBoxItem id="gnd">Ground control</ComboBoxItem>
          </ComboBox>
          <button type="submit">Save</button>
        </form>,
      );
      const form = container.querySelector("form") as HTMLFormElement;
      expect(form.checkValidity()).toBe(false);
      await userEvent.setup().click(screen.getByRole("button", { name: "Save" }));
      expect(onSubmit).not.toHaveBeenCalled();
      // The browser focuses the first invalid control; the hidden input hands focus on.
      (container.querySelector('[data-slot="combo-box-form-input"]') as HTMLElement).focus();
      expect(screen.getByRole("button", { name: /^Position/ })).toHaveFocus();
    });
  });
});
