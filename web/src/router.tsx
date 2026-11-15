// SPDX-License-Identifier: AGPL-3.0-or-later
// The route tree (docs/specs/m0/11-app-shell.md B2, B7, B16): a signed-out layout for the sign-in
// steps and the enrollment page, and a signed-in layout for everything else that exists. Any other path, the M1 deep link included,
// renders the not-found view. A route that fails to load renders its error inside its layout (B28).
import {
  createRootRouteWithContext,
  createRoute,
  createRouter,
  type ErrorComponentProps,
  type HistoryState,
  type RouterHistory,
  useMatch,
} from "@tanstack/react-router";
import type { AppContext } from "./context";
import { Root } from "./layouts/root";
import { type ErrorPlace, RouteError } from "./layouts/route-error";
import { SignedInLayout } from "./layouts/signed-in";
import { SignedOutLayout } from "./layouts/signed-out";
import { AccountView } from "./routes/account";
import { EnrollView } from "./routes/enroll";
import { HomeView } from "./routes/home";
import { NotFoundView } from "./routes/not-found";
import { SignInView } from "./routes/sign-in";
import { SignInPasskeyView } from "./routes/sign-in-passkey";
import { SignInPasswordView } from "./routes/sign-in-password";
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

export interface MethodListSearch extends SignInSearch {
  /** The code a failed browser flow sent the browser back with (B18); shown, then removed. */
  error?: string;
}

function validateMethodListSearch(search: Record<string, unknown>): MethodListSearch {
  const kept = validateSignInSearch(search);
  return typeof search.error === "string" ? { ...kept, error: search.error } : kept;
}

export const signInRoute = createRoute({
  getParentRoute: () => signedOutRoute,
  path: "sign-in",
  validateSearch: validateMethodListSearch,
  component: SignInView,
});

const signInPasskeyRoute = createRoute({
  getParentRoute: () => signedOutRoute,
  path: "sign-in/passkey",
  validateSearch: validateSignInSearch,
  component: SignInPasskeyView,
});

const signInPasswordRoute = createRoute({
  getParentRoute: () => signedOutRoute,
  path: "sign-in/password",
  validateSearch: validateSignInSearch,
  component: SignInPasswordView,
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

const accountRoute = createRoute({
  getParentRoute: () => signedInRoute,
  path: "account",
  staticData: { view: { title: (t) => t("account.title"), nav: "account" } },
  component: AccountView,
});

export const routeTree = rootRoute.addChildren([
  signedOutRoute.addChildren([signInRoute, signInPasskeyRoute, signInPasswordRoute, enrollRoute]),
  signedInRoute.addChildren([homeRoute, accountRoute]),
]);

/** The layout a route sits in: a route under one draws its error inside it. */
function errorPlace(routeId: string | undefined): ErrorPlace {
  if (routeId === undefined) return "bare";
  if (routeId.startsWith(`${signedInRoute.id}/`)) return "signed-in";
  if (routeId.startsWith(`${signedOutRoute.id}/`)) return "signed-out";
  return "bare";
}

/** Every route's error component: the error of the nearest route that failed, in its place. */
function RouteErrorInLayout({ error }: ErrorComponentProps) {
  const routeId = useMatch({ strict: false, select: (match) => match.routeId });
  return <RouteError error={error} place={errorPlace(routeId)} />;
}

export function createAppRouter(context: AppContext, history?: RouterHistory) {
  return createRouter({ routeTree, context, history, defaultErrorComponent: RouteErrorInLayout });
}

declare module "@tanstack/react-router" {
  interface Register {
    router: ReturnType<typeof createAppRouter>;
  }
  interface HistoryState {
    /** The address the email step hands to the passkey or password step (routes/sign-in.tsx). */
    email?: string;
    /** The enrollment link's token, moved out of the fragment (routes/enroll.tsx). */
    enrollToken?: string;
  }
}

declare module "react-aria-components" {
  interface RouterConfig {
    /**
     * `reloadDocument` opens the link as a new page instead of a client-side navigation: a
     * server route such as a provider's sign-in start (spec 11 B14). `state` is the new history
     * entry's, such as the address a sign-in step hands to the next.
     */
    routerOptions: { reloadDocument?: boolean; state?: HistoryState };
  }
}
