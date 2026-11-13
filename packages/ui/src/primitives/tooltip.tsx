// SPDX-License-Identifier: AGPL-3.0-or-later
// Tooltip: a short label shown on hover and keyboard focus. It is never the only place
// information lives (docs/specs/m0/13-base-ui-components.md M0 inventory): touch screens never
// show it, so what it says must also be the trigger's accessible name or visible text.
import {
  Tooltip as AriaTooltip,
  type TooltipProps as AriaTooltipProps,
  TooltipTrigger as AriaTooltipTrigger,
  type TooltipTriggerComponentProps,
} from "react-aria-components";
import { tv } from "../lib/tv";
import { useAppearanceAttributes } from "../theme/theme-provider";

/** Drawn in the strong text color on the canvas color, so it reads in every theme. The border
 * keeps its outline in forced-colors mode, which drops backgrounds. */
export const tooltip = tv({
  base: [
    "max-w-64 rounded-xs border border-fg-strong bg-fg-strong px-2 py-1",
    "font-ui text-caption text-canvas text-balance",
  ],
});

/** Shows its Tooltip when its first child is hovered or focused. React Aria's `TooltipTrigger`. */
export function TooltipTrigger(props: TooltipTriggerComponentProps) {
  return <AriaTooltipTrigger {...props} />;
}

export interface TooltipProps extends Omit<AriaTooltipProps, "className"> {
  /** Layout only. */
  className?: string;
}

/** The label, portaled to the end of `body` in the surrounding ThemeProvider's appearance. */
export function Tooltip({ className, offset = 6, ...props }: TooltipProps) {
  const appearance = useAppearanceAttributes();
  return (
    <AriaTooltip
      data-slot="tooltip"
      offset={offset}
      {...appearance}
      {...props}
      className={tooltip({ className })}
    />
  );
}
