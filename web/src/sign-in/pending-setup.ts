// SPDX-License-Identifier: AGPL-3.0-or-later
// The TOTP secret the password step received for a set-up during sign-in (docs/specs/m0/
// 04-local-accounts.md B15; 11-app-shell.md B16), handed to the set-up step in memory only. The
// secret is never put in the URL or the history entry's state, which the browser may keep on disk;
// a reload loses it, and the set-up step then sends the user back to the password step, whose new
// sign-in answers a new secret.

export interface PendingSetup {
  /** The `otpauth://` URI, for an app that takes a setup link. */
  otpauthUri: string;
  /** The secret in base32, for typing into the app. */
  secret: string;
}

let pending: PendingSetup | undefined;

/** Keep `setup` for the set-up step, replacing any earlier one. */
export function holdSetup(setup: PendingSetup): void {
  pending = setup;
}

/** The set-up the password step kept, if any; it stays until `forgetSetup`. */
export function heldSetup(): PendingSetup | undefined {
  return pending;
}

/** Drop the kept set-up, once the step has it or no longer needs it. */
export function forgetSetup(): void {
  pending = undefined;
}
