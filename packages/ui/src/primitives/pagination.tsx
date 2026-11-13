// SPDX-License-Identifier: AGPL-3.0-or-later
// Pagination: Previous and Next through a list the server pages with cursors
// (docs/specs/m0/13-base-ui-components.md M0 inventory, Q9). Cursor pages have no numbers, so it
// shows none; it shows the number of results when the API gives one. A button without a cursor to
// go to is disabled. When the page a person asked for arrives, a live region says so, and if the
// button they pressed is now disabled (the last or first page), focus moves to the other one
// instead of falling to the top of the document.
import { CaretLeftIcon, CaretRightIcon } from "@phosphor-icons/react";
import { useEffect, useRef, useState } from "react";
import { tv } from "../lib/tv";
import { useStepclimbStrings } from "../strings/strings";
import { Icon } from "../theme/icon";
import { Button } from "./button";
import { VisuallyHidden } from "./visually-hidden";

export const pagination = tv({
  slots: {
    root: "flex flex-wrap items-center gap-3 font-ui",
    count: "me-auto text-sm text-fg-sub",
    buttons: "ms-auto flex items-center gap-2",
    caret: "rtl:-scale-x-100",
  },
});

const styles = pagination();

type Direction = "previous" | "next";

export interface PaginationProps {
  /** The cursor of the page before this one; `null` or absent on the first page. */
  previousCursor?: string | null;
  /** The cursor of the page after this one; `null` or absent on the last page. */
  nextCursor?: string | null;
  /** Called with the cursor of the page to load. */
  onPageChange: (cursor: string, direction: Direction) => void;
  /** The number of results, when the API gives one. */
  count?: number;
  /** Layout only. */
  className?: string;
}

/**
 * After a press, waits for the cursors to change (the new page has arrived), then announces it
 * and, when the pressed button is now disabled, focuses the other one.
 */
function usePageArrival(previousCursor: string | null, nextCursor: string | null) {
  const pressed = useRef<Direction | null>(null);
  const navRef = useRef<HTMLElement>(null);
  const [arrived, setArrived] = useState<Direction | null>(null);
  useEffect(() => {
    const direction = pressed.current;
    if (direction === null) return;
    pressed.current = null;
    setArrived(direction);
    const other = direction === "next" ? "previous" : "next";
    const ownCursor = direction === "next" ? nextCursor : previousCursor;
    const otherCursor = direction === "next" ? previousCursor : nextCursor;
    const button = (which: Direction) =>
      navRef.current?.querySelector<HTMLButtonElement>(`[data-slot="pagination-${which}"]`);
    const focusLost =
      document.activeElement === button(direction) || document.activeElement === document.body;
    if (ownCursor === null && otherCursor !== null && focusLost) button(other)?.focus();
  }, [previousCursor, nextCursor]);
  return { pressed, navRef, arrived };
}

export function Pagination({
  previousCursor = null,
  nextCursor = null,
  onPageChange,
  count,
  className,
}: PaginationProps) {
  const { t } = useStepclimbStrings();
  const { pressed, navRef, arrived } = usePageArrival(previousCursor, nextCursor);
  const go = (cursor: string | null, direction: Direction) => {
    if (cursor === null) return;
    pressed.current = direction;
    onPageChange(cursor, direction);
  };
  return (
    <nav
      ref={navRef}
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
          isDisabled={previousCursor === null}
          onPress={() => go(previousCursor, "previous")}
        >
          <Icon icon={CaretLeftIcon} size="sm" className={styles.caret()} />
          {t("pagination.previous")}
        </Button>
        <Button
          data-slot="pagination-next"
          isDisabled={nextCursor === null}
          onPress={() => go(nextCursor, "next")}
        >
          {t("pagination.next")}
          <Icon icon={CaretRightIcon} size="sm" className={styles.caret()} />
        </Button>
      </div>
      <VisuallyHidden data-slot="pagination-status" role="status">
        {arrived === null ? "" : t("pagination.arrived", { direction: arrived })}
      </VisuallyHidden>
    </nav>
  );
}
