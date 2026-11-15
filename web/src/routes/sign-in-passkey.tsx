// SPDX-License-Identifier: AGPL-3.0-or-later
// `/sign-in/passkey`, the passkey step (docs/specs/m0/04-local-accounts.md B11, B12;
// 11-app-shell.md B15 to B17). On arrival it asks the server for the request options, so pressing
// the button goes straight to the browser's passkey prompt; the answer goes to the finish route,
// and a session ends the step at the flow's return target. When the server has deleted the flow
// (expired, used, or the passkey refused), the step asks for new options. Without the address in
// the history entry (a link opened directly) it goes back to the email step.
import { Button, FormError, Heading, Link, Text, useStepclimbStrings } from "@stepclimb/ui";
import { useMutation, useQuery } from "@tanstack/react-query";
import { Navigate, useRouter, useSearch } from "@tanstack/react-router";
import { api, unwrap } from "../api";
import { useDocumentTitle } from "../layouts/document-title";
import type { SignInSearch } from "../router";
import { useCompleteSignIn } from "../sign-in/complete";
import { discoveryQuery, emailMethods, useOrganizationName } from "../sign-in/discovery";
import { useLocationHere } from "../sign-in/location-here";
import { getPasskey, PasskeyError, passkeyUnavailable } from "../sign-in/passkey";
import { needsNewCeremony, signInProblem } from "../sign-in/problems";

export function SignInPasskeyView() {
  // While the navigation after sign-in is pending, the location's state already holds no address;
  // the step must not send the browser back to the email step then.
  const email = useLocationHere("/sign-in/passkey", (location) => location.state.email);
  const search = useSearch({ from: "/signed-out/sign-in/passkey" });
  if (email === undefined || email === "")
    return <Navigate to="/sign-in" search={search} replace />;
  return <PasskeyStep key={email} email={email} search={search} />;
}

async function startSignIn(email: string, returnTo: string | undefined) {
  const body = returnTo === undefined ? { email } : { email, return_to: returnTo };
  return unwrap(await api.POST("/auth/local/passkey/start", { body })).options;
}

function PasskeyStep({ email, search }: { email: string; search: SignInSearch }) {
  const { t } = useStepclimbStrings();
  const title = t("signIn.passkey.title");
  useDocumentTitle(title, useOrganizationName());
  const router = useRouter();
  const offersPassword = useQuery({
    ...discoveryQuery,
    select: (document) => emailMethods(document).includes("password"),
  }).data;
  const complete = useCompleteSignIn();
  const unavailable = passkeyUnavailable();
  const supported = unavailable === undefined;

  // A POST that starts a flow: fetched once per visit, never again on focus or reconnect, and
  // dropped when the step is left, so a later visit starts a new flow.
  const start = useQuery({
    queryKey: ["passkey-sign-in", email, search.return_to ?? null],
    queryFn: () => startSignIn(email, search.return_to),
    enabled: supported,
    staleTime: Number.POSITIVE_INFINITY,
    gcTime: 0,
    refetchOnWindowFocus: false,
    refetchOnReconnect: false,
  });

  const finish = useMutation({
    mutationFn: async (options: Awaited<ReturnType<typeof startSignIn>>) => {
      const credential = await getPasskey(options);
      return unwrap(await api.POST("/auth/local/passkey/finish", { body: { credential } }));
    },
    onSuccess: (signedIn) => complete(signedIn.return_to),
    onError: (error) => {
      if (needsNewCeremony(error)) void start.refetch();
    },
  });

  const signIn = () => {
    finish.reset();
    if (start.data) finish.mutate(start.data);
    else void start.refetch();
  };

  const failure = unavailable
    ? new PasskeyError(unavailable)
    : (finish.error ?? (start.isError ? start.error : null));
  const problem = failure ? signInProblem(failure, "signIn", t) : undefined;
  const otherEmail = router.buildLocation({ to: "/sign-in", search }).href;
  const password = router.buildLocation({ to: "/sign-in/password", search }).href;

  return (
    <>
      <Heading level={1}>{title}</Heading>
      <Text elementType="p" className="wrap-anywhere">
        {t("signIn.passkey.as", { email })}
      </Text>
      {problem ? <FormError code={problem.code}>{problem.message}</FormError> : null}
      {supported ? (
        <Button
          intent="primary"
          onPress={signIn}
          isPending={finish.isPending || start.isFetching || finish.isSuccess}
        >
          {t("signIn.passkey.submit")}
        </Button>
      ) : null}
      {offersPassword ? (
        <Link
          href={password}
          routerOptions={{ state: { email } }}
          className="inline-flex min-h-11 w-fit items-center"
        >
          {t("signIn.passkey.usePassword")}
        </Link>
      ) : null}
      <Link href={otherEmail} className="inline-flex min-h-11 w-fit items-center">
        {t("signIn.passkey.otherEmail")}
      </Link>
    </>
  );
}
