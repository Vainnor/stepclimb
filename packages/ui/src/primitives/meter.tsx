// SPDX-License-Identifier: AGPL-3.0-or-later
// Meter: a quantity within a known range, such as storage used, with its label and its value as a
// percentage from the strings catalog (docs/specs/m0/13-base-ui-components.md M0 inventory). It
// is drawn like ProgressBar in a neutral fill: the brand marks progress, which a meter is not.
import type { ReactNode } from "react";
import {
  Meter as AriaMeter,
  type MeterProps as AriaMeterProps,
  Label,
} from "react-aria-components";
import { useStepclimbStrings } from "../strings/strings";
import { fractionOf, progressBar } from "./progress-bar";

export const meter = progressBar;

export interface MeterProps extends Omit<AriaMeterProps, "className" | "children"> {
  /** What is measured. Without it, give `aria-label`. */
  label?: ReactNode;
  /** Layout only. */
  className?: string;
}

export function Meter({ label, className, valueLabel, ...props }: MeterProps) {
  const { t } = useStepclimbStrings();
  const { value = 0, minValue = 0, maxValue = 100 } = props;
  const fraction = fractionOf(value, minValue, maxValue);
  const styles = meter({ tone: "neutral" });
  return (
    <AriaMeter
      data-slot="meter"
      {...props}
      valueLabel={valueLabel ?? t("meter.value", { value: fraction })}
      className={styles.root({ className })}
    >
      {({ valueText }) => (
        <>
          <div data-slot="meter-header" className={styles.header()}>
            {label === undefined ? null : (
              <Label data-slot="meter-label" className={styles.label()}>
                {label}
              </Label>
            )}
            <span data-slot="meter-value" className={styles.value()}>
              {valueText}
            </span>
          </div>
          <div data-slot="meter-track" className={styles.track()}>
            <div
              data-slot="meter-fill"
              className={styles.fill()}
              style={{ width: `${fraction * 100}%` }}
            />
          </div>
        </>
      )}
    </AriaMeter>
  );
}
