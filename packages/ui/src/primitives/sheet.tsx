// SPDX-License-Identifier: AGPL-3.0-or-later
// Sheet: a modal panel for editing, at the inline end of the screen on wide screens and from the
// bottom on phones (docs/specs/m0/13-base-ui-components.md M0 inventory). It keeps inside the
// safe areas. Built on React Aria's ModalOverlay and Modal, like Dialog: focus moves in and stays,
// Escape, the close button or a press on the scrim closes it and returns focus.
import { XIcon } from "@phosphor-icons/react";
import type { ReactNode } from "react";
import { Dialog as AriaDialog, Modal, ModalOverlay } from "react-aria-components";
import { WIDE_QUERY } from "../lib/breakpoints";
import { tv } from "../lib/tv";
import { useMediaQuery } from "../lib/use-media-query";
import { useStepclimbStrings } from "../strings/strings";
import { useAppearanceAttributes } from "../theme/theme-provider";
import type { DialogChildren, DialogProps } from "./dialog";
import { Heading } from "./heading";
import { IconButton } from "./icon-button";

export const sheet = tv({
  slots: {
    // Top-anchored and as tall as the visual viewport, which React Aria's ModalOverlay sets as
    // --visual-viewport-height: iOS does not shrink the layout viewport for the on-screen
    // keyboard, so a panel sized to it would sit under the keyboard.
    overlay:
      "fixed inset-x-0 top-0 z-50 flex h-(--visual-viewport-height) bg-overlay backdrop-blur-sm",
    panel: [
      "flex flex-col border-line bg-raised font-ui text-fg-default shadow-float outline-none",
      "ps-[env(safe-area-inset-left)] pe-[env(safe-area-inset-right)]",
      "rtl:ps-[env(safe-area-inset-right)] rtl:pe-[env(safe-area-inset-left)]",
    ],
    // Takes focus on opening, like Dialog's, and draws no ring for it.
    root: "flex min-h-0 flex-1 flex-col outline-none focus-visible:shadow-none",
    header: "flex items-start gap-4 px-6 pt-5 pb-3",
    title: "min-w-0 flex-1",
    close: "-me-2 -mt-1 shrink-0",
    // No overscroll-behavior of its own: while the sheet is open, React Aria's ModalOverlay stops
    // the page from scrolling (and sets overscroll-behavior on every element in iOS WebKit), and
    // base.css keeps the document from bouncing. With one set, Chromium composites the body as a
    // scroller even when nothing overflows, and the fields in it render a pixel differently
    // depending on when the layer is rastered.
    body: "min-h-0 flex-1 overflow-y-auto px-6 pb-6 text-sm",
    footer: "flex flex-wrap justify-end gap-2 border-t border-line px-6 py-4",
  },
  variants: {
    placement: {
      end: {
        overlay: "justify-end",
        panel: [
          "h-full w-full max-w-md border-s",
          "pt-[env(safe-area-inset-top)] pb-[env(safe-area-inset-bottom)]",
        ],
      },
      bottom: {
        overlay: "items-end",
        panel:
          "max-h-[calc(var(--visual-viewport-height)*0.85)] w-full rounded-t-md border-t pb-[env(safe-area-inset-bottom)]",
      },
    },
  },
  defaultVariants: { placement: "end" },
});

export type SheetPlacement = "auto" | "end" | "bottom";

export interface SheetProps
  extends Pick<DialogProps, "isOpen" | "defaultOpen" | "onOpenChange" | "isDismissable"> {
  /** The visible title, which also names the sheet. */
  title: ReactNode;
  /** The content, which scrolls; a function is given `close`. */
  children: DialogChildren;
  /** Actions pinned under the content: the cancel first, the action that ends it last. */
  footer?: ReactNode | ((opts: { close: () => void }) => ReactNode);
  /**
   * `auto` (the default): at the inline end at 768 px and wider, from the bottom under it. `end`
   * and `bottom` fix it.
   */
  placement?: SheetPlacement;
  /** Layout only, on the panel. */
  className?: string;
}

/** The placement `auto` resolves to at the current width. */
function usePlacement(placement: SheetPlacement): "end" | "bottom" {
  const wide = useMediaQuery(WIDE_QUERY);
  if (placement !== "auto") return placement;
  return wide ? "end" : "bottom";
}

/** Inside a `DialogTrigger`, opened by its button; on its own, controlled with `isOpen`. */
export function Sheet({
  title,
  children,
  footer,
  placement = "auto",
  className,
  isDismissable = true,
  ...overlay
}: SheetProps) {
  const { t } = useStepclimbStrings();
  const appearance = useAppearanceAttributes();
  const resolved = usePlacement(placement);
  const styles = sheet({ placement: resolved });
  return (
    <ModalOverlay
      data-slot="sheet-overlay"
      isDismissable={isDismissable}
      {...overlay}
      {...appearance}
      className={styles.overlay()}
    >
      <Modal data-slot="sheet" data-placement={resolved} className={styles.panel({ className })}>
        <AriaDialog data-slot="sheet-dialog" className={styles.root()}>
          {({ close }) => (
            <>
              <div data-slot="sheet-header" className={styles.header()}>
                <Heading slot="title" level={2} size="lg" className={styles.title()}>
                  {title}
                </Heading>
                <IconButton
                  data-slot="sheet-close"
                  icon={XIcon}
                  label={t("sheet.close")}
                  size="sm"
                  onPress={close}
                  className={styles.close()}
                />
              </div>
              <div data-slot="sheet-body" className={styles.body()}>
                {typeof children === "function" ? children({ close }) : children}
              </div>
              {footer === undefined ? null : (
                <div data-slot="sheet-footer" className={styles.footer()}>
                  {typeof footer === "function" ? footer({ close }) : footer}
                </div>
              )}
            </>
          )}
        </AriaDialog>
      </Modal>
    </ModalOverlay>
  );
}
