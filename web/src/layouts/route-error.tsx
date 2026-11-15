// SPDX-License-Identifier: AGPL-3.0-or-later
// What a route that failed to load draws (docs/specs/m0/11-app-shell.md B28, B29): LoadError with
// "Try again" in place of the view, inside the layout the route belongs to, so in the signed-in
// layout the SideNav, BottomTabs and TopBar stay usable. "Try again" follows TanStack's pattern for
// queries under the router: the query error boundary is reset when the error is drawn, so the next
// render fetches again, and `router.invalidate()` reruns the loaders and resets the route's error
// boundary. A route outside both layouts (the layouts themselves, the root) draws the error in the
// signed-out frame. A query's 401 is never drawn here: the query client's handler starts the
// browser's way to the sign-in screen before the view renders the failure (B10).
import { useStepclimbStrings } from "@stepclimb/ui";
import { useQueryErrorResetBoundary } from "@tanstack/react-query";
import { useRouter } from "@tanstack/react-router";
import { useEffect } from "react";
import { useDocumentTitle } from "./document-title";
import { LoadError } from "./load-error";
import { SignedOutFrame } from "./signed-out";

/** Where the failed route is drawn: in one of the two layouts, or with no layout around it. */
export type ErrorPlace = "signed-in" | "signed-out" | "bare";

function useRetry(): () => void {
  const router = useRouter();
  const { reset } = useQueryErrorResetBoundary();
  useEffect(() => reset(), [reset]);
  return () => void router.invalidate();
}

function SignedOutRouteError({ error, bare }: { error: unknown; bare: boolean }) {
  const { t } = useStepclimbStrings();
  const retry = useRetry();
  useDocumentTitle(t("loadError.documentTitle"));
  const view = <LoadError error={error} level={1} onRetry={retry} />;
  return bare ? <SignedOutFrame>{view}</SignedOutFrame> : view;
}

function SignedInRouteError({ error }: { error: unknown }) {
  const retry = useRetry();
  // The TopBar holds the view's `h1`.
  return (
    <div className="p-4 md:p-6">
      <LoadError error={error} level={2} onRetry={retry} />
    </div>
  );
}

export function RouteError({ error, place }: { error: unknown; place: ErrorPlace }) {
  if (place === "signed-in") return <SignedInRouteError error={error} />;
  return <SignedOutRouteError error={error} bare={place === "bare"} />;
}
