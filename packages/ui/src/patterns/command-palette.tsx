// SPDX-License-Identifier: AGPL-3.0-or-later
// CommandPalette: a search box over the app's commands, in a modal at the top of the screen
// (docs/specs/m0/13-base-ui-components.md M0 inventory). The app passes its commands from M1; this
// is the component and its keyboard model:
//
//   Ctrl+K or ⌘K   opens it from anywhere on the page, and closes it again
//   typing         filters the commands by label and keywords, ignoring case and accents
//   Up, Down       move through the matches while focus stays in the search box
//   Enter          runs the highlighted command and closes the palette
//   Escape         clears the search, then closes the palette; focus returns where it was
//
// Built on React Aria's Autocomplete, Menu and ModalOverlay. Like every modal overlay it is as tall
// as `--visual-viewport-height`, so the list ends above the iOS on-screen keyboard.
import { MagnifyingGlassIcon, type Icon as PhosphorIcon } from "@phosphor-icons/react";
import { useEffect, useState } from "react";
import {
  Dialog as AriaDialog,
  Menu as AriaMenu,
  MenuItem as AriaMenuItem,
  SearchField as AriaSearchField,
  Autocomplete,
  type AutocompleteProps,
  Input,
  Modal,
  ModalOverlay,
  useFilter,
} from "react-aria-components";
import { tv } from "../lib/tv";
import { useStepclimbStrings } from "../strings/strings";
import { Icon } from "../theme/icon";
import { useAppearanceAttributes } from "../theme/theme-provider";

export const commandPalette = tv({
  slots: {
    // Dimmed, not blurred, like the Menu's sheet: the palette is a quick pick.
    overlay: [
      "fixed inset-x-0 top-0 z-50 flex h-(--visual-viewport-height) items-start justify-center bg-overlay",
      "px-3 pt-[max(0.75rem,env(safe-area-inset-top))] pb-3 md:pt-[12vh]",
    ],
    modal: [
      "flex max-h-full w-full max-w-xl flex-col overflow-hidden rounded-md border border-line",
      "bg-raised font-ui text-fg-default shadow-float outline-none md:max-h-[min(28rem,100%)]",
    ],
    // Takes focus while the search box mounts, like Dialog's, and draws no ring for it.
    dialog: "flex min-h-0 flex-1 flex-col outline-none focus-visible:shadow-none",
    search: "flex shrink-0 items-center gap-2 border-b border-line px-4 focus-within:border-brand",
    searchIcon: "shrink-0 text-fg-soft",
    // The search box always has focus while the palette is open; the brand rule under it marks
    // that, instead of a ring inside the panel.
    input: [
      "h-12 min-w-0 flex-1 bg-transparent text-base text-fg-strong outline-none focus-visible:shadow-none",
      "placeholder:text-fg-sub [&::-webkit-search-cancel-button]:hidden",
    ],
    // The commands take virtual focus, so none of them is focusable; the arrow keys in the search
    // box scroll the highlighted one into view. The box that scrolls is a tab stop of its own, so a
    // keyboard can scroll it as well (axe scrollable-region-focusable, WCAG 2.1.1).
    scroll: [
      "min-h-0 flex-1 overflow-y-auto overscroll-contain outline-none",
      "focus-visible:ring-2 focus-visible:ring-inset focus-visible:ring-ring",
    ],
    list: "p-1 outline-none",
    item: [
      "flex min-h-(--sc-control-h) cursor-default items-center gap-3 rounded-xs px-3 py-1.5 text-sm",
      "text-fg-default outline-none",
      "data-[focused]:bg-sunken data-[pressed]:bg-sunken data-[focused]:text-fg-strong",
      "data-[disabled]:cursor-not-allowed data-[disabled]:text-fg-disabled",
    ],
    itemIcon: "shrink-0 text-fg-sub",
    itemLabel: "min-w-0 flex-1",
    empty: "px-3 py-6 text-center text-sm text-fg-sub",
  },
});

const styles = commandPalette();

/** One command: what the palette lists, and what `onAction` is called with. */
export interface Command {
  id: string;
  /** What the command does, as a person would say it ("Go to courses"), from the strings. */
  label: string;
  /** Other words that find it ("classes", "catalog"), from the strings. */
  keywords?: readonly string[];
  /** A Phosphor icon drawn before the label, decorative. */
  icon?: PhosphorIcon;
  isDisabled?: boolean;
}

export interface CommandPaletteProps {
  /** The commands, in the order to list them. */
  commands: readonly Command[];
  /** Called with the command's `id` after the palette has closed. */
  onAction: (id: string) => void;
  /** Controlled: whether it is open. */
  isOpen?: boolean;
  /** Uncontrolled: whether it starts open. */
  defaultOpen?: boolean;
  onOpenChange?: (isOpen: boolean) => void;
}

/** Open state, controlled through `isOpen` or held here. */
function useOpenState({ isOpen, defaultOpen = false, onOpenChange }: CommandPaletteProps) {
  const [own, setOwn] = useState(defaultOpen);
  const open = isOpen ?? own;
  const setOpen = (next: boolean) => {
    if (isOpen === undefined) setOwn(next);
    onOpenChange?.(next);
  };
  return [open, setOpen] as const;
}

/** Whether `event` is the palette's shortcut: Ctrl+K, or ⌘K on Apple keyboards. */
export function isPaletteShortcut(event: KeyboardEvent): boolean {
  return (
    (event.metaKey || event.ctrlKey) &&
    !event.altKey &&
    !event.shiftKey &&
    event.key.toLowerCase() === "k"
  );
}

/**
 * Toggles the palette on its shortcut anywhere in the document. A handler that already took the
 * key (an editor's own Ctrl+K) keeps it: an event whose default was prevented is left alone.
 */
function useShortcut(open: boolean, setOpen: (next: boolean) => void) {
  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.defaultPrevented || !isPaletteShortcut(event)) return;
      event.preventDefault();
      setOpen(!open);
    };
    document.addEventListener("keydown", onKeyDown);
    return () => document.removeEventListener("keydown", onKeyDown);
  });
}

/** Render one CommandPalette near the root of the app; its shortcut works while it is mounted. */
export function CommandPalette(props: CommandPaletteProps) {
  const { commands, onAction } = props;
  const { t } = useStepclimbStrings();
  const appearance = useAppearanceAttributes();
  const [open, setOpen] = useOpenState(props);
  useShortcut(open, setOpen);
  const { contains } = useFilter({ sensitivity: "base" });
  const matches: AutocompleteProps<Command>["filter"] = (text, input, node) => {
    const keywords = node.value?.keywords ?? [];
    return contains(text, input) || keywords.some((word) => contains(word, input));
  };
  const run = (id: string) => {
    setOpen(false);
    onAction(id);
  };
  return (
    <ModalOverlay
      data-slot="command-palette-overlay"
      isDismissable
      isOpen={open}
      onOpenChange={setOpen}
      {...appearance}
      className={styles.overlay()}
    >
      <Modal data-slot="command-palette-modal" className={styles.modal()}>
        <AriaDialog
          data-slot="command-palette"
          aria-label={t("commandPalette.label")}
          className={styles.dialog()}
        >
          <Autocomplete<Command> filter={matches}>
            <AriaSearchField
              data-slot="command-palette-search"
              aria-label={t("commandPalette.search")}
              autoFocus
              className={styles.search()}
            >
              <Icon icon={MagnifyingGlassIcon} size="sm" className={styles.searchIcon()} />
              <Input
                data-slot="command-palette-input"
                placeholder={t("commandPalette.placeholder")}
                className={styles.input()}
              />
            </AriaSearchField>
            {/* biome-ignore lint/a11y/noNoninteractiveTabindex: a scrolling box with nothing focusable in it is reached by Tab, or a keyboard cannot scroll it (axe scrollable-region-focusable) */}
            <div data-slot="command-palette-scroll" tabIndex={0} className={styles.scroll()}>
              <AriaMenu
                data-slot="command-palette-list"
                aria-label={t("commandPalette.results")}
                items={commands}
                onAction={(key) => run(String(key))}
                renderEmptyState={() => (
                  <p data-slot="command-palette-empty" className={styles.empty()}>
                    {t("commandPalette.empty")}
                  </p>
                )}
                className={styles.list()}
              >
                {(command) => (
                  <AriaMenuItem
                    data-slot="command-palette-item"
                    id={command.id}
                    textValue={command.label}
                    isDisabled={command.isDisabled}
                    className={styles.item()}
                  >
                    {command.icon ? (
                      <Icon icon={command.icon} size="sm" className={styles.itemIcon()} />
                    ) : null}
                    <span className={styles.itemLabel()}>{command.label}</span>
                  </AriaMenuItem>
                )}
              </AriaMenu>
            </div>
          </Autocomplete>
        </AriaDialog>
      </Modal>
    </ModalOverlay>
  );
}
