// SPDX-License-Identifier: AGPL-3.0-or-later
import { screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import { renderWithStrings, unmarkedText } from "../test/strings";
import { SearchField } from "./search-field";

describe("SearchField", () => {
  it("labels its search input, with its data-slots", () => {
    renderWithStrings(<SearchField label="Search courses" />);
    const input = screen.getByRole("searchbox", { name: "Search courses" });
    expect(input).toHaveAttribute("data-slot", "search-field-input");
    expect(input.closest('[data-slot="search-field"]')).toHaveAttribute("data-empty", "true");
  });

  it("clears with its labeled button, which shows only while there is text", async () => {
    const user = userEvent.setup();
    const onClear = vi.fn();
    renderWithStrings(
      <SearchField label="Search courses" defaultValue="tower" onClear={onClear} />,
    );
    const clear = screen.getByRole("button", { name: "Clear search" });
    expect(clear).toHaveAttribute("data-slot", "search-field-clear");
    expect(clear).toHaveClass("group-data-[empty]:hidden");
    await user.click(clear);
    expect(screen.getByRole("searchbox")).toHaveValue("");
    expect(onClear).toHaveBeenCalledOnce();
    expect(screen.getByRole("searchbox").closest('[data-slot="search-field"]')).toHaveAttribute(
      "data-empty",
      "true",
    );
  });

  it("clears on Escape and submits on Enter", async () => {
    const user = userEvent.setup();
    const onSubmit = vi.fn();
    renderWithStrings(<SearchField label="Search courses" onSubmit={onSubmit} />);
    const input = screen.getByRole("searchbox");
    await user.type(input, "ground{Enter}");
    expect(onSubmit).toHaveBeenCalledWith("ground");
    await user.keyboard("{Escape}");
    expect(input).toHaveValue("");
  });

  it("works controlled", async () => {
    const user = userEvent.setup();
    const onChange = vi.fn();
    renderWithStrings(<SearchField label="Search courses" value="" onChange={onChange} />);
    await user.type(screen.getByRole("searchbox"), "a");
    expect(onChange).toHaveBeenCalledWith("a");
    expect(screen.getByRole("searchbox")).toHaveValue("");
  });

  it("names its clear button only from the strings catalog (T2)", () => {
    const { container } = renderWithStrings(<SearchField label="«caller»" defaultValue="«x»" />, {
      marked: true,
    });
    expect(screen.getByRole("button")).toHaveAccessibleName("«searchField.clear»");
    expect(unmarkedText(container)).toEqual([]);
  });
});
