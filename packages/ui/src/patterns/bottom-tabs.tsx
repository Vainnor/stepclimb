// SPDX-License-Identifier: AGPL-3.0-or-later
// BottomTabs: the app's main navigation on phones, under 768 px
// (docs/specs/m0/13-base-ui-components.md M0 inventory; spec 11 B3). Navigation links in a `nav`,
// not ARIA tabs: each goes to a page. Fixed to the bottom inside the safe area, and it does not
// rubber-band. The current entry carries `aria-current="page"`, a brand bar above its icon and a
// heavier label, so color is not the only sign.
import { Link as AriaLink } from "react-aria-components";
import { tv } from "../lib/tv";
import { useStepclimbStrings } from "../strings/strings";
import { Icon } from "../theme/icon";
import type { NavEntry } from "./side-nav";

export const bottomTabs = tv({
  slots: {
    root: [
      // As tall as `--sc-bottom-tabs-inset` (base.css), which AppShell and the scroll padding read.
      "fixed inset-x-0 bottom-0 z-40 h-(--sc-bottom-tabs-inset) border-t border-line bg-raised font-ui",
      "overscroll-none touch-manipulation",
      "pb-[env(safe-area-inset-bottom)] ps-[env(safe-area-inset-left)] pe-[env(safe-area-inset-right)]",
      "rtl:ps-[env(safe-area-inset-right)] rtl:pe-[env(safe-area-inset-left)]",
    ],
    list: "flex",
    item: "flex min-w-0 flex-1",
    link: [
      "group relative flex min-h-14 min-w-0 flex-1 flex-col items-center justify-center gap-0.5 px-1",
      "text-caption text-fg-sub outline-none transition-colors duration-fast ease-out-expo",
      "data-[pressed]:bg-sunken",
      "data-[focus-visible]:ring-2 data-[focus-visible]:ring-inset data-[focus-visible]:ring-ring",
      "aria-[current=page]:font-medium aria-[current=page]:text-brand-fg",
    ],
    indicator: [
      "absolute inset-x-4 top-0 h-0.5 rounded-b-xs bg-transparent",
      "group-aria-[current=page]:bg-brand",
    ],
    label: "max-w-full truncate",
  },
});

const styles = bottomTabs();

export interface BottomTabsProps {
  /** The entries, in order; at most five fit a phone. */
  entries: readonly NavEntry[];
  /** The `id` of the entry for the current page. */
  current?: string;
  /** Layout only. */
  className?: string;
}

export function BottomTabs({ entries, current, className }: BottomTabsProps) {
  const { t } = useStepclimbStrings();
  return (
    <nav
      data-slot="bottom-tabs"
      aria-label={t("bottomTabs.label")}
      className={styles.root({ className })}
    >
      <ul data-slot="bottom-tabs-list" className={styles.list()}>
        {entries.map((entry) => (
          <li key={entry.id} className={styles.item()}>
            <AriaLink
              data-slot="bottom-tabs-link"
              href={entry.href}
              aria-current={entry.id === current ? "page" : undefined}
              className={styles.link()}
            >
              <span data-slot="bottom-tabs-indicator" className={styles.indicator()} />
              <Icon icon={entry.icon} size="md" />
              <span className={styles.label()}>{entry.label}</span>
            </AriaLink>
          </li>
        ))}
      </ul>
    </nav>
  );
}
