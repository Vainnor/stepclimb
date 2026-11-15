// SPDX-License-Identifier: AGPL-3.0-or-later
// The signed-in layout (docs/specs/m0/11-app-shell.md B2 to B6, B9 to B11): AppShell with the
// SideNav, BottomTabs and TopBar, drawn only once `/me` has loaded. While it loads, the delayed
// ShellSkeleton. On a 401 it draws the skeleton too, never the cached user, while the session
// handler clears the cache and takes the browser to the sign-in screen. Any other failure shows
// LoadError.
import {
  AppShell,
  BottomTabs,
  initials,
  SideNav,
  Text,
  TopBar,
  useStepclimbStrings,
  VisuallyHidden,
} from "@stepclimb/ui";
import { useQuery } from "@tanstack/react-query";
import { Outlet, useMatches } from "@tanstack/react-router";
import { type ReactNode, useEffect } from "react";
import { isUnauthorized } from "../api";
import { applyAppearance } from "../appearance";
import type { ViewInfo } from "../context";
import { navRoutes, visibleNavEntries } from "../navigation";
import { type Me, meQuery } from "../session";
import { useDocumentTitle } from "./document-title";
import { LoadError } from "./load-error";
import { ShellSkeleton } from "./shell-skeleton";
import { SignedOutFrame } from "./signed-out";
import { UserMenu } from "./user-menu";

/** The organization's name at the top of the SideNav; its initials where the SideNav is icons only. */
function OrgMark({ name }: { name: string }) {
  return (
    <>
      <Text size="lg" tone="strong" className="hidden min-w-0 truncate lg:inline">
        {name}
      </Text>
      <Text size="lg" tone="strong" className="lg:hidden">
        <span aria-hidden="true">{initials(name)}</span>
        <VisuallyHidden>{name}</VisuallyHidden>
      </Text>
    </>
  );
}

export interface SignedInShellProps {
  me: Me;
  /** The view's title: the TopBar's `h1` and the start of the document title. */
  title: string;
  /** The `id` of the current navigation entry. */
  current?: string;
  children: ReactNode;
}

export function SignedInShell({ me, title, current, children }: SignedInShellProps) {
  const { t } = useStepclimbStrings();
  useDocumentTitle(title, me.organization.name);
  const entries = visibleNavEntries(navRoutes, me, t);
  const { theme, density } = me.appearance;
  // The server wrote the choice into the page; this keeps a page it did not write in step.
  useEffect(() => applyAppearance({ theme, density }), [theme, density]);
  return (
    <AppShell
      sideNav={
        <SideNav
          entries={entries}
          current={current}
          logo={<OrgMark name={me.organization.name} />}
        />
      }
      bottomTabs={<BottomTabs entries={entries} current={current} />}
      topBar={
        <TopBar
          title={title}
          actions={<UserMenu name={me.user.display_name} appearance={me.appearance} />}
        />
      }
    >
      {children}
    </AppShell>
  );
}

/** The deepest matched route's view; every route under the signed-in layout declares one. */
function useCurrentView(): ViewInfo {
  const view = useMatches({
    select: (matches) => matches.findLast((match) => match.staticData.view)?.staticData.view,
  });
  if (!view) throw new Error("a signed-in route has no staticData.view");
  return view;
}

/** Shown when `/me` failed without a 401: there is no user to draw the shell for. */
function MeLoadError({ error, onRetry }: { error: unknown; onRetry: () => void }) {
  const { t } = useStepclimbStrings();
  useDocumentTitle(t("loadError.documentTitle"));
  return (
    <SignedOutFrame>
      <LoadError error={error} level={1} onRetry={onRetry} />
    </SignedOutFrame>
  );
}

export function SignedInLayout() {
  const { t } = useStepclimbStrings();
  const me = useQuery(meQuery);
  const view = useCurrentView();
  // A 401 the session handler did not act on (it arrived on a page outside this layout) leaves the
  // expired user's data in the cache: never draw it.
  if (isUnauthorized(me.error)) return <ShellSkeleton />;
  if (me.data) {
    return (
      <SignedInShell me={me.data} title={view.title(t)} current={view.nav}>
        <Outlet />
      </SignedInShell>
    );
  }
  if (me.isPending) return <ShellSkeleton />;
  return <MeLoadError error={me.error} onRetry={() => void me.refetch()} />;
}
