// SPDX-License-Identifier: AGPL-3.0-or-later
// The convention example from the plan's UI tab (Component conventions), with one change for
// Tailwind 4: a CSS variable in an arbitrary value is written `h-(--sc-control-h)`, where the
// plan's `h-[--sc-control-h]` is Tailwind 3 syntax and generates no height in Tailwind 4.
import {
  Button as AriaButton,
  type ButtonProps as AriaButtonProps,
  composeRenderProps,
} from "react-aria-components";
import { tv, type VariantProps } from "tailwind-variants";

export const button = tv({
  base: [
    "inline-flex items-center justify-center gap-2 whitespace-nowrap rounded-sm font-medium",
    "h-(--sc-control-h) px-4 text-sm transition-colors duration-fast ease-out-expo outline-none",
    "data-[focus-visible]:ring-2 data-[focus-visible]:ring-ring data-[focus-visible]:ring-offset-2 ring-offset-canvas",
    "data-[disabled]:opacity-50 data-[disabled]:cursor-not-allowed",
  ],
  variants: {
    intent: {
      primary: "bg-brand text-on-brand data-[hovered]:bg-brand-hover",
      secondary: "bg-raised text-fg-strong border border-line-strong data-[hovered]:bg-sunken",
      ghost: "text-fg-default data-[hovered]:bg-sunken",
      danger: "bg-raised text-danger-fg border border-line-strong data-[hovered]:bg-danger-soft",
    },
    size: { sm: "h-(--sc-control-h-sm) px-3 text-[13px]", md: "", lg: "h-12 px-5 text-base" },
  },
  defaultVariants: { intent: "secondary", size: "md" }, // primary is opt-in: one per view
});

export interface ButtonProps extends AriaButtonProps, VariantProps<typeof button> {}

export function Button({ intent, size, className, ...props }: ButtonProps) {
  return (
    <AriaButton
      data-slot="button"
      {...props}
      className={composeRenderProps(className, (cls) => button({ intent, size, className: cls }))}
    />
  );
}
