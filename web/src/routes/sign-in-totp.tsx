// SPDX-License-Identifier: AGPL-3.0-or-later
// `/sign-in/totp`, the second step after a password (docs/specs/m0/04-local-accounts.md B15, B20,
// B21; 11-app-shell.md B16, B17). The password step comes here when the server answered
// `{next: "totp"}`: the second step's flow is in the `__Host-sc_flow` cookie, and the address in
// the history entry's state is only for the page to name the account. The step sends the code
// from the authenticator app, or a recovery code, to `POST /auth/local/totp`; a session ends it at
// the return target the server answers.
//
// The server uses the flow up on a wrong code, and refuses a flow that is missing, finished or
// expired, so after `auth.totp_invalid`, `auth.flow_invalid` or `auth.flow_expired` the step goes
// back to the password step with the reason, replacing its own history entry. Any other failure
// stays here. Opened without an address (a typed URL) it goes back to the email step.
import {
  Button,
  FormError,
  Heading,
  Link,
  Text,
  TextField,
  useStepclimbStrings,
} from "@stepclimb/ui";
import { useMutation } from "@tanstack/react-query";
import { Navigate, useRouter, useSearch } from "@tanstack/react-router";
import { type FormEvent, useState } from "react";
import { Form } from "react-aria-components";
import { api, unwrap } from "../api";
import { useDocumentTitle } from "../layouts/document-title";
import type { SignInSearch } from "../router";
import { useCompleteSignIn } from "../sign-in/complete";
import { useOrganizationName } from "../sign-in/discovery";
import { useLocationHere } from "../sign-in/location-here";
import { passwordAgain, signInProblem } from "../sign-in/problems";

export function SignInTotpView() {
  const email = useLocationHere("/sign-in/totp", (location) => location.state.email);
  const search = useSearch({ from: "/signed-out/sign-in/totp" });
  if (email === undefined || email === "")
    return <Navigate to="/sign-in" search={search} replace />;
  return <TotpStep email={email} search={search} />;
}

async function sendCode(code: string) {
  return unwrap(await api.POST("/auth/local/totp", { body: { code } }));
}

function TotpStep({ email, search }: { email: string; search: SignInSearch }) {
  const { t } = useStepclimbStrings();
  const title = t("signIn.totp.title");
  useDocumentTitle(title, useOrganizationName());
  const router = useRouter();
  const complete = useCompleteSignIn();
  const [recovery, setRecovery] = useState(false);
  const [code, setCode] = useState("");
  // After a switch the new field takes focus; on arrival focus stays on the view's `h1` (B6).
  const [switched, setSwitched] = useState(false);

  const submit = useMutation({
    mutationFn: sendCode,
    onSuccess: (signedIn) => complete(signedIn.return_to),
    onError: async (error) => {
      const reason = passwordAgain(error);
      if (reason === undefined) return;
      await router.navigate({
        to: "/sign-in/password",
        search,
        state: { email, passwordAgain: reason },
        replace: true,
      });
    },
  });

  const send = (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault();
    // One request at a time: the flow is used once, so a second request could only fail.
    if (submit.isPending || submit.isSuccess) return;
    submit.mutate(code);
  };

  // Switching between the app's code and a recovery code empties the field and moves focus to it.
  const toggle = () => {
    setRecovery((value) => !value);
    setCode("");
    submit.reset();
    setSwitched(true);
  };

  const leaving = submit.isError && passwordAgain(submit.error) !== undefined;
  const problem = submit.isError && !leaving ? signInProblem(submit.error, "totp", t) : undefined;
  const startAgain = router.buildLocation({ to: "/sign-in", search }).href;

  return (
    <>
      <Heading level={1}>{title}</Heading>
      <Text elementType="p" className="wrap-anywhere">
        {t("signIn.totp.as", { email })}
      </Text>
      {problem ? <FormError code={problem.code}>{problem.message}</FormError> : null}
      <Form onSubmit={send} className="flex flex-col gap-4">
        {recovery ? (
          <TextField
            key="recovery"
            autoFocus={switched}
            label={t("signIn.totp.recovery.label")}
            description={t("signIn.totp.recovery.description")}
            name="recovery-code"
            autoComplete="off"
            isRequired
            value={code}
            onChange={setCode}
          />
        ) : (
          <TextField
            key="app"
            autoFocus={switched}
            label={t("signIn.totp.label")}
            description={t("signIn.totp.description")}
            name="one-time-code"
            autoComplete="one-time-code"
            inputMode="numeric"
            isRequired
            value={code}
            onChange={setCode}
          />
        )}
        <Button
          type="submit"
          intent="primary"
          isPending={submit.isPending || submit.isSuccess || leaving}
        >
          {t("signIn.totp.submit")}
        </Button>
      </Form>
      <Button intent="ghost" onPress={toggle} className="w-fit">
        {recovery ? t("signIn.totp.useApp") : t("signIn.totp.useRecovery")}
      </Button>
      <Link href={startAgain} className="inline-flex min-h-11 w-fit items-center">
        {t("signIn.totp.startAgain")}
      </Link>
    </>
  );
}
