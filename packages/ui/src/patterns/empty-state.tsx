// SPDX-License-Identifier: AGPL-3.0-or-later
// EmptyState: what an empty list or view shows: one sentence on why it is empty and, when the
// person can act, at most one action (docs/specs/m0/13-base-ui-components.md M0 inventory; spec 11
// B30). Built from plain elements.
import type { Icon as PhosphorIcon } from "@phosphor-icons/react";
import type { ReactElement, ReactNode } from "react";
import { tv } from "../lib/tv";
import { Icon } from "../theme/icon";

export const emptyState = tv({
  slots: {
    root: "flex flex-col items-center gap-4 px-6 py-10 text-center font-ui",
    icon: "text-fg-soft",
    description: "max-w-md text-base text-fg-sub text-balance",
    action: "flex",
  },
});

export interface EmptyStateProps {
  /** One sentence on why it is empty, from `useStepclimbStrings()`. */
  children: ReactNode;
  /** A Phosphor icon drawn above the sentence, decorative. */
  icon?: PhosphorIcon;
  /** The one action, usually a primary Button. */
  action?: ReactElement;
  /** Layout only. */
  className?: string;
}

export function EmptyState({ children, icon, action, className }: EmptyStateProps) {
  const styles = emptyState();
  return (
    <div data-slot="empty-state" className={styles.root({ className })}>
      {icon ? (
        <span data-slot="empty-state-icon" className={styles.icon()}>
          <Icon icon={icon} size="lg" />
        </span>
      ) : null}
      <p data-slot="empty-state-description" className={styles.description()}>
        {children}
      </p>
      {action ? (
        <div data-slot="empty-state-action" className={styles.action()}>
          {action}
        </div>
      ) : null}
    </div>
  );
}
