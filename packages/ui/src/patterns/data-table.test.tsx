// SPDX-License-Identifier: AGPL-3.0-or-later
import { act, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { useState } from "react";
import type { Selection } from "react-aria-components";
import { afterEach, describe, expect, it, vi } from "vitest";
import { SKELETON_DELAY_MS } from "../primitives/skeleton";
import { renderWithStrings } from "../test/strings";
import { DataTable, type DataTableColumn, type DataTableProps } from "./data-table";

interface Learner {
  id: string;
  name: string;
  rating: string;
  hours: number;
}

const rows: Learner[] = [
  { id: "a", name: "Ada Park", rating: "S1", hours: 12 },
  { id: "b", name: "Ben Ortiz", rating: "S2", hours: 40 },
  { id: "c", name: "Cleo Novak", rating: "S3", hours: 75 },
];

const columns: DataTableColumn<Learner>[] = [
  { id: "name", label: "Name", allowsSorting: true, render: (r) => r.name },
  { id: "rating", label: "Rating", render: (r) => r.rating },
  { id: "hours", label: "Hours", align: "end", allowsSorting: true, render: (r) => r.hours },
];

function Example(props: Partial<DataTableProps<Learner>>) {
  return <DataTable aria-label="Learners" columns={columns} rows={rows} {...props} />;
}

function grid() {
  return screen.getByRole("grid", { name: "Learners" });
}

function Controlled() {
  const [selected, setSelected] = useState<Selection>(new Set(["c"]));
  return <Example selectionMode="single" selectedKeys={selected} onSelectionChange={setSelected} />;
}

describe("DataTable", () => {
  afterEach(() => vi.useRealTimers());

  it("renders a grid with its data-slots, the first column naming each row", () => {
    renderWithStrings(<Example />);
    expect(grid()).toHaveAttribute("data-slot", "table");
    expect(grid().closest('[data-slot="table-scroller"]')).toHaveClass("overflow-x-auto");
    expect(grid().closest('[data-slot="data-table"]')).not.toBeNull();
    expect(screen.getAllByRole("columnheader")[0]).toHaveAttribute("data-slot", "table-column");
    const row = screen.getByRole("row", { name: "Ben Ortiz" });
    expect(row).toHaveAttribute("data-slot", "table-row");
    expect(within(row).getByRole("rowheader")).toHaveTextContent("Ben Ortiz");
    expect(within(row).getAllByRole("gridcell")[1]).toHaveAttribute("data-slot", "table-cell");
    expect(within(row).getAllByRole("gridcell")[1]).toHaveClass("text-end", "h-(--sc-control-h)");
  });

  it("names rows by the column marked as the row header", () => {
    renderWithStrings(
      <Example columns={columns.map((c) => ({ ...c, isRowHeader: c.id === "rating" }))} />,
    );
    expect(screen.getByRole("row", { name: "S2" })).toBeInTheDocument();
  });

  it("moves between rows and cells with the arrow keys (T12)", async () => {
    const user = userEvent.setup();
    renderWithStrings(<Example />);
    await user.tab();
    expect(screen.getByRole("row", { name: "Ada Park" })).toHaveFocus();
    await user.keyboard("{ArrowDown}");
    expect(screen.getByRole("row", { name: "Ben Ortiz" })).toHaveFocus();
    await user.keyboard("{ArrowRight}");
    expect(screen.getByRole("rowheader", { name: "Ben Ortiz" })).toHaveFocus();
    await user.keyboard("{ArrowRight}");
    expect(screen.getByRole("gridcell", { name: "S2" })).toHaveFocus();
  });

  it("marks the sorted column and asks for a new sort on press and Enter", async () => {
    const user = userEvent.setup();
    const onSortChange = vi.fn();
    renderWithStrings(
      <Example
        sortDescriptor={{ column: "name", direction: "ascending" }}
        onSortChange={onSortChange}
      />,
    );
    const [name, rating, hours] = screen.getAllByRole("columnheader");
    expect(name).toHaveAttribute("aria-sort", "ascending");
    expect(hours).toHaveAttribute("aria-sort", "none");
    expect(rating).not.toHaveAttribute("aria-sort");
    expect(name?.querySelector("[data-sort-direction]")).toHaveAttribute(
      "data-sort-direction",
      "ascending",
    );
    await user.click(name as HTMLElement);
    expect(onSortChange).toHaveBeenLastCalledWith({ column: "name", direction: "descending" });
    await user.click(rating as HTMLElement);
    expect(onSortChange).toHaveBeenCalledTimes(1);
    act(() => (hours as HTMLElement).focus());
    await user.keyboard("{Enter}");
    expect(onSortChange).toHaveBeenLastCalledWith({ column: "hours", direction: "ascending" });
  });

  it("selects rows with their checkboxes and with Space, uncontrolled", async () => {
    const user = userEvent.setup();
    const onSelectionChange = vi.fn();
    renderWithStrings(<Example selectionMode="multiple" onSelectionChange={onSelectionChange} />);
    const selectAll = screen.getByRole("checkbox", { name: "Select all" });
    expect(selectAll.closest('[data-slot="table-selection-column"]')).not.toBeNull();
    await user.click(screen.getByRole("checkbox", { name: "Select Ada Park" }));
    expect(screen.getByRole("row", { name: "Ada Park" })).toHaveAttribute("aria-selected", "true");
    await user.keyboard("{ArrowDown}");
    await user.keyboard(" ");
    expect(screen.getByRole("row", { name: "Ben Ortiz" })).toHaveAttribute("aria-selected", "true");
    const last = onSelectionChange.mock.lastCall?.[0] as Set<string> | undefined;
    expect([...(last ?? [])]).toEqual(["a", "b"]);
    await user.click(selectAll);
    expect(onSelectionChange).toHaveBeenLastCalledWith("all");
  });

  it("names the selection checkboxes from the strings catalog, not React Aria's", () => {
    renderWithStrings(<Example selectionMode="multiple" />, { marked: true });
    const labels = screen.getAllByRole("checkbox").map((box) => box.getAttribute("aria-label"));
    expect(labels).toEqual([
      "«table.selectAll»",
      "«table.selectRow»",
      "«table.selectRow»",
      "«table.selectRow»",
    ]);
  });

  it("selects one row at a time in single mode, controlled", async () => {
    const user = userEvent.setup();
    renderWithStrings(<Controlled />);
    expect(screen.queryByRole("checkbox", { name: "Select all" })).toBeNull();
    expect(screen.getByRole("row", { name: "Cleo Novak" })).toHaveAttribute(
      "aria-selected",
      "true",
    );
    await user.click(screen.getByRole("checkbox", { name: "Select Ada Park" }));
    expect(screen.getByRole("row", { name: "Ada Park" })).toHaveAttribute("aria-selected", "true");
    expect(screen.getByRole("row", { name: "Cleo Novak" })).toHaveAttribute(
      "aria-selected",
      "false",
    );
  });

  it("shows the empty state when there are no rows", () => {
    renderWithStrings(<Example rows={[]} emptyState={<p>No learners yet.</p>} />);
    expect(
      screen.getByText("No learners yet.").closest('[data-slot="data-table-empty"]'),
    ).not.toBeNull();
  });

  it("shows the loading line only after 300 ms, never the empty state while loading", () => {
    vi.useFakeTimers();
    const { rerender } = renderWithStrings(
      <Example rows={[]} isLoading emptyState={<p>No learners yet.</p>} />,
    );
    const scroller = () => grid().closest('[data-slot="table-scroller"]');
    expect(scroller()).toHaveAttribute("aria-busy", "true");
    expect(screen.queryByText("No learners yet.")).toBeNull();
    act(() => vi.advanceTimersByTime(SKELETON_DELAY_MS - 1));
    expect(screen.queryByText("Loading")).toBeNull();
    act(() => vi.advanceTimersByTime(1));
    expect(screen.getByRole("status")).toHaveTextContent("Loading");
    expect(screen.getByRole("status")).toHaveAttribute("data-slot", "data-table-loading");
    rerender(<Example rows={rows} />);
    expect(screen.queryByText("Loading")).toBeNull();
    expect(scroller()).not.toHaveAttribute("aria-busy");
  });

  it("pages with cursors under the table", async () => {
    const user = userEvent.setup();
    const onPageChange = vi.fn();
    renderWithStrings(<Example pagination={{ nextCursor: "n2", count: 48, onPageChange }} />);
    await user.click(screen.getByRole("button", { name: "Next" }));
    expect(onPageChange).toHaveBeenCalledWith("n2", "next");
  });
});
