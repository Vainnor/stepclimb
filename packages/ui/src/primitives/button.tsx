// SPDX-License-Identifier: AGPL-3.0-or-later
// The convention example from the plan's UI tab (Component conventions), with one change for
// Tailwind 4: a CSS variable in an arbitrary value is written `h-(--sc-control-h)`, where the
// plan's `h-[--sc-control-h]` is Tailwind 3 syntax and generates no height in Tailwind 4.
// `isPending` draws the loading state: once the wait has lasted SKELETON_DELAY_MS (300 ms), so a
// fast request never flashes it, the label turns invisible, still the button's accessible name and
// width, under a spinner named from the strings catalog.
import { CircleNotchIcon } from "@phosphor-icons/react";
import { type ReactNode, useEffect, useState } from "react";
import {
  Button as AriaButton,
  type ButtonProps as AriaButtonProps,
  composeRenderProps,
  ProgressBar,
} from "react-aria-components";
import { tv, type VariantProps } from "../lib/tv";
import { useStepclimbStrings } from "../strings/strings";
import { Icon } from "../theme/icon";
import { SKELETON_DELAY_MS } from "./skeleton";

export const button = tv({
  base: [
    "relative inline-flex items-center justify-center gap-2 whitespace-nowrap rounded-sm font-medium",
    "h-(--sc-control-h) px-4 text-sm transition-colors duration-fast ease-out-expo outline-none",
    "data-[focus-visible]:ring-2 data-[focus-visible]:ring-ring data-[focus-visible]:ring-offset-2 ring-offset-canvas",
    "data-[disabled]:opacity-50 data-[disabled]:cursor-not-allowed data-[pending]:cursor-progress",
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

/**
 * A pending button's content. The spinner is in the tree from the start, so React Aria can name
 * the button by it at once; it and the hidden label only show after the delay.
 */
function Pending({ children }: { children: ReactNode }) {
  const { t } = useStepclimbStrings();
  const [shown, setShown] = useState(false);
  useEffect(() => {
    const timer = setTimeout(() => setShown(true), SKELETON_DELAY_MS);
    return () => clearTimeout(timer);
  }, []);
  return (
    <>
      <span
        data-slot="button-label"
        className={
          shown ? "inline-flex items-center gap-2 opacity-0" : "inline-flex items-center gap-2"
        }
      >
        {children}
      </span>
      <ProgressBar
        data-slot="button-pending"
        data-shown={shown || undefined}
        isIndeterminate
        aria-label={t("button.pending")}
        className={
          shown
            ? "absolute inset-0 flex items-center justify-center"
            : "absolute inset-0 flex items-center justify-center opacity-0"
        }
      >
        <Icon icon={CircleNotchIcon} size="md" className="animate-spin" />
      </ProgressBar>
    </>
  );
}

export function Button({ intent, size, className, children, ...props }: ButtonProps) {
  return (
    <AriaButton
      data-slot="button"
      {...props}
      className={composeRenderProps(className, (cls) => button({ intent, size, className: cls }))}
    >
      {composeRenderProps(children, (content, { isPending }) =>
        isPending ? <Pending>{content}</Pending> : content,
      )}
    </AriaButton>
  );
}
