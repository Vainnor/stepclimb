// SPDX-License-Identifier: AGPL-3.0-or-later
// Tabs: views of one subject that share a page, switched in place
// (docs/specs/m0/13-base-ui-components.md M0 inventory). The arrow keys move between tabs; Tab
// moves into the panel. The selected tab is marked by a brand underline and a stronger label, so
// color is not the only sign. A tab list wider than the screen scrolls on its own.
import {
  Tab as AriaTab,
  TabList as AriaTabList,
  type TabListProps as AriaTabListProps,
  TabPanel as AriaTabPanel,
  type TabPanelProps as AriaTabPanelProps,
  type TabProps as AriaTabProps,
  Tabs as AriaTabs,
  type TabsProps as AriaTabsProps,
} from "react-aria-components";
import { tv } from "../lib/tv";

export const tabs = tv({
  slots: {
    root: "flex flex-col gap-4 font-ui",
    list: [
      "flex overflow-x-auto overscroll-x-contain border-b border-line",
      "[scrollbar-width:none]",
    ],
    tab: [
      "relative -mb-px flex h-(--sc-control-h) shrink-0 cursor-default items-center whitespace-nowrap",
      "border-b-2 border-transparent px-3 text-sm font-medium text-fg-sub outline-none",
      "transition-colors duration-fast ease-out-expo",
      "data-[hovered]:text-fg-strong",
      "data-[selected]:border-brand data-[selected]:text-fg-strong",
      "data-[focus-visible]:ring-2 data-[focus-visible]:ring-inset data-[focus-visible]:ring-ring",
      "data-[disabled]:cursor-not-allowed data-[disabled]:text-fg-disabled",
    ],
    panel: [
      "rounded-xs outline-none",
      "data-[focus-visible]:ring-2 data-[focus-visible]:ring-ring data-[focus-visible]:ring-offset-2 ring-offset-canvas",
    ],
  },
});

const styles = tabs();

export interface TabsProps extends Omit<AriaTabsProps, "className"> {
  /** Layout only. */
  className?: string;
}

/** Controlled with `selectedKey` and `onSelectionChange`, uncontrolled with `defaultSelectedKey`. */
export function Tabs({ className, ...props }: TabsProps) {
  return <AriaTabs data-slot="tabs" {...props} className={styles.root({ className })} />;
}

export interface TabListProps<T> extends Omit<AriaTabListProps<T>, "className"> {
  /** Layout only. */
  className?: string;
}

/** The row of tabs. Name it with `aria-label` when no heading above it says what it switches. */
export function TabList<T extends object>({ className, ...props }: TabListProps<T>) {
  return <AriaTabList data-slot="tab-list" {...props} className={styles.list({ className })} />;
}

export interface TabProps extends Omit<AriaTabProps, "className"> {
  /** Layout only. */
  className?: string;
}

/** One tab; its `id` matches its TabPanel's. */
export function Tab({ className, ...props }: TabProps) {
  return <AriaTab data-slot="tab" {...props} className={styles.tab({ className })} />;
}

export interface TabPanelProps extends Omit<AriaTabPanelProps, "className"> {
  /** Layout only. */
  className?: string;
}

/** The content of the tab with the same `id`. */
export function TabPanel({ className, ...props }: TabPanelProps) {
  return <AriaTabPanel data-slot="tab-panel" {...props} className={styles.panel({ className })} />;
}
