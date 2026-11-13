// SPDX-License-Identifier: AGPL-3.0-or-later
// Switch: turns a setting on or off at once, without a save step
// (docs/specs/m0/13-base-ui-components.md M0 inventory). For a choice that is saved with a form,
// use a Checkbox. The thumb's position and the track's fill both change, so color is not the only
// sign. Built on React Aria's SwitchField and SwitchButton; on touch screens the label is at least
// 44 px tall.
import type { ReactNode } from "react";
import { SwitchButton, SwitchField, type SwitchFieldProps } from "react-aria-components";
import { tv } from "../lib/tv";
import { FieldDescription } from "./field";

export const switchField = tv({
  slots: {
    root: "flex flex-col gap-1 font-ui",
    button: [
      "group flex w-fit min-h-(--sc-control-h) cursor-default items-center gap-2.5 text-sm text-fg-default",
      "pointer-coarse:min-h-(--sc-control-touch)",
      "data-[disabled]:cursor-not-allowed data-[disabled]:text-fg-disabled",
    ],
    track: [
      "flex h-5 w-9 shrink-0 items-center rounded-full border border-line-strong bg-sunken px-0.5",
      "transition-colors duration-fast ease-out-expo",
      "group-data-[hovered]:border-fg-sub",
      "group-data-[selected]:border-brand group-data-[selected]:bg-brand",
      "group-data-[focus-visible]:ring-2 group-data-[focus-visible]:ring-ring group-data-[focus-visible]:ring-offset-2 ring-offset-canvas",
      "group-data-[disabled]:border-line group-data-[disabled]:bg-sunken",
    ],
    thumb: [
      "size-3.5 rounded-full bg-fg-sub transition-transform duration-fast ease-out-expo",
      "group-data-[selected]:translate-x-4 group-data-[selected]:bg-on-brand",
      "rtl:group-data-[selected]:-translate-x-4",
      "group-data-[disabled]:bg-fg-disabled",
    ],
    label: "min-w-0",
    description: "ps-11.5",
  },
});

const styles = switchField();

export interface SwitchProps extends Omit<SwitchFieldProps, "className" | "children"> {
  /** The visible label: the setting, not "On" or "Off". Without one, give `aria-label`. */
  children?: ReactNode;
  /** Help under the switch. */
  description?: ReactNode;
  /** Layout only. */
  className?: string;
}

/** Controlled with `isSelected` and `onChange`, uncontrolled with `defaultSelected`. */
export function Switch({ children, description, className, ...props }: SwitchProps) {
  return (
    <SwitchField data-slot="switch" {...props} className={styles.root({ className })}>
      <SwitchButton data-slot="switch-button" className={styles.button()}>
        <span data-slot="switch-track" className={styles.track()}>
          <span data-slot="switch-thumb" className={styles.thumb()} />
        </span>
        {children === undefined ? null : (
          <span data-slot="switch-label" className={styles.label()}>
            {children}
          </span>
        )}
      </SwitchButton>
      {description === undefined ? null : (
        <FieldDescription className={styles.description()}>{description}</FieldDescription>
      )}
    </SwitchField>
  );
}
