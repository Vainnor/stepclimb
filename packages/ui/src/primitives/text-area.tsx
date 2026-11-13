// SPDX-License-Identifier: AGPL-3.0-or-later
// TextArea: a labeled multi-line text input with its description and error, resizable in the block
// direction (docs/specs/m0/13-base-ui-components.md M0 inventory).
import {
  TextArea as AriaTextArea,
  TextField as AriaTextField,
  type TextFieldProps as AriaTextFieldProps,
} from "react-aria-components";
import { tv } from "../lib/tv";
import { FieldParts, type FieldPartsProps, field } from "./field";

const parts = field();

export const textArea = tv({
  slots: {
    root: parts.root(),
    input: [parts.control(), "min-h-24 resize-y px-3 py-2 leading-normal placeholder:text-fg-soft"],
  },
});

export interface TextAreaProps
  extends Omit<AriaTextFieldProps, "className" | "children">,
    FieldPartsProps {
  /** Shown while the input is empty. Never instead of a label. */
  placeholder?: string;
  /** The visible number of lines before it scrolls. */
  rows?: number;
  /** Layout only. */
  className?: string;
}

/** Controlled with `value` and `onChange`, uncontrolled with `defaultValue`. */
export function TextArea({
  label,
  description,
  errorMessage,
  placeholder,
  rows = 4,
  className,
  ...props
}: TextAreaProps) {
  const styles = textArea();
  return (
    <AriaTextField data-slot="text-area" {...props} className={styles.root({ className })}>
      <FieldParts label={label} description={description} errorMessage={errorMessage}>
        <AriaTextArea
          data-slot="text-area-input"
          placeholder={placeholder}
          rows={rows}
          className={styles.input()}
        />
      </FieldParts>
    </AriaTextField>
  );
}
