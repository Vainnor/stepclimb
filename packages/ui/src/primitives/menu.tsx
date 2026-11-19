// SPDX-License-Identifier: AGPL-3.0-or-later
// Menu: a list of actions opened from a button. At 768 px and wider it opens in a Popover; under
// 768 px, the breakpoint of the bottom tab bar, it opens as a bottom sheet
// (docs/specs/m0/13-base-ui-components.md M0 inventory, Q10). A submenu always opens in a Popover,
// beside its item, or above it under 768 px. Arrow keys move between items; typing jumps to an item.
import { CaretRightIcon, CheckIcon } from "@phosphor-icons/react";
import {
  createContext,
  type ReactNode,
  type RefObject,
  use,
  useEffect,
  useRef,
  useState,
} from "react";
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
import { WIDE_QUERY } from "../lib/breakpoints";
import { tv } from "../lib/tv";
import { useMediaQuery } from "../lib/use-media-query";
import { Icon } from "../theme/icon";
import { useAppearanceAttributes } from "../theme/theme-provider";
import { Popover } from "./popover";

/** Where the menu stops being a bottom sheet: BottomTabs' breakpoint (spec 11 B3). */
export const MENU_POPOVER_QUERY = WIDE_QUERY;

export const menu = tv({
  slots: {
    list: "max-h-[inherit] min-w-48 overflow-auto p-1 outline-none",
    // Top-anchored and as tall as the visual viewport, which React Aria's ModalOverlay sets as
    // --visual-viewport-height: iOS does not shrink the layout viewport for the on-screen
    // keyboard, so a panel sized to it would sit under the keyboard.
    overlay: "fixed inset-x-0 top-0 z-50 flex h-(--visual-viewport-height) items-end bg-overlay",
    sheet: [
      "max-h-[calc(var(--visual-viewport-height)*0.85)] w-full overflow-auto rounded-t-md border-t border-line bg-raised font-ui",
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

/**
 * Whether a primary-button mouse or pen press on the trigger is still held: a pressed mouse opens
 * the menu at once, so its release comes after the menu is drawn. A key pressed on the trigger
 * clears it, so a menu opened from the keyboard never inherits a press whose release was lost.
 */
const HeldPressContext = createContext<RefObject<boolean> | null>(null);

/** Opens its Menu when its first child, a Button or IconButton, is pressed. */
export function MenuTrigger({ children, ...props }: AriaMenuTriggerProps) {
  const held = useRef(false);
  return (
    <HeldPressContext value={held}>
      <span
        data-slot="menu-trigger"
        className="contents"
        onPointerDownCapture={(event) => {
          // Only the primary button opens the menu; another button's release may never come.
          if (event.pointerType === "touch" || event.button !== 0) return;
          held.current = true;
          const release = () => {
            held.current = false;
          };
          const doc = event.currentTarget.ownerDocument;
          doc.addEventListener("pointerup", release, { capture: true, once: true });
          doc.addEventListener("pointercancel", release, { capture: true, once: true });
        }}
        onKeyDownCapture={() => {
          held.current = false;
        }}
      >
        <AriaMenuTrigger {...props}>{children}</AriaMenuTrigger>
      </span>
    </HeldPressContext>
  );
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
      // A submenu's trigger is an item as wide as its menu: the submenu does not take its width.
      // Under 768 px the item spans the sheet, so there is no room beside it: the submenu opens
      // above it instead.
      <Popover
        data-slot="menu-popover"
        placement={isSubmenu && !wide ? "top end" : placement}
        className={isSubmenu ? "min-w-48" : undefined}
      >
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
        <SheetBody>{list}</SheetBody>
      </Modal>
    </ModalOverlay>
  );
}

/**
 * The sheet's items. A mouse opens the menu on the press, so the sheet can appear under the
 * pointer, and the release of that same press would run the item it lands on. While that press is
 * held the items take no pointer events, so its release lands on the sheet itself. Touch opens the
 * menu on the release, so it is not held.
 */
function SheetBody({ children }: { children: ReactNode }) {
  const held = use(HeldPressContext);
  const [pressing, setPressing] = useState(() => held?.current === true);
  useEffect(() => {
    if (!pressing) return;
    const release = () => setPressing(false);
    document.addEventListener("pointerup", release, true);
    document.addEventListener("pointercancel", release, true);
    return () => {
      document.removeEventListener("pointerup", release, true);
      document.removeEventListener("pointercancel", release, true);
    };
  }, [pressing]);
  return (
    <div
      data-slot="menu-sheet-body"
      data-pressing={pressing || undefined}
      className="contents"
      // Inherited by every item; inline, so it holds before any stylesheet loads.
      style={pressing ? { pointerEvents: "none" } : undefined}
    >
      {children}
    </div>
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
