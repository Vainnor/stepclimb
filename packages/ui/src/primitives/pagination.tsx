// SPDX-License-Identifier: AGPL-3.0-or-later
// Pagination: Previous and Next through a list the server pages with cursors
// (docs/specs/m0/13-base-ui-components.md M0 inventory, Q9). Cursor pages have no numbers, so it
// shows none; it shows the number of results when the API gives one. A button without a cursor to
// go to is disabled.
import { CaretLeftIcon, CaretRightIcon } from "@phosphor-icons/react";
import { tv } from "../lib/tv";
import { useStepclimbStrings } from "../strings/strings";
import { Icon } from "../theme/icon";
import { Button } from "./button";

export const pagination = tv({
  slots: {
    root: "flex flex-wrap items-center gap-3 font-ui",
    count: "me-auto text-sm text-fg-sub",
    buttons: "ms-auto flex items-center gap-2",
    caret: "rtl:-scale-x-100",
  },
});

const styles = pagination();

export interface PaginationProps {
  /** The cursor of the page before this one; `null` or absent on the first page. */
  previousCursor?: string | null;
  /** The cursor of the page after this one; `null` or absent on the last page. */
  nextCursor?: string | null;
  /** Called with the cursor of the page to load. */
  onPageChange: (cursor: string, direction: "previous" | "next") => void;
  /** The number of results, when the API gives one. */
  count?: number;
  /** Layout only. */
  className?: string;
}

export function Pagination({
  previousCursor,
  nextCursor,
  onPageChange,
  count,
  className,
}: PaginationProps) {
  const { t } = useStepclimbStrings();
  return (
    <nav
      data-slot="pagination"
      aria-label={t("pagination.label")}
      className={styles.root({ className })}
    >
      {count === undefined ? null : (
        <p data-slot="pagination-count" className={styles.count()}>
          {t("pagination.count", { count })}
        </p>
      )}
      <div data-slot="pagination-buttons" className={styles.buttons()}>
        <Button
          data-slot="pagination-previous"
          isDisabled={previousCursor == null}
          onPress={() => {
            if (previousCursor != null) onPageChange(previousCursor, "previous");
          }}
        >
          <Icon icon={CaretLeftIcon} size="sm" className={styles.caret()} />
          {t("pagination.previous")}
        </Button>
        <Button
          data-slot="pagination-next"
          isDisabled={nextCursor == null}
          onPress={() => {
            if (nextCursor != null) onPageChange(nextCursor, "next");
          }}
        >
          {t("pagination.next")}
          <Icon icon={CaretRightIcon} size="sm" className={styles.caret()} />
        </Button>
      </div>
    </nav>
  );
}
