// SPDX-License-Identifier: AGPL-3.0-or-later
// Tree: a hierarchy of items that open and close, such as the account tree or a course's file
// folders (docs/specs/m1/16-learning-ui-components.md B7). Built on React Aria's Tree: the arrow
// keys move between rows, Right opens a row and Left closes it or moves to its parent, Home and
// End jump, and typing jumps to a row by its text. Rows are at least as tall as a control, 44 px
// on touch screens; nesting indents by the row's level. Nothing animates when a row opens.
import { CaretRightIcon, type Icon as PhosphorIcon } from "@phosphor-icons/react";
import type { ReactNode } from "react";
import {
  Tree as AriaTree,
  TreeItem as AriaTreeItem,
  type TreeItemProps as AriaTreeItemProps,
  type TreeProps as AriaTreeProps,
  Button,
  TreeItemContent,
} from "react-aria-components";
import { tv } from "../lib/tv";
import { useStepclimbStrings } from "../strings/strings";
import { Icon } from "../theme/icon";

export const tree = tv({
  slots: {
    root: "flex flex-col rounded-md border border-line bg-raised p-1 font-ui text-sm text-fg-default outline-none",
    item: [
      "flex min-h-(--sc-control-h) cursor-default items-center gap-1 rounded-xs pe-3 outline-none",
      "pointer-coarse:min-h-(--sc-control-touch)",
      // One control height of indent per level below the first.
      "ps-[calc((var(--tree-item-level)-1)*var(--sc-control-h-sm))]",
      "data-[hovered]:bg-sunken data-[selected]:bg-brand-soft data-[selected]:text-fg-strong",
      "data-[disabled]:cursor-not-allowed data-[disabled]:text-fg-disabled",
      "data-[focus-visible]:ring-2 data-[focus-visible]:ring-inset data-[focus-visible]:ring-ring",
    ],
    chevron: [
      "flex size-(--sc-control-h-sm) shrink-0 items-center justify-center rounded-xs text-fg-sub outline-none",
      "data-[hovered]:bg-raised data-[hovered]:text-fg-strong",
    ],
    chevronIcon: "rtl:-scale-x-100",
    // Keeps a leaf's title in line with the titles of rows that have a chevron.
    spacer: "size-(--sc-control-h-sm) shrink-0",
    icon: "shrink-0 text-fg-sub",
    title: "min-w-0 flex-1 truncate py-1.5",
    empty: "px-3 py-2 text-fg-sub",
  },
  variants: {
    // The individual `scale` applies before `rotate`: the mirrored caret turns the other way.
    isExpanded: { true: { chevronIcon: "rotate-90 rtl:-rotate-90" } },
  },
});

const styles = tree();

export interface TreeProps<T>
  extends Omit<AriaTreeProps<T>, "className" | "renderEmptyState" | "style"> {
  /** What the tree shows when it has no items. */
  emptyState?: ReactNode;
  /** Layout only. */
  className?: string;
}

/**
 * Name it with `aria-label` or `aria-labelledby`. Open rows are controlled with `expandedKeys`
 * and `onExpandedChange`, uncontrolled with `defaultExpandedKeys`; selection likewise with
 * `selectedKeys` or `defaultSelectedKeys` and a `selectionMode`. `onAction` makes a press on a row
 * open what it stands for.
 */
export function Tree<T extends object>({ emptyState, className, ...props }: TreeProps<T>) {
  return (
    <AriaTree
      data-slot="tree"
      {...props}
      renderEmptyState={
        emptyState === undefined
          ? undefined
          : () => (
              <div data-slot="tree-empty" className={styles.empty()}>
                {emptyState}
              </div>
            )
      }
      className={styles.root({ className })}
    />
  );
}

export interface TreeItemProps<T>
  extends Omit<AriaTreeItemProps<T>, "className" | "children" | "textValue" | "style"> {
  /** What the row shows. */
  title: ReactNode;
  /** The row's text for typeahead; required when `title` is not a string. */
  textValue?: string;
  /** A Phosphor icon before the title, such as a folder. */
  icon?: PhosphorIcon;
  /** The nested rows. */
  children?: ReactNode;
  /** Layout only. */
  className?: string;
}

/** One row; its children are the rows nested under it, shown while it is open. */
export function TreeItem<T extends object>({
  title,
  textValue,
  icon,
  children,
  className,
  ...props
}: TreeItemProps<T>) {
  const { t } = useStepclimbStrings();
  const text = textValue ?? (typeof title === "string" ? title : "");
  return (
    <AriaTreeItem
      data-slot="tree-item"
      textValue={text}
      {...props}
      className={styles.item({ className })}
    >
      <TreeItemContent>
        {({ hasChildItems, isExpanded }) => (
          <>
            {hasChildItems ? (
              <Button
                data-slot="tree-item-chevron"
                slot="chevron"
                aria-label={t("tree.toggle", { expanded: isExpanded ? "yes" : "no" })}
                className={styles.chevron()}
              >
                <Icon
                  icon={CaretRightIcon}
                  size="sm"
                  className={styles.chevronIcon({ isExpanded })}
                />
              </Button>
            ) : (
              <span data-slot="tree-item-spacer" className={styles.spacer()} />
            )}
            {icon === undefined ? null : <Icon icon={icon} size="sm" className={styles.icon()} />}
            <span data-slot="tree-item-title" className={styles.title()}>
              {title}
            </span>
          </>
        )}
      </TreeItemContent>
      {children}
    </AriaTreeItem>
  );
}
