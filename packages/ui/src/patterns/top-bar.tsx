// SPDX-License-Identifier: AGPL-3.0-or-later
// TopBar: the `header` above the current view, with its title and, at the inline end, a slot for
// actions and the user menu (docs/specs/m0/13-base-ui-components.md M0 inventory; spec 11 B4).
// Under 768 px it sticks to the top inside the safe area over a blurred, translucent canvas: a
// sticky mobile bar is the other place a blur is allowed (plan UI tab -> Pitfalls: glass).
import type { ReactNode } from "react";
import { tv } from "../lib/tv";
import { Heading, type HeadingLevel } from "../primitives/heading";

export const topBar = tv({
  slots: {
    root: [
      "z-30 border-b border-line bg-canvas font-ui",
      "max-md:sticky max-md:top-0 max-md:bg-canvas/85 max-md:backdrop-blur-md",
      // Clear of the safe areas at every width: a phone in landscape is wider than 768 px.
      "pt-[env(safe-area-inset-top)]",
      "ps-[max(1rem,env(safe-area-inset-left))] pe-[max(1rem,env(safe-area-inset-right))]",
      "md:ps-[max(1.5rem,env(safe-area-inset-left))] md:pe-[max(1.5rem,env(safe-area-inset-right))]",
      "rtl:ps-[max(1rem,env(safe-area-inset-right))] rtl:pe-[max(1rem,env(safe-area-inset-left))]",
      "rtl:md:ps-[max(1.5rem,env(safe-area-inset-right))] rtl:md:pe-[max(1.5rem,env(safe-area-inset-left))]",
    ],
    row: "flex min-h-14 items-center gap-3",
    title: "min-w-0 flex-1 truncate",
    actions: "flex shrink-0 items-center gap-2",
  },
});

const styles = topBar();

export interface TopBarProps {
  /** The current view's title. */
  title: ReactNode;
  /** The title's heading level: 1, unless the view has its own `h1`. */
  titleLevel?: HeadingLevel;
  /** At the inline end: the view's actions and the user menu. */
  actions?: ReactNode;
  /** Layout only. */
  className?: string;
}

export function TopBar({ title, titleLevel = 1, actions, className }: TopBarProps) {
  return (
    <header data-slot="top-bar" className={styles.root({ className })}>
      <div data-slot="top-bar-row" className={styles.row()}>
        <Heading data-slot="top-bar-title" level={titleLevel} size="lg" className={styles.title()}>
          {title}
        </Heading>
        {actions === undefined ? null : (
          <div data-slot="top-bar-actions" className={styles.actions()}>
            {actions}
          </div>
        )}
      </div>
    </header>
  );
}
