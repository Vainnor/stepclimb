// SPDX-License-Identifier: AGPL-3.0-or-later
// ModuleRow: one module on the course home, with its number, title, learning state and progress
// ("3 / 5"), or, while it is locked, what it waits for in words ("Unlocks after Ground control"),
// then its items as ItemRows (docs/specs/m1/16-learning-ui-components.md B9, B16). The lock
// reasons come from the server with their module ids resolved by the caller to the titles the
// learner sees (docs/specs/m1/04-unlock-and-completion-rules.md B12, B14). The module is a heading,
// not a control or a landmark; its items are the links.
import type { ReactNode } from "react";
import { tv } from "../lib/tv";
import { Heading, type HeadingLevel } from "../primitives/heading";
import { VisuallyHidden } from "../primitives/visually-hidden";
import { useStepclimbStrings } from "../strings/strings";
import { ItemRow, type ItemRowItem } from "./item-row";
import { type LockReason, useLockReasonList, useLockReasonText } from "./lock-reason";
import { type LearningState, StatusChip } from "./status-chip";

export const moduleRow = tv({
  slots: {
    root: "flex flex-col gap-1 font-ui",
    header: "flex flex-wrap items-center gap-x-4 gap-y-2 ps-2 pe-3",
    heading: "flex min-w-0 flex-[1_1_14rem] items-baseline gap-3",
    number: "shrink-0 text-fg-sub tabular-nums",
    title: "min-w-0 break-words",
    progress: "text-sm text-fg-sub tabular-nums",
    reasons: "ps-2 pe-3 text-sm text-fg-sub break-words",
    items: "flex flex-col",
    empty: "ps-2 pe-3 py-2 text-sm text-fg-sub",
  },
});

export type ModuleRowProps = {
  /** The module's place in the course, from 1. */
  number: number;
  /** The module's title. */
  title: string;
  /** The module's items, in module order. */
  items: readonly ItemRowItem[];
  /** The learner's progress in the module (`ModuleProgress`); none for staff. */
  progress?: { completed: number; required: number };
  /** The user's IANA time zone (`users.tz`), for opening dates and due labels. */
  timeZone: string;
  /** The current instant, from the app's clock, for due labels. */
  now: Date;
  /** What shows when the module has no items. */
  emptyState?: ReactNode;
  /**
   * The module's place in the page outline; 2 by default. Header items take the next level, so
   * the module's is at most 5.
   */
  headingLevel?: Exclude<HeadingLevel, 6>;
  /** Layout only. */
  className?: string;
} & (
  | {
      state: "locked";
      /** What the module waits for, with module ids resolved to their titles. */
      lockReasons: readonly [LockReason, ...LockReason[]];
    }
  | { state: Exclude<LearningState, "locked">; lockReasons?: never }
);

export function ModuleRow(props: ModuleRowProps) {
  const { number, title, items, progress, timeZone, now, emptyState, className } = props;
  const headingLevel = props.headingLevel ?? 2;
  const { t } = useStepclimbStrings();
  const styles = moduleRow();
  return (
    <div data-slot="module-row" data-state={props.state} className={styles.root({ className })}>
      <div data-slot="module-row-header" className={styles.header()}>
        <Heading level={headingLevel} size="lg" className={styles.heading()}>
          <span data-slot="module-row-number" className={styles.number()}>
            {t("moduleRow.number", { number })}
          </span>{" "}
          <span data-slot="module-row-title" className={styles.title()}>
            {title}
          </span>
        </Heading>
        <StatusChip state={props.state} />
        {progress === undefined || props.state === "locked" ? null : (
          <ModuleProgress {...progress} className={styles.progress()} />
        )}
      </div>
      {props.state === "locked" ? (
        <LockReasons reasons={props.lockReasons} timeZone={timeZone} className={styles.reasons()} />
      ) : null}
      {items.length === 0 ? (
        emptyState === undefined ? null : (
          <div data-slot="module-row-empty" className={styles.empty()}>
            {emptyState}
          </div>
        )
      ) : (
        <ol data-slot="module-row-items" className={styles.items()}>
          {items.map((item) => (
            <ItemRow
              key={item.id}
              item={item}
              timeZone={timeZone}
              now={now}
              headingLevel={(headingLevel + 1) as HeadingLevel}
            />
          ))}
        </ol>
      )}
    </div>
  );
}

/** "3 / 5" on screen; "3 of 5 complete" to a screen reader. */
function ModuleProgress({
  completed,
  required,
  className,
}: {
  completed: number;
  required: number;
  className: string;
}) {
  const { t } = useStepclimbStrings();
  return (
    <span data-slot="module-row-progress" className={className}>
      <span aria-hidden="true">{t("moduleRow.progress", { completed, required })}</span>
      <VisuallyHidden>{t("moduleRow.progressLabel", { completed, required })}</VisuallyHidden>
    </span>
  );
}

/** The reasons as one line, joined as the locale lists units, as ProgressRail describes a node. */
function LockReasons({
  reasons,
  timeZone,
  className,
}: {
  reasons: readonly LockReason[];
  timeZone: string;
  className: string;
}) {
  const reasonText = useLockReasonText(timeZone);
  const reasonList = useLockReasonList();
  return (
    <p data-slot="module-row-reasons" className={className}>
      {reasonList(reasons.map(reasonText))}
    </p>
  );
}
