// SPDX-License-Identifier: AGPL-3.0-or-later
// CourseHeader: the top of the course home, with the course's cover, title and code, the
// learner's progress bar and, for staff, the course's status (docs/specs/m1/16-learning-ui-components.md
// B9, B16). The cover is the signed `cover_url` the server issues (docs/specs/m1/06-files.md B19);
// it is decorative, since the title beside it names the course, and a cover that fails to load
// (an expired URL) is left out rather than drawn broken.
import { useState } from "react";
import { tv } from "../lib/tv";
import { Heading, type HeadingLevel } from "../primitives/heading";
import { ProgressBar } from "../primitives/progress-bar";
import { useStepclimbStrings } from "../strings/strings";

/** The API's `CourseStatus`. */
export type CourseStatus = "draft" | "published" | "archived";

export const courseHeader = tv({
  slots: {
    root: "flex flex-wrap items-center gap-x-8 gap-y-4 font-ui",
    identity: "flex min-w-0 flex-[2_1_20rem] items-center gap-4",
    cover:
      "aspect-video w-24 shrink-0 rounded-md border border-line bg-sunken object-cover sm:w-40",
    summary: "flex min-w-0 flex-col gap-1.5",
    details: "flex flex-wrap items-center gap-x-3 gap-y-1 text-sm text-fg-sub",
    code: "break-words",
    status: [
      "inline-flex items-center rounded-pill border border-line-strong bg-raised px-2.5 py-0.5",
      "text-caption font-control text-fg-default",
    ],
    progress: "flex-[1_1_14rem]",
  },
});

export interface CourseHeaderProps {
  /** The course's title. */
  title: string;
  /** The course's code. */
  code: string;
  /** The course's `cover_url`; none draws no cover. */
  coverUrl?: string | null;
  /** The learner's course progress (`CourseProgress`), drawn as a bar; none for staff. */
  progress?: { completed: number; required: number };
  /** The course's status, for staff only: give it where `Course.permissions` holds `course.view_all`. */
  status?: CourseStatus;
  /** The title's place in the page outline; 1 by default. */
  headingLevel?: HeadingLevel;
  /** Layout only. */
  className?: string;
}

export function CourseHeader({
  title,
  code,
  coverUrl,
  progress,
  status,
  headingLevel = 1,
  className,
}: CourseHeaderProps) {
  const { t } = useStepclimbStrings();
  const styles = courseHeader();
  return (
    <header data-slot="course-header" className={styles.root({ className })}>
      <div data-slot="course-header-identity" className={styles.identity()}>
        <Cover url={coverUrl ?? null} className={styles.cover()} />
        <div data-slot="course-header-summary" className={styles.summary()}>
          <Heading level={headingLevel} size="2xl">
            {title}
          </Heading>
          <p data-slot="course-header-details" className={styles.details()}>
            <span data-slot="course-header-code" className={styles.code()}>
              {code}
            </span>
            {status === undefined ? null : (
              <span
                data-slot="course-header-status"
                data-status={status}
                className={styles.status()}
              >
                {t("courseHeader.status", { status })}
              </span>
            )}
          </p>
        </div>
      </div>
      {progress === undefined ? null : (
        <ProgressBar
          label={t("courseHeader.progress")}
          value={progress.completed}
          maxValue={progress.required}
          className={styles.progress()}
        />
      )}
    </header>
  );
}

/** The cover image, or nothing when there is none or it failed to load. */
function Cover({ url, className }: { url: string | null; className: string }) {
  const [failed, setFailed] = useState<string | null>(null);
  if (url === null || failed === url) return null;
  return (
    <img
      data-slot="course-header-cover"
      src={url}
      alt=""
      onError={() => setFailed(url)}
      className={className}
    />
  );
}
