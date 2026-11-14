// SPDX-License-Identifier: AGPL-3.0-or-later
// What every route and layout can reach through the router's context.
import type { Strings, ToastQueue } from "@stepclimb/ui";
import type { QueryClient } from "@tanstack/react-query";
import { useRouter } from "@tanstack/react-router";

export interface AppContext {
  queryClient: QueryClient;
  /** The app's one toast queue, shown by the root's ToastRegion. */
  toasts: ToastQueue;
  /**
   * Leaves the signed-in area: clears the query cache, then goes to the sign-in screen, carrying
   * `returnTo` when given (spec 11 B10, B12).
   */
  leaveSession: (returnTo?: string) => Promise<void>;
}

/** The view a signed-in route shows: its TopBar and document title, and its navigation entry. */
export interface ViewInfo {
  title: (t: Strings["t"]) => string;
  /** The `id` of the navigation entry to mark current. */
  nav?: string;
}

declare module "@tanstack/react-router" {
  interface StaticDataRouteOption {
    view?: ViewInfo;
  }
}

export function useAppContext(): AppContext {
  // The router is created with an AppContext (createAppRouter); its type is not threaded through
  // the hook without importing the route tree, which imports the layouts that call this.
  return useRouter().options.context as AppContext;
}
