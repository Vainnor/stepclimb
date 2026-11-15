// SPDX-License-Identifier: AGPL-3.0-or-later
// What the account page says when a passkey request fails (docs/specs/m0/04-local-accounts.md B13;
// 11-app-shell.md B17): one message per problem `code` the passkey routes return, and per way the
// browser's ceremony ends, each saying what happened and what to do next. An unknown code gets a
// general message and the code itself, so the user can report it.
import type { Strings } from "@stepclimb/ui";
import { ApiError, isUnreachable } from "../api";
import { PasskeyError } from "../sign-in/passkey";
import { passkeyMessage } from "../sign-in/problems";

export interface AccountProblem {
  /** What happened and what to do. */
  message: string;
  /** The server's code, shown in mono, when the page has no message of its own for it. */
  code?: string;
}

function codeOf(error: unknown): string | undefined {
  return error instanceof ApiError ? error.problem?.code : undefined;
}

/** The message for a problem `code` the page explains, or undefined. */
function codeMessage(code: string, t: Strings["t"]): string | undefined {
  switch (code) {
    case "auth.method_disabled":
      return t("account.passkeys.problem.methodDisabled");
    case "local.passkeys_not_allowed":
      return t("account.passkeys.problem.notAllowed");
    case "local.last_method":
      return t("account.passkeys.problem.lastMethod");
    case "auth.flow_expired":
      return t("account.passkeys.problem.flowExpired");
    case "auth.flow_invalid":
      return t("account.passkeys.problem.flowInvalid");
    case "auth.passkey_invalid":
      return t("account.passkeys.problem.passkeyInvalid");
    case "passkey.not_found":
      return t("account.passkeys.problem.notFound");
    default:
      return undefined;
  }
}

/** What to tell the user about `error`, a failed passkey request or ceremony. */
export function passkeyProblem(error: unknown, t: Strings["t"]): AccountProblem {
  if (error instanceof PasskeyError) {
    if (error.failure === "exists") return { message: t("account.passkeys.exists") };
    return { message: passkeyMessage(error, "signIn", t) };
  }
  if (isUnreachable(error)) return { message: t("account.problem.unreachable") };
  if (!(error instanceof ApiError)) return { message: t("account.problem.failed") };
  const code = error.problem?.code;
  const known = code === undefined ? undefined : codeMessage(code, t);
  if (known !== undefined) return { message: known };
  return { message: t("account.problem.unknown"), code: code ?? `HTTP ${error.status}` };
}

/** The message for a rule the server says a passkey's name broke. */
function nameMessage(code: string, t: Strings["t"]): string {
  switch (code) {
    case "name.empty":
      return t("account.passkeys.name.empty");
    case "name.too_long":
      return t("account.passkeys.name.tooLong");
    default:
      return t("account.passkeys.name.invalid");
  }
}

/**
 * The name field's message when the server refused the name (422 `request.invalid` naming
 * `/name`), or undefined for any other error, which `passkeyProblem` explains.
 */
export function nameError(error: unknown, t: Strings["t"]): string | undefined {
  if (!(error instanceof ApiError) || error.status !== 422) return undefined;
  const rule = error.problem?.errors?.find((broken) => broken.field === "/name");
  return rule === undefined ? undefined : nameMessage(rule.code, t);
}

/** True when the server refused to manage passkeys at all: the method is off for the organization. */
export function isMethodDisabled(error: unknown): boolean {
  return codeOf(error) === "auth.method_disabled";
}

/** True when adding a passkey cannot work however often it is tried. */
export function isFinalAddRefusal(error: unknown): boolean {
  const code = codeOf(error);
  return code === "auth.method_disabled" || code === "local.passkeys_not_allowed";
}

/** True when the passkey the request named is gone, so the list on screen is out of date. */
export function isPasskeyGone(error: unknown): boolean {
  return codeOf(error) === "passkey.not_found";
}
