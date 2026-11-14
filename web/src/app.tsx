// SPDX-License-Identifier: AGPL-3.0-or-later
// The app: one query client, one router and one toast queue, wired so that a 401 from any query
// or mutation made inside the signed-in area ends the session in the browser
// (docs/specs/m0/11-app-shell.md B10, B12).
import { createToastQueue } from "@stepclimb/ui";
import { QueryClientProvider } from "@tanstack/react-query";
import { type RouterHistory, RouterProvider } from "@tanstack/react-router";
import type { AppContext } from "./context";
import { createAppRouter, signedInRoute } from "./router";
import { createQueryClient, safeReturnTarget } from "./session";

export interface App {
  router: ReturnType<typeof createAppRouter>;
  context: AppContext;
}

export function createApp(history?: RouterHistory): App {
  let leaving = false;
  const queryClient = createQueryClient(() => {
    // A 401 outside the signed-in area (the not-found view asking `/me` for a signed-out
    // browser) is an answer, not an expiry.
    if (!router.state.matches.some((match) => match.routeId === signedInRoute.id)) return;
    const { pathname, searchStr } = router.state.location;
    void leaveSession(pathname + searchStr);
  });
  // Several requests can answer 401 at once; the browser leaves once. While it leaves, a query
  // the cleared cache sets off again answers 401 too, and is ignored here.
  const leaveSession = async (returnTo?: string) => {
    if (leaving) return;
    leaving = true;
    try {
      queryClient.clear();
      const search = returnTo === undefined ? {} : { return_to: safeReturnTarget(returnTo) };
      await router.navigate({ to: "/sign-in", search, replace: true });
    } finally {
      leaving = false;
    }
  };
  const context: AppContext = { queryClient, toasts: createToastQueue(), leaveSession };
  const router = createAppRouter(context, history);
  return { router, context };
}

export function AppRoot({ app }: { app: App }) {
  return (
    <QueryClientProvider client={app.context.queryClient}>
      <RouterProvider router={app.router} />
    </QueryClientProvider>
  );
}
