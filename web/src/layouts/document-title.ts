// SPDX-License-Identifier: AGPL-3.0-or-later
// The document title, `<view> · <organization>` (docs/specs/m0/11-app-shell.md B6).
import { useStepclimbStrings } from "@stepclimb/ui";
import { useEffect } from "react";

/**
 * Sets the document title for the current view. `organization` is the signed-in user's
 * organization; signed-out screens pass none and get the product name in its place, until
 * `/.well-known/stepclimb` (spec 08 I9) gives them the organization's name.
 */
export function useDocumentTitle(view: string, organization?: string): void {
  const { t } = useStepclimbStrings();
  const title =
    organization === undefined
      ? t("document.titleSignedOut", { view })
      : t("document.title", { view, organization });
  useEffect(() => {
    document.title = title;
  }, [title]);
}
