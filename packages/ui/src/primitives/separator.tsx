// SPDX-License-Identifier: AGPL-3.0-or-later
// Separator: a horizontal or vertical rule in `line` or `line-soft`
// (docs/specs/m0/13-base-ui-components.md M0 inventory).
import {
  Separator as AriaSeparator,
  type SeparatorProps as AriaSeparatorProps,
} from "react-aria-components";
import { tv, type VariantProps } from "../lib/tv";

export const separator = tv({
  base: "shrink-0 border-0",
  variants: {
    orientation: {
      horizontal: "h-px w-full",
      vertical: "w-px self-stretch",
    },
    /** `line` for dividers, `line-soft` for row separators. */
    tone: {
      default: "bg-line",
      soft: "bg-line-soft",
    },
  },
  defaultVariants: { orientation: "horizontal", tone: "default" },
});

export interface SeparatorProps
  extends Omit<AriaSeparatorProps, "className">,
    Omit<VariantProps<typeof separator>, "orientation"> {
  /** Layout only. */
  className?: string;
}

/** An `hr` when horizontal; a `div` with `role="separator"` when vertical. */
export function Separator({
  tone,
  className,
  orientation = "horizontal",
  ...props
}: SeparatorProps) {
  return (
    <AriaSeparator
      data-slot="separator"
      orientation={orientation}
      {...props}
      className={separator({ orientation, tone, className })}
    />
  );
}
