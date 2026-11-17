// SPDX-License-Identifier: AGPL-3.0-or-later
// What a locked module or item is waiting for, in words (docs/specs/m1/16-learning-ui-components.md
// B8, B9; docs/specs/m1/04-unlock-and-completion-rules.md B8, B9). The server's reasons name
// modules and items by id; the caller resolves them to the titles the learner already sees, so
// the text never names anything the response did not show (spec 04 B14).
import { useCallback, useMemo } from "react";
import { useLocale } from "react-aria-components";
import { useStepclimbStrings } from "../strings/strings";

/** One of spec 04's lock reasons, with its ids resolved to titles. */
export type LockReason =
  /** The module opens at `at`. */
  | { kind: "unlock_at"; at: Date }
  /** The module waits for these modules, in course order. */
  | { kind: "prerequisites"; moduleTitles: readonly [string, ...string[]] }
  /** The item's module is locked. */
  | { kind: "module_locked" }
  /** The item waits for an earlier item of its sequential module. */
  | { kind: "sequential"; itemTitle: string };

/**
 * Formats a lock reason as one sentence. `unlock_at` reads in `timeZone` (the user's, `users.tz`)
 * with the zone's short name, so a learner abroad sees when that is for them.
 */
export function useLockReasonText(timeZone: string): (reason: LockReason) => string {
  const { t } = useStepclimbStrings();
  const { locale } = useLocale();
  const dates = useMemo(
    () =>
      new Intl.DateTimeFormat(locale, {
        year: "numeric",
        month: "short",
        day: "numeric",
        hour: "numeric",
        minute: "2-digit",
        timeZone,
        timeZoneName: "short",
      }),
    [locale, timeZone],
  );
  const lists = useMemo(
    () => new Intl.ListFormat(locale, { style: "long", type: "conjunction" }),
    [locale],
  );
  return useCallback(
    (reason: LockReason) => {
      switch (reason.kind) {
        case "unlock_at":
          return t("lockReason.unlockAt", { when: dates.format(reason.at) });
        case "prerequisites":
          return t("lockReason.prerequisites", { titles: lists.format(reason.moduleTitles) });
        case "module_locked":
          return t("lockReason.moduleLocked");
        case "sequential":
          return t("lockReason.sequential", { title: reason.itemTitle });
      }
    },
    [t, dates, lists],
  );
}
