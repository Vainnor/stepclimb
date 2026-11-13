// SPDX-License-Identifier: AGPL-3.0-or-later
// ProgressBar: how far a task has got, with its label and its value as a percentage from the
// strings catalog (docs/specs/m0/13-base-ui-components.md M0 inventory). Determinate only: a wait
// with no known length is a pending Button or a Skeleton.
import type { ReactNode } from "react";
import {
  ProgressBar as AriaProgressBar,
  type ProgressBarProps as AriaProgressBarProps,
  Label,
} from "react-aria-components";
import { tv } from "../lib/tv";
import { useStepclimbStrings } from "../strings/strings";

/** Shared with Meter. The fill keeps its color in forced-colors mode, which drops backgrounds. */
export const progressBar = tv({
  slots: {
    root: "flex w-full flex-col gap-1.5 font-ui",
    header: "flex items-baseline justify-between gap-3 text-sm",
    label: "font-medium text-fg-strong",
    value: "text-fg-sub tabular-nums",
    track: "h-2 w-full overflow-hidden rounded-pill border border-transparent bg-sunken",
    fill: "h-full rounded-pill forced-colors:bg-[Highlight] forced-colors:forced-color-adjust-none",
  },
  variants: {
    tone: { brand: { fill: "bg-brand" }, neutral: { fill: "bg-fg-sub" } },
  },
  defaultVariants: { tone: "brand" },
});

export interface ProgressBarProps
  extends Omit<AriaProgressBarProps, "className" | "children" | "isIndeterminate"> {
  /** What is progressing. Without it, give `aria-label`. */
  label?: ReactNode;
  /** Layout only. */
  className?: string;
}

/** The fraction done, from 0 to 1, for the `{value, number, percent}` message. */
export function fractionOf(value: number, min: number, max: number): number {
  if (max <= min) return 0;
  return Math.min(1, Math.max(0, (value - min) / (max - min)));
}

export function ProgressBar({ label, className, valueLabel, ...props }: ProgressBarProps) {
  const { t } = useStepclimbStrings();
  const { value = 0, minValue = 0, maxValue = 100 } = props;
  const fraction = fractionOf(value, minValue, maxValue);
  const styles = progressBar();
  return (
    <AriaProgressBar
      data-slot="progress-bar"
      {...props}
      valueLabel={valueLabel ?? t("progressBar.value", { value: fraction })}
      className={styles.root({ className })}
    >
      {({ valueText }) => (
        <>
          <div data-slot="progress-bar-header" className={styles.header()}>
            {label === undefined ? null : (
              <Label data-slot="progress-bar-label" className={styles.label()}>
                {label}
              </Label>
            )}
            <span data-slot="progress-bar-value" className={styles.value()}>
              {valueText}
            </span>
          </div>
          <div data-slot="progress-bar-track" className={styles.track()}>
            <div
              data-slot="progress-bar-fill"
              className={styles.fill()}
              style={{ width: `${fraction * 100}%` }}
            />
          </div>
        </>
      )}
    </AriaProgressBar>
  );
}
