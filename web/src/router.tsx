// SPDX-License-Identifier: AGPL-3.0-or-later
// The route tree (docs/specs/m0/11-app-shell.md B2, B7, B16): a signed-out layout for the sign-in
// steps and the enrollment page, and a signed-in layout for everything else that exists. Any other path, the M1 deep link included,
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
import { EnrollView } from "./routes/enroll";
import { HomeView } from "./routes/home";
import { NotFoundView } from "./routes/not-found";
import { SignInView } from "./routes/sign-in";
import { SignInPasskeyView } from "./routes/sign-in-passkey";
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

function validateSignInSearch(search: Record<string, unknown>): SignInSearch {
  return search.return_to === undefined ? {} : { return_to: safeReturnTarget(search.return_to) };
}

export const signInRoute = createRoute({
  getParentRoute: () => signedOutRoute,
  path: "sign-in",
  validateSearch: validateSignInSearch,
  component: SignInView,
});

const signInPasskeyRoute = createRoute({
  getParentRoute: () => signedOutRoute,
  path: "sign-in/passkey",
  validateSearch: validateSignInSearch,
  component: SignInPasskeyView,
});

const enrollRoute = createRoute({
  getParentRoute: () => signedOutRoute,
  path: "enroll",
  component: EnrollView,
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
  signedOutRoute.addChildren([signInRoute, signInPasskeyRoute, enrollRoute]),
  signedInRoute.addChildren([homeRoute]),
]);

export function createAppRouter(context: AppContext, history?: RouterHistory) {
  return createRouter({ routeTree, context, history });
}

declare module "@tanstack/react-router" {
  interface Register {
    router: ReturnType<typeof createAppRouter>;
  }
  interface HistoryState {
    /** The address the email step hands to the passkey step (routes/sign-in.tsx). */
    email?: string;
  }
}
