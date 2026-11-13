// SPDX-License-Identifier: AGPL-3.0-or-later
// SideNav: the app's main navigation at the inline start of wide screens
// (docs/specs/m0/13-base-ui-components.md M0 inventory; spec 11 B3). The organization's logo at the
// top, the entries, and a footer for the organization's back link. The current entry carries
// `aria-current="page"` and is drawn in the brand tint with a heavier label. From 768 to 1023 px
// it collapses to icons, each label kept as the link's name and shown as a Tooltip (spec 11 Q1).
// Entries are data the app passes in (spec 11 B5); links navigate through the RouterProvider.
import type { Icon as PhosphorIcon } from "@phosphor-icons/react";
import type { ReactNode } from "react";
import { Link as AriaLink } from "react-aria-components";
import { SIDE_NAV_EXPANDED_QUERY } from "../lib/breakpoints";
import { tv } from "../lib/tv";
import { useMediaQuery } from "../lib/use-media-query";
import { Tooltip, TooltipTrigger } from "../primitives/tooltip";
import { VisuallyHidden } from "../primitives/visually-hidden";
import { useStepclimbStrings } from "../strings/strings";
import { Icon } from "../theme/icon";

/** One navigation entry. The app builds the list from its routes and the user's permissions. */
export interface NavEntry {
  /** Unique among the entries; `current` names the active one by it. */
  id: string;
  /** The route. */
  href: string;
  /** The label, from `useStepclimbStrings()`. */
  label: string;
  /** The Phosphor icon. */
  icon: PhosphorIcon;
}

export const sideNav = tv({
  slots: {
    root: [
      "sticky top-0 flex h-dvh flex-col gap-4 border-e border-line bg-canvas py-4 font-ui",
      "pt-[max(1rem,env(safe-area-inset-top))] pb-[max(1rem,env(safe-area-inset-bottom))]",
      "ps-[env(safe-area-inset-left)] rtl:ps-[env(safe-area-inset-right)]",
    ],
    logo: "flex min-h-10 items-center px-4",
    list: "flex flex-1 flex-col gap-1 overflow-y-auto px-2",
    link: [
      "flex min-h-(--sc-control-h) items-center gap-3 rounded-sm px-3 text-sm text-fg-default",
      "pointer-coarse:min-h-(--sc-control-touch) outline-none transition-colors duration-fast ease-out-expo",
      "data-[hovered]:bg-sunken data-[pressed]:bg-sunken",
      "data-[focus-visible]:ring-2 data-[focus-visible]:ring-ring",
      "aria-[current=page]:bg-brand-soft aria-[current=page]:font-medium aria-[current=page]:text-brand-fg",
    ],
    label: "min-w-0 truncate",
    footer: "px-4 text-sm",
  },
  variants: {
    collapsed: {
      true: {
        root: "w-16 items-center",
        logo: "justify-center px-2",
        list: "w-full items-center",
        link: "size-11 justify-center px-0",
        footer: "px-2",
      },
      false: { root: "w-60" },
    },
  },
});

export interface SideNavProps {
  /** The entries, in order. */
  entries: readonly NavEntry[];
  /** The `id` of the entry for the current page. */
  current?: string;
  /** The organization's logo, or its name. */
  logo?: ReactNode;
  /** Under the entries: the organization's back link, when it has one. */
  footer?: ReactNode;
  /**
   * Icons only, with labels in tooltips. Without a value it follows the width: collapsed from
   * 768 to 1023 px, full at 1024 px and wider.
   */
  collapsed?: boolean;
  /** Layout only. */
  className?: string;
}

export function SideNav({ entries, current, logo, footer, collapsed, className }: SideNavProps) {
  const { t } = useStepclimbStrings();
  const expanded = useMediaQuery(SIDE_NAV_EXPANDED_QUERY);
  const isCollapsed = collapsed ?? !expanded;
  const styles = sideNav({ collapsed: isCollapsed });
  return (
    <nav
      data-slot="side-nav"
      data-collapsed={isCollapsed || undefined}
      aria-label={t("sideNav.label")}
      className={styles.root({ className })}
    >
      {logo === undefined ? null : (
        <div data-slot="side-nav-logo" className={styles.logo()}>
          {logo}
        </div>
      )}
      <ul data-slot="side-nav-list" className={styles.list()}>
        {entries.map((entry) => (
          <li key={entry.id}>
            <SideNavLink entry={entry} isCurrent={entry.id === current} collapsed={isCollapsed} />
          </li>
        ))}
      </ul>
      {footer === undefined ? null : (
        <div data-slot="side-nav-footer" className={styles.footer()}>
          {footer}
        </div>
      )}
    </nav>
  );
}

function SideNavLink({
  entry,
  isCurrent,
  collapsed,
}: {
  entry: NavEntry;
  isCurrent: boolean;
  collapsed: boolean;
}) {
  const styles = sideNav({ collapsed });
  const link = (
    <AriaLink
      data-slot="side-nav-link"
      href={entry.href}
      aria-current={isCurrent ? "page" : undefined}
      className={styles.link()}
    >
      <Icon icon={entry.icon} size="md" />
      {collapsed ? (
        <VisuallyHidden>{entry.label}</VisuallyHidden>
      ) : (
        <span className={styles.label()}>{entry.label}</span>
      )}
    </AriaLink>
  );
  if (!collapsed) return link;
  return (
    <TooltipTrigger>
      {link}
      <Tooltip data-slot="side-nav-tooltip" placement="end">
        {entry.label}
      </Tooltip>
    </TooltipTrigger>
  );
}
