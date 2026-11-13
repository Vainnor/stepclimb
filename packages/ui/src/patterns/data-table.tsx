// SPDX-License-Identifier: AGPL-3.0-or-later
// DataTable: a server-paged table of records built from Table's parts
// (docs/specs/m0/13-base-ui-components.md M0 inventory): sortable columns, row selection, a loading
// line under the table, an empty state and cursor pagination. It does not sort or page the rows itself: the server
// does, and the caller passes the page it got back. The table scrolls sideways on its own on a
// narrow screen; in a compact ThemeProvider its rows are shorter.
//
// The loading line under the table appears once the load has taken SKELETON_DELAY_MS (300 ms),
// like every spinner here. Screen readers hear "Loading" from a visually hidden live region that is
// in the page, empty, from the first render, since a live region added together with its text is
// often not announced; the visible line is hidden from them. The table's box is `aria-busy` while
// it loads. The line is not a row of the grid: React Aria's `TableLoadMoreItem` puts `aria-level`
// on a grid row, which ARIA allows only in a treegrid. While it loads the table never shows the
// empty state: a page that is still loading is not empty yet.
import { CircleNotchIcon } from "@phosphor-icons/react";
import { type ReactNode, useEffect, useState } from "react";
import type { Key, Selection, SelectionMode, SortDescriptor } from "react-aria-components";
import { tv } from "../lib/tv";
import { Pagination, type PaginationProps } from "../primitives/pagination";
import { SKELETON_DELAY_MS } from "../primitives/skeleton";
import { Cell, Column, Row, Table, TableBody, TableHeader } from "../primitives/table";
import { VisuallyHidden } from "../primitives/visually-hidden";
import { useStepclimbStrings } from "../strings/strings";
import { Icon } from "../theme/icon";

export const dataTable = tv({
  slots: {
    root: "flex flex-col gap-3",
    loading: "flex items-center gap-2 px-3 text-sm text-fg-sub",
  },
});

const styles = dataTable();

/** One column: its header and how to draw a row's cell in it. */
export interface DataTableColumn<T> {
  id: string;
  /** The header, from the strings. */
  label: string;
  /**
   * Whether this column names the row (a person's name, a course's title). The first column does
   * unless another sets this.
   */
  isRowHeader?: boolean;
  /** Whether a press on the header asks for this column's sort (`onSortChange`). */
  allowsSorting?: boolean;
  /** `end` for numbers. */
  align?: "start" | "end";
  /** The cell's content for a row. */
  render: (row: T) => ReactNode;
}

export interface DataTableProps<T extends { id: Key }> {
  /** The table's name, when no heading names it through `aria-labelledby`. */
  "aria-label"?: string;
  "aria-labelledby"?: string;
  columns: readonly DataTableColumn<T>[];
  /** This page's rows, in the order the server returned them. */
  rows: readonly T[];
  /** The column and direction the rows are sorted by. Sorting is always controlled. */
  sortDescriptor?: SortDescriptor;
  /** Called with the sort a person asked for; fetch the rows in that order. */
  onSortChange?: (descriptor: SortDescriptor) => void;
  /** `single` or `multiple` adds a checkbox to each row. */
  selectionMode?: SelectionMode;
  /** Controlled selection; `defaultSelectedKeys` for uncontrolled. */
  selectedKeys?: "all" | Iterable<Key>;
  defaultSelectedKeys?: "all" | Iterable<Key>;
  onSelectionChange?: (keys: Selection) => void;
  /** Rows that cannot be selected. */
  disabledKeys?: Iterable<Key>;
  /** The rows are loading: the loading line under the table, and no empty state. */
  isLoading?: boolean;
  /** What the table shows when it has no rows, usually an EmptyState. */
  emptyState?: ReactNode;
  /** Previous and Next under the table, when the list has more than one page. */
  pagination?: Omit<PaginationProps, "className">;
  /** Layout only. */
  className?: string;
}

/** Whether the load has lasted SKELETON_DELAY_MS. */
function useDelayedLoading(isLoading: boolean): boolean {
  const [shown, setShown] = useState(false);
  useEffect(() => {
    if (!isLoading) {
      setShown(false);
      return;
    }
    const timer = setTimeout(() => setShown(true), SKELETON_DELAY_MS);
    return () => clearTimeout(timer);
  }, [isLoading]);
  return isLoading && shown;
}

export function DataTable<T extends { id: Key }>({
  columns,
  rows,
  isLoading = false,
  emptyState,
  pagination,
  className,
  ...props
}: DataTableProps<T>) {
  const { t } = useStepclimbStrings();
  const showLoading = useDelayedLoading(isLoading);
  const rowHeader = columns.find((c) => c.isRowHeader)?.id ?? columns[0]?.id;
  return (
    <div data-slot="data-table" className={styles.root({ className })}>
      <Table {...props} aria-busy={isLoading || undefined}>
        <TableHeader columns={columns}>
          {(column) => (
            <Column
              id={column.id}
              isRowHeader={column.id === rowHeader}
              allowsSorting={column.allowsSorting}
              align={column.align}
            >
              {column.label}
            </Column>
          )}
        </TableHeader>
        <TableBody
          renderEmptyState={() =>
            isLoading ? null : <div data-slot="data-table-empty">{emptyState}</div>
          }
        >
          {rows.map((row) => (
            <Row key={row.id} id={row.id} columns={columns}>
              {(column) => <Cell align={column.align}>{column.render(row)}</Cell>}
            </Row>
          ))}
        </TableBody>
      </Table>
      {showLoading ? (
        <div data-slot="data-table-loading" aria-hidden="true" className={styles.loading()}>
          <Icon icon={CircleNotchIcon} size="sm" className="animate-spin" />
          {t("dataTable.loading")}
        </div>
      ) : null}
      <VisuallyHidden data-slot="data-table-status" role="status">
        {showLoading ? t("dataTable.loading") : ""}
      </VisuallyHidden>
      {pagination ? <Pagination {...pagination} /> : null}
    </div>
  );
}
