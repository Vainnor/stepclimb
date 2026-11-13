// SPDX-License-Identifier: AGPL-3.0-or-later
// NumberField: a labeled number input that React Aria parses and formats in the locale, with
// tabular numerals and buttons to step down and up (docs/specs/m0/13-base-ui-components.md M0
// inventory). The arrow keys step too; the buttons are not tab stops.
import { MinusIcon, PlusIcon } from "@phosphor-icons/react";
import {
  NumberField as AriaNumberField,
  type NumberFieldProps as AriaNumberFieldProps,
  Button,
  Group,
  Input,
} from "react-aria-components";
import { tv } from "../lib/tv";
import { useStepclimbStrings } from "../strings/strings";
import { Icon } from "../theme/icon";
import { FieldParts, type FieldPartsProps, field } from "./field";

const parts = field();

export const numberField = tv({
  slots: {
    root: parts.root(),
    group: [parts.control(), "flex min-h-(--sc-control-h) items-stretch overflow-hidden"],
    input: "min-w-0 flex-1 bg-transparent px-3 tabular-nums outline-none placeholder:text-fg-soft",
    stepper: [
      "flex w-(--sc-control-h) shrink-0 items-center justify-center border-s border-line text-fg-default",
      "outline-none transition-colors duration-fast ease-out-expo",
      "data-[hovered]:bg-raised data-[pressed]:bg-raised data-[disabled]:text-fg-disabled",
    ],
  },
});

export interface NumberFieldProps
  extends Omit<AriaNumberFieldProps, "className" | "children">,
    FieldPartsProps {
  /** Shown while the input is empty. Never instead of a label. */
  placeholder?: string;
  /** Layout only. */
  className?: string;
}

/**
 * Controlled with `value` and `onChange`, uncontrolled with `defaultValue`. `formatOptions` are
 * `Intl.NumberFormat` options (a percent, a unit, a number of decimals).
 */
export function NumberField({
  label,
  description,
  errorMessage,
  placeholder,
  className,
  ...props
}: NumberFieldProps) {
  const { t } = useStepclimbStrings();
  const styles = numberField();
  return (
    <AriaNumberField data-slot="number-field" {...props} className={styles.root({ className })}>
      <FieldParts label={label} description={description} errorMessage={errorMessage}>
        <Group data-slot="number-field-group" className={styles.group()}>
          <Input
            data-slot="number-field-input"
            placeholder={placeholder}
            className={styles.input()}
          />
          <Button
            data-slot="number-field-decrement"
            slot="decrement"
            aria-label={t("numberField.decrement")}
            className={styles.stepper()}
          >
            <Icon icon={MinusIcon} size="sm" />
          </Button>
          <Button
            data-slot="number-field-increment"
            slot="increment"
            aria-label={t("numberField.increment")}
            className={styles.stepper()}
          >
            <Icon icon={PlusIcon} size="sm" />
          </Button>
        </Group>
      </FieldParts>
    </AriaNumberField>
  );
}
