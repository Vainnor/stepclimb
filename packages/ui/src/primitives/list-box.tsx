// SPDX-License-Identifier: AGPL-3.0-or-later
// The list of options Select and ComboBox open, and its overlay: a Popover under the field at
// 768 px and wider, a bottom sheet under 768 px (docs/specs/m0/13-base-ui-components.md Q10, the
// same breakpoint as the Menu). Not exported on its own: Select and ComboBox export its item as
// `SelectItem` and `ComboBoxItem`.
import { CheckIcon } from "@phosphor-icons/react";
import { type ReactNode, type Ref, use } from "react";
import {
  ListBox as AriaListBox,
  ListBoxItem as AriaListBoxItem,
  type ListBoxItemProps as AriaListBoxItemProps,
  Popover as AriaPopover,
  composeRenderProps,
  Modal,
  ModalOverlay,
  PopoverContext,
} from "react-aria-components";
import { WIDE_QUERY } from "../lib/breakpoints";
import { tv } from "../lib/tv";
import { useMediaQuery } from "../lib/use-media-query";
import { Icon } from "../theme/icon";
import { useAppearanceAttributes } from "../theme/theme-provider";
import { Popover } from "./popover";
import { sheet } from "./sheet";

export const listBox = tv({
  slots: {
    list: "max-h-[inherit] overflow-auto p-1 outline-none",
    // The Menu's sheet scrim: dimmed, not blurred, since the list is a quick pick, not a modal task.
    // Top-anchored and as tall as the visual viewport, which React Aria's ModalOverlay sets as
    // --visual-viewport-height: iOS does not shrink the layout viewport for the on-screen
    // keyboard, so a panel sized to it would sit under the keyboard.
    sheetOverlay:
      "fixed inset-x-0 top-0 z-50 flex h-(--visual-viewport-height) items-end bg-overlay",
    sheetList: "overflow-auto p-1 outline-none",
    item: [
      "flex min-h-(--sc-control-h) cursor-default items-center gap-2 rounded-xs px-3 py-1.5",
      "text-sm text-fg-default outline-none",
      "data-[focused]:bg-sunken data-[pressed]:bg-sunken",
      "data-[focus-visible]:ring-2 data-[focus-visible]:ring-ring",
      "data-[selected]:font-medium data-[selected]:text-fg-strong",
      "data-[disabled]:cursor-not-allowed data-[disabled]:text-fg-disabled",
    ],
    itemLabel: "min-w-0 flex-1",
    itemCheck: "flex size-4 shrink-0 items-center justify-center text-brand-fg",
  },
});

const styles = listBox();

export interface ListBoxItemProps<T> extends Omit<AriaListBoxItemProps<T>, "className"> {
  /** Layout only. */
  className?: string;
}

/** One option. A check marks the selected one. Give `textValue` when the content is not text. */
export function ListBoxItem<T extends object>({
  className,
  children,
  ...props
}: ListBoxItemProps<T>) {
  const textValue = props.textValue ?? (typeof children === "string" ? children : undefined);
  return (
    <AriaListBoxItem
      data-slot="list-box-item"
      textValue={textValue}
      {...props}
      className={styles.item({ className })}
    >
      {composeRenderProps(children, (content, { isSelected }) => (
        <>
          <span data-slot="list-box-item-label" className={styles.itemLabel()}>
            {content}
          </span>
          <span data-slot="list-box-item-check" className={styles.itemCheck()}>
            {isSelected ? <Icon icon={CheckIcon} size="sm" /> : null}
          </span>
        </>
      ))}
    </AriaListBoxItem>
  );
}

/**
 * The bottom sheet's panel. Select and ComboBox hand their Popover a ref through React Aria's
 * PopoverContext; the sheet takes it instead, so ComboBox counts a press in the sheet as inside its
 * overlay and keeps the options visible to screen readers.
 */
function SheetPanel({ header, children }: { header?: ReactNode; children: ReactNode }) {
  const context = use(PopoverContext);
  const ref =
    context && "ref" in context ? (context.ref as Ref<HTMLDivElement> | undefined) : undefined;
  const parts = sheet({ placement: "bottom" });
  return (
    <Modal ref={ref} data-slot="list-box-sheet" className={parts.panel()}>
      {header}
      {children}
    </Modal>
  );
}

/** A list's options: items, or a function that renders each of the field's `items`. */
export type ListBoxChildren<T> = ReactNode | ((item: T) => ReactNode);

export interface ListBoxOverlayProps<T> {
  /** The options. */
  children: ListBoxChildren<T>;
  /** Above the list in the bottom sheet: ComboBox puts its input here. */
  sheetHeader?: ReactNode;
  /** What the list shows when it has no options. */
  renderEmptyState?: () => ReactNode;
}

/**
 * The options in a Popover at 768 px and wider, or in a bottom sheet under it. React Aria builds
 * the field's collection from a hidden render of its children, which only its own Popover passes
 * through; in sheet mode a Popover that never opens carries the options for that render.
 */
export function ListBoxOverlay<T extends object>({
  children,
  sheetHeader,
  renderEmptyState,
}: ListBoxOverlayProps<T>) {
  const wide = useMediaQuery(WIDE_QUERY);
  const appearance = useAppearanceAttributes();
  if (wide) {
    return (
      <Popover data-slot="list-box-popover" placement="bottom start">
        <AriaListBox<T>
          data-slot="list-box"
          renderEmptyState={renderEmptyState}
          className={styles.list()}
        >
          {children}
        </AriaListBox>
      </Popover>
    );
  }
  return (
    <>
      <AriaPopover isOpen={false}>
        <AriaListBox<T>>{children}</AriaListBox>
      </AriaPopover>
      <ModalOverlay
        data-slot="list-box-sheet-overlay"
        isDismissable
        {...appearance}
        className={styles.sheetOverlay()}
      >
        <SheetPanel header={sheetHeader}>
          <AriaListBox<T>
            data-slot="list-box"
            renderEmptyState={renderEmptyState}
            className={styles.sheetList()}
          >
            {children}
          </AriaListBox>
        </SheetPanel>
      </ModalOverlay>
    </>
  );
}
