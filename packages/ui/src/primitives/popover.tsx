// SPDX-License-Identifier: AGPL-3.0-or-later
// Popover: a floating panel anchored to its trigger, with the one soft shadow of a floating layer
// (docs/specs/m0/13-base-ui-components.md M0 inventory; plan UI tab -> Tokens: Elevation). Menu
// draws its list in one; `PopoverTrigger` and `PopoverDialog` hold other content.
import {
  Popover as AriaPopover,
  type PopoverProps as AriaPopoverProps,
  Dialog,
  type DialogProps,
  DialogTrigger,
  type DialogTriggerProps,
} from "react-aria-components";
import { tv } from "../lib/tv";
import { useAppearanceAttributes } from "../theme/theme-provider";

export const popover = tv({
  slots: {
    root: [
      "min-w-(--trigger-width) max-w-[calc(100vw-1rem)] overflow-auto rounded-md",
      "border border-line bg-raised text-fg-default shadow-float outline-none font-ui",
    ],
    dialog: "p-4 outline-none",
  },
});

export interface PopoverProps extends Omit<AriaPopoverProps, "className"> {
  /** Layout only. */
  className?: string;
  "data-slot"?: string;
}

/**
 * The floating panel. It repeats the surrounding ThemeProvider's theme, density and brand, since
 * React Aria portals it to the end of `body`.
 */
export function Popover({ className, offset = 8, "data-slot": slot, ...props }: PopoverProps) {
  const appearance = useAppearanceAttributes();
  return (
    <AriaPopover
      data-slot={slot ?? "popover"}
      offset={offset}
      {...appearance}
      {...props}
      className={popover().root({ className })}
    />
  );
}

/** Opens its Popover when its first child, a button, is pressed. React Aria's `DialogTrigger`. */
export function PopoverTrigger(props: DialogTriggerProps) {
  return <DialogTrigger {...props} />;
}

export interface PopoverDialogProps extends Omit<DialogProps, "className"> {
  /** Layout only. */
  className?: string;
}

/**
 * The content of a Popover that is not a menu or a list: a dialog, named by a `Heading` with
 * `slot="title"` inside it or by `aria-label`.
 */
export function PopoverDialog({ className, ...props }: PopoverDialogProps) {
  return (
    <Dialog data-slot="popover-dialog" {...props} className={popover().dialog({ className })} />
  );
}
