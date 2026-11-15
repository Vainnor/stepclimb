// SPDX-License-Identifier: AGPL-3.0-or-later
// The end of a sign-in (docs/specs/m0/11-app-shell.md B15): the server set the session cookie and
// answered the flow's return target. The page forgets whatever it cached while signed out, then
// opens the target, checked again by the client's own rule, in place of the sign-in step so Back
// does not return to it. A target the server answers itself, such as the OAuth 2.1 authorization
// endpoint an app's sign-in returns to (docs/specs/m0/08-sessions-and-csrf.md B31), is loaded as a
// new page, since the web app has no route for it.
import { useRouter } from "@tanstack/react-router";
import { useCallback } from "react";
import { useAppContext } from "../context";
import { safeReturnTarget } from "../session";

/** Path prefixes the server answers itself, outside the web app's routes. */
const SERVER_PATHS = ["/oauth/"];

/** Whether `target`, a checked return target, is a page the server answers itself. */
export function isServerPath(target: string): boolean {
  return SERVER_PATHS.some((prefix) => target.startsWith(prefix));
}

export function useCompleteSignIn(): (returnTo: unknown) => Promise<void> {
  const router = useRouter();
  const { queryClient } = useAppContext();
  return useCallback(
    async (returnTo: unknown) => {
      queryClient.clear();
      const target = safeReturnTarget(returnTo);
      await router.navigate({ href: target, replace: true, reloadDocument: isServerPath(target) });
    },
    [router, queryClient],
  );
}
