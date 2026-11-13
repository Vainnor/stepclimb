// SPDX-License-Identifier: AGPL-3.0-or-later
// RadioGroup and Radio: one choice from a few options, all visible at once
// (docs/specs/m0/13-base-ui-components.md M0 inventory). Tab enters the group at the chosen option
// and the arrow keys move the choice. Built on React Aria's RadioGroup, RadioField and RadioButton:
// the label is the press target, at least 44 px tall on touch screens.
import type { ReactNode } from "react";
import {
  RadioGroup as AriaRadioGroup,
  type RadioGroupProps as AriaRadioGroupProps,
  RadioButton,
  RadioField,
  type RadioFieldProps,
} from "react-aria-components";
import { tv } from "../lib/tv";
import { FieldDescription, FieldError, FieldLabel, type FieldPartsProps, field } from "./field";

export const radioGroup = tv({
  slots: {
    root: field().root(),
    items:
      "flex flex-col data-[orientation=horizontal]:flex-row data-[orientation=horizontal]:flex-wrap data-[orientation=horizontal]:gap-x-6",
    radio: "flex flex-col gap-1 font-ui",
    button: [
      "group flex w-fit min-h-(--sc-control-h) cursor-default items-center gap-2.5 text-sm text-fg-default",
      "pointer-coarse:min-h-(--sc-control-touch)",
      "data-[disabled]:cursor-not-allowed data-[disabled]:text-fg-disabled",
    ],
    circle: [
      "flex size-5 shrink-0 items-center justify-center rounded-full border border-line-strong bg-sunken",
      "transition-colors duration-fast ease-out-expo",
      "group-data-[hovered]:border-fg-sub",
      "group-data-[selected]:border-brand",
      "group-data-[focus-visible]:ring-2 group-data-[focus-visible]:ring-ring group-data-[focus-visible]:ring-offset-2 ring-offset-canvas",
      "group-data-[invalid]:border-danger",
      "group-data-[disabled]:border-line",
    ],
    dot: "size-2.5 rounded-full bg-brand group-data-[disabled]:bg-fg-disabled",
    label: "min-w-0",
    description: "ps-7.5",
  },
});

const styles = radioGroup();

export interface RadioGroupProps
  extends Omit<AriaRadioGroupProps, "className" | "children">,
    FieldPartsProps {
  /** The group's Radios, each with a `value`. */
  children: ReactNode;
  /** Layout only. */
  className?: string;
}

/** Controlled with `value` and `onChange`, uncontrolled with `defaultValue`. */
export function RadioGroup({
  label,
  description,
  errorMessage,
  children,
  className,
  orientation = "vertical",
  ...props
}: RadioGroupProps) {
  return (
    <AriaRadioGroup
      data-slot="radio-group"
      orientation={orientation}
      {...props}
      className={styles.root({ className })}
    >
      {label === undefined ? null : <FieldLabel>{label}</FieldLabel>}
      <div data-slot="radio-group-items" data-orientation={orientation} className={styles.items()}>
        {children}
      </div>
      {description === undefined ? null : <FieldDescription>{description}</FieldDescription>}
      <FieldError>{errorMessage}</FieldError>
    </AriaRadioGroup>
  );
}

export interface RadioProps extends Omit<RadioFieldProps, "className" | "children"> {
  /** The visible label. */
  children: ReactNode;
  /** Help under this option. */
  description?: ReactNode;
  /** Layout only. */
  className?: string;
}

/** One option of a RadioGroup. */
export function Radio({ children, description, className, ...props }: RadioProps) {
  return (
    <RadioField data-slot="radio" {...props} className={styles.radio({ className })}>
      <RadioButton data-slot="radio-button" className={styles.button()}>
        {({ isSelected }) => (
          <>
            <span data-slot="radio-circle" className={styles.circle()}>
              {isSelected ? <span data-slot="radio-dot" className={styles.dot()} /> : null}
            </span>
            <span data-slot="radio-label" className={styles.label()}>
              {children}
            </span>
          </>
        )}
      </RadioButton>
      {description === undefined ? null : (
        <FieldDescription className={styles.description()}>{description}</FieldDescription>
      )}
    </RadioField>
  );
}
