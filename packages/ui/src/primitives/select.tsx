// SPDX-License-Identifier: AGPL-3.0-or-later
// Select: a labeled field that picks one value from a short list
// (docs/specs/m0/13-base-ui-components.md M0 inventory). Its options open in a Popover at 768 px
// and wider and in a bottom sheet under it (Q10). Arrow keys move through the options, typing jumps
// to one, Escape closes the list and returns focus to the field.
import { CaretDownIcon } from "@phosphor-icons/react";
import type { ReactNode } from "react";
import {
  Select as AriaSelect,
  type SelectProps as AriaSelectProps,
  Button,
  SelectValue,
} from "react-aria-components";
import { tv } from "../lib/tv";
import { useStepclimbStrings } from "../strings/strings";
import { Icon } from "../theme/icon";
import { FieldParts, type FieldPartsProps, field } from "./field";
import { ListBoxItem, type ListBoxItemProps, ListBoxOverlay } from "./list-box";

const parts = field();

export const select = tv({
  slots: {
    root: parts.root(),
    trigger: [
      parts.control(),
      "flex h-(--sc-control-h) items-center gap-2 px-3 text-start cursor-default",
      "data-[pressed]:border-brand",
    ],
    // SelectValue draws the chosen option's own parts, its label and its check slot. The check
    // slot is a flex box, which would take a line of its own under the label and push the text
    // up, so the value lays them out in one row, keeps only the label and cuts it short.
    value: [
      "flex min-w-0 flex-1 items-center truncate data-[placeholder]:text-fg-sub",
      "[&_[data-slot=list-box-item-check]]:hidden [&_[data-slot=list-box-item-label]]:truncate",
    ],
    caret: "shrink-0 text-fg-sub",
  },
});

export interface SelectProps<T extends object>
  extends Omit<AriaSelectProps<T>, "className" | "children" | "selectionMode">,
    FieldPartsProps {
  /** The options: `SelectItem`s, each with an `id`. */
  children: ReactNode;
  /** Layout only. */
  className?: string;
}

/**
 * Controlled with `value` and `onChange` (the selected item's id), uncontrolled with
 * `defaultValue`. Without a value it shows a placeholder from the strings catalog.
 */
export function Select<T extends object>({
  label,
  description,
  errorMessage,
  placeholder,
  className,
  children,
  ...props
}: SelectProps<T>) {
  const { t } = useStepclimbStrings();
  const styles = select();
  return (
    <AriaSelect
      data-slot="select"
      placeholder={placeholder ?? t("select.placeholder")}
      {...props}
      className={styles.root({ className })}
    >
      <FieldParts label={label} description={description} errorMessage={errorMessage}>
        <Button data-slot="select-trigger" className={styles.trigger()}>
          <SelectValue data-slot="select-value" className={styles.value()} />
          <Icon icon={CaretDownIcon} size="sm" className={styles.caret()} />
        </Button>
      </FieldParts>
      <ListBoxOverlay>{children}</ListBoxOverlay>
    </AriaSelect>
  );
}

/** One option of a Select. */
export function SelectItem<T extends object>(props: ListBoxItemProps<T>) {
  return <ListBoxItem {...props} />;
}
