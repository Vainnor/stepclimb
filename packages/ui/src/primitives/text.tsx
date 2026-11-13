// SPDX-License-Identifier: AGPL-3.0-or-later
// Text: running text at a step of the type scale, in one of the text tokens' strengths
// (docs/specs/m0/13-base-ui-components.md M0 inventory; plan UI tab -> Tokens).
import { Text as AriaText, type TextProps as AriaTextProps } from "react-aria-components";
import { tv, type VariantProps } from "../lib/tv";

export const text = tv({
  base: "font-ui",
  variants: {
    size: {
      xs: "text-xs",
      caption: "text-caption",
      sm: "text-sm",
      base: "text-base",
      lg: "text-lg",
    },
    /** The text tokens: strong for emphasis, sub for secondary text, soft for placeholders. */
    tone: {
      strong: "text-fg-strong",
      default: "text-fg-default",
      sub: "text-fg-sub",
      soft: "text-fg-soft",
    },
  },
  defaultVariants: { size: "base", tone: "default" },
});

export interface TextProps extends Omit<AriaTextProps, "className">, VariantProps<typeof text> {
  /** Layout only. */
  className?: string;
}

/** A `span` unless `elementType` names another element (`p`, `div`). */
export function Text({ size, tone, className, ...props }: TextProps) {
  return <AriaText data-slot="text" {...props} className={text({ size, tone, className })} />;
}
