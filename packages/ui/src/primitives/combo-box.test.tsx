// SPDX-License-Identifier: AGPL-3.0-or-later
import { screen } from "@testing-library/react";
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
    expect(screen.getByRole("progressbar", { name: "Loading" })).toHaveAttribute(
      "data-slot",
      "combo-box-loading",
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
});
