// SPDX-License-Identifier: AGPL-3.0-or-later
// The root of the route tree (docs/specs/m0/11-app-shell.md B1, B6): React Aria's RouterProvider
// sends every Stepclimb UI link through TanStack Router; after a client-side navigation to another
// path, focus moves to the new view's `h1`; the toast region sits once at the root.
import { type ToastQueue, ToastRegion } from "@stepclimb/ui";
import { Outlet, useRouter } from "@tanstack/react-router";
import { useEffect } from "react";
import { RouterProvider } from "react-aria-components";

/**
 * Focuses the page's `h1`, so a screen reader announces the new view. The heading becomes
 * focusable by script only (`tabindex="-1"`), never a Tab stop.
 */
export function focusViewHeading(): void {
  const heading = document.querySelector<HTMLElement>("h1");
  if (!heading) return;
  if (!heading.hasAttribute("tabindex")) heading.setAttribute("tabindex", "-1");
  heading.focus();
}

export function Root({ toasts }: { toasts: ToastQueue }) {
  const router = useRouter();
  // The first render is a page load, which the browser announces itself: only a later change of
  // path moves focus. A change of search or hash alone (a filter, an anchor) leaves it.
  useEffect(
    () =>
      router.subscribe("onRendered", (event) => {
        if (event.fromLocation !== undefined && event.pathChanged) focusViewHeading();
      }),
    [router],
  );
  return (
    <RouterProvider
      navigate={(to) => router.navigate({ to })}
      useHref={(to) => router.buildLocation({ to }).href}
    >
      <Outlet />
      <ToastRegion queue={toasts} />
    </RouterProvider>
  );
}
