// SPDX-License-Identifier: AGPL-3.0-or-later
// The not-found view (docs/specs/m0/11-app-shell.md B27): an unknown path, in the layout the user
// would see. Signed in, it is drawn inside AppShell; otherwise, or when `/me` cannot say, in the
// signed-out layout. It never sends the browser to sign in. `NotFound` is its content, which a
// signed-in route draws when its path names nothing the caller may see (a deep link,
// docs/specs/m1/03-modules-and-items.md B16).
import { ErrorState, type HeadingLevel, Link, useStepclimbStrings } from "@stepclimb/ui";
import { useQuery } from "@tanstack/react-query";
import { useLocation } from "@tanstack/react-router";
import { useDocumentTitle } from "../layouts/document-title";
import { ShellSkeleton } from "../layouts/shell-skeleton";
import { SignedInShell } from "../layouts/signed-in";
import { SignedOutFrame } from "../layouts/signed-out";
import { meQuery } from "../session";

/** What a path that names nothing shows: the path, and a link home. */
export function NotFound({ level }: { level: HeadingLevel }) {
  const { t } = useStepclimbStrings();
  const path = useLocation({ select: (location) => location.pathname });
  return (
    <ErrorState
      title={t("notFound.heading", { path })}
      level={level}
      action={<Link href="/">{t("notFound.home")}</Link>}
      className="wrap-anywhere"
    >
      {t("notFound.description")}
    </ErrorState>
  );
}

function SignedOutNotFound() {
  const { t } = useStepclimbStrings();
  useDocumentTitle(t("notFound.title"));
  return (
    <SignedOutFrame>
      <NotFound level={1} />
    </SignedOutFrame>
  );
}

export function NotFoundView() {
  const { t } = useStepclimbStrings();
  const me = useQuery(meQuery);
  if (me.isPending) return <ShellSkeleton />;
  if (me.data && !me.isError) {
    return (
      <SignedInShell me={me.data} title={t("notFound.title")}>
        <NotFound level={2} />
      </SignedInShell>
    );
  }
  return <SignedOutNotFound />;
}
