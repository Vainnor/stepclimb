// SPDX-License-Identifier: AGPL-3.0-or-later
// TextField: a labeled one-line text input with its description and error
// (docs/specs/m0/13-base-ui-components.md M0 inventory). On touch screens the input is 16 px, so
// iOS does not zoom when it is focused (base.css).
import {
  TextField as AriaTextField,
  type TextFieldProps as AriaTextFieldProps,
  Input,
} from "react-aria-components";
import { tv } from "../lib/tv";
import { FieldParts, type FieldPartsProps, field } from "./field";

const parts = field();

export const textField = tv({
  slots: { root: parts.root(), input: [parts.control(), parts.input()] },
});

export interface TextFieldProps
  extends Omit<AriaTextFieldProps, "className" | "children">,
    FieldPartsProps {
  /** Shown while the input is empty. Never instead of a label. */
  placeholder?: string;
  /** Layout only. */
  className?: string;
}

/** Controlled with `value` and `onChange`, uncontrolled with `defaultValue`. */
export function TextField({
  label,
  description,
  errorMessage,
  placeholder,
  className,
  ...props
}: TextFieldProps) {
  const styles = textField();
  return (
    <AriaTextField data-slot="text-field" {...props} className={styles.root({ className })}>
      <FieldParts label={label} description={description} errorMessage={errorMessage}>
        <Input data-slot="text-field-input" placeholder={placeholder} className={styles.input()} />
      </FieldParts>
    </AriaTextField>
  );
}
