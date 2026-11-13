// SPDX-License-Identifier: AGPL-3.0-or-later
// Icon: a Phosphor icon (the one icon set, plan UI tab -> Library architecture) at a token size,
// with the weight that encodes state: regular = available, fill = complete
// (docs/specs/m0/13-base-ui-components.md B10).
import type { Icon as PhosphorIcon } from "@phosphor-icons/react";
import { tv, type VariantProps } from "../lib/tv";

export const icon = tv({
  base: "inline-block shrink-0",
  variants: {
    size: {
      /** The surrounding text's size. */
      inherit: "size-[1em]",
      sm: "size-4",
      md: "size-5",
      lg: "size-6",
    },
  },
  defaultVariants: { size: "inherit" },
});

/** The state an icon shows, and the Phosphor weight that draws it. */
export const iconWeights = { available: "regular", complete: "fill" } as const;
export type IconState = keyof typeof iconWeights;

export interface IconProps extends VariantProps<typeof icon> {
  /** The Phosphor icon, imported from `@phosphor-icons/react` (`CheckCircleIcon`). */
  icon: PhosphorIcon;
  /** Regular for available (the default), fill for complete. */
  state?: IconState;
  /**
   * What the icon means, from `useStepclimbStrings()`, for an icon that carries meaning on its own.
   * Without one the icon is decorative and hidden from assistive technology.
   */
  label?: string;
  /** Layout only. */
  className?: string;
}

export function Icon({ icon: Glyph, state = "available", label, size, className }: IconProps) {
  const a11y = label === undefined ? { "aria-hidden": true } : { role: "img", "aria-label": label };
  return (
    <Glyph
      data-slot="icon"
      weight={iconWeights[state]}
      focusable="false"
      className={icon({ size, className })}
      {...a11y}
    />
  );
}
