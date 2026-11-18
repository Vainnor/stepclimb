// SPDX-License-Identifier: AGPL-3.0-or-later
// The root of the route tree (docs/specs/m0/11-app-shell.md B1, B6): React Aria's RouterProvider
// sends every Stepclimb UI link through TanStack Router; after a client-side navigation to another
// path, focus moves to the new view's `h1`, or to the failure the view opened on; the toast region and the service worker's update offer sit once at the root. A link with
// `routerOptions={{ reloadDocument: true }}` leaves the app instead and loads its address as a page.
// An absolute or protocol-relative `href` is no route: it keeps its address, and React Aria lets
// the browser follow a link to another origin.
import { type ToastQueue, ToastRegion } from "@stepclimb/ui";
import { Outlet, useRouter } from "@tanstack/react-router";
import { useEffect } from "react";
import { RouterProvider } from "react-aria-components";
import { openDocument } from "../document-navigation";
import { AppUpdate } from "./app-update";

/** True when `href` names its scheme (`https:`, `mailto:`) or starts with `//`. */
function isAbsoluteHref(href: string): boolean {
  return /^(?:[a-z][a-z\d+.-]*:|\/\/)/i.test(href);
}

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

/**
 * After a navigation: the failure a view opened on when it marks one with
 * `data-focus-on-arrival` (a sign-in step that sent the user back, with the reason), so a screen
 * reader reads why before the form; otherwise the view's `h1`.
 */
function focusArrival(): void {
  const reason = document.querySelector<HTMLElement>("main [data-focus-on-arrival]");
  if (reason) reason.focus();
  else focusViewHeading();
}

export function Root({ toasts }: { toasts: ToastQueue }) {
  const router = useRouter();
  // The first render is a page load, which the browser announces itself: only a later change of
  // path moves focus. A change of search or hash alone (a filter, an anchor) leaves it.
  useEffect(
    () =>
      router.subscribe("onRendered", (event) => {
        if (event.fromLocation !== undefined && event.pathChanged) focusArrival();
      }),
    [router],
  );
  return (
    <RouterProvider
      navigate={(to, options) =>
        options?.reloadDocument ? openDocument(to) : router.navigate({ to, state: options?.state })
      }
      useHref={(to) => (isAbsoluteHref(to) ? to : router.buildLocation({ to }).href)}
    >
      <Outlet />
      <ToastRegion queue={toasts} />
      <AppUpdate toasts={toasts} />
    </RouterProvider>
  );
}
