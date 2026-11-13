// SPDX-License-Identifier: AGPL-3.0-or-later
// Table: the styled parts of a React Aria Table, which DataTable puts together
// (docs/specs/m0/13-base-ui-components.md B1, M0 inventory). A grid: arrow keys move between rows
// and cells, Space selects a row when the table allows selection, and a sortable column header
// sorts on press or Enter. React Aria sets `aria-sort` on the sorted column and announces each
// change of sort. Rows are as tall as a control (`--sc-control-h`), so a compact ThemeProvider
// makes a dense table.
//
// With `selectionMode`, TableHeader and Row add the selection checkboxes themselves: a select-all
// box when several rows can be chosen, one box per row.
import { ArrowDownIcon, ArrowsDownUpIcon, ArrowUpIcon } from "@phosphor-icons/react";
import type { ReactNode } from "react";
import {
  Cell as AriaCell,
  type CellProps as AriaCellProps,
  Column as AriaColumn,
  type ColumnProps as AriaColumnProps,
  Row as AriaRow,
  type RowProps as AriaRowProps,
  Table as AriaTable,
  TableBody as AriaTableBody,
  type TableBodyProps as AriaTableBodyProps,
  TableHeader as AriaTableHeader,
  type TableHeaderProps as AriaTableHeaderProps,
  type TableProps as AriaTableProps,
  Collection,
  useTableOptions,
} from "react-aria-components";
import { tv } from "../lib/tv";
import { useStepclimbStrings } from "../strings/strings";
import { Icon } from "../theme/icon";
import { Checkbox } from "./checkbox";

const focusRing =
  "data-[focus-visible]:ring-2 data-[focus-visible]:ring-inset data-[focus-visible]:ring-ring";

export const table = tv({
  slots: {
    // A table wider than its box scrolls on its own, not the page (plan UI tab -> PageReader).
    // overscroll-x-contain: a sideways swipe that reaches the table's edge stops there instead of
    // becoming the browser's back or forward gesture. contain-paint: Chromium gives a scroller
    // that overflows a composited layer, and without paint containment it also composites what
    // the page paints after it (the Pagination) for overlap; with it, the scroller's layer ends at
    // its own box.
    scroller: [
      "w-full overflow-x-auto overscroll-x-contain contain-paint",
      "rounded-md border border-line bg-raised",
    ],
    root: "w-full border-separate border-spacing-0 font-ui text-sm text-fg-default outline-none",
    column: [
      "h-(--sc-control-h) cursor-default border-b border-line bg-sunken px-3 text-start align-middle",
      "whitespace-nowrap text-caption font-medium text-fg-sub outline-none",
      "pointer-coarse:h-(--sc-control-touch) data-[allows-sorting]:data-[hovered]:text-fg-strong",
      focusRing,
    ],
    columnContent: "flex items-center gap-1.5",
    sortIcon: "shrink-0",
    row: [
      "outline-none data-[hovered]:bg-sunken data-[selected]:bg-brand-soft",
      "data-[disabled]:text-fg-disabled",
      focusRing,
    ],
    cell: [
      // A table cell ignores `min-height`, so base.css's 44 px touch minimum for gridcells does not
      // reach it; the height does.
      "h-(--sc-control-h) pointer-coarse:h-(--sc-control-touch)",
      "border-b border-line-soft px-3 py-1.5 align-middle outline-none",
      "[tr:last-child>&]:border-b-0",
      focusRing,
    ],
    // The checkbox column: as narrow as its box.
    selectionCell: "w-0 pe-0",
  },
  variants: {
    align: {
      start: { column: "text-start", cell: "text-start" },
      end: { column: "text-end", columnContent: "justify-end", cell: "text-end tabular-nums" },
    },
  },
  defaultVariants: { align: "start" },
});

const styles = table();

export interface TableProps extends Omit<AriaTableProps, "className"> {
  /** Set while the rows load; React Aria's table does not pass it on, so the scrolling box takes it. */
  "aria-busy"?: boolean;
  /** Layout only, on the scrolling box around the table. */
  className?: string;
}

/** The table in its own horizontally scrolling box. Name it with `aria-label` or `aria-labelledby`. */
export function Table({ className, "aria-busy": busy, ...props }: TableProps) {
  return (
    <div data-slot="table-scroller" aria-busy={busy} className={styles.scroller({ className })}>
      <AriaTable data-slot="table" {...props} className={styles.root()} />
    </div>
  );
}

export interface TableHeaderProps<T> extends Omit<AriaTableHeaderProps<T>, "className"> {}

/** The header row; with multiple selection it starts with the select-all checkbox. */
export function TableHeader<T extends object>({
  columns,
  children,
  ...props
}: TableHeaderProps<T>) {
  const { t } = useStepclimbStrings();
  const { selectionBehavior, selectionMode } = useTableOptions();
  return (
    <AriaTableHeader data-slot="table-header" {...props}>
      {selectionBehavior === "toggle" ? (
        <AriaColumn
          data-slot="table-selection-column"
          className={styles.column({ className: styles.selectionCell() })}
        >
          {selectionMode === "multiple" ? (
            <Checkbox slot="selection" aria-label={t("table.selectAll")} />
          ) : null}
        </AriaColumn>
      ) : null}
      <Collection items={columns}>{children}</Collection>
    </AriaTableHeader>
  );
}

export interface ColumnProps extends Omit<AriaColumnProps, "className" | "children"> {
  /** The column's name: plain text, so the sort announcement can say it. */
  children: ReactNode;
  /** `end` for numbers. */
  align?: "start" | "end";
  /** Layout only. */
  className?: string;
}

/** A column header. With `allowsSorting` it shows the sort direction, or that it can be sorted. */
export function Column({ children, align, className, ...props }: ColumnProps) {
  const parts = table({ align });
  return (
    <AriaColumn data-slot="table-column" {...props} className={parts.column({ className })}>
      {({ allowsSorting, sortDirection }) => (
        <span
          data-slot="table-column-content"
          data-sort-direction={allowsSorting ? (sortDirection ?? "none") : undefined}
          className={parts.columnContent()}
        >
          {children}
          {allowsSorting ? (
            <Icon
              icon={
                sortDirection === "ascending"
                  ? ArrowUpIcon
                  : sortDirection === "descending"
                    ? ArrowDownIcon
                    : ArrowsDownUpIcon
              }
              size="sm"
              className={parts.sortIcon()}
            />
          ) : null}
        </span>
      )}
    </AriaColumn>
  );
}

export interface TableBodyProps<T> extends Omit<AriaTableBodyProps<T>, "className"> {}

export function TableBody<T extends object>(props: TableBodyProps<T>) {
  return <AriaTableBody data-slot="table-body" {...props} />;
}

export interface RowProps<T> extends Omit<AriaRowProps<T>, "className"> {
  /** Layout only. */
  className?: string;
}

/** A row; when the table allows selection it starts with the row's checkbox. */
export function Row<T extends object>({ columns, children, className, ...props }: RowProps<T>) {
  const { t } = useStepclimbStrings();
  const { selectionBehavior } = useTableOptions();
  return (
    <AriaRow data-slot="table-row" {...props} className={styles.row({ className })}>
      {selectionBehavior === "toggle" ? (
        <AriaCell
          data-slot="table-selection-cell"
          className={styles.cell({ className: styles.selectionCell() })}
        >
          <Checkbox slot="selection" aria-label={t("table.selectRow")} />
        </AriaCell>
      ) : null}
      <Collection items={columns}>{children}</Collection>
    </AriaRow>
  );
}

export interface CellProps extends Omit<AriaCellProps, "className"> {
  /** `end` for numbers. */
  align?: "start" | "end";
  /** Layout only. */
  className?: string;
}

export function Cell({ align, className, children, ...props }: CellProps) {
  return (
    <AriaCell data-slot="table-cell" {...props} className={table({ align }).cell({ className })}>
      {children}
    </AriaCell>
  );
}
