// SPDX-License-Identifier: AGPL-3.0-or-later
// AssignmentList: the caller's assignments, each a link to what was assigned with its learning
// state and, until it is complete, when it is due, overdue first
// (docs/specs/m1/16-learning-ui-components.md B12; docs/specs/m1/13-assignments-due-dates-reminders-and-escalation.md
// B7, B18). The server decides each row's status; the list maps it to a learning state and keeps
// the server's order rule, so a list in any order reads the same. Each link is named by the
// target's title, as ItemRow's are; what was assigned, its course, its state and its due label
// are the link's description, and the drawn ones are hidden from screen readers so they are not
// read twice.
import { useId } from "react";
import { Link as AriaLink } from "react-aria-components";
import { tv } from "../lib/tv";
import { useUnitList } from "../lib/unit-list";
import { EmptyState } from "../patterns/empty-state";
import { VisuallyHidden } from "../primitives/visually-hidden";
import { useStepclimbStrings } from "../strings/strings";
import { DueBadge, useDueLabel } from "./due-badge";
import { type LearningState, StatusChip } from "./status-chip";

/** The API's assignment row status (spec 13 B7). */
export const assignmentStatuses = ["not_started", "in_progress", "completed", "overdue"] as const;
export type AssignmentStatus = (typeof assignmentStatuses)[number];

/** The learning state each status is drawn as (spec 16 B3). */
export const assignmentStatusStates: Record<AssignmentStatus, LearningState> = {
  not_started: "available",
  in_progress: "progress",
  completed: "complete",
  overdue: "overdue",
};

/** What an assignment targets (spec 13 B1). */
export type AssignmentTargetType = "course" | "module" | "item";

/** One of the caller's assignment rows (`GET /api/v1/me/assignments`, spec 13 B18). */
export interface AssignmentListEntry {
  /** Unique in the list. */
  id: string;
  target: { type: AssignmentTargetType; title: string };
  /** The target's course title, for a module or item target. */
  courseTitle?: string;
  /** The target's path in the app. */
  href: string;
  /** The caller's due date; null when the assignment has none. */
  due: Date | null;
  status: AssignmentStatus;
}

/**
 * The server's order (spec 13 B18): overdue first, then by due date, then rows without one. The
 * sort is stable, so rows the rule ties keep the caller's order.
 */
export function sortAssignments(
  entries: readonly AssignmentListEntry[],
): readonly AssignmentListEntry[] {
  const rank = (entry: AssignmentListEntry) =>
    entry.status === "overdue" ? 0 : entry.due === null ? 2 : 1;
  return [...entries].sort((a, b) => {
    const byRank = rank(a) - rank(b);
    if (byRank !== 0 || a.due === null || b.due === null) return byRank;
    return a.due.getTime() - b.due.getTime();
  });
}

export const assignmentList = tv({
  slots: {
    root: "flex flex-col gap-1 font-ui",
    row: [
      "relative flex flex-wrap items-center gap-x-4 gap-y-1 rounded-sm ps-2 pe-3 text-sm",
      "transition-colors duration-fast ease-out-expo",
      "has-[[data-hovered]]:bg-sunken has-[[data-pressed]]:bg-sunken",
      "has-[[data-focus-visible]]:ring-2 has-[[data-focus-visible]]:ring-inset has-[[data-focus-visible]]:ring-ring",
    ],
    link: [
      "flex min-h-(--sc-control-h) min-w-0 flex-[1_1_14rem] cursor-pointer flex-col justify-center py-1.5",
      // The row draws the focus ring, so the link drops base.css's own.
      "pointer-coarse:min-h-(--sc-control-touch) text-fg-default outline-none shadow-none",
      // The whole row is the target: the pseudo-element covers it, over the description.
      "after:absolute after:inset-0 after:rounded-sm after:content-['']",
    ],
    title: "min-w-0 break-words font-medium text-fg-strong",
    context: "min-w-0 break-words text-fg-sub",
    meta: "flex flex-wrap items-center gap-x-3 gap-y-1 pb-1.5 sm:py-1.5",
  },
});

export interface AssignmentListProps {
  /** The caller's assignment rows, in any order. */
  assignments: readonly AssignmentListEntry[];
  /** The user's IANA time zone (`users.tz`), for the due labels. */
  timeZone: string;
  /** The current instant, from the app's clock, for the due labels. */
  now: Date;
  /** Layout only. */
  className?: string;
}

export function AssignmentList({ assignments, timeZone, now, className }: AssignmentListProps) {
  const { t } = useStepclimbStrings();
  const styles = assignmentList();
  if (assignments.length === 0) {
    return <EmptyState className={className}>{t("assignmentList.empty")}</EmptyState>;
  }
  return (
    <ul data-slot="assignment-list" className={styles.root({ className })}>
      {sortAssignments(assignments).map((entry) => (
        <AssignmentRow key={entry.id} entry={entry} timeZone={timeZone} now={now} />
      ))}
    </ul>
  );
}

interface RowProps {
  entry: AssignmentListEntry;
  timeZone: string;
  now: Date;
}

function AssignmentRow({ entry, timeZone, now }: RowProps) {
  const descriptionId = useId();
  const description = useRowDescription(entry, timeZone, now);
  const state = assignmentStatusStates[entry.status];
  const styles = assignmentList();
  return (
    <li
      data-slot="assignment-list-row"
      data-status={entry.status}
      data-target={entry.target.type}
      className={styles.row()}
    >
      <AriaLink
        data-slot="assignment-list-link"
        href={entry.href}
        aria-describedby={descriptionId}
        className={styles.link()}
      >
        <span data-slot="assignment-list-title" className={styles.title()}>
          {entry.target.title}
        </span>
        {entry.courseTitle === undefined ? null : (
          // Inside the link so it shares the row's target; hidden here, as the description says it.
          <span data-slot="assignment-list-course" aria-hidden="true" className={styles.context()}>
            {entry.courseTitle}
          </span>
        )}
      </AriaLink>
      <VisuallyHidden id={descriptionId} data-slot="assignment-list-description">
        {description}
      </VisuallyHidden>
      <span data-slot="assignment-list-meta" aria-hidden="true" className={styles.meta()}>
        <StatusChip state={state} />
        {showsDue(entry) && entry.due !== null ? (
          <DueBadge due={entry.due} now={now} timeZone={timeZone} />
        ) : null}
      </span>
    </li>
  );
}

/** A completed row drops its due label: how long it is to or past the date no longer matters. */
function showsDue(entry: AssignmentListEntry): boolean {
  return entry.due !== null && entry.status !== "completed";
}

/** What was assigned, its course, its state and its due label, as one list. */
function useRowDescription(entry: AssignmentListEntry, timeZone: string, now: Date): string {
  const { t } = useStepclimbStrings();
  const list = useUnitList();
  // Called for every row, so the hooks run in the same order; only a row with a due date uses it.
  const due = useDueLabel(entry.due ?? now, now, timeZone);
  return list([
    t("assignmentList.target", { type: entry.target.type }),
    ...(entry.courseTitle === undefined ? [] : [entry.courseTitle]),
    t("statusChip.state", { state: assignmentStatusStates[entry.status] }),
    ...(showsDue(entry) ? [due.label] : []),
  ]);
}
