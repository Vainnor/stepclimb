// SPDX-License-Identifier: AGPL-3.0-or-later
// `/enroll`, where an enrollment link opens (docs/specs/m0/04-local-accounts.md B7, B8, Q6, Q7):
// `https://<host>/enroll#token=<token>`. The token is read from the fragment, which a browser never
// sends to a server, then moved into the history entry's state and the fragment removed, so the
// address bar, a screenshot or a shared screen no longer shows it while a reload still finds it. It
// goes only into the body of the start and passkey requests. The start
// answer names the account and the methods the link can set up; registering a passkey signs the
// user in and opens the return target in place of this page, so the link's entry leaves the
// history. A link that is used, replaced, unknown or expired can never work again: the page says
// so, offers no retry, and links to the sign-in screen.
import {
  Button,
  Code,
  ErrorState,
  FormError,
  Heading,
  Link,
  Skeleton,
  type Strings,
  Text,
  useStepclimbStrings,
} from "@stepclimb/ui";
import { useMutation, useQuery } from "@tanstack/react-query";
import { useNavigate } from "@tanstack/react-router";
import { useEffect } from "react";
import { ApiError, api, unwrap } from "../api";
import { useDocumentTitle } from "../layouts/document-title";
import { useCompleteSignIn } from "../sign-in/complete";
import { useLocationHere } from "../sign-in/location-here";
import { createPasskey, PasskeyError, passkeyUnavailable } from "../sign-in/passkey";
import { isDeadLink, needsNewCeremony, signInProblem } from "../sign-in/problems";

/** The token in a fragment `token=<token>`, or undefined when there is none. */
export function enrollmentToken(hash: string): string | undefined {
  const token = new URLSearchParams(hash.startsWith("#") ? hash.slice(1) : hash).get("token");
  return token === null || token === "" ? undefined : token;
}

/**
 * The link's token: from the fragment the first time, which this then moves into the history
 * entry's state (replacing the entry, without the fragment); from that state after, so a reload
 * keeps it. A new link pasted into the same tab changes only the fragment, and wins.
 */
function useEnrollmentToken(): string | undefined {
  const navigate = useNavigate();
  const fromHash = useLocationHere("/enroll", (location) => enrollmentToken(location.hash));
  const fromState = useLocationHere("/enroll", (location) => location.state.enrollToken);
  useEffect(() => {
    if (fromHash === undefined) return;
    void navigate({ to: "/enroll", hash: "", state: { enrollToken: fromHash }, replace: true });
  }, [fromHash, navigate]);
  return fromHash ?? fromState;
}

export function EnrollView() {
  const { t } = useStepclimbStrings();
  useDocumentTitle(t("enroll.title"));
  const token = useEnrollmentToken();
  if (token === undefined) {
    return (
      <ErrorState title={t("enroll.noToken.title")} level={1}>
        {t("enroll.noToken")}
      </ErrorState>
    );
  }
  return <Enrollment key={token} token={token} />;
}

async function startEnrollment(token: string) {
  return unwrap(await api.POST("/auth/local/enrollment/start", { body: { token } }));
}

type Started = Awaited<ReturnType<typeof startEnrollment>>;

/** The heading of the page that replaces the form when the link cannot be opened. */
function failureTitle(error: unknown, t: Strings["t"]): string {
  const code = error instanceof ApiError ? error.problem?.code : undefined;
  if (code === "auth.enrollment_invalid") return t("enroll.problem.invalidTitle");
  if (code === "auth.enrollment_expired") return t("enroll.problem.expiredTitle");
  return t("enroll.problem.title");
}

/** Errors after which trying again cannot help: a dead link, or a method the organization turned off. */
function isFinal(error: unknown): boolean {
  return (
    isDeadLink(error) ||
    (error instanceof ApiError && error.problem?.code === "auth.method_disabled")
  );
}

/** A dead link can only be replaced: it points to signing in instead. Other errors may retry. */
function failureAction(error: unknown, t: Strings["t"], onRetry?: () => void) {
  if (isDeadLink(error)) return <Link href="/sign-in">{t("enroll.problem.signIn")}</Link>;
  if (isFinal(error) || onRetry === undefined) return undefined;
  return <Button onPress={onRetry}>{t("loadError.retry")}</Button>;
}

function LinkFailure({ error, onRetry }: { error: unknown; onRetry?: () => void }) {
  const { t } = useStepclimbStrings();
  const problem = signInProblem(error, "enroll", t);
  return (
    <ErrorState title={failureTitle(error, t)} level={1} action={failureAction(error, t, onRetry)}>
      {problem.message}
      {problem.code === undefined ? null : (
        <>
          {" "}
          <Code>{problem.code}</Code>
        </>
      )}
    </ErrorState>
  );
}

function Enrollment({ token }: { token: string }) {
  const { t } = useStepclimbStrings();
  const complete = useCompleteSignIn();
  // A POST that starts the registration's flow: fetched once per visit, never again on focus or
  // reconnect, and dropped when the page is left.
  const start = useQuery({
    queryKey: ["enrollment", token],
    queryFn: () => startEnrollment(token),
    staleTime: Number.POSITIVE_INFINITY,
    gcTime: 0,
    refetchOnWindowFocus: false,
    refetchOnReconnect: false,
  });
  const register = useMutation({
    mutationFn: async (options: NonNullable<Started["passkey_options"]>) => {
      const credential = await createPasskey(options);
      return unwrap(
        await api.POST("/auth/local/enrollment/passkey", { body: { token, credential } }),
      );
    },
    onSuccess: (signedIn) => complete(signedIn.return_to),
    onError: (error) => {
      if (needsNewCeremony(error)) void start.refetch();
    },
  });

  if (start.isPending) return <EnrollmentSkeleton />;
  // Retrying the start clears the registration's error too, or it would show again beside the
  // new options.
  const retry = () => {
    register.reset();
    void start.refetch();
  };
  if (start.isError) return <LinkFailure error={start.error} onRetry={retry} />;
  if (isDeadLink(register.error)) return <LinkFailure error={register.error} />;

  const options = start.data.methods.includes("passkey") ? start.data.passkey_options : undefined;
  const unavailable = passkeyUnavailable();
  const failure = unavailable ? new PasskeyError(unavailable) : register.error;
  const create = () => {
    register.reset();
    if (options) register.mutate(options);
  };

  return (
    <>
      <Heading level={1}>{t("enroll.title")}</Heading>
      <Text elementType="p" className="wrap-anywhere">
        {t("enroll.as", { email: start.data.email })}
      </Text>
      {options === undefined ? (
        <Text elementType="p">{t("enroll.noBrowserMethod")}</Text>
      ) : (
        <>
          <Text elementType="p">{t("enroll.passkey.description")}</Text>
          {failure ? <Failure error={failure} /> : null}
          {unavailable === undefined ? (
            <Button
              intent="primary"
              onPress={create}
              isPending={register.isPending || start.isFetching || register.isSuccess}
            >
              {t("enroll.passkey.submit")}
            </Button>
          ) : null}
        </>
      )}
    </>
  );
}

function Failure({ error }: { error: unknown }) {
  const { t } = useStepclimbStrings();
  const problem = signInProblem(error, "enroll", t);
  return <FormError code={problem.code}>{problem.message}</FormError>;
}

function EnrollmentSkeleton() {
  const { t } = useStepclimbStrings();
  return (
    <>
      <Heading level={1}>{t("enroll.title")}</Heading>
      <div aria-busy="true" className="flex flex-col gap-3">
        <Skeleton className="h-5 max-w-64" />
        <Skeleton shape="block" className="h-11" />
      </div>
    </>
  );
}
