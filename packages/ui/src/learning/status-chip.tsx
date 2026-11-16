// SPDX-License-Identifier: AGPL-3.0-or-later
// StatusChip: one learning state as a chip with its icon and label
// (docs/specs/m1/16-learning-ui-components.md B3, B7). Each state has one icon, one weight and one
// token everywhere, and color is never the only signal: the icon's shape and the label tell the
// states apart without it. The label can be visually hidden where the row around it already
// reads clearly, but a screen reader always hears it.
import {
  CheckCircleIcon,
  CircleHalfIcon,
  CircleIcon,
  EyeIcon,
  LockIcon,
  type Icon as PhosphorIcon,
  WarningIcon,
} from "@phosphor-icons/react";
import { tv } from "../lib/tv";
import { VisuallyHidden } from "../primitives/visually-hidden";
import { useStepclimbStrings } from "../strings/strings";
import { Icon, type IconState } from "../theme/icon";

/** The learning states, in the order a learner meets them (plan, UI tab -> Principles). */
export const learningStates = [
  "locked",
  "available",
  "progress",
  "complete",
  "overdue",
  "review",
] as const;
export type LearningState = (typeof learningStates)[number];

/** Each state's icon and the Phosphor weight that draws it (B3). */
export const learningStateIcons: Record<LearningState, { icon: PhosphorIcon; weight: IconState }> =
  {
    locked: { icon: LockIcon, weight: "available" },
    available: { icon: CircleIcon, weight: "available" },
    progress: { icon: CircleHalfIcon, weight: "available" },
    complete: { icon: CheckCircleIcon, weight: "complete" },
    overdue: { icon: WarningIcon, weight: "available" },
    review: { icon: EyeIcon, weight: "available" },
  };

export const statusChip = tv({
  slots: {
    root: [
      "inline-flex max-w-full items-center gap-1.5 rounded-pill border border-line bg-raised",
      "px-2.5 py-0.5 font-ui text-caption font-control text-fg-default",
    ],
    icon: "",
    label: "min-w-0 truncate",
  },
  variants: {
    state: {
      locked: { icon: "text-state-locked" },
      available: { icon: "text-state-available" },
      progress: { icon: "text-state-progress" },
      complete: { icon: "text-state-complete" },
      overdue: { icon: "text-state-overdue" },
      review: { icon: "text-state-review" },
    },
    /** Icon only, with the label for screen readers: a round chip the size of the icon's box. */
    labelHidden: { true: { root: "px-1" } },
  },
});

export interface StatusChipProps {
  /** The learning state the chip shows. */
  state: LearningState;
  /**
   * Hides the label visually where the context already says the state in words; a screen reader
   * still reads it. The icon's shape alone then carries the state for sighted people.
   */
  labelHidden?: boolean;
  /** Layout only. */
  className?: string;
}

export function StatusChip({ state, labelHidden = false, className }: StatusChipProps) {
  const { t } = useStepclimbStrings();
  const styles = statusChip({ state, labelHidden });
  const { icon, weight } = learningStateIcons[state];
  const label = t("statusChip.state", { state });
  return (
    <span data-slot="status-chip" data-state={state} className={styles.root({ className })}>
      <Icon icon={icon} state={weight} size="sm" className={styles.icon()} />
      {labelHidden ? (
        <VisuallyHidden data-slot="status-chip-label">{label}</VisuallyHidden>
      ) : (
        <span data-slot="status-chip-label" className={styles.label()}>
          {label}
        </span>
      )}
    </span>
  );
}
