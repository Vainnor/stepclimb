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
import { use, useMemo } from "react";
import {
  DateInput as AriaDateInput,
  DateFieldStateContext,
  type DateInputProps,
  DateSegment,
  TimeFieldStateContext,
} from "react-aria-components";
import { tv } from "../lib/tv";
import { field } from "./field";

const parts = field();

export const dateInput = tv({
  slots: {
    /** The bordered well around the input(s) and the calendar button. */
    group: [
      parts.control(),
      "flex min-h-(--sc-control-h) items-stretch pointer-coarse:min-h-(--sc-control-touch)",
    ],
    // The segments sit in a line of text that may break only at a `break` after a literal, so a
    // narrow field wraps between the date, the time and the zone, never inside one of them.
    input: [
      "min-w-0 flex-1 content-center whitespace-nowrap px-3 py-1.5 tabular-nums",
      "pointer-coarse:py-0",
    ],
    // Every segment but the literals is a target, the zone's name included (React Aria makes it a
    // focusable read-only textbox): at least 24 px square with a fine pointer (WCAG 2.5.8) and
    // 44 px under a coarse one (the plan's touch floor); the field wraps when that does not fit.
    // The fields force leading zeros, so every number segment is two digits or more and the
    // segments sit evenly.
    segment: [
      "inline-flex items-center justify-center rounded-xs px-px align-middle text-fg-strong",
      "outline-none caret-transparent",
      "not-data-[type=literal]:min-h-6",
      "not-data-[type=literal]:min-w-6",
      "pointer-coarse:not-data-[type=literal]:min-h-(--sc-control-touch)",
      "pointer-coarse:not-data-[type=literal]:min-w-(--sc-control-touch)",
      "data-[placeholder]:text-fg-soft data-[type=literal]:text-fg-sub",
      "data-[type=timeZoneName]:ps-1.5 data-[type=timeZoneName]:text-fg-sub",
      "data-[focused]:bg-brand data-[focused]:text-on-brand",
      "data-[disabled]:text-fg-disabled",
    ],
    /**
     * Where the line may wrap: a space that wraps inside the field's no-wrap line, at a font size
     * of zero, so it takes no room and the literal before it looks as it does without it.
     */
    break: "whitespace-normal text-[0px]",
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
      {(segment) => <Segment segment={segment} />}
    </AriaDateInput>
  );
}

/** One segment of a field's value, as React Aria hands it to `DateInput`'s children. */
export type DateSegmentValue = Parameters<DateInputProps["children"]>[0];

const DATE_TYPES = new Set(["era", "year", "month", "day"]);
const TIME_TYPES = new Set(["hour", "minute", "second", "dayPeriod"]);

/**
 * Whether the line may break after the literal at `index`: one with a space in it, between the
 * date and the time or before the zone's name. Segments come in the locale's order, so the groups
 * are found by type; Intl's bidirectional isolation marks are literals too, with no space.
 */
export function breaksAfter(segments: readonly DateSegmentValue[], index: number): boolean {
  const segment = segments[index];
  if (segment?.type !== "literal" || !/\s/.test(segment.text)) return false;
  const next = segments.slice(index + 1).find((s) => s.type !== "literal");
  const previous = segments
    .slice(0, index)
    .reverse()
    .find((s) => s.type !== "literal");
  if (next === undefined || previous === undefined) return false;
  if (next.type === "timeZoneName") return true;
  return (
    (DATE_TYPES.has(previous.type) && TIME_TYPES.has(next.type)) ||
    (TIME_TYPES.has(previous.type) && DATE_TYPES.has(next.type))
  );
}

/** One segment, which reads the field's segments to know whether the line may break after it. */
function Segment({ segment }: { segment: DateSegmentValue }) {
  const state = use(DateFieldStateContext) ?? use(TimeFieldStateContext);
  const segments = state?.segments ?? [];
  const breaks = breaksAfter(segments, segments.indexOf(segment));
  return (
    <>
      <DateSegment data-slot="date-segment" segment={segment} className={styles.segment()} />
      {breaks ? (
        <span data-slot="date-break" aria-hidden="true" className={styles.break()}>
          {" "}
        </span>
      ) : null}
    </>
  );
}

/** `zonedPlaceholder(timeZone)`, made again only when the zone changes. */
export function useZonedPlaceholder(timeZone: string): ZonedDateTime {
  return useMemo(() => zonedPlaceholder(timeZone), [timeZone]);
}

/**
 * Midnight today in `timeZone`: the value a field's segments start from while it is empty, so a
 * value typed into it is a ZonedDateTime in that zone. The date is read from the browser's clock.
 */
export function zonedPlaceholder(timeZone: string): ZonedDateTime {
  const midnight: CalendarDateTime = toCalendarDateTime(today(timeZone));
  return toZoned(midnight, timeZone);
}
