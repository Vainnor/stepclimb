// SPDX-License-Identifier: AGPL-3.0-or-later
// The document title, `<view> · <organization>` (docs/specs/m0/11-app-shell.md B6).
import { useStepclimbStrings } from "@stepclimb/ui";
import { useEffect } from "react";

/**
 * Sets the document title for the current view. `organization` is the organization's name: the
 * signed-in user's, or on a signed-out screen the discovery document's (spec 08 B38). Without one
 * (the document has not loaded, or a screen outside both layouts) the product name takes its place.
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
