// SPDX-License-Identifier: AGPL-3.0-or-later
// DataList: a list of records, one per row, for lists that are not tables: a person's courses, an
// organization's pending admissions (docs/specs/m0/13-base-ui-components.md M0 inventory). Built on
// React Aria's GridList: arrow keys move between items, an item's own buttons and links are reached
// with the arrow keys inside it, and with `selectionMode` each item starts with a checkbox that
// Space toggles. Items are at least as tall as a control, 44 px on touch screens.
import type { ReactNode } from "react";
import {
  GridList as AriaGridList,
  GridListItem as AriaGridListItem,
  type GridListItemProps as AriaGridListItemProps,
  type GridListProps as AriaGridListProps,
  composeRenderProps,
} from "react-aria-components";
import { tv } from "../lib/tv";
import { useStepclimbStrings } from "../strings/strings";
import { Checkbox } from "./checkbox";

export const dataList = tv({
  slots: {
    root: "flex flex-col rounded-md border border-line bg-raised font-ui text-sm text-fg-default outline-none",
    item: [
      "flex min-h-(--sc-control-h) cursor-default items-center gap-3 border-b border-line-soft px-3 py-2",
      "outline-none last:border-b-0",
      "data-[hovered]:bg-sunken data-[selected]:bg-brand-soft data-[disabled]:text-fg-disabled",
      "data-[focus-visible]:ring-2 data-[focus-visible]:ring-inset data-[focus-visible]:ring-ring",
    ],
    content: "flex min-w-0 flex-1 items-center gap-3",
  },
});

const styles = dataList();

export interface DataListProps<T>
  extends Omit<AriaGridListProps<T>, "className" | "renderEmptyState" | "layout"> {
  /** What the list shows when it has no items, usually an EmptyState. */
  emptyState?: ReactNode;
  /** Layout only. */
  className?: string;
}

/**
 * Name it with `aria-label` or `aria-labelledby`. Controlled with `selectedKeys`, uncontrolled
 * with `defaultSelectedKeys`; `onAction` makes a press on an item open it.
 */
export function DataList<T extends object>({ emptyState, className, ...props }: DataListProps<T>) {
  return (
    <AriaGridList
      data-slot="data-list"
      {...props}
      renderEmptyState={() => <div data-slot="data-list-empty">{emptyState}</div>}
      className={styles.root({ className })}
    />
  );
}

export interface DataListItemProps<T> extends Omit<AriaGridListItemProps<T>, "className"> {
  /** Layout only. */
  className?: string;
}

/** One item. Give `textValue` when its content is not plain text. */
export function DataListItem<T extends object>({
  children,
  className,
  ...props
}: DataListItemProps<T>) {
  const { t } = useStepclimbStrings();
  const textValue = props.textValue ?? (typeof children === "string" ? children : undefined);
  return (
    <AriaGridListItem
      data-slot="data-list-item"
      textValue={textValue}
      {...props}
      className={styles.item({ className })}
    >
      {composeRenderProps(children, (content, { selectionMode, selectionBehavior }) => (
        <>
          {selectionMode !== "none" && selectionBehavior === "toggle" ? (
            <Checkbox slot="selection" aria-label={t("dataList.selectItem")} />
          ) : null}
          <div data-slot="data-list-item-content" className={styles.content()}>
            {content}
          </div>
        </>
      ))}
    </AriaGridListItem>
  );
}
