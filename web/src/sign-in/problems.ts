// SPDX-License-Identifier: AGPL-3.0-or-later
// What the sign-in and enrollment screens say when something fails (docs/specs/m0/11-app-shell.md
// B17): one message per problem `code` the server returns, and per way a passkey ceremony ends in
// the browser, each saying what happened and what to do next. An unknown code gets a general
// message and the code itself, so the user can report it.
import type { Strings } from "@stepclimb/ui";
import { ApiError, isUnreachable } from "../api";
import { PasskeyError } from "./passkey";

/** Which screen the failure is on: some codes read differently while setting up an account. */
export type SignInScreen = "enroll" | "signIn";

export interface SignInProblem {
  /** What happened and what to do. */
  message: string;
  /** The server's code, shown in mono, when the screen has no message of its own for it. */
  code?: string;
}

/** The codes after which the same enrollment link can never work. */
const DEAD_LINK = new Set(["auth.enrollment_invalid", "auth.enrollment_expired"]);

/** True when the error says the enrollment link is used up, replaced, unknown or expired. */
export function isDeadLink(error: unknown): boolean {
  return error instanceof ApiError && DEAD_LINK.has(error.problem?.code ?? "");
}

/** True when the error means the server deleted the ceremony's flow, so new options are needed. */
export function needsNewCeremony(error: unknown): boolean {
  const code = error instanceof ApiError ? error.problem?.code : undefined;
  return (
    code === "auth.flow_invalid" || code === "auth.flow_expired" || code === "auth.passkey_invalid"
  );
}

/** What to say when a passkey ceremony ended in the browser without a credential. */
export function passkeyMessage(error: PasskeyError, screen: SignInScreen, t: Strings["t"]): string {
  switch (error.failure) {
    case "insecure":
      return t("signIn.passkey.insecure");
    case "unsupported":
      return t("signIn.passkey.unsupported");
    case "cancelled":
      return t("signIn.passkey.cancelled");
    case "exists":
      return screen === "enroll" ? t("enroll.passkey.exists") : t("signIn.passkey.failed");
    case "failed":
      return t("signIn.passkey.failed");
  }
}

function rateLimited(error: ApiError, t: Strings["t"]): string {
  return error.retryAfter === undefined
    ? t("signIn.problem.rateLimited")
    : t("signIn.problem.rateLimitedFor", { seconds: error.retryAfter });
}

/** The message for a problem `code` the screens explain, or undefined. */
function codeMessage(code: string, screen: SignInScreen, t: Strings["t"]): string | undefined {
  const enroll = screen === "enroll";
  switch (code) {
    case "auth.enrollment_invalid":
      return t("enroll.problem.invalid");
    case "auth.enrollment_expired":
      return t("enroll.problem.expired");
    case "auth.method_disabled":
      return enroll ? t("enroll.problem.methodDisabled") : t("signIn.problem.methodDisabled");
    case "auth.suspended":
      return t("signIn.problem.suspended");
    case "local.passkeys_not_allowed":
      return t("signIn.problem.passkeysNotAllowed");
    case "auth.flow_expired":
      return t("signIn.problem.flowExpired");
    case "auth.flow_invalid":
      return t("signIn.problem.flowInvalid");
    case "auth.passkey_invalid":
      return enroll ? t("enroll.problem.passkeyInvalid") : t("signIn.problem.passkeyInvalid");
    case "org.none":
      return t("signIn.problem.orgNone");
    default:
      return undefined;
  }
}

/** What to tell the user about `error`, a failed request or passkey ceremony. */
export function signInProblem(
  error: unknown,
  screen: SignInScreen,
  t: Strings["t"],
): SignInProblem {
  if (error instanceof PasskeyError) return { message: passkeyMessage(error, screen, t) };
  if (isUnreachable(error)) return { message: t("signIn.problem.unreachable") };
  if (!(error instanceof ApiError)) return { message: t("signIn.problem.failed") };
  const code = error.problem?.code;
  if (error.status === 429 || code === "auth.rate_limited") {
    return { message: rateLimited(error, t) };
  }
  const known = code === undefined ? undefined : codeMessage(code, screen, t);
  if (known !== undefined) return { message: known };
  return { message: t("signIn.problem.unknown"), code: code ?? `HTTP ${error.status}` };
}
