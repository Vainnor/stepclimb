// SPDX-License-Identifier: AGPL-3.0-or-later
// The month grid DatePicker and DateRangePicker open, and the overlay it opens in: a Popover under
// the field at 768 px and wider, a bottom sheet under 768 px, as Select does
// (docs/specs/m0/13-base-ui-components.md Q10). Not exported on its own: no screen picks a date
// without a field (docs/specs/m1/16-learning-ui-components.md B5). React Aria's Calendar moves
// with the arrow keys, Page Up and Page Down change the month, and each day is named in full.
import { CaretLeftIcon, CaretRightIcon } from "@phosphor-icons/react";
import type { ReactNode } from "react";
import {
  Calendar as AriaCalendar,
  RangeCalendar as AriaRangeCalendar,
  Button,
  CalendarCell,
  CalendarGrid,
  CalendarGridBody,
  CalendarGridHeader,
  CalendarHeaderCell,
  Dialog,
  Heading,
  Modal,
  ModalOverlay,
} from "react-aria-components";
import { WIDE_QUERY } from "../lib/breakpoints";
import { tv } from "../lib/tv";
import { useMediaQuery } from "../lib/use-media-query";
import { useStepclimbStrings } from "../strings/strings";
import { Icon } from "../theme/icon";
import { useAppearanceAttributes } from "../theme/theme-provider";
import { listBox } from "./list-box";
import { Popover } from "./popover";
import { sheet } from "./sheet";

export const calendar = tv({
  slots: {
    root: "flex w-fit flex-col gap-3 font-ui text-sm text-fg-default",
    header: "flex items-center gap-2",
    nav: [
      "flex size-(--sc-control-h) shrink-0 items-center justify-center rounded-sm text-fg-default outline-none",
      "data-[hovered]:bg-sunken data-[pressed]:bg-sunken data-[disabled]:text-fg-disabled",
      "data-[focus-visible]:ring-2 data-[focus-visible]:ring-ring",
    ],
    navIcon: "rtl:-scale-x-100",
    heading: "min-w-0 flex-1 text-center text-base font-heading text-fg-strong",
    grid: "border-separate border-spacing-0.5",
    headerCell: "pb-1 text-caption font-control text-fg-sub",
    cell: [
      "flex size-(--sc-control-h) cursor-default items-center justify-center rounded-sm tabular-nums outline-none",
      "data-[hovered]:bg-sunken data-[pressed]:bg-sunken",
      // Offset from the cell, so the ring stays visible around a selected day's brand fill.
      "data-[focus-visible]:ring-2 data-[focus-visible]:ring-ring data-[focus-visible]:ring-offset-2 ring-offset-raised",
      "data-[today]:border data-[today]:border-line-strong",
      "data-[outside-month]:invisible",
      "data-[disabled]:text-fg-disabled data-[unavailable]:text-fg-disabled data-[unavailable]:line-through",
      "data-[invalid]:text-danger-fg",
    ],
    dialog: "flex justify-center p-3 outline-none",
    sheetDialog: "flex justify-center p-4 outline-none",
  },
  variants: {
    range: {
      false: {
        cell: "data-[selected]:bg-brand data-[selected]:font-heading data-[selected]:text-on-brand",
      },
      true: {
        cell: [
          "data-[selected]:bg-brand-soft data-[selected]:text-fg-strong",
          "data-[selection-start]:bg-brand data-[selection-start]:font-heading data-[selection-start]:text-on-brand",
          "data-[selection-end]:bg-brand data-[selection-end]:font-heading data-[selection-end]:text-on-brand",
        ],
      },
    },
  },
});

function CalendarParts({ range }: { range: boolean }) {
  const { t } = useStepclimbStrings();
  const styles = calendar({ range });
  return (
    <>
      <header data-slot="calendar-header" className={styles.header()}>
        <Button
          data-slot="calendar-previous"
          slot="previous"
          aria-label={t("calendar.previous")}
          className={styles.nav()}
        >
          <Icon icon={CaretLeftIcon} size="sm" className={styles.navIcon()} />
        </Button>
        <Heading data-slot="calendar-heading" className={styles.heading()} />
        <Button
          data-slot="calendar-next"
          slot="next"
          aria-label={t("calendar.next")}
          className={styles.nav()}
        >
          <Icon icon={CaretRightIcon} size="sm" className={styles.navIcon()} />
        </Button>
      </header>
      <CalendarGrid data-slot="calendar-grid" className={styles.grid()}>
        <CalendarGridHeader>
          {(day) => (
            <CalendarHeaderCell data-slot="calendar-header-cell" className={styles.headerCell()}>
              {day}
            </CalendarHeaderCell>
          )}
        </CalendarGridHeader>
        <CalendarGridBody>
          {(date) => (
            <CalendarCell data-slot="calendar-cell" date={date} className={styles.cell()} />
          )}
        </CalendarGridBody>
      </CalendarGrid>
    </>
  );
}

/** The single-date calendar inside a DatePicker. */
export function PickerCalendar() {
  return (
    <AriaCalendar data-slot="calendar" className={calendar().root()}>
      <CalendarParts range={false} />
    </AriaCalendar>
  );
}

/** The range calendar inside a DateRangePicker. */
export function PickerRangeCalendar() {
  return (
    <AriaRangeCalendar data-slot="calendar" className={calendar().root()}>
      <CalendarParts range />
    </AriaRangeCalendar>
  );
}

/**
 * The calendar's overlay: a Popover at 768 px and wider, a bottom sheet under it. Both take the
 * picker's open state and its dialog's name (the field's label) from React Aria's contexts.
 */
export function PickerOverlay({ children }: { children: ReactNode }) {
  const wide = useMediaQuery(WIDE_QUERY);
  const appearance = useAppearanceAttributes();
  const styles = calendar();
  if (wide) {
    return (
      <Popover data-slot="calendar-popover" placement="bottom start">
        <Dialog data-slot="calendar-dialog" className={styles.dialog()}>
          {children}
        </Dialog>
      </Popover>
    );
  }
  return (
    <ModalOverlay
      data-slot="calendar-sheet-overlay"
      isDismissable
      {...appearance}
      className={listBox().sheetOverlay()}
    >
      <Modal data-slot="calendar-sheet" className={sheet({ placement: "bottom" }).panel()}>
        <Dialog data-slot="calendar-dialog" className={styles.sheetDialog()}>
          {children}
        </Dialog>
      </Modal>
    </ModalOverlay>
  );
}
