// SPDX-License-Identifier: AGPL-3.0-or-later
// The route tree (docs/specs/m0/11-app-shell.md B2, B7): a signed-out layout for `/sign-in` and a
// signed-in layout for everything else that exists. Any other path, the M1 deep link included,
// renders the not-found view.
import {
  createRootRouteWithContext,
  createRoute,
  createRouter,
  type RouterHistory,
} from "@tanstack/react-router";
import type { AppContext } from "./context";
import { Root } from "./layouts/root";
import { SignedInLayout } from "./layouts/signed-in";
import { SignedOutLayout } from "./layouts/signed-out";
import { HomeView } from "./routes/home";
import { NotFoundView } from "./routes/not-found";
import { SignInView } from "./routes/sign-in";
import { safeReturnTarget } from "./session";

const rootRoute = createRootRouteWithContext<AppContext>()({
  component: function RootRoute() {
    return <Root toasts={rootRoute.useRouteContext({ select: (c) => c.toasts })} />;
  },
  notFoundComponent: NotFoundView,
});

const signedOutRoute = createRoute({
  getParentRoute: () => rootRoute,
  id: "signed-out",
  component: SignedOutLayout,
});

export interface SignInSearch {
  /** Where to go after signing in: a path on this origin, checked by `safeReturnTarget` (B15). */
  return_to?: string;
}

export const signInRoute = createRoute({
  getParentRoute: () => signedOutRoute,
  path: "sign-in",
  validateSearch: (search: Record<string, unknown>): SignInSearch =>
    search.return_to === undefined ? {} : { return_to: safeReturnTarget(search.return_to) },
  component: SignInView,
});

export const signedInRoute = createRoute({
  getParentRoute: () => rootRoute,
  id: "signed-in",
  component: SignedInLayout,
});

const homeRoute = createRoute({
  getParentRoute: () => signedInRoute,
  path: "/",
  staticData: { view: { title: (t) => t("home.title"), nav: "home" } },
  component: HomeView,
});

export const routeTree = rootRoute.addChildren([
  signedOutRoute.addChildren([signInRoute]),
  signedInRoute.addChildren([homeRoute]),
]);

export function createAppRouter(context: AppContext, history?: RouterHistory) {
  return createRouter({ routeTree, context, history });
}

declare module "@tanstack/react-router" {
  interface Register {
    router: ReturnType<typeof createAppRouter>;
  }
}
