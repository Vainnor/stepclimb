// SPDX-License-Identifier: AGPL-3.0-or-later
// `/sign-in/totp-setup`, TOTP set up during sign-in (docs/specs/m0/04-local-accounts.md B15, Q11 (a);
// 11-app-shell.md B16, B17). While the organization requires TOTP, a right password for an account
// without it answers `{next: "totp_enroll", otpauth_uri, secret}` and no session: the password step
// keeps the secret in memory (`sign-in/pending-setup`) and comes here with the address in the
// history entry's state. The step shows the setup key and the setup link as text, and sends the
// app's code to `POST /auth/local/totp/enrollment`. The answer signs the user in and carries the
// ten recovery codes, shown once on a second screen in the same view; Continue opens the return
// target the server answered.
//
// A wrong code leaves the set-up waiting on the server, so it stays here with its message. A
// missing, finished or expired flow (`auth.flow_invalid`, `auth.flow_expired`) needs the password
// again, as does arriving without a secret (a reload): the step replaces its history entry with
// the password step, carrying the reason. Opened without an address it goes back to the email step.
import {
  Button,
  Code,
  FormError,
  Heading,
  Link,
  Text,
  TextField,
  useStepclimbStrings,
} from "@stepclimb/ui";
import { useMutation } from "@tanstack/react-query";
import { Navigate, useRouter, useSearch } from "@tanstack/react-router";
import { type FormEvent, useEffect, useState } from "react";
import { Form } from "react-aria-components";
import { ApiError, api, unwrap } from "../api";
import { useDocumentTitle } from "../layouts/document-title";
import { focusViewHeading } from "../layouts/root";
import type { SignInSearch } from "../router";
import { useCompleteSignIn } from "../sign-in/complete";
import { useOrganizationName } from "../sign-in/discovery";
import { useLocationHere } from "../sign-in/location-here";
import { forgetSetup, heldSetup, type PendingSetup } from "../sign-in/pending-setup";
import { passwordAgain, signInProblem } from "../sign-in/problems";

export function SignInTotpSetupView() {
  const email = useLocationHere("/sign-in/totp-setup", (location) => location.state.email);
  const search = useSearch({ from: "/signed-out/sign-in/totp-setup" });
  // Read once: the secret stays with this view while it is shown, and nowhere else.
  const [setup] = useState(heldSetup);
  useEffect(forgetSetup, []);
  if (email === undefined || email === "")
    return <Navigate to="/sign-in" search={search} replace />;
  if (setup === undefined) {
    return (
      <Navigate
        to="/sign-in/password"
        search={search}
        state={{ email, passwordAgain: "auth.flow_invalid" }}
        replace
      />
    );
  }
  return <SetUp email={email} search={search} setup={setup} />;
}

async function sendCode(code: string) {
  return unwrap(await api.POST("/auth/local/totp/enrollment", { body: { code } }));
}

type Enrolled = Awaited<ReturnType<typeof sendCode>>;

interface SetUpProps {
  email: string;
  search: SignInSearch;
  setup: PendingSetup;
}

function SetUp({ email, search, setup }: SetUpProps) {
  const [enrolled, setEnrolled] = useState<Enrolled>();
  if (enrolled !== undefined) return <RecoveryCodes enrolled={enrolled} />;
  return <CodeForm email={email} search={search} setup={setup} onEnrolled={setEnrolled} />;
}

function CodeForm({
  email,
  search,
  setup,
  onEnrolled,
}: SetUpProps & { onEnrolled: (e: Enrolled) => void }) {
  const { t } = useStepclimbStrings();
  const title = t("signIn.totpSetup.title");
  useDocumentTitle(title, useOrganizationName());
  const router = useRouter();
  const [code, setCode] = useState("");

  const submit = useMutation({
    mutationFn: sendCode,
    onSuccess: onEnrolled,
    onError: async (error) => {
      const reason = setupAgain(error);
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
    // One request at a time: the set-up signs in once.
    if (submit.isPending || submit.isSuccess) return;
    submit.mutate(code);
  };

  const leaving = submit.isError && setupAgain(submit.error) !== undefined;
  const problem = submit.isError && !leaving ? setupProblem(submit.error, t) : undefined;
  const startAgain = router.buildLocation({ to: "/sign-in", search }).href;

  return (
    <>
      <Heading level={1}>{title}</Heading>
      <Text elementType="p" className="wrap-anywhere">
        {t("signIn.totpSetup.intro", { email })}
      </Text>
      <div data-slot="totp-setup-key" className="flex flex-col gap-1">
        <Text elementType="p">{t("signIn.totpSetup.key")}</Text>
        <Code>{setup.secret}</Code>
      </div>
      <div data-slot="totp-setup-link" className="flex flex-col gap-1">
        <Text elementType="p" tone="sub">
          {t("signIn.totpSetup.link")}
        </Text>
        <Code>{setup.otpauthUri}</Code>
      </div>
      {problem ? <FormError code={problem.code}>{problem.message}</FormError> : null}
      <Form onSubmit={send} className="flex flex-col gap-4">
        <TextField
          label={t("signIn.totpSetup.code.label")}
          description={t("signIn.totpSetup.code.description")}
          name="one-time-code"
          autoComplete="one-time-code"
          inputMode="numeric"
          isRequired
          value={code}
          onChange={setCode}
        />
        <Button
          type="submit"
          intent="primary"
          isPending={submit.isPending || submit.isSuccess || leaving}
        >
          {t("signIn.totpSetup.submit")}
        </Button>
      </Form>
      <Link href={startAgain} className="inline-flex min-h-11 w-fit items-center">
        {t("signIn.totp.startAgain")}
      </Link>
    </>
  );
}

/** The set-up's codes that need the password again: the flow is gone or expired. */
function setupAgain(error: unknown): string | undefined {
  const reason = passwordAgain(error);
  return reason === "auth.totp_invalid" ? undefined : reason;
}

/** What the set-up step says about `error`; a wrong code has a message of its own. */
function setupProblem(error: unknown, t: ReturnType<typeof useStepclimbStrings>["t"]) {
  if (error instanceof ApiError && error.problem?.code === "auth.totp_invalid") {
    return { message: t("signIn.totpSetup.problem.invalid") };
  }
  return signInProblem(error, "totp", t);
}

/** The recovery codes, shown once; the user is already signed in. */
function RecoveryCodes({ enrolled }: { enrolled: Enrolled }) {
  const { t } = useStepclimbStrings();
  const title = t("signIn.totpSetup.recovery.title");
  useDocumentTitle(title, useOrganizationName());
  const complete = useCompleteSignIn();
  const [leaving, setLeaving] = useState(false);
  // The view changed in place: move focus to its new `h1`, as a navigation would (B6).
  useEffect(focusViewHeading, []);
  const finish = () => {
    setLeaving(true);
    void complete(enrolled.return_to);
  };
  return (
    <>
      <Heading level={1}>{title}</Heading>
      <Text elementType="p">{t("signIn.totpSetup.recovery.intro")}</Text>
      <ul data-slot="recovery-codes" className="grid gap-x-6 gap-y-2 sm:grid-cols-2">
        {enrolled.recovery_codes.map((code) => (
          <li key={code}>
            <Code>{code}</Code>
          </li>
        ))}
      </ul>
      <Button intent="primary" onPress={finish} isPending={leaving} className="w-fit">
        {t("signIn.totpSetup.recovery.continue")}
      </Button>
    </>
  );
}
