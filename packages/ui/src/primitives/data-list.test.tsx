// SPDX-License-Identifier: AGPL-3.0-or-later
import { screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { useState } from "react";
import type { Selection } from "react-aria-components";
import { describe, expect, it, vi } from "vitest";
import { renderWithStrings } from "../test/strings";
import { DataList, DataListItem, type DataListProps } from "./data-list";

function Example(props: Partial<DataListProps<object>>) {
  return (
    <DataList aria-label="Pending admissions" {...props}>
      <DataListItem id="a">Ada Park</DataListItem>
      <DataListItem id="b">Ben Ortiz</DataListItem>
      <DataListItem id="c" isDisabled>
        Cleo Novak
      </DataListItem>
    </DataList>
  );
}

function Controlled() {
  const [selected, setSelected] = useState<Selection>(new Set(["b"]));
  return <Example selectionMode="single" selectedKeys={selected} onSelectionChange={setSelected} />;
}

describe("DataList", () => {
  it("renders a grid of rows with its data-slots", () => {
    renderWithStrings(<Example />);
    const list = screen.getByRole("grid", { name: "Pending admissions" });
    expect(list).toHaveAttribute("data-slot", "data-list");
    const row = screen.getByRole("row", { name: "Ada Park" });
    expect(row).toHaveAttribute("data-slot", "data-list-item");
    expect(row.querySelector('[data-slot="data-list-item-content"]')).toHaveTextContent("Ada Park");
    expect(screen.queryByRole("checkbox")).toBeNull();
  });

  it("moves between items with the arrow keys", async () => {
    const user = userEvent.setup();
    renderWithStrings(<Example />);
    await user.tab();
    expect(screen.getByRole("row", { name: "Ada Park" })).toHaveFocus();
    await user.keyboard("{ArrowDown}");
    expect(screen.getByRole("row", { name: "Ben Ortiz" })).toHaveFocus();
  });

  it("runs onAction on press and Enter", async () => {
    const user = userEvent.setup();
    const onAction = vi.fn();
    renderWithStrings(<Example onAction={onAction} />);
    await user.click(screen.getByRole("row", { name: "Ben Ortiz" }));
    expect(onAction).toHaveBeenLastCalledWith("b");
    await user.keyboard("{ArrowUp}{Enter}");
    expect(onAction).toHaveBeenLastCalledWith("a");
  });

  it("selects with its checkboxes and Space, uncontrolled, and not a disabled item", async () => {
    const user = userEvent.setup();
    const onSelectionChange = vi.fn();
    renderWithStrings(<Example selectionMode="multiple" onSelectionChange={onSelectionChange} />);
    await user.click(screen.getByRole("checkbox", { name: "Select Ada Park" }));
    expect(screen.getByRole("row", { name: "Ada Park" })).toHaveAttribute("aria-selected", "true");
    await user.keyboard("{ArrowDown} ");
    expect(screen.getByRole("row", { name: "Ben Ortiz" })).toHaveAttribute("aria-selected", "true");
    expect(screen.getByRole("checkbox", { name: "Select Cleo Novak" })).toBeDisabled();
    const last = onSelectionChange.mock.lastCall?.[0] as Set<string> | undefined;
    expect([...(last ?? [])]).toEqual(["a", "b"]);
  });

  it("names its checkboxes from the strings catalog, not React Aria's", () => {
    renderWithStrings(<Example selectionMode="multiple" />, { marked: true });
    const labels = screen.getAllByRole("checkbox").map((box) => box.getAttribute("aria-label"));
    expect(labels).toEqual([
      "«dataList.selectItem»",
      "«dataList.selectItem»",
      "«dataList.selectItem»",
    ]);
  });

  it("works controlled", async () => {
    const user = userEvent.setup();
    renderWithStrings(<Controlled />);
    expect(screen.getByRole("row", { name: "Ben Ortiz" })).toHaveAttribute("aria-selected", "true");
    await user.click(screen.getByRole("checkbox", { name: "Select Ada Park" }));
    expect(screen.getByRole("row", { name: "Ada Park" })).toHaveAttribute("aria-selected", "true");
    expect(screen.getByRole("row", { name: "Ben Ortiz" })).toHaveAttribute(
      "aria-selected",
      "false",
    );
  });

  it("shows the empty state when it has no items", () => {
    renderWithStrings(
      <DataList aria-label="Pending admissions" emptyState={<p>No one is waiting.</p>}>
        {[]}
      </DataList>,
    );
    expect(
      screen.getByText("No one is waiting.").closest('[data-slot="data-list-empty"]'),
    ).not.toBeNull();
  });
});
