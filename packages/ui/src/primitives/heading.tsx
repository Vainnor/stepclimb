// SPDX-License-Identifier: AGPL-3.0-or-later
// Heading: a heading whose level is required and separate from its size, so the outline of a page
// stays right whatever size a design asks for (docs/specs/m0/13-base-ui-components.md M0 inventory).
import {
  Heading as AriaHeading,
  type HeadingProps as AriaHeadingProps,
} from "react-aria-components";
import { tv, type VariantProps } from "../lib/tv";

export const heading = tv({
  base: "font-ui font-heading text-fg-strong text-balance",
  variants: {
    size: {
      base: "text-base",
      lg: "text-lg",
      xl: "text-xl",
      "2xl": "text-2xl",
      "3xl": "text-3xl",
      "4xl": "text-4xl",
    },
  },
});

export type HeadingLevel = 1 | 2 | 3 | 4 | 5 | 6;
export type HeadingSize = NonNullable<VariantProps<typeof heading>["size"]>;

/** The size a level gets when none is asked for. */
export const headingSizes: Record<HeadingLevel, HeadingSize> = {
  1: "3xl",
  2: "2xl",
  3: "xl",
  4: "lg",
  5: "base",
  6: "base",
};

export interface HeadingProps extends Omit<AriaHeadingProps, "className" | "level"> {
  /** The heading's place in the page outline (`h1` to `h6`). */
  level: HeadingLevel;
  /** The type scale step; without one, the level's step in `headingSizes`. */
  size?: HeadingSize;
  /** Layout only. */
  className?: string;
}

export function Heading({ level, size, className, ...props }: HeadingProps) {
  return (
    <AriaHeading
      data-slot="heading"
      level={level}
      {...props}
      className={heading({ size: size ?? headingSizes[level], className })}
    />
  );
}
