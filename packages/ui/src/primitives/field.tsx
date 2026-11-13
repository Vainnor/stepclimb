// SPDX-License-Identifier: AGPL-3.0-or-later
// Field: the label, description and error every form field shares, and the look of its input
// (docs/specs/m0/13-base-ui-components.md M0 inventory). TextField, TextArea, NumberField and
// SearchField are built from these parts; React Aria wires the label, description and error to
// the input.
import { WarningCircleIcon } from "@phosphor-icons/react";
import type { ReactNode } from "react";
import {
  FieldError as AriaFieldError,
  type FieldErrorProps as AriaFieldErrorProps,
  Label as AriaLabel,
  type LabelProps as AriaLabelProps,
  Text as AriaText,
  type TextProps as AriaTextProps,
  composeRenderProps,
  type ValidationResult,
} from "react-aria-components";
import { tv } from "../lib/tv";
import { useStepclimbStrings } from "../strings/strings";
import { Icon } from "../theme/icon";

export const field = tv({
  slots: {
    root: "flex flex-col gap-1.5 font-ui",
    label: "w-fit text-sm font-medium text-fg-strong",
    description: "text-caption text-fg-sub",
    error: "flex items-start gap-1.5 text-caption text-danger-fg",
    errorIcon: "mt-px size-[1.1em]",
    /** The bordered well an input sits in: the input itself, or a group around it. */
    control: [
      "w-full min-w-0 rounded-sm border border-line-strong bg-sunken text-sm text-fg-strong",
      "transition-colors duration-fast ease-out-expo outline-none",
      "data-[hovered]:border-fg-sub",
      "data-[focused]:border-brand data-[focus-within]:border-brand",
      "data-[focus-visible]:ring-2 data-[focus-visible]:ring-ring data-[focus-visible]:ring-offset-2 ring-offset-canvas",
      "data-[invalid]:border-danger",
      "data-[disabled]:cursor-not-allowed data-[disabled]:border-line data-[disabled]:text-fg-disabled",
    ],
    /** A one-line input. */
    input: "h-(--sc-control-h) px-3 placeholder:text-fg-soft",
  },
});

const styles = field();

/** An error message: text, or a function of React Aria's validation result. */
export type FieldErrorMessage = ReactNode | ((validation: ValidationResult) => ReactNode);

/** What every field takes besides its input's own props. */
export interface FieldPartsProps {
  /** The visible label. Without one, give `aria-label`. */
  label?: ReactNode;
  /** Help under the input. */
  description?: ReactNode;
  /**
   * Shown when the field is invalid: what is wrong and how to fix it ("Enter a number from 1 to
   * 20"). Without one, `validate`'s message shows, or a general one from the strings catalog.
   */
  errorMessage?: FieldErrorMessage;
}

export interface FieldLabelProps extends Omit<AriaLabelProps, "className"> {
  /** Layout only. */
  className?: string;
}

export function FieldLabel({ className, ...props }: FieldLabelProps) {
  return <AriaLabel data-slot="field-label" {...props} className={styles.label({ className })} />;
}

export interface FieldDescriptionProps extends Omit<AriaTextProps, "className" | "slot"> {
  /** Layout only. */
  className?: string;
}

export function FieldDescription({ className, ...props }: FieldDescriptionProps) {
  return (
    <AriaText
      data-slot="field-description"
      slot="description"
      {...props}
      className={styles.description({ className })}
    />
  );
}

export interface FieldErrorProps extends Omit<AriaFieldErrorProps, "className"> {
  /** Layout only. */
  className?: string;
}

/**
 * The message for a field that is invalid with no message of its own. A `validate` function's
 * text is the caller's; the browser's text for a native constraint (`isRequired`, `type="email"`)
 * is generic and in the browser's language, so it is replaced from the strings catalog.
 */
function FallbackMessage({ validation }: { validation: ValidationResult }) {
  const { t } = useStepclimbStrings();
  // `isInvalid` alone also reports a custom error, with no text.
  const custom = validation.validationErrors.join(" ");
  if (validation.validationDetails.customError && custom !== "") return custom;
  return validation.validationDetails.valueMissing ? t("field.valueMissing") : t("field.invalid");
}

/**
 * Renders only while the field is invalid, after a warning icon so color is not the only sign.
 * Without text of its own it shows a `validate` function's message, or a message from the strings
 * catalog for a native constraint, never the browser's.
 */
export function FieldError({ className, children, ...props }: FieldErrorProps) {
  return (
    <AriaFieldError data-slot="field-error" {...props} className={styles.error({ className })}>
      {composeRenderProps(children, (content, validation) => (
        <>
          <Icon icon={WarningCircleIcon} className={styles.errorIcon()} />
          <span>{content ?? <FallbackMessage validation={validation} />}</span>
        </>
      ))}
    </AriaFieldError>
  );
}

/** The label, the input, then the description and the error, as every field lays them out. */
export function FieldParts({
  label,
  description,
  errorMessage,
  children,
}: FieldPartsProps & { children: ReactNode }) {
  return (
    <>
      {label === undefined ? null : <FieldLabel>{label}</FieldLabel>}
      {children}
      {description === undefined ? null : <FieldDescription>{description}</FieldDescription>}
      <FieldError>{errorMessage}</FieldError>
    </>
  );
}
