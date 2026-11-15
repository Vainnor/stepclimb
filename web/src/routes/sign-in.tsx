// SPDX-License-Identifier: AGPL-3.0-or-later
// `/sign-in`, the method list and the email step (docs/specs/m0/11-app-shell.md B13 to B18). The
// organization's discovery document says what it offers: local sign-in draws the email field, which
// leads to the passkey step when passkeys are offered and to the password step otherwise; each
// outside provider draws a button that opens its start route as a full page. Nothing else is drawn.
// The address travels to the next step in the history entry's state, never in the URL, so it stays
// out of server logs and a refresh of that step still has it; this step's own entry keeps it too,
// so Back finds the field filled in. The return target rides along in the search.
//
// A browser flow that failed lands here with `?error=<code>` (B18): the screen shows that code's
// message, moves focus to it once, and removes the parameter from the address bar, so a reload or
// a shared link does not show it again.
import {
  Button,
  ButtonLink,
  EmptyState,
  FormError,
  Heading,
  Separator,
  Skeleton,
  TextField,
  useStepclimbStrings,
} from "@stepclimb/ui";
import { useQuery } from "@tanstack/react-query";
import { useLocation, useNavigate, useSearch } from "@tanstack/react-router";
import { type FormEvent, useEffect, useRef, useState } from "react";
import { Form } from "react-aria-components";
import { useDocumentTitle } from "../layouts/document-title";
import { LoadError } from "../layouts/load-error";
import {
  type Discovery,
  discoveryQuery,
  emailMethods,
  type LocalMethod,
  providerStartHref,
  providers,
  type SignInMethod,
  useOrganizationName,
} from "../sign-in/discovery";
import { navigationProblem } from "../sign-in/problems";

export function SignInView() {
  const { t } = useStepclimbStrings();
  const title = t("signIn.title");
  useDocumentTitle(title, useOrganizationName());
  const discovery = useQuery(discoveryQuery);
  const callbackError = useCallbackError();
  // The page opens on the failure, so a screen reader reads it first: focus moves to it once.
  const errorBox = useRef<HTMLDivElement>(null);
  const hasCallbackError = callbackError !== undefined;
  useEffect(() => {
    if (hasCallbackError) errorBox.current?.focus();
  }, [hasCallbackError]);

  return (
    <>
      {discovery.isError ? null : <Heading level={1}>{title}</Heading>}
      {callbackError ? (
        <div ref={errorBox} tabIndex={-1} data-slot="callback-error" className="outline-none">
          <FormError code={callbackError.code}>{callbackError.message}</FormError>
        </div>
      ) : null}
      {discovery.isPending ? <MethodsSkeleton /> : null}
      {discovery.isError ? (
        <LoadError error={discovery.error} level={1} onRetry={() => void discovery.refetch()} />
      ) : null}
      {discovery.isSuccess ? <Methods document={discovery.data} /> : null}
    </>
  );
}

/**
 * The problem the page was opened with, read once from `?error=`, and the parameter's removal from
 * the address bar. The message stays on screen until the user leaves the step.
 */
function useCallbackError() {
  const { t } = useStepclimbStrings();
  const search = useSearch({ from: "/signed-out/sign-in" });
  const navigate = useNavigate();
  const [problem] = useState(() =>
    search.error === undefined ? undefined : navigationProblem(search.error, t),
  );
  const { error, return_to: returnTo } = search;
  useEffect(() => {
    if (error === undefined) return;
    const kept = returnTo === undefined ? {} : { return_to: returnTo };
    void navigate({ to: "/sign-in", search: kept, replace: true });
  }, [error, returnTo, navigate]);
  return problem;
}

function Methods({ document }: { document: Discovery }) {
  const { t } = useStepclimbStrings();
  const local = emailMethods(document);
  const outside = providers(document);
  if (local.length === 0 && outside.length === 0) {
    return <EmptyState>{t("signIn.empty")}</EmptyState>;
  }
  return (
    <>
      {local.length > 0 ? <EmailStep methods={local} /> : null}
      {local.length > 0 && outside.length > 0 ? <Separator /> : null}
      {outside.length > 0 ? (
        // The view's one primary action: the email step's Continue, or a provider's button when it
        // is the only way in.
        <ProviderButtons providers={outside} primary={local.length === 0 && outside.length === 1} />
      ) : null}
    </>
  );
}

function ProviderButtons({ providers, primary }: { providers: SignInMethod[]; primary: boolean }) {
  const { t } = useStepclimbStrings();
  const search = useSearch({ from: "/signed-out/sign-in" });
  return (
    <ul className="flex flex-col gap-3">
      {providers.map((provider) => (
        <li key={provider.key} className="flex flex-col">
          <ButtonLink
            href={providerStartHref(provider.key, search.return_to)}
            routerOptions={{ reloadDocument: true }}
            intent={primary ? "primary" : "secondary"}
          >
            {provider.label ?? t("signIn.provider.unlabeled", { key: provider.key })}
          </ButtonLink>
        </li>
      ))}
    </ul>
  );
}

function EmailStep({ methods }: { methods: LocalMethod[] }) {
  const { t } = useStepclimbStrings();
  const search = useSearch({ from: "/signed-out/sign-in" });
  const remembered = useLocation({ select: (location) => location.state.email });
  const navigate = useNavigate();
  const [email, setEmail] = useState(remembered ?? "");
  // Passkey first (04-local-accounts Q9); the password step when passkeys are not offered.
  const next = methods.includes("passkey") ? "/sign-in/passkey" : "/sign-in/password";

  const submit = async (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault();
    const state = { email: email.trim() };
    const { return_to } = search;
    const kept = return_to === undefined ? {} : { return_to };
    await navigate({ to: "/sign-in", search: kept, state, replace: true });
    await navigate({ to: next, search: kept, state });
  };

  return (
    <Form onSubmit={(event) => void submit(event)} className="flex flex-col gap-4">
      <TextField
        label={t("signIn.email.label")}
        type="email"
        name="email"
        autoComplete="username"
        inputMode="email"
        isRequired
        value={email}
        onChange={setEmail}
      />
      <Button type="submit" intent="primary">
        {t("signIn.continue")}
      </Button>
    </Form>
  );
}

function MethodsSkeleton() {
  return (
    <div aria-busy="true" className="flex flex-col gap-3">
      <Skeleton className="h-5 max-w-24" />
      <Skeleton shape="block" className="h-11" />
      <Skeleton shape="block" className="h-11" />
    </div>
  );
}
