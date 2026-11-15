-- SPDX-License-Identifier: AGPL-3.0-or-later
-- When a passkey sign-in was first refused because the sign counter did not go up
-- (docs/specs/m0/04-local-accounts.md B12, Q12: "refuse and flag"): a sign that two copies of
-- the passkey may exist. NULL until then. Nothing reads it to block sign-in; whether a flagged
-- passkey is blocked is open (Q15).
ALTER TABLE passkeys ADD COLUMN counter_regressed_at timestamptz;
