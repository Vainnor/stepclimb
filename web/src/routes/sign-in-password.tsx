// SPDX-License-Identifier: AGPL-3.0-or-later
// `/sign-in/password`, the password step (docs/specs/m0/04-local-accounts.md B14; 11-app-shell.md
// B15 to B17). The address comes from the email step in the history entry's state; the password
// is only ever in the form and the request body, never in the URL or the history. A session ends
// the step at the return target the server answers. Without the address in the history entry (a
// link opened directly) it goes back to the email step. When the organization offers passkeys
// too, the step links to the passkey step.
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
import { type FormEvent, useState } from "react";
import { Form } from "react-aria-components";
import { api, unwrap } from "../api";
import { useDocumentTitle } from "../layouts/document-title";
import type { SignInSearch } from "../router";
import { useCompleteSignIn } from "../sign-in/complete";
import { discoveryQuery, emailMethods, useOrganizationName } from "../sign-in/discovery";
import { useLocationHere } from "../sign-in/location-here";
import { signInProblem } from "../sign-in/problems";

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
  const offersPasskey = useQuery({
    ...discoveryQuery,
    select: (document) => emailMethods(document).includes("passkey"),
  }).data;

  const submit = useMutation({
    mutationFn: (entered: string) => signIn(email, entered, search.return_to),
    onSuccess: (signedIn) => complete(signedIn.return_to),
  });

  const send = (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault();
    // One request at a time: Enter while one is in flight sends nothing more.
    if (submit.isPending || submit.isSuccess) return;
    submit.mutate(password);
  };

  const problem = submit.isError ? signInProblem(submit.error, "password", t) : undefined;
  const otherEmail = router.buildLocation({ to: "/sign-in", search }).href;
  const passkey = router.buildLocation({ to: "/sign-in/passkey", search }).href;

  return (
    <>
      <Heading level={1}>{title}</Heading>
      <Text elementType="p" className="wrap-anywhere">
        {t("signIn.password.as", { email })}
      </Text>
      {problem ? <FormError code={problem.code}>{problem.message}</FormError> : null}
      <Form onSubmit={send} className="flex flex-col gap-4">
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
