// SPDX-License-Identifier: AGPL-3.0-or-later
// What the sign-in and enrollment screens say when something fails (docs/specs/m0/11-app-shell.md
// B17): one message per problem `code` the server returns, and per way a passkey ceremony ends in
// the browser, each saying what happened and what to do next. An unknown code gets a general
// message and the code itself, so the user can report it.
import type { Strings } from "@stepclimb/ui";
import { ApiError, isUnreachable } from "../api";
import { PasskeyError } from "./passkey";

/**
 * Which screen the failure is on: some codes read differently while setting up an account or on
 * the password step.
 */
export type SignInScreen = "enroll" | "signIn" | "password" | "totp";

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

function rateLimited(error: ApiError, screen: SignInScreen, t: Strings["t"]): string {
  // The password and TOTP routes also count failed checks per account, so their message names no
  // network.
  if (screen === "password" || screen === "totp") {
    return error.retryAfter === undefined
      ? t("signIn.problem.passwordRateLimited")
      : t("signIn.problem.passwordRateLimitedFor", { seconds: error.retryAfter });
  }
  return error.retryAfter === undefined
    ? t("signIn.problem.rateLimited")
    : t("signIn.problem.rateLimitedFor", { seconds: error.retryAfter });
}

function methodDisabled(screen: SignInScreen, t: Strings["t"]): string {
  if (screen === "enroll") return t("enroll.problem.methodDisabled");
  if (screen === "password") return t("signIn.problem.passwordDisabled");
  if (screen === "totp") return t("signIn.problem.totpDisabled");
  return t("signIn.problem.methodDisabled");
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
      return methodDisabled(screen, t);
    case "auth.credentials_invalid":
      return t("signIn.problem.credentialsInvalid");
    case "auth.suspended":
      return t("signIn.problem.suspended");
    case "auth.admission_refused":
      return t("signIn.problem.admissionRefused");
    case "auth.not_provisioned":
      return t("signIn.problem.notProvisioned");
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
    return { message: rateLimited(error, screen, t) };
  }
  const known = code === undefined ? undefined : codeMessage(code, screen, t);
  if (known !== undefined) return { message: known };
  return { message: t("signIn.problem.unknown"), code: code ?? `HTTP ${error.status}` };
}

/**
 * The message for a code a browser flow sent the sign-in screen with (`/sign-in?error=<code>`,
 * B18): the provider start and callback routes (05-oidc, 06-oauth2-and-vatsim-preset) and
 * admission's refusals (07-admission B2), or undefined.
 */
function navigationMessage(code: string, t: Strings["t"]): string | undefined {
  switch (code) {
    case "auth.rate_limited":
      return t("signIn.problem.rateLimited");
    case "auth.provider_not_found":
      return t("signIn.problem.providerNotFound");
    case "auth.provider_unavailable":
      return t("signIn.problem.providerUnavailable");
    case "auth.provider_denied":
      return t("signIn.problem.providerDenied");
    case "auth.provider_error":
      return t("signIn.problem.providerError");
    case "auth.id_token_invalid":
      return t("signIn.problem.idTokenInvalid");
    case "auth.subject_missing":
      return t("signIn.problem.subjectMissing");
    case "auth.flow_expired":
      return t("signIn.problem.providerFlowExpired");
    case "auth.flow_invalid":
      return t("signIn.problem.flowInvalid");
    case "auth.suspended":
      return t("signIn.problem.suspended");
    case "auth.admission_refused":
      return t("signIn.problem.admissionRefused");
    case "auth.not_provisioned":
      return t("signIn.problem.notProvisioned");
    case "auth.signup_closed":
      return t("signIn.problem.signupClosed");
    case "entitlement.limit_reached":
      return t("signIn.problem.limitReached");
    default:
      return undefined;
  }
}

/** The namespaces of the codes the sign-in routes send; an unknown code is shown only in one. */
const SHOWN_NAMESPACES = new Set(["auth", "admission", "entitlement", "local", "org"]);

/** The longest unknown code the screen shows; a longer one is not a server's. */
const MAX_SHOWN_CODE = 64;

/**
 * True when `code` has the shape of a sign-in code the server could send: two or three
 * lowercase words joined by dots, the first one of `SHOWN_NAMESPACES`. Anyone can write a link to
 * `/sign-in?error=...`, so a value of any other shape is never put on the page, where it could
 * pass for a message from the organization.
 */
function looksLikeCode(code: string): boolean {
  if (code.length > MAX_SHOWN_CODE || !/^[a-z][a-z0-9_]*(\.[a-z][a-z0-9_]*){1,2}$/.test(code)) {
    return false;
  }
  return SHOWN_NAMESPACES.has(code.slice(0, code.indexOf(".")));
}

/** What to tell the user about the code a browser flow ended with (B18). */
export function navigationProblem(code: string, t: Strings["t"]): SignInProblem {
  const known = navigationMessage(code, t);
  if (known !== undefined) return { message: known };
  if (!looksLikeCode(code)) return { message: t("signIn.problem.failed") };
  return { message: t("signIn.problem.unknown"), code };
}

/**
 * The codes after which the TOTP step cannot go on: the server used up or never had the second
 * step's flow, so the password is needed again (04-local-accounts B20). A wrong code ends the flow
 * too, so every new guess starts from the password.
 */
const PASSWORD_AGAIN = new Set(["auth.totp_invalid", "auth.flow_invalid", "auth.flow_expired"]);

/** The code of `error` when the TOTP step must go back to the password step, or undefined. */
export function passwordAgain(error: unknown): string | undefined {
  const code = error instanceof ApiError ? error.problem?.code : undefined;
  return code !== undefined && PASSWORD_AGAIN.has(code) ? code : undefined;
}

/** What the password step says when the TOTP step sent the user back to it with `code`. */
export function passwordAgainMessage(code: string, t: Strings["t"]): string {
  switch (code) {
    case "auth.totp_invalid":
      return t("signIn.problem.totpInvalid");
    case "auth.flow_expired":
      return t("signIn.problem.totpExpired");
    default:
      return t("signIn.problem.totpRestart");
  }
}
