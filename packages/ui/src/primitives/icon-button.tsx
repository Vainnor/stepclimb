// SPDX-License-Identifier: AGPL-3.0-or-later
// IconButton: a button whose only content is an icon. Its accessible label is required, and it is
// shown as a Tooltip on hover and keyboard focus (docs/specs/m0/13-base-ui-components.md M0
// inventory).
import type { Icon as PhosphorIcon } from "@phosphor-icons/react";
import {
  Button as AriaButton,
  type ButtonProps as AriaButtonProps,
  composeRenderProps,
} from "react-aria-components";
import { tv, type VariantProps } from "../lib/tv";
import { Icon, type IconState } from "../theme/icon";
import { button } from "./button";
import { Tooltip, TooltipTrigger } from "./tooltip";

/** Button's recipe, square: as wide as it is tall. */
export const iconButton = tv({
  extend: button,
  base: "aspect-square px-0",
  variants: { size: { sm: "px-0", md: "", lg: "px-0" } },
  defaultVariants: { intent: "ghost" },
});

export interface IconButtonProps
  extends Omit<AriaButtonProps, "children" | "aria-label" | "aria-labelledby">,
    VariantProps<typeof iconButton> {
  /** The Phosphor icon, imported from `@phosphor-icons/react`. */
  icon: PhosphorIcon;
  /** Regular for available (the default), fill for complete. */
  iconState?: IconState;
  /** The accessible name and the tooltip, from `useStepclimbStrings()`. Required. */
  label: string;
}

/** Ghost unless another intent is asked for: icon buttons sit in toolbars and rows. */
export function IconButton({
  icon,
  iconState,
  label,
  intent,
  size,
  className,
  ...props
}: IconButtonProps) {
  return (
    <TooltipTrigger>
      <AriaButton
        data-slot="icon-button"
        {...props}
        aria-label={label}
        className={composeRenderProps(className, (cls) =>
          iconButton({ intent, size, className: cls }),
        )}
      >
        <Icon icon={icon} state={iconState} size={size === "lg" ? "lg" : "md"} />
      </AriaButton>
      <Tooltip data-slot="icon-button-tooltip">{label}</Tooltip>
    </TooltipTrigger>
  );
}
