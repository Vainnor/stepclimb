// SPDX-License-Identifier: AGPL-3.0-or-later
// `/sign-in`, where a signed-out browser is sent with its return target
// (docs/specs/m0/11-app-shell.md B10, B15). The sign-in methods are spec 11 I5; until it lands the
// screen says the browser cannot sign in here yet.
import { Heading, Text, useStepclimbStrings } from "@stepclimb/ui";
import { useDocumentTitle } from "../layouts/document-title";

export function SignInView() {
  const { t } = useStepclimbStrings();
  const title = t("signIn.title");
  useDocumentTitle(title);
  return (
    <>
      <Heading level={1}>{title}</Heading>
      <Text elementType="p">{t("signIn.unavailable")}</Text>
    </>
  );
}
