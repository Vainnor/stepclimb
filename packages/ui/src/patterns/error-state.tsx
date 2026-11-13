// SPDX-License-Identifier: AGPL-3.0-or-later
// ErrorState: what a view shows when it could not load: what happened, how to fix it, and a slot
// for the retry action (docs/specs/m0/13-base-ui-components.md M0 inventory; spec 11 B27 to B29).
// Built from plain elements; the icon and the heading carry the state, not the color alone.
import { WarningCircleIcon } from "@phosphor-icons/react";
import type { ReactNode } from "react";
import { tv } from "../lib/tv";
import { Heading, type HeadingLevel } from "../primitives/heading";
import { Icon } from "../theme/icon";

export const errorState = tv({
  slots: {
    root: "flex flex-col items-center gap-3 px-6 py-10 text-center font-ui",
    icon: "text-danger-fg",
    description: "max-w-md text-base text-fg-default text-balance",
    action: "mt-1 flex",
  },
});

export interface ErrorStateProps {
  /** What happened, as a short heading: the problem's `title` from the server where there is one. */
  title: ReactNode;
  /** The heading's place in the page outline. */
  level: HeadingLevel;
  /** How to fix it, from `useStepclimbStrings()`. */
  children: ReactNode;
  /** The retry action, usually a Button. */
  action?: ReactNode;
  /** Layout only. */
  className?: string;
}

export function ErrorState({ title, level, children, action, className }: ErrorStateProps) {
  const styles = errorState();
  return (
    <div data-slot="error-state" className={styles.root({ className })}>
      <span data-slot="error-state-icon" className={styles.icon()}>
        <Icon icon={WarningCircleIcon} size="lg" />
      </span>
      <Heading data-slot="error-state-title" level={level} size="lg">
        {title}
      </Heading>
      <p data-slot="error-state-description" className={styles.description()}>
        {children}
      </p>
      {action ? (
        <div data-slot="error-state-action" className={styles.action()}>
          {action}
        </div>
      ) : null}
    </div>
  );
}
