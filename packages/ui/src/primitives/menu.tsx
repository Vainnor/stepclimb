// SPDX-License-Identifier: AGPL-3.0-or-later
// Menu: a list of actions opened from a button. At 768 px and wider it opens in a Popover; under
// 768 px, the breakpoint of the bottom tab bar, it opens as a bottom sheet
// (docs/specs/m0/13-base-ui-components.md M0 inventory, Q10). A submenu always opens in a Popover
// beside its item. Arrow keys move between items; typing jumps to an item.
import { CaretRightIcon, CheckIcon } from "@phosphor-icons/react";
import { use } from "react";
import {
  Menu as AriaMenu,
  MenuItem as AriaMenuItem,
  type MenuItemProps as AriaMenuItemProps,
  type MenuProps as AriaMenuProps,
  MenuTrigger as AriaMenuTrigger,
  type MenuTriggerProps as AriaMenuTriggerProps,
  SubmenuTrigger as AriaSubmenuTrigger,
  type SubmenuTriggerProps as AriaSubmenuTriggerProps,
  composeRenderProps,
  Modal,
  ModalOverlay,
  PopoverContext,
  type PopoverProps,
} from "react-aria-components";
import { tv } from "../lib/tv";
import { useMediaQuery } from "../lib/use-media-query";
import { Icon } from "../theme/icon";
import { useAppearanceAttributes } from "../theme/theme-provider";
import { Popover } from "./popover";

/** Where the menu stops being a bottom sheet: BottomTabs' breakpoint (spec 11 B3). */
export const MENU_POPOVER_QUERY = "(min-width: 768px)";

export const menu = tv({
  slots: {
    list: "max-h-[inherit] min-w-48 overflow-auto p-1 outline-none",
    overlay: "fixed inset-0 z-50 flex items-end bg-overlay",
    sheet: [
      "max-h-[85dvh] w-full overflow-auto rounded-t-md border-t border-line bg-raised font-ui",
      "pb-[env(safe-area-inset-bottom)] text-fg-default shadow-float outline-none",
    ],
    item: [
      "flex h-(--sc-control-h) cursor-default items-center gap-2 rounded-xs px-3 text-sm",
      "text-fg-default outline-none",
      "data-[focused]:bg-sunken data-[pressed]:bg-sunken data-[focus-visible]:ring-2 data-[focus-visible]:ring-ring",
      "data-[disabled]:cursor-not-allowed data-[disabled]:text-fg-disabled",
    ],
    itemLabel: "min-w-0 flex-1 truncate",
    itemCheck: "flex size-4 shrink-0 items-center justify-center text-brand-fg",
    itemCaret: "shrink-0 text-fg-sub rtl:-scale-x-100",
  },
});

const styles = menu();

/** Opens its Menu when its first child, a Button or IconButton, is pressed. */
export function MenuTrigger(props: AriaMenuTriggerProps) {
  return <AriaMenuTrigger {...props} />;
}

export interface MenuProps<T> extends Omit<AriaMenuProps<T>, "className"> {
  /** Where the Popover sits beside the trigger at 768 px and wider. */
  placement?: PopoverProps["placement"];
  /** Layout only, on the list. */
  className?: string;
}

/** The list, in a Popover or a bottom sheet. Controlled and uncontrolled through `MenuTrigger`. */
export function Menu<T extends object>({ placement, className, ...props }: MenuProps<T>) {
  const isSubmenu = useSlottedPopoverTrigger() === "SubmenuTrigger";
  const wide = useMediaQuery(MENU_POPOVER_QUERY);
  const appearance = useAppearanceAttributes();
  const list = <AriaMenu data-slot="menu" {...props} className={styles.list({ className })} />;
  if (wide || isSubmenu) {
    return (
      <Popover data-slot="menu-popover" placement={placement}>
        {list}
      </Popover>
    );
  }
  return (
    <ModalOverlay
      data-slot="menu-sheet-overlay"
      isDismissable
      {...appearance}
      className={styles.overlay()}
    >
      <Modal data-slot="menu-sheet" className={styles.sheet()}>
        {list}
      </Modal>
    </ModalOverlay>
  );
}

/** Which trigger opened the surrounding overlay: React Aria tells its Popover through context. */
function useSlottedPopoverTrigger(): string | undefined {
  const context = use(PopoverContext);
  if (context && "trigger" in context && typeof context.trigger === "string") {
    return context.trigger;
  }
  return undefined;
}

export interface MenuItemProps<T> extends Omit<AriaMenuItemProps<T>, "className"> {
  /** Layout only. */
  className?: string;
}

/**
 * One action. In a menu with `selectionMode`, a check marks the selected items; an item that
 * opens a submenu ends in a caret. Give `textValue` when the content is not plain text.
 */
export function MenuItem<T extends object>({ className, children, ...props }: MenuItemProps<T>) {
  const textValue = props.textValue ?? (typeof children === "string" ? children : undefined);
  return (
    <AriaMenuItem
      data-slot="menu-item"
      textValue={textValue}
      {...props}
      className={styles.item({ className })}
    >
      {composeRenderProps(children, (content, { selectionMode, isSelected, hasSubmenu }) => (
        <>
          {selectionMode === "none" ? null : (
            <span data-slot="menu-item-check" className={styles.itemCheck()}>
              {isSelected ? <Icon icon={CheckIcon} size="sm" /> : null}
            </span>
          )}
          <span data-slot="menu-item-label" className={styles.itemLabel()}>
            {content}
          </span>
          {hasSubmenu ? (
            <Icon icon={CaretRightIcon} size="sm" className={styles.itemCaret()} />
          ) : null}
        </>
      ))}
    </AriaMenuItem>
  );
}

/** A MenuItem followed by the Menu it opens on hover, Enter or the arrow key toward it. */
export function SubmenuTrigger(props: AriaSubmenuTriggerProps) {
  return <AriaSubmenuTrigger {...props} />;
}
