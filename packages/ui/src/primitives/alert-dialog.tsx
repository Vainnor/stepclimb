// SPDX-License-Identifier: AGPL-3.0-or-later
// AlertDialog: a Dialog with `role="alertdialog"` that asks before an action
// (docs/specs/m0/13-base-ui-components.md M0 inventory). Its confirm button names the action
// ("Delete course"), never "OK"; a destructive confirm uses the danger intent. It has no close
// button and a press on the scrim does not close it: it ends through Cancel, Escape or the action.
// Focus starts on Cancel, so Enter on arrival never runs the action.
import type { ReactNode } from "react";
import type { DialogTriggerProps } from "react-aria-components";
import { useStepclimbStrings } from "../strings/strings";
import { Button } from "./button";
import { Dialog, DialogBody, DialogFooter, type DialogProps } from "./dialog";

export interface AlertDialogProps
  extends Pick<DialogTriggerProps, "isOpen" | "defaultOpen" | "onOpenChange">,
    Pick<DialogProps, "size" | "className"> {
  /** The question: "Delete Ground control?". */
  title: ReactNode;
  /** What the action does and whether it can be undone. */
  children: ReactNode;
  /** The confirm button's text, naming the action: "Delete course". Required. */
  confirmLabel: string;
  /** `danger` for an action that destroys or cannot be undone. */
  intent?: "primary" | "danger";
  /** Called when the confirm button is pressed; the dialog then closes. */
  onConfirm: () => void;
}

/**
 * Inside a `DialogTrigger`, opened by its button; on its own, controlled with `isOpen` and
 * `onOpenChange`. Cancel and Escape close it without calling `onConfirm`.
 */
export function AlertDialog({
  title,
  children,
  confirmLabel,
  intent = "primary",
  onConfirm,
  ...props
}: AlertDialogProps) {
  const { t } = useStepclimbStrings();
  return (
    <Dialog
      role="alertdialog"
      title={title}
      hideCloseButton
      isDismissable={false}
      size={props.size ?? "sm"}
      {...props}
    >
      {({ close }) => (
        <>
          <DialogBody>{children}</DialogBody>
          <DialogFooter>
            <Button data-slot="alert-dialog-cancel" autoFocus onPress={close}>
              {t("alertDialog.cancel")}
            </Button>
            <Button
              data-slot="alert-dialog-confirm"
              intent={intent}
              onPress={() => {
                onConfirm();
                close();
              }}
            >
              {confirmLabel}
            </Button>
          </DialogFooter>
        </>
      )}
    </Dialog>
  );
}
