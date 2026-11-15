// SPDX-License-Identifier: AGPL-3.0-or-later
// `/sign-in/password`, the password step (docs/specs/m0/04-local-accounts.md B14; 11-app-shell.md
// B15 to B17). The address comes from the email step in the history entry's state; the password
// is only ever in the form and the request body, never in the URL or the history. A session ends
// the step at the return target the server answers. Without the address in the history entry (a
// link opened directly) it goes back to the email step. When the organization offers passkeys
// too, the step links to the passkey step. A right password for an account with TOTP answers
// `{next: "totp"}` and no session (04-local-accounts B15): the step goes on to `/sign-in/totp`. While
// the organization requires TOTP, an account without it answers `{next: "totp_enroll"}` with the
// secret to set up: the step keeps the secret in memory and goes on to `/sign-in/totp-setup`.
// When that step needs the password again, it comes back here with the reason in the history
// entry's state, shown once above the form and focused.
import {
  Button,
  FormError,
  Heading,
  Link,
  Text,
  TextField,
  useStepclimbStrings,
} from "@stepclimb/ui";
import { useMutation, useQuery } from "@tanstack/react-query";
import { Navigate, useRouter, useSearch } from "@tanstack/react-router";
import { type FormEvent, useEffect, useRef, useState } from "react";
import { Form } from "react-aria-components";
import { api, unwrap } from "../api";
import { useDocumentTitle } from "../layouts/document-title";
import type { SignInSearch } from "../router";
import { useCompleteSignIn } from "../sign-in/complete";
import { discoveryQuery, emailMethods, useOrganizationName } from "../sign-in/discovery";
import { useLocationHere } from "../sign-in/location-here";
import { holdSetup } from "../sign-in/pending-setup";
import { passwordAgainMessage, signInProblem } from "../sign-in/problems";

export function SignInPasswordView() {
  // While the navigation after sign-in is pending, the location's state already holds no address;
  // the step must not send the browser back to the email step then.
  const email = useLocationHere("/sign-in/password", (location) => location.state.email);
  const search = useSearch({ from: "/signed-out/sign-in/password" });
  if (email === undefined || email === "")
    return <Navigate to="/sign-in" search={search} replace />;
  return <PasswordStep key={email} email={email} search={search} />;
}

async function signIn(email: string, password: string, returnTo: string | undefined) {
  const body =
    returnTo === undefined ? { email, password } : { email, password, return_to: returnTo };
  return unwrap(await api.POST("/auth/local/password", { body }));
}

function PasswordStep({ email, search }: { email: string; search: SignInSearch }) {
  const { t } = useStepclimbStrings();
  const title = t("signIn.password.title");
  useDocumentTitle(title, useOrganizationName());
  const router = useRouter();
  const complete = useCompleteSignIn();
  const [password, setPassword] = useState("");
  const carried = useCarriedProblem();
  const offersPasskey = useQuery({
    ...discoveryQuery,
    select: (document) => emailMethods(document).includes("passkey"),
  }).data;

  const submit = useMutation({
    mutationFn: (entered: string) => signIn(email, entered, search.return_to),
    onSuccess: async (answer) => {
      if ("next" in answer && answer.next === "totp_enroll" && "secret" in answer) {
        holdSetup({ otpauthUri: answer.otpauth_uri, secret: answer.secret });
        await router.navigate({ to: "/sign-in/totp-setup", search, state: { email } });
        return;
      }
      if ("next" in answer) {
        await router.navigate({ to: "/sign-in/totp", search, state: { email } });
        return;
      }
      await complete(answer.return_to);
    },
  });

  const send = (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault();
    // One request at a time: Enter while one is in flight sends nothing more.
    if (submit.isPending || submit.isSuccess) return;
    submit.mutate(password);
  };

  const problem = submit.isError ? signInProblem(submit.error, "password", t) : undefined;
  // The TOTP step's reason stays until the next attempt.
  const shownCarried = submit.isIdle ? carried : undefined;
  const otherEmail = router.buildLocation({ to: "/sign-in", search }).href;
  const passkey = router.buildLocation({ to: "/sign-in/passkey", search }).href;

  return (
    <>
      <Heading level={1}>{title}</Heading>
      <Text elementType="p" className="wrap-anywhere">
        {t("signIn.password.as", { email })}
      </Text>
      {shownCarried ? (
        <div
          ref={shownCarried.ref}
          tabIndex={-1}
          data-slot="password-again"
          data-focus-on-arrival
          className="outline-none"
        >
          <FormError>{passwordAgainMessage(shownCarried.code, t)}</FormError>
        </div>
      ) : null}
      {problem ? <FormError code={problem.code}>{problem.message}</FormError> : null}
      <Form onSubmit={send} className="flex flex-col gap-4">
        {/* The address, hidden and read-only, so a password manager saves and fills the password
            for this account. */}
        <input
          type="email"
          name="username"
          autoComplete="username"
          value={email}
          readOnly
          hidden
          data-slot="password-username"
        />
        <TextField
          label={t("signIn.password.label")}
          type="password"
          name="password"
          autoComplete="current-password"
          isRequired
          value={password}
          onChange={setPassword}
        />
        <Button type="submit" intent="primary" isPending={submit.isPending || submit.isSuccess}>
          {t("signIn.password.submit")}
        </Button>
      </Form>
      {offersPasskey ? (
        <Link
          href={passkey}
          routerOptions={{ state: { email } }}
          className="inline-flex min-h-11 w-fit items-center"
        >
          {t("signIn.password.usePasskey")}
        </Link>
      ) : null}
      <Link href={otherEmail} className="inline-flex min-h-11 w-fit items-center">
        {t("signIn.passkey.otherEmail")}
      </Link>
    </>
  );
}

/**
 * The reason the TOTP step sent the user back, read once from the history entry's state and
 * focused once, so a screen reader reads it before the form.
 */
function useCarriedProblem() {
  const code = useLocationHere("/sign-in/password", (location) => location.state.passwordAgain);
  const [kept] = useState(code);
  const ref = useRef<HTMLDivElement>(null);
  useEffect(() => {
    if (kept !== undefined) ref.current?.focus();
  }, [kept]);
  return kept === undefined ? undefined : { code: kept, ref };
}
