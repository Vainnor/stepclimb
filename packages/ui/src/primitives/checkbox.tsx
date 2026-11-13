// SPDX-License-Identifier: AGPL-3.0-or-later
// Checkbox and CheckboxGroup (docs/specs/m0/13-base-ui-components.md M0 inventory). A checkbox is
// checked, unchecked or indeterminate (some of what it stands for is checked); each state has its
// own mark, so color is never the only sign. Built on React Aria's CheckboxField and CheckboxButton:
// the label is the press target, and on touch screens it is at least 44 px tall.
import { CheckIcon, MinusIcon } from "@phosphor-icons/react";
import type { ReactNode } from "react";
import {
  CheckboxGroup as AriaCheckboxGroup,
  type CheckboxGroupProps as AriaCheckboxGroupProps,
  CheckboxButton,
  CheckboxField,
  type CheckboxFieldProps,
  composeRenderProps,
} from "react-aria-components";
import { tv } from "../lib/tv";
import { Icon } from "../theme/icon";
import { FieldDescription, FieldError, FieldLabel, type FieldPartsProps, field } from "./field";

export const checkbox = tv({
  slots: {
    root: "flex flex-col gap-1 font-ui",
    button: [
      "group flex w-fit min-h-(--sc-control-h) cursor-default items-center gap-2.5 text-sm text-fg-default",
      "pointer-coarse:min-h-(--sc-control-touch)",
      "data-[disabled]:cursor-not-allowed data-[disabled]:text-fg-disabled",
    ],
    box: [
      "flex size-5 shrink-0 items-center justify-center rounded-xs border border-line-strong bg-sunken",
      "text-on-brand transition-colors duration-fast ease-out-expo",
      "group-data-[hovered]:border-fg-sub",
      "group-data-[selected]:border-brand group-data-[selected]:bg-brand",
      "group-data-[indeterminate]:border-brand group-data-[indeterminate]:bg-brand",
      "group-data-[focus-visible]:ring-2 group-data-[focus-visible]:ring-ring group-data-[focus-visible]:ring-offset-2 ring-offset-canvas",
      "group-data-[invalid]:border-danger",
      "group-data-[disabled]:border-line group-data-[disabled]:bg-sunken group-data-[disabled]:text-fg-disabled",
    ],
    label: "min-w-0",
    description: "ps-7.5",
    group: field().root(),
    groupItems: "flex flex-col",
  },
});

const styles = checkbox();

export interface CheckboxProps
  extends Omit<CheckboxFieldProps, "className" | "children">,
    Pick<FieldPartsProps, "description" | "errorMessage"> {
  /** The visible label. Without one, give `aria-label`. */
  children?: ReactNode;
  /** Layout only. */
  className?: string;
}

/**
 * Controlled with `isSelected` and `onChange`, uncontrolled with `defaultSelected`. Inside a
 * CheckboxGroup it takes a `value` and the group holds the selection. `isIndeterminate` draws the
 * mixed state, which the caller sets and clears.
 */
export function Checkbox({
  children,
  description,
  errorMessage,
  className,
  ...props
}: CheckboxProps) {
  return (
    <CheckboxField data-slot="checkbox" {...props} className={styles.root({ className })}>
      <CheckboxButton data-slot="checkbox-button" className={styles.button()}>
        {composeRenderProps(children, (content, { isSelected, isIndeterminate }) => (
          <>
            <span data-slot="checkbox-box" className={styles.box()}>
              {isIndeterminate ? (
                <Icon icon={MinusIcon} size="sm" />
              ) : isSelected ? (
                <Icon icon={CheckIcon} size="sm" />
              ) : null}
            </span>
            {content === undefined ? null : (
              <span data-slot="checkbox-label" className={styles.label()}>
                {content}
              </span>
            )}
          </>
        ))}
      </CheckboxButton>
      {description === undefined ? null : (
        <FieldDescription className={styles.description()}>{description}</FieldDescription>
      )}
      <FieldError className={styles.description()}>{errorMessage}</FieldError>
    </CheckboxField>
  );
}

export interface CheckboxGroupProps
  extends Omit<AriaCheckboxGroupProps, "className" | "children">,
    FieldPartsProps {
  /** The group's Checkboxes, each with a `value`. */
  children: ReactNode;
  /** Layout only. */
  className?: string;
}

/** Controlled with `value` and `onChange` (the checked values), uncontrolled with `defaultValue`. */
export function CheckboxGroup({
  label,
  description,
  errorMessage,
  children,
  className,
  ...props
}: CheckboxGroupProps) {
  return (
    <AriaCheckboxGroup
      data-slot="checkbox-group"
      {...props}
      className={styles.group({ className })}
    >
      {label === undefined ? null : <FieldLabel>{label}</FieldLabel>}
      <div data-slot="checkbox-group-items" className={styles.groupItems()}>
        {children}
      </div>
      {description === undefined ? null : <FieldDescription>{description}</FieldDescription>}
      <FieldError>{errorMessage}</FieldError>
    </AriaCheckboxGroup>
  );
}
