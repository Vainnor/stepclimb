// SPDX-License-Identifier: AGPL-3.0-or-later
import { screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, describe, expect, it, vi } from "vitest";
import { WIDE_QUERY } from "../lib/breakpoints";
import { stubMatchMedia } from "../test/media";
import { renderWithStrings } from "../test/strings";
import { Button } from "./button";
import { DialogTrigger } from "./dialog";
import { Select, SelectItem } from "./select";
import { Sheet } from "./sheet";

function wide(isWide: boolean) {
  stubMatchMedia((query) => (query === WIDE_QUERY ? isWide : false));
}

function Example(props: {
  value?: string | null;
  defaultValue?: string;
  onChange?: (key: unknown) => void;
  isInvalid?: boolean;
}) {
  return (
    <Select
      label="Rating"
      description="The position this course trains for."
      errorMessage="Choose a rating."
      {...props}
    >
      <SelectItem id="s1">S1 Ground</SelectItem>
      <SelectItem id="s2">S2 Tower</SelectItem>
      <SelectItem id="s3" isDisabled>
        S3 Approach
      </SelectItem>
      <SelectItem id="c1">C1 Enroute</SelectItem>
    </Select>
  );
}

const trigger = () => screen.getByRole("button", { name: /Rating/ });

describe("Select", () => {
  afterEach(() => vi.unstubAllGlobals());

  it("shows the placeholder from the strings catalog, with its data-slots", () => {
    wide(true);
    renderWithStrings(<Example />);
    expect(trigger()).toHaveAttribute("data-slot", "select-trigger");
    expect(trigger()).toHaveTextContent("Choose one");
    expect(trigger().closest('[data-slot="select"]')).not.toBeNull();
    expect(screen.getByText("The position this course trains for.")).toHaveAttribute(
      "data-slot",
      "field-description",
    );
  });

  it("opens in a Popover at 768 px and wider and picks with the keyboard", async () => {
    wide(true);
    const user = userEvent.setup();
    const onChange = vi.fn();
    renderWithStrings(<Example onChange={onChange} />);
    trigger().focus();
    await user.keyboard("{ArrowDown}");
    const list = screen.getByRole("listbox");
    expect(list.closest('[data-slot="list-box-popover"]')).not.toBeNull();
    expect(screen.getByRole("option", { name: "S1 Ground" })).toHaveFocus();
    await user.keyboard("{ArrowDown}{ArrowDown}");
    // The disabled option is skipped.
    expect(screen.getByRole("option", { name: "C1 Enroute" })).toHaveFocus();
    await user.keyboard("{Enter}");
    expect(onChange).toHaveBeenCalledWith("c1");
    expect(screen.queryByRole("listbox")).toBeNull();
    expect(trigger()).toHaveTextContent("C1 Enroute");
    await waitFor(() => expect(trigger()).toHaveFocus());
  });

  it("opens as a bottom sheet under 768 px (Q10)", async () => {
    wide(false);
    const user = userEvent.setup();
    renderWithStrings(<Example />);
    await user.click(trigger());
    const list = screen.getByRole("listbox");
    expect(list.closest('[data-slot="list-box-sheet"]')).toHaveClass("rounded-t-md");
    expect(list.closest('[data-slot="list-box-popover"]')).toBeNull();
    await user.click(screen.getByRole("option", { name: "S2 Tower" }));
    expect(screen.queryByRole("listbox")).toBeNull();
    expect(trigger()).toHaveTextContent("S2 Tower");
  });

  it("opens its bottom sheet only when asked, inside a Sheet under 768 px", async () => {
    wide(false);
    const user = userEvent.setup();
    renderWithStrings(
      <DialogTrigger>
        <Button>Add an item</Button>
        <Sheet title="Add an item">
          <Example defaultValue="s1" />
        </Sheet>
      </DialogTrigger>,
    );
    await user.click(screen.getByRole("button", { name: "Add an item" }));
    // Closed: no options are drawn and the Sheet is not made inert under a second modal.
    expect(screen.queryByRole("listbox")).toBeNull();
    expect(document.querySelector('[data-slot="list-box-sheet-overlay"]')).toBeNull();
    await user.click(trigger());
    await user.click(await screen.findByRole("option", { name: "S2 Tower" }));
    await waitFor(() => expect(screen.queryByRole("listbox")).toBeNull());
    expect(trigger()).toHaveTextContent("S2 Tower");
    expect(screen.getByRole("dialog", { name: "Add an item" })).toBeInTheDocument();
  });

  it("marks the selected option with a check", async () => {
    wide(true);
    const user = userEvent.setup();
    renderWithStrings(<Example defaultValue="s2" />);
    await user.click(trigger());
    const option = screen.getByRole("option", { name: "S2 Tower" });
    expect(option).toHaveAttribute("aria-selected", "true");
    expect(
      option.querySelector('[data-slot="list-box-item-check"] [data-slot="icon"]'),
    ).not.toBeNull();
    expect(
      screen
        .getByRole("option", { name: "S1 Ground" })
        .querySelector('[data-slot="list-box-item-check"] [data-slot="icon"]'),
    ).toBeNull();
  });

  it("works controlled", async () => {
    wide(true);
    const user = userEvent.setup();
    const onChange = vi.fn();
    const { rerender } = renderWithStrings(<Example value="s1" onChange={onChange} />);
    await user.click(trigger());
    await user.click(screen.getByRole("option", { name: "S2 Tower" }));
    expect(onChange).toHaveBeenCalledWith("s2");
    expect(trigger()).toHaveTextContent("S1 Ground");
    rerender(<Example value="s2" onChange={onChange} />);
    expect(trigger()).toHaveTextContent("S2 Tower");
  });

  it("shows its error when invalid", () => {
    wide(true);
    renderWithStrings(<Example isInvalid />);
    expect(
      screen.getByText("Choose a rating.").closest('[data-slot="field-error"]'),
    ).not.toBeNull();
  });
});
