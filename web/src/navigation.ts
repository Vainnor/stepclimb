// SPDX-License-Identifier: AGPL-3.0-or-later
// The shell's navigation entries, as data (docs/specs/m0/11-app-shell.md B5, Q2). M0 lists only
// destinations that exist; each milestone adds its entry with its feature. An entry with a
// `permission` shows only to a user who holds it at some scope according to `/me`. Hiding an entry
// is presentation: the server still refuses the route's API calls.
import { HouseIcon, type Icon as PhosphorIcon } from "@phosphor-icons/react";
import type { NavEntry, Strings } from "@stepclimb/ui";
import { holdsPermission, type Me } from "./session";

export interface NavRoute {
  /** Unique among the entries; a route's `staticData.view.nav` names its entry by it. */
  id: string;
  /** The route's path. */
  to: string;
  icon: PhosphorIcon;
  /** The label, looked up with a literal message id so the catalog check can read it. */
  label: (t: Strings["t"]) => string;
  /** A permission catalog key; without one, every signed-in user sees the entry. */
  permission?: string;
}

export const navRoutes: readonly NavRoute[] = [
  { id: "home", to: "/", icon: HouseIcon, label: (t) => t("nav.home") },
];

/** The entries `me` may see, in order, labeled. */
export function visibleNavEntries(
  routes: readonly NavRoute[],
  me: Me,
  t: Strings["t"],
): NavEntry[] {
  return routes
    .filter((route) => route.permission === undefined || holdsPermission(me, route.permission))
    .map((route) => ({ id: route.id, href: route.to, label: route.label(t), icon: route.icon }));
}
