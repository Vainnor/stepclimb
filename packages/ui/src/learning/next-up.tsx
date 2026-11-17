// SPDX-License-Identifier: AGPL-3.0-or-later
// NextUp: the one thing a learner should do next in a course, with the view's one primary action,
// "Continue" (docs/specs/m1/16-learning-ui-components.md B10; docs/specs/m1/10-learner-dashboard-and-next-up.md
// B2, B10, B11). The server decides what is next, so the panel shows its answer: the next item,
// or, when there is none, when the course opens further or that it is complete. Dates read in the
// user's time zone. The course home shows it under the course header; the dashboard shows it for
// its first course, with the course's title.
import { ArrowRightIcon } from "@phosphor-icons/react";
import { useId, useMemo } from "react";
import { useLocale } from "react-aria-components";
import { tv } from "../lib/tv";
import { ButtonLink } from "../primitives/button-link";
import { Heading, type HeadingLevel } from "../primitives/heading";
import { ProgressBar } from "../primitives/progress-bar";
import { useStepclimbStrings } from "../strings/strings";
import { Icon } from "../theme/icon";
import { StatusChip } from "./status-chip";

/** What the server answered for the course (spec 10 B1, B4). */
export type NextUpStatus =
  /** An item to do: the deep link to it, and whether the learner has started it. */
  | { kind: "next"; itemTitle: string; moduleTitle: string; href: string; started: boolean }
  /** Nothing to do until `until`, the earliest date a locked module opens (`waiting_until`). */
  | { kind: "waiting"; until: Date }
  /** The course is complete (`completed_at`). */
  | { kind: "complete"; completedAt: Date };

export const nextUp = tv({
  slots: {
    root: "flex flex-col gap-4 rounded-md border border-line bg-raised p-5 font-ui",
    row: "flex flex-wrap items-center gap-x-6 gap-y-4",
    summary: "flex min-w-0 flex-[2_1_16rem] flex-col gap-1.5",
    course: "text-sm text-fg-sub break-words",
    module: "text-sm text-fg-sub break-words",
    item: "text-lg font-medium text-fg-strong break-words",
    status: "flex items-center gap-2 text-base text-fg-default",
    progress: "flex-[1_1_10rem]",
    action: "shrink-0",
    arrow: "rtl:-scale-x-100",
  },
});

export interface NextUpProps {
  /** What is next in the course. */
  status: NextUpStatus;
  /** The course's title, where the page does not already say it (the dashboard). */
  courseTitle?: string;
  /** The learner's course progress (`CourseProgress`), drawn as a bar when given. */
  progress?: { completed: number; required: number };
  /** The user's IANA time zone (`users.tz`), in which dates read. */
  timeZone: string;
  /** The heading's place in the page outline; 2 by default. */
  headingLevel?: HeadingLevel;
  /** Layout only. */
  className?: string;
}

/** The date-time and date formats the panel's sentences use, in `timeZone`. */
function useDateFormats(timeZone: string) {
  const { locale } = useLocale();
  return useMemo(
    () => ({
      // An opening has a time of day; say it with the zone's short name, as the lock reasons do.
      opens: new Intl.DateTimeFormat(locale, {
        year: "numeric",
        month: "short",
        day: "numeric",
        hour: "numeric",
        minute: "2-digit",
        timeZone,
        timeZoneName: "short",
      }),
      day: new Intl.DateTimeFormat(locale, { dateStyle: "medium", timeZone }),
    }),
    [locale, timeZone],
  );
}

export function NextUp({
  status,
  courseTitle,
  progress,
  timeZone,
  headingLevel = 2,
  className,
}: NextUpProps) {
  const { t } = useStepclimbStrings();
  const headingId = useId();
  const itemId = useId();
  const formats = useDateFormats(timeZone);
  const styles = nextUp();
  return (
    <section
      data-slot="next-up"
      data-status={status.kind}
      aria-labelledby={headingId}
      className={styles.root({ className })}
    >
      <Heading id={headingId} level={headingLevel} size="base">
        {t("nextUp.heading")}
      </Heading>
      <div data-slot="next-up-row" className={styles.row()}>
        <div data-slot="next-up-summary" className={styles.summary()}>
          {courseTitle === undefined ? null : (
            <p data-slot="next-up-course" className={styles.course()}>
              {courseTitle}
            </p>
          )}
          {status.kind === "next" ? (
            <>
              <p data-slot="next-up-module" className={styles.module()}>
                {status.moduleTitle}
              </p>
              <p id={itemId} data-slot="next-up-item" className={styles.item()}>
                {status.itemTitle}
              </p>
              <div>
                <StatusChip state={status.started ? "progress" : "available"} />
              </div>
            </>
          ) : (
            <p data-slot="next-up-status" className={styles.status()}>
              <StatusChip state={status.kind === "waiting" ? "locked" : "complete"} labelHidden />
              {status.kind === "waiting"
                ? t("nextUp.waiting", { when: formats.opens.format(status.until) })
                : t("nextUp.completed", { when: formats.day.format(status.completedAt) })}
            </p>
          )}
        </div>
        {progress === undefined ? null : (
          <ProgressBar
            label={t("nextUp.progress")}
            value={progress.completed}
            maxValue={progress.required}
            className={styles.progress()}
          />
        )}
        {status.kind === "next" ? (
          <ButtonLink
            intent="primary"
            href={status.href}
            aria-describedby={itemId}
            className={styles.action()}
          >
            {t("nextUp.continue")}
            <Icon icon={ArrowRightIcon} size="sm" className={styles.arrow()} />
          </ButtonLink>
        ) : null}
      </div>
    </section>
  );
}
