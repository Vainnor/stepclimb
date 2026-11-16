// SPDX-License-Identifier: AGPL-3.0-or-later
// DateRangePicker: a labeled start and end, each a date and time with an explicit time zone, typed
// into segments or picked as a range from a month calendar
// (docs/specs/m1/16-learning-ui-components.md B5, Q2). Like DatePicker, a value entered into the
// empty field takes `timeZone`, the user's own, and each end shows its zone's short name.
import type { ZonedDateTime } from "@internationalized/date";
import { CalendarBlankIcon } from "@phosphor-icons/react";
import {
  DateRangePicker as AriaDateRangePicker,
  type DateRangePickerProps as AriaDateRangePickerProps,
  Button,
  Group,
} from "react-aria-components";
import { tv } from "../lib/tv";
import { useStepclimbStrings } from "../strings/strings";
import { Icon } from "../theme/icon";
import { PickerOverlay, PickerRangeCalendar } from "./calendar";
import { DateInput, dateInput, zonedPlaceholder } from "./date-input";
import { FieldParts, type FieldPartsProps, field } from "./field";

const parts = field();
const input = dateInput();

export const dateRangePicker = tv({
  slots: {
    root: parts.root(),
    group: input.group(),
    inputs: "flex min-w-0 flex-1 flex-wrap items-center",
    // Each end takes its own width and wraps within itself only when it alone is too wide, so a
    // narrow field puts the end under the start instead of splitting both.
    input: "flex-initial",
    separator: "px-1 text-fg-sub",
    button: input.button(),
  },
});

export interface DateRangePickerProps
  extends Omit<
      AriaDateRangePickerProps<ZonedDateTime>,
      "className" | "children" | "style" | "hideTimeZone"
    >,
    FieldPartsProps {
  /**
   * The IANA zone a value entered into the empty field is in: the user's time zone
   * (`users.tz`). A value the field is given keeps its own zones.
   */
  timeZone: string;
  /** Layout only. */
  className?: string;
}

/**
 * Controlled with `value` and `onChange` (`{ start, end }`), uncontrolled with `defaultValue`.
 * The start and end wrap onto two lines when the field is narrow.
 */
export function DateRangePicker({
  label,
  description,
  errorMessage,
  timeZone,
  placeholderValue,
  className,
  ...props
}: DateRangePickerProps) {
  const { t } = useStepclimbStrings();
  const styles = dateRangePicker();
  return (
    <AriaDateRangePicker
      data-slot="date-range-picker"
      shouldForceLeadingZeros
      placeholderValue={placeholderValue ?? zonedPlaceholder(timeZone)}
      {...props}
      className={styles.root({ className })}
    >
      <FieldParts label={label} description={description} errorMessage={errorMessage}>
        <Group data-slot="date-range-picker-group" className={styles.group()}>
          <div data-slot="date-range-picker-inputs" className={styles.inputs()}>
            <DateInput slot="start" className={styles.input()} />
            <span
              aria-hidden
              data-slot="date-range-picker-separator"
              className={styles.separator()}
            >
              –
            </span>
            <DateInput slot="end" className={styles.input()} />
          </div>
          <Button
            data-slot="date-range-picker-button"
            aria-label={t("datePicker.openCalendar")}
            className={styles.button()}
          >
            <Icon icon={CalendarBlankIcon} size="sm" />
          </Button>
        </Group>
      </FieldParts>
      <PickerOverlay>
        <PickerRangeCalendar />
      </PickerOverlay>
    </AriaDateRangePicker>
  );
}
