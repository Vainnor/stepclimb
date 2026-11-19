// SPDX-License-Identifier: AGPL-3.0-or-later
// ColorField: a labeled hex color, such as an organization's brand color, typed into an input
// with a swatch of the color beside it (docs/specs/m1/16-learning-ui-components.md B6; the
// branding screen). Built on React Aria's ColorField. The input takes six hex
// digits with or without "#"; React Aria commits the color on blur or Enter and puts back the
// last color when what was typed is not one.
import { useContext } from "react";
import {
  ColorField as AriaColorField,
  type ColorFieldProps as AriaColorFieldProps,
  ColorFieldStateContext,
  Group,
  Input,
} from "react-aria-components";
import { tv } from "../lib/tv";
import { FieldParts, type FieldPartsProps, field } from "./field";

const parts = field();

export const colorField = tv({
  slots: {
    root: parts.root(),
    group: [parts.control(), "flex min-h-(--sc-control-h) items-center gap-2 ps-2"],
    // The border keeps the swatch's edge visible whatever its color. Forced-colors mode keeps the
    // color: it is the value, not decoration.
    swatch: "size-6 shrink-0 rounded-xs border border-line-strong forced-color-adjust-none",
    input:
      "min-w-0 flex-1 self-stretch bg-transparent pe-3 tabular-nums outline-none placeholder:text-fg-soft",
  },
});

export interface ColorFieldProps
  extends Omit<AriaColorFieldProps, "className" | "children" | "style" | "channel" | "colorSpace">,
    FieldPartsProps {
  /** Shown while the input is empty. Never instead of a label. */
  placeholder?: string;
  /** Layout only. */
  className?: string;
}

/**
 * The committed color, or an empty swatch while there is none. Hidden from assistive technology:
 * the input already says the color. React Aria's ColorSwatch would name it ("dark vibrant blue")
 * in React Aria's words rather than the strings catalog's, and only repeat the input.
 */
function Swatch() {
  const color = useContext(ColorFieldStateContext)?.colorValue;
  const styles = colorField();
  return (
    <span
      data-slot="color-field-swatch"
      aria-hidden="true"
      data-empty={color ? undefined : true}
      className={styles.swatch()}
      style={color ? { backgroundColor: color.toString("css") } : undefined}
    />
  );
}

/**
 * Controlled with `value` and `onChange`, uncontrolled with `defaultValue`. Values are React
 * Aria's `Color` (or a hex string going in); `onChange` gives a `Color`, or null when the input is
 * cleared, and `color.toString("hex")` gives "#RRGGBB".
 */
export function ColorField({
  label,
  description,
  errorMessage,
  placeholder,
  className,
  ...props
}: ColorFieldProps) {
  const styles = colorField();
  return (
    <AriaColorField data-slot="color-field" {...props} className={styles.root({ className })}>
      <FieldParts label={label} description={description} errorMessage={errorMessage}>
        <Group data-slot="color-field-group" className={styles.group()}>
          <Swatch />
          <Input
            data-slot="color-field-input"
            placeholder={placeholder}
            className={styles.input()}
          />
        </Group>
      </FieldParts>
    </AriaColorField>
  );
}
