// SPDX-License-Identifier: AGPL-3.0-or-later
// Kbd: a keyboard shortcut, in the mono face (docs/specs/m0/13-base-ui-components.md M0 inventory).
import type { ComponentProps } from "react";
import { Keyboard as AriaKeyboard } from "react-aria-components";
import { tv } from "../lib/tv";

export const kbd = tv({
  base: [
    "inline-flex items-center rounded-xs border border-line-strong bg-sunken px-1.5",
    "font-mono text-xs text-fg-strong whitespace-nowrap",
  ],
});

export interface KbdProps extends Omit<ComponentProps<typeof AriaKeyboard>, "className"> {
  /** Layout only. */
  className?: string;
}

/** A `kbd` element; React Aria sets `dir="ltr"`, so a shortcut reads the same in every direction. */
export function Kbd({ className, ...props }: KbdProps) {
  return <AriaKeyboard data-slot="kbd" {...props} className={kbd({ className })} />;
}
