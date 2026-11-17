// SPDX-License-Identifier: AGPL-3.0-or-later
// Announcement: a message staff posted to a course or an account, with its title, author, date
// and body (docs/specs/m1/16-learning-ui-components.md B11; docs/specs/m1/11-announcements-and-in-app-notifications.md
// B1, B4). The body is the HTML the server rendered and sanitized, drawn as Prose; the date reads
// in the user's time zone.
import { useId, useMemo } from "react";
import { useLocale } from "react-aria-components";
import type { ServerHtml } from "../lib/server-html";
import { tv } from "../lib/tv";
import { Heading, type HeadingLevel } from "../primitives/heading";
import { Prose } from "../primitives/prose";
import { useStepclimbStrings } from "../strings/strings";

export const announcement = tv({
  slots: {
    root: "flex flex-col gap-3",
    header: "flex flex-col gap-1",
    byline: "font-ui text-sm text-fg-sub",
  },
});

export interface AnnouncementProps {
  /** The announcement's title. */
  title: string;
  /** The author's display name. */
  author: string;
  /** When it was published. */
  publishedAt: Date;
  /** The user's IANA time zone (`users.tz`), in which the date reads. */
  timeZone: string;
  /** The body as the server's sanitized HTML (`Announcement.html`), inserted as given. */
  html: ServerHtml;
  /** The title's place in the page outline; 2 by default. */
  headingLevel?: HeadingLevel;
  /** Layout only. */
  className?: string;
}

export function Announcement({
  title,
  author,
  publishedAt,
  timeZone,
  html,
  headingLevel = 2,
  className,
}: AnnouncementProps) {
  const { t } = useStepclimbStrings();
  const { locale } = useLocale();
  const titleId = useId();
  const date = useMemo(
    () => new Intl.DateTimeFormat(locale, { dateStyle: "medium", timeZone }).format(publishedAt),
    [locale, timeZone, publishedAt],
  );
  const styles = announcement();
  return (
    <article
      data-slot="announcement"
      aria-labelledby={titleId}
      className={styles.root({ className })}
    >
      <header data-slot="announcement-header" className={styles.header()}>
        <Heading id={titleId} level={headingLevel} size="lg">
          {title}
        </Heading>
        <p data-slot="announcement-byline" className={styles.byline()}>
          {t("announcement.byline", { author, date })}
        </p>
      </header>
      <Prose html={html} />
    </article>
  );
}
