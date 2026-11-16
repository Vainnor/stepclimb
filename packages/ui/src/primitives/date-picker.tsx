// SPDX-License-Identifier: AGPL-3.0-or-later
// DatePicker: a labeled date and time, such as a due date or an unlock date, typed into segments or
// picked from a month calendar (docs/specs/m1/16-learning-ui-components.md B5, Q2). Its value is an
// instant with an explicit time zone (a ZonedDateTime), shown with the zone's short name; a value
// entered into the empty field takes `timeZone`, the user's own (`users.tz`). React Aria works out
// the offset, including across a daylight-saving change.
import type { ZonedDateTime } from "@internationalized/date";
import { CalendarBlankIcon } from "@phosphor-icons/react";
import {
  DatePicker as AriaDatePicker,
  type DatePickerProps as AriaDatePickerProps,
  Button,
  Group,
} from "react-aria-components";
import { tv } from "../lib/tv";
import { useStepclimbStrings } from "../strings/strings";
import { Icon } from "../theme/icon";
import { PickerCalendar, PickerOverlay } from "./calendar";
import { DateInput, dateInput, zonedPlaceholder } from "./date-input";
import { FieldParts, type FieldPartsProps, field } from "./field";

const parts = field();
const input = dateInput();

export const datePicker = tv({
  slots: { root: parts.root(), group: input.group(), button: input.button() },
});

export interface DatePickerProps
  extends Omit<
      AriaDatePickerProps<ZonedDateTime>,
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

/**
 * Controlled with `value` and `onChange`, uncontrolled with `defaultValue`. Down to the minute by
 * default; `granularity="day"` keeps the time out of view (the value is still an instant).
 */
export function DatePicker({
  label,
  description,
  errorMessage,
  timeZone,
  placeholderValue,
  className,
  ...props
}: DatePickerProps) {
  const { t } = useStepclimbStrings();
  const styles = datePicker();
  return (
    <AriaDatePicker
      data-slot="date-picker"
      shouldForceLeadingZeros
      placeholderValue={placeholderValue ?? zonedPlaceholder(timeZone)}
      {...props}
      className={styles.root({ className })}
    >
      <FieldParts label={label} description={description} errorMessage={errorMessage}>
        <Group data-slot="date-picker-group" className={styles.group()}>
          <DateInput />
          <Button
            data-slot="date-picker-button"
            aria-label={t("datePicker.openCalendar")}
            className={styles.button()}
          >
            <Icon icon={CalendarBlankIcon} size="sm" />
          </Button>
        </Group>
      </FieldParts>
      <PickerOverlay>
        <PickerCalendar />
      </PickerOverlay>
    </AriaDatePicker>
  );
}
