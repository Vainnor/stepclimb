// SPDX-License-Identifier: AGPL-3.0-or-later
// Callout: a note set apart from the text around it, as an icon on a tinted surface and never a
// colored side stripe (docs/specs/m1/16-learning-ui-components.md B11; plan, UI tab -> Pitfalls).
// Each kind has its own icon, and the icon is named by the kind ("Warning"), so the kind is said
// in words and shape as well as color. A transparent border draws the edge in forced colors,
// which drops the tint.
import {
  InfoIcon,
  LightbulbIcon,
  type Icon as PhosphorIcon,
  ProhibitIcon,
  WarningCircleIcon,
} from "@phosphor-icons/react";
import type { ReactNode } from "react";
import { tv } from "../lib/tv";
import { useStepclimbStrings } from "../strings/strings";
import { Icon } from "../theme/icon";

/** The kinds a callout can be, as page bodies store them (spec 07 B8). */
export const calloutKinds = ["info", "tip", "warning", "danger"] as const;
export type CalloutKind = (typeof calloutKinds)[number];

/** Each kind's icon. */
export const calloutIcons: Record<CalloutKind, PhosphorIcon> = {
  info: InfoIcon,
  tip: LightbulbIcon,
  warning: WarningCircleIcon,
  danger: ProhibitIcon,
};

/**
 * A status tint per kind, with `<status>-fg`, which reads at 4.5:1 on its tint, for the icon and
 * the title. A tip takes the success tint, not the brand's: a brand can be any hue, red included,
 * and a tip must not look like a danger.
 */
export const callout = tv({
  slots: {
    root: "flex items-start gap-3 rounded-md border border-transparent p-4 font-ui text-base",
    icon: "mt-0.5",
    body: "flex min-w-0 flex-1 flex-col gap-1 text-fg-default break-words",
    title: "font-medium",
  },
  variants: {
    kind: {
      info: { root: "bg-info-soft", icon: "text-info-fg", title: "text-info-fg" },
      tip: { root: "bg-success-soft", icon: "text-success-fg", title: "text-success-fg" },
      warning: { root: "bg-warning-soft", icon: "text-warning-fg", title: "text-warning-fg" },
      danger: { root: "bg-danger-soft", icon: "text-danger-fg", title: "text-danger-fg" },
    },
  },
});

export interface CalloutProps {
  /** What kind of note it is: its icon, tint and the name the icon is read with. */
  kind: CalloutKind;
  /** A short title above the text. */
  title?: ReactNode;
  /** The text. */
  children: ReactNode;
  /** Layout only. */
  className?: string;
}

export function Callout({ kind, title, children, className }: CalloutProps) {
  const { t } = useStepclimbStrings();
  const styles = callout({ kind });
  return (
    <div role="note" data-slot="callout" data-kind={kind} className={styles.root({ className })}>
      <Icon
        icon={calloutIcons[kind]}
        size="md"
        label={t("callout.kind", { kind })}
        className={styles.icon()}
      />
      <div data-slot="callout-body" className={styles.body()}>
        {title === undefined ? null : (
          <p data-slot="callout-title" className={styles.title()}>
            {title}
          </p>
        )}
        <div data-slot="callout-content">{children}</div>
      </div>
    </div>
  );
}
