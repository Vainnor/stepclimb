// SPDX-License-Identifier: AGPL-3.0-or-later
// ToggleButton and ToggleButtonGroup: a button that stays pressed, and a set of them with single or
// multiple selection (docs/specs/m0/13-base-ui-components.md M0 inventory). Selected is drawn in
// the brand tint and, in forced-colors mode, with base.css's outline.
import {
  ToggleButton as AriaToggleButton,
  ToggleButtonGroup as AriaToggleButtonGroup,
  type ToggleButtonGroupProps as AriaToggleButtonGroupProps,
  type ToggleButtonProps as AriaToggleButtonProps,
  composeRenderProps,
} from "react-aria-components";
import { tv, type VariantProps } from "../lib/tv";
import { button } from "./button";

export const toggleButton = tv({
  extend: button,
  base: [
    "data-[selected]:border-brand data-[selected]:bg-brand-soft data-[selected]:text-brand-fg",
    "data-[selected]:data-[hovered]:bg-brand-soft",
  ],
  defaultVariants: { intent: "secondary" },
});

export const toggleButtonGroup = tv({
  base: "inline-flex flex-wrap items-center gap-1 data-[orientation=vertical]:flex-col",
});

export interface ToggleButtonProps
  extends AriaToggleButtonProps,
    Pick<VariantProps<typeof button>, "size"> {
  /** Secondary (the default) or ghost; selected looks the same in both. */
  intent?: "secondary" | "ghost";
}

/** Controlled with `isSelected` and `onChange`, uncontrolled with `defaultSelected`. */
export function ToggleButton({ intent, size, className, ...props }: ToggleButtonProps) {
  return (
    <AriaToggleButton
      data-slot="toggle-button"
      {...props}
      className={composeRenderProps(className, (cls) =>
        toggleButton({ intent, size, className: cls }),
      )}
    />
  );
}

export type ToggleButtonGroupProps = AriaToggleButtonGroupProps;

/**
 * Toggle buttons with `selectionMode` single (the default) or multiple; controlled with
 * `selectedKeys`, uncontrolled with `defaultSelectedKeys`. Arrow keys move between them.
 */
export function ToggleButtonGroup({ className, ...props }: ToggleButtonGroupProps) {
  return (
    <AriaToggleButtonGroup
      data-slot="toggle-button-group"
      {...props}
      className={composeRenderProps(className, (cls) => toggleButtonGroup({ className: cls }))}
    />
  );
}
