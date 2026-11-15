// SPDX-License-Identifier: AGPL-3.0-or-later
// The end of a sign-in (docs/specs/m0/11-app-shell.md B15): the server set the session cookie and
// answered the flow's return target. The page forgets whatever it cached while signed out, then
// opens the target, checked again by the client's own rule, in place of the sign-in step so Back
// does not return to it.
import { useRouter } from "@tanstack/react-router";
import { useCallback } from "react";
import { useAppContext } from "../context";
import { safeReturnTarget } from "../session";

export function useCompleteSignIn(): (returnTo: unknown) => Promise<void> {
  const router = useRouter();
  const { queryClient } = useAppContext();
  return useCallback(
    async (returnTo: unknown) => {
      queryClient.clear();
      await router.navigate({ href: safeReturnTarget(returnTo), replace: true });
    },
    [router, queryClient],
  );
}
