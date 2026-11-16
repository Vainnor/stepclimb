// SPDX-License-Identifier: AGPL-3.0-or-later
// The segmented input DatePicker, DateRangePicker and TimeField share: one editable segment per
// part of the date and time, typed or stepped with the arrow keys, in the locale's order, and the
// value's time zone as its short name after the time ("EDT")
// (docs/specs/m1/16-learning-ui-components.md B5). Not exported on its own.
import {
  type CalendarDateTime,
  toCalendarDateTime,
  today,
  toZoned,
  type ZonedDateTime,
} from "@internationalized/date";
import {
  DateInput as AriaDateInput,
  type DateInputProps,
  DateSegment,
} from "react-aria-components";
import { tv } from "../lib/tv";
import { field } from "./field";

const parts = field();

export const dateInput = tv({
  slots: {
    /** The bordered well around the input(s) and the calendar button. */
    group: [parts.control(), "flex min-h-(--sc-control-h) items-stretch"],
    input: "flex min-w-0 flex-1 flex-wrap items-center px-3 py-1.5 tabular-nums",
    // An editable segment is at least 24 px square (WCAG 2.5.8). The fields force leading zeros,
    // so every number segment is two digits or more and the segments sit evenly.
    segment: [
      "inline-flex min-h-6 min-w-6 items-center justify-center rounded-xs px-px text-fg-strong",
      "outline-none caret-transparent",
      "data-[type=literal]:min-w-0 data-[type=timeZoneName]:min-w-0",
      "data-[placeholder]:text-fg-soft data-[type=literal]:text-fg-sub",
      "data-[type=timeZoneName]:ps-1.5 data-[type=timeZoneName]:text-fg-sub",
      "data-[focused]:bg-brand data-[focused]:text-on-brand",
      "data-[disabled]:text-fg-disabled",
    ],
    /** The calendar button at the group's inline end. */
    button: [
      "flex w-(--sc-control-h) shrink-0 items-center justify-center border-s border-line text-fg-default",
      "outline-none transition-colors duration-fast ease-out-expo",
      "data-[hovered]:bg-raised data-[pressed]:bg-raised data-[disabled]:text-fg-disabled",
      "data-[focus-visible]:ring-2 data-[focus-visible]:ring-inset data-[focus-visible]:ring-ring",
    ],
  },
});

const styles = dateInput();

/**
 * One date and time input; `slot` is `start` or `end` inside a DateRangePicker. `className` adds
 * the bordered well where the input is the whole control (TimeField).
 */
export function DateInput({
  className,
  ...props
}: Omit<DateInputProps, "children" | "className"> & { className?: string }) {
  return (
    <AriaDateInput data-slot="date-input" {...props} className={styles.input({ className })}>
      {(segment) => (
        <DateSegment data-slot="date-segment" segment={segment} className={styles.segment()} />
      )}
    </AriaDateInput>
  );
}

/**
 * Midnight today in `timeZone`: the value a field's segments start from while it is empty, so a
 * value typed into it is a ZonedDateTime in that zone. The date is read from the browser's clock.
 */
export function zonedPlaceholder(timeZone: string): ZonedDateTime {
  const midnight: CalendarDateTime = toCalendarDateTime(today(timeZone));
  return toZoned(midnight, timeZone);
}
