// SPDX-License-Identifier: AGPL-3.0-or-later
// Slider: one number in a range, set by dragging a thumb along a track or typed into the number
// field beside it for an exact value (docs/specs/m1/16-learning-ui-components.md B6). Built on
// React Aria's Slider and NumberField, which share one value, so both always show the same
// number. The arrow keys step either one. The label names both by `aria-labelledby`.
import { type ReactNode, useId, useRef, useState } from "react";
import {
  Slider as AriaSlider,
  type SliderProps as AriaSliderProps,
  Input,
  Label,
  NumberField,
  SliderFill,
  SliderThumb,
  SliderTrack,
} from "react-aria-components";
import { tv } from "../lib/tv";
import { FieldLabel, field } from "./field";

const parts = field();

export const slider = tv({
  slots: {
    root: parts.root(),
    controls: "flex items-center gap-4",
    slider: "group min-w-0 flex-1",
    // The track is as tall as a control, so it is easy to press; the rail is drawn inside it.
    track: "relative h-(--sc-control-h) w-full pointer-coarse:h-(--sc-control-touch)",
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
  /** The visible label. Without one, give `aria-labelledby` or `aria-label`. */
  label?: ReactNode;
  /** Layout only. */
  className?: string;
}

/** The value, held here unless `value` controls it; the minimum when neither value is given. */
function useSliderValue({ value, defaultValue, minValue = 0, onChange }: SliderProps) {
  const [own, setOwn] = useState(defaultValue ?? minValue);
  const set = (next: number) => {
    if (value === undefined) setOwn(next);
    onChange?.(next);
  };
  return [value ?? own, set] as const;
}

/**
 * Controlled with `value` and `onChange`, uncontrolled with `defaultValue`. `onChange` runs while
 * the thumb moves, `onChangeEnd` once it is let go or a value is typed; a typed value is kept in
 * range and on a step. `formatOptions` format the number field (a percent, a unit).
 */
export function Slider(props: SliderProps) {
  const {
    label,
    className,
    value: _value,
    defaultValue: _defaultValue,
    onChange: _onChange,
    onChangeEnd,
    minValue = 0,
    maxValue = 100,
    step = 1,
    formatOptions,
    isDisabled,
    "aria-label": ariaLabel,
    "aria-labelledby": ariaLabelledBy,
    "aria-describedby": ariaDescribedBy,
    ...rest
  } = props;
  const styles = slider();
  const [current, setCurrent] = useSliderValue(props);
  const labelId = useId();
  const thumbInput = useRef<HTMLInputElement>(null);
  // As React Aria's useLabel joins them: the visible label first, then the caller's.
  const labelledBy =
    [label === undefined ? undefined : labelId, ariaLabelledBy].filter(Boolean).join(" ") ||
    undefined;
  const named = {
    "aria-label": label === undefined ? ariaLabel : undefined,
    "aria-labelledby": labelledBy,
    "aria-describedby": ariaDescribedBy,
  };
  const range = { minValue, maxValue, step, formatOptions, isDisabled };
  return (
    <div data-slot="slider" className={styles.root({ className })}>
      {label === undefined ? null : (
        // Pressing the label focuses the thumb, as React Aria's own slider label does, rather than
        // the number field, which would open the on-screen keyboard.
        <FieldLabel id={labelId} onClick={() => thumbInput.current?.focus()}>
          {label}
        </FieldLabel>
      )}
      <div data-slot="slider-controls" className={styles.controls()}>
        <AriaSlider<number>
          data-slot="slider-group"
          {...rest}
          {...named}
          {...range}
          value={current}
          onChange={setCurrent}
          onChangeEnd={onChangeEnd}
          className={styles.slider()}
        >
          {/*
            React Aria names the thumb by an inner label, or else by the slider group, whose own
            `aria-labelledby` a name computation does not follow; it would read the thumb's value
            instead. An empty hidden label takes that place, and the thumb's own
            `aria-labelledby` adds the name.
          */}
          {named["aria-labelledby"] === undefined ? null : (
            <Label hidden data-slot="slider-anchor" />
          )}
          <SliderTrack data-slot="slider-track" className={styles.track()}>
            <div data-slot="slider-rail" className={styles.rail()}>
              <SliderFill data-slot="slider-fill" className={styles.fill()} />
            </div>
            <SliderThumb
              data-slot="slider-thumb"
              aria-labelledby={named["aria-labelledby"]}
              inputRef={thumbInput}
              className={styles.thumb()}
            />
          </SliderTrack>
        </AriaSlider>
        <NumberField
          data-slot="slider-number"
          {...named}
          {...range}
          value={current}
          onChange={(typed) => {
            if (Number.isNaN(typed)) return;
            setCurrent(typed);
            onChangeEnd?.(typed);
          }}
          className={styles.number()}
        >
          <Input data-slot="slider-input" className={styles.input()} />
        </NumberField>
      </div>
    </div>
  );
}
