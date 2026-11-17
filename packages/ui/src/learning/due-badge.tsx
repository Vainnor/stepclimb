// SPDX-License-Identifier: AGPL-3.0-or-later
// DueBadge: when something is due, counted in calendar days in the user's time zone: "Due in 3
// days", "Due today", "3 days overdue" (docs/specs/m1/16-learning-ui-components.md B10). Past the
// due instant it takes the overdue state, with the overdue icon and token; the words say it too,
// so color is never the only signal (B3).
import { fromDate, toCalendarDate } from "@internationalized/date";
import { CalendarBlankIcon } from "@phosphor-icons/react";
import { tv } from "../lib/tv";
import { useStepclimbStrings } from "../strings/strings";
import { Icon } from "../theme/icon";
import { learningStateIcons } from "./status-chip";

/**
 * Whether `due` has passed at `now`, and the calendar days between the two in `timeZone`: days
 * left when it has not, days late when it has. A due time earlier today that has passed is
 * overdue by 0 days.
 */
export function dueDays(
  due: Date,
  now: Date,
  timeZone: string,
): { overdue: boolean; days: number } {
  const dueDay = toCalendarDate(fromDate(due, timeZone));
  const today = toCalendarDate(fromDate(now, timeZone));
  const overdue = now.getTime() > due.getTime();
  return { overdue, days: overdue ? today.compare(dueDay) : dueDay.compare(today) };
}

export const dueBadge = tv({
  slots: {
    root: [
      "inline-flex max-w-full items-center gap-1.5 rounded-pill border border-line bg-raised",
      "px-2.5 py-0.5 font-ui text-caption font-control text-fg-default",
    ],
    icon: "text-fg-sub",
    label: "min-w-0 truncate",
  },
  variants: {
    overdue: { true: { icon: "text-state-overdue" } },
  },
});

export interface DueBadgeProps {
  /** The due instant. */
  due: Date;
  /** The current instant, from the app's clock. */
  now: Date;
  /** The user's IANA time zone (`users.tz`), which decides where a day starts. */
  timeZone: string;
  /** Layout only. */
  className?: string;
}

export function DueBadge({ due, now, timeZone, className }: DueBadgeProps) {
  const { t } = useStepclimbStrings();
  const { overdue, days } = dueDays(due, now, timeZone);
  const styles = dueBadge({ overdue });
  const label = overdue ? t("dueBadge.overdue", { days }) : t("dueBadge.due", { days });
  return (
    <span
      data-slot="due-badge"
      data-overdue={overdue || undefined}
      className={styles.root({ className })}
    >
      <Icon
        icon={overdue ? learningStateIcons.overdue.icon : CalendarBlankIcon}
        size="sm"
        className={styles.icon()}
      />
      <time data-slot="due-badge-label" dateTime={due.toISOString()} className={styles.label()}>
        {label}
      </time>
    </span>
  );
}
