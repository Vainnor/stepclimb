// SPDX-License-Identifier: AGPL-3.0-or-later
import { createRootRoute, createRoute, createRouter, Outlet } from "@tanstack/react-router";
import { HealthRoute } from "./routes/index";

const rootRoute = createRootRoute({ component: Outlet });
const indexRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/",
  component: HealthRoute,
});

export const routeTree = rootRoute.addChildren([indexRoute]);

export function createAppRouter() {
  return createRouter({ routeTree });
}

declare module "@tanstack/react-router" {
  interface Register {
    router: ReturnType<typeof createAppRouter>;
  }
}
