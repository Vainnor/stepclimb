// SPDX-License-Identifier: AGPL-3.0-or-later
// Slider: one number in a range, set by dragging a thumb along a track or typed into the number
// field beside it for an exact value (docs/specs/m1/16-learning-ui-components.md B6). Built on
// React Aria's Slider; the number field is React Aria's NumberField, bound to the slider's state,
// so both always show the same value. The arrow keys step either one.
import { type ReactNode, useContext } from "react";
import {
  Slider as AriaSlider,
  type SliderProps as AriaSliderProps,
  Input,
  LabelContext,
  NumberField,
  SliderFill,
  SliderStateContext,
  SliderThumb,
  SliderTrack,
  useSlottedContext,
} from "react-aria-components";
import { tv } from "../lib/tv";
import { FieldLabel, field } from "./field";

const parts = field();

export const slider = tv({
  slots: {
    root: parts.root(),
    controls: "flex items-center gap-4",
    // The track is as tall as a control, so it is easy to press; the rail is drawn inside it.
    track: "relative h-(--sc-control-h) min-w-0 flex-1 pointer-coarse:h-(--sc-control-touch)",
    rail: [
      "absolute inset-x-0 top-1/2 h-1.5 -translate-y-1/2 rounded-pill bg-sunken",
      "border border-line-strong",
    ],
    // The fill keeps its color in forced-colors mode, which drops backgrounds.
    fill: [
      "rounded-pill bg-brand forced-colors:bg-[Highlight] forced-colors:forced-color-adjust-none",
      "group-data-[disabled]:bg-fg-disabled",
    ],
    // Drawn as a border, which forced-colors mode keeps. Under a coarse pointer an invisible ring
    // makes it a 44 px target.
    thumb: [
      "top-1/2 size-5 rounded-pill border-2 border-brand bg-canvas outline-none",
      "transition-colors duration-fast ease-out-expo",
      "pointer-coarse:before:absolute pointer-coarse:before:-inset-3 pointer-coarse:before:content-['']",
      "data-[hovered]:bg-brand-soft data-[dragging]:bg-brand-soft",
      "data-[focus-visible]:ring-2 data-[focus-visible]:ring-ring data-[focus-visible]:ring-offset-2 ring-offset-canvas",
      "data-[disabled]:border-fg-disabled data-[disabled]:bg-sunken",
    ],
    number: "w-24 shrink-0",
    input: [parts.control(), parts.input(), "tabular-nums"],
  },
});

export interface SliderProps
  extends Omit<
    AriaSliderProps<number>,
    "className" | "children" | "style" | "orientation" | "render"
  > {
  /** The visible label. Without one, give `aria-label`. */
  label?: ReactNode;
  /** Layout only. */
  className?: string;
}

/** The number field, reading and setting the slider's one value. */
function ExactValue({
  formatOptions,
  ariaLabel,
}: {
  formatOptions?: Intl.NumberFormatOptions;
  ariaLabel?: string;
}) {
  const state = useContext(SliderStateContext);
  const labelId = useSlottedContext(LabelContext)?.id;
  if (state === null) return null;
  const styles = slider();
  const named =
    labelId === undefined ? { "aria-label": ariaLabel } : { "aria-labelledby": labelId };
  return (
    <NumberField
      data-slot="slider-number"
      {...named}
      value={state.getThumbValue(0)}
      // As React Aria's thumb does for a key press: a typed value ends a change, so `onChangeEnd`
      // runs as it does after a drag.
      onChange={(value) => {
        if (Number.isNaN(value)) return;
        state.setThumbDragging(0, true);
        state.setThumbValue(0, value);
        state.setThumbDragging(0, false);
      }}
      minValue={state.getThumbMinValue(0)}
      maxValue={state.getThumbMaxValue(0)}
      step={state.step}
      formatOptions={formatOptions}
      isDisabled={state.isDisabled}
      className={styles.number()}
    >
      <Input data-slot="slider-input" className={styles.input()} />
    </NumberField>
  );
}

/**
 * Controlled with `value` and `onChange`, uncontrolled with `defaultValue` (the minimum when
 * neither is given). `onChange` runs while the thumb moves, `onChangeEnd` once it is let go or a
 * value is typed. `formatOptions` format the number field (a percent, a unit).
 */
export function Slider({
  label,
  className,
  value,
  defaultValue,
  minValue = 0,
  formatOptions,
  ...props
}: SliderProps) {
  const styles = slider();
  return (
    <AriaSlider<number>
      data-slot="slider"
      {...props}
      value={value}
      // React Aria gives `onChange` a number only when a value or default value is a number.
      defaultValue={value === undefined ? (defaultValue ?? minValue) : undefined}
      minValue={minValue}
      formatOptions={formatOptions}
      className={styles.root({ className: ["group", className] })}
    >
      {label === undefined ? null : <FieldLabel>{label}</FieldLabel>}
      <div data-slot="slider-controls" className={styles.controls()}>
        <SliderTrack data-slot="slider-track" className={styles.track()}>
          <div data-slot="slider-rail" className={styles.rail()}>
            <SliderFill data-slot="slider-fill" className={styles.fill()} />
          </div>
          <SliderThumb data-slot="slider-thumb" className={styles.thumb()} />
        </SliderTrack>
        <ExactValue formatOptions={formatOptions} ariaLabel={props["aria-label"]} />
      </div>
    </AriaSlider>
  );
}
