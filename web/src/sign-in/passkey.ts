// SPDX-License-Identifier: AGPL-3.0-or-later
// The browser's half of a passkey ceremony (docs/specs/m0/04-local-accounts.md B8, B11, B12): the
// server's options go to the WebAuthn API, and the credential comes back as JSON for the server to
// verify. The page checks nothing itself; the server decides whether the passkey is accepted.
import type { components } from "@stepclimb/api-client";

type CreationOptions = components["schemas"]["PasskeyCreationOptions"];
type RequestOptions = components["schemas"]["PasskeyRequestOptions"];

/** Why a ceremony ended in the browser without a credential to send. */
export type PasskeyFailure =
  /** The page is not a secure context (plain `http` on a host other than localhost). */
  | "insecure"
  /** The browser has no WebAuthn, or not the JSON methods the server's options need. */
  | "unsupported"
  /** The user dismissed the prompt, or it timed out (`NotAllowedError`, `AbortError`). */
  | "cancelled"
  /** The authenticator already holds a passkey for this account (`InvalidStateError`). */
  | "exists"
  /** Anything else the browser refused, such as a relying party ID it rejects. */
  | "failed";

export class PasskeyError extends Error {
  readonly failure: PasskeyFailure;

  constructor(failure: PasskeyFailure, cause?: unknown) {
    super(`passkey ceremony: ${failure}`, { cause });
    this.name = "PasskeyError";
    this.failure = failure;
  }
}

/**
 * Why this page cannot run the ceremonies as the server describes them, or undefined when it can.
 * Browsers expose WebAuthn only in a secure context, so an `http` address reads as "unsupported"
 * unless that is checked first.
 */
export function passkeyUnavailable(): "insecure" | "unsupported" | undefined {
  if (!globalThis.isSecureContext) return "insecure";
  const supported =
    typeof PublicKeyCredential === "function" &&
    typeof PublicKeyCredential.parseCreationOptionsFromJSON === "function" &&
    typeof PublicKeyCredential.parseRequestOptionsFromJSON === "function" &&
    typeof navigator.credentials?.create === "function" &&
    typeof navigator.credentials?.get === "function";
  return supported ? undefined : "unsupported";
}

function failureOf(error: unknown): PasskeyFailure {
  if (!(error instanceof DOMException)) return "failed";
  if (error.name === "NotAllowedError" || error.name === "AbortError") return "cancelled";
  if (error.name === "InvalidStateError") return "exists";
  return "failed";
}

/** The credential as JSON, or a PasskeyError when the browser returned something else. */
function toJson(credential: Credential | null): PublicKeyCredentialJSON {
  if (!(credential instanceof PublicKeyCredential)) throw new PasskeyError("failed");
  return credential.toJSON();
}

// The generated schema types the options' enumerations (`type`, `userVerification`) as plain
// strings, where the DOM types list their values. The browser's JSON parser checks every member
// against WebAuthn's own definitions and throws a TypeError for one it does not know, so the cast
// below hands it nothing it does not check.

/** Registers a new passkey with the creation options from the server. */
export async function createPasskey(options: CreationOptions): Promise<PublicKeyCredentialJSON> {
  const unavailable = passkeyUnavailable();
  if (unavailable) throw new PasskeyError(unavailable);
  try {
    const publicKey = PublicKeyCredential.parseCreationOptionsFromJSON(
      options.publicKey as PublicKeyCredentialCreationOptionsJSON,
    );
    return toJson(await navigator.credentials.create({ publicKey }));
  } catch (error) {
    if (error instanceof PasskeyError) throw error;
    throw new PasskeyError(failureOf(error), error);
  }
}

/** Signs with a passkey, answering the request options from the server. */
export async function getPasskey(options: RequestOptions): Promise<PublicKeyCredentialJSON> {
  const unavailable = passkeyUnavailable();
  if (unavailable) throw new PasskeyError(unavailable);
  try {
    const publicKey = PublicKeyCredential.parseRequestOptionsFromJSON(
      options.publicKey as PublicKeyCredentialRequestOptionsJSON,
    );
    return toJson(await navigator.credentials.get({ publicKey }));
  } catch (error) {
    if (error instanceof PasskeyError) throw error;
    // `InvalidStateError` means "already registered" only when creating one.
    const failure = failureOf(error);
    throw new PasskeyError(failure === "exists" ? "failed" : failure, error);
  }
}
