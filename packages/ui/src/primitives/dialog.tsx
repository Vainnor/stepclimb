// SPDX-License-Identifier: AGPL-3.0-or-later
// Dialog: a modal for a confirmation or a short input, never for work: editing and grading use
// pages or sheets (docs/specs/m0/13-base-ui-components.md M0 inventory; plan UI tab -> Pitfalls:
// modals for work). Focus moves into it and stays there; Escape, the close button or a press on the
// scrim closes it and returns focus to what opened it. The scrim is `bg-overlay`, the one place a
// blur is allowed.
import { XIcon } from "@phosphor-icons/react";
import type { ReactNode } from "react";
import {
  Dialog as AriaDialog,
  type DialogProps as AriaDialogProps,
  DialogTrigger as AriaDialogTrigger,
  type DialogTriggerProps,
  Modal,
  ModalOverlay,
  type ModalOverlayProps,
} from "react-aria-components";
import { tv, type VariantProps } from "../lib/tv";
import { useStepclimbStrings } from "../strings/strings";
import { useAppearanceAttributes } from "../theme/theme-provider";
import { Heading } from "./heading";
import { IconButton } from "./icon-button";

export const dialog = tv({
  slots: {
    overlay: [
      "fixed inset-0 z-50 flex items-center justify-center overflow-y-auto bg-overlay backdrop-blur-sm",
      "px-4 pt-[max(1rem,env(safe-area-inset-top))] pb-[max(1rem,env(safe-area-inset-bottom))]",
    ],
    modal: [
      "w-full rounded-md border border-line bg-raised font-ui text-fg-default shadow-float",
      "outline-none",
    ],
    // The dialog takes focus when it opens, to start the focus trap; it draws no ring of its own,
    // since what has focus is the whole window the person is looking at.
    root: "flex flex-col gap-4 p-6 outline-none focus-visible:shadow-none",
    header: "flex items-start gap-4",
    title: "min-w-0 flex-1",
    close: "-me-2 -mt-2 shrink-0",
    body: "flex flex-col gap-3 text-sm",
    footer: "flex flex-wrap justify-end gap-2",
  },
  variants: {
    size: {
      sm: { modal: "max-w-sm" },
      md: { modal: "max-w-md" },
      lg: { modal: "max-w-lg" },
    },
  },
  defaultVariants: { size: "md" },
});

/** Opens its Dialog, AlertDialog or Sheet when its first child, a button, is pressed. */
export function DialogTrigger(props: DialogTriggerProps) {
  return <AriaDialogTrigger {...props} />;
}

/** What a Dialog's content can be: nodes, or a function given `close`. */
export type DialogChildren = ReactNode | ((opts: { close: () => void }) => ReactNode);

export interface DialogProps
  extends Pick<
      ModalOverlayProps,
      "isOpen" | "defaultOpen" | "onOpenChange" | "isDismissable" | "isKeyboardDismissDisabled"
    >,
    Pick<AriaDialogProps, "role" | "aria-describedby">,
    VariantProps<typeof dialog> {
  /** The visible title, which also names the dialog. Required: a dialog without a name is lost. */
  title: ReactNode;
  /** The content; a function is given `close`. Put actions in a `DialogFooter`. */
  children: DialogChildren;
  /** Hides the close button, for a dialog that must end through its own actions. */
  hideCloseButton?: boolean;
  /** Layout only, on the panel. */
  className?: string;
}

/**
 * Inside a `DialogTrigger`, opened by its button; on its own, controlled with `isOpen` and
 * `onOpenChange`. A press on the scrim closes it unless `isDismissable` is false.
 */
export function Dialog({
  title,
  children,
  hideCloseButton = false,
  size,
  className,
  role,
  "aria-describedby": describedBy,
  isDismissable = true,
  ...overlay
}: DialogProps) {
  const { t } = useStepclimbStrings();
  const appearance = useAppearanceAttributes();
  const styles = dialog({ size });
  return (
    <ModalOverlay
      data-slot="dialog-overlay"
      isDismissable={isDismissable}
      {...overlay}
      {...appearance}
      className={styles.overlay()}
    >
      <Modal data-slot="dialog-modal" className={styles.modal({ className })}>
        <AriaDialog
          data-slot="dialog"
          role={role}
          aria-describedby={describedBy}
          className={styles.root()}
        >
          {({ close }) => (
            <>
              <div data-slot="dialog-header" className={styles.header()}>
                <Heading slot="title" level={2} size="lg" className={styles.title()}>
                  {title}
                </Heading>
                {hideCloseButton ? null : (
                  <IconButton
                    data-slot="dialog-close"
                    icon={XIcon}
                    label={t("dialog.close")}
                    size="sm"
                    onPress={close}
                    className={styles.close()}
                  />
                )}
              </div>
              {typeof children === "function" ? children({ close }) : children}
            </>
          )}
        </AriaDialog>
      </Modal>
    </ModalOverlay>
  );
}

export interface DialogPartProps {
  children?: ReactNode;
  /** Layout only. */
  className?: string;
  id?: string;
}

/** The dialog's text and fields. */
export function DialogBody({ className, ...props }: DialogPartProps) {
  return <div data-slot="dialog-body" {...props} className={dialog().body({ className })} />;
}

/** The dialog's actions, at the inline end: the cancel first, the action that ends it last. */
export function DialogFooter({ className, ...props }: DialogPartProps) {
  return <div data-slot="dialog-footer" {...props} className={dialog().footer({ className })} />;
}
