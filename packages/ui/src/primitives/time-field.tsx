// SPDX-License-Identifier: AGPL-3.0-or-later
// TimeField: a labeled time of day with an explicit time zone, typed into segments, with the zone's
// short name after it (docs/specs/m1/16-learning-ui-components.md B5, Q2). Its value is a
// ZonedDateTime, so the time names an instant; a value entered into the empty field is today in
// `timeZone`, the user's own.
import type { ZonedDateTime } from "@internationalized/date";
import {
  TimeField as AriaTimeField,
  type TimeFieldProps as AriaTimeFieldProps,
} from "react-aria-components";
import { tv } from "../lib/tv";
import { DateInput, useZonedPlaceholder } from "./date-input";
import { FieldParts, type FieldPartsProps, field } from "./field";

const parts = field();

export const timeField = tv({
  slots: {
    root: parts.root(),
    group: [parts.control(), "min-h-(--sc-control-h) pointer-coarse:min-h-(--sc-control-touch)"],
  },
});

export interface TimeFieldProps
  extends Omit<
      AriaTimeFieldProps<ZonedDateTime>,
      "className" | "children" | "style" | "hideTimeZone"
    >,
    FieldPartsProps {
  /**
   * The IANA zone a value entered into the empty field is in: the user's time zone
   * (`users.tz`). A value the field is given keeps its own zone.
   */
  timeZone: string;
  /** Layout only. */
  className?: string;
}

/** Controlled with `value` and `onChange`, uncontrolled with `defaultValue`. */
export function TimeField({
  label,
  description,
  errorMessage,
  timeZone,
  placeholderValue,
  className,
  ...props
}: TimeFieldProps) {
  const styles = timeField();
  const fallbackPlaceholder = useZonedPlaceholder(timeZone);
  return (
    <AriaTimeField
      data-slot="time-field"
      shouldForceLeadingZeros
      placeholderValue={placeholderValue ?? fallbackPlaceholder}
      {...props}
      className={styles.root({ className })}
    >
      <FieldParts label={label} description={description} errorMessage={errorMessage}>
        <DateInput className={styles.group()} />
      </FieldParts>
    </AriaTimeField>
  );
}
