-- SPDX-License-Identifier: AGPL-3.0-or-later
-- The admission refusal marker (docs/specs/m0/07-admission.md I3, coordinator decision
-- 2026-11-15). When admission's rules (step 2) refuse a known user's sign-in through an outside
-- provider, the server records when and through which provider key, outside the sign-in's
-- rolled-back writes. A local sign-in (passkey or password) refuses a user who carries the
-- marker, so a rule cannot be escaped by signing in without the provider. The next sign-in
-- through an outside provider that passes the rules clears it. The key is kept as text, not a
-- reference, so deleting the provider never clears the marker.
ALTER TABLE users
    ADD COLUMN admission_refused_at       timestamptz,
    ADD COLUMN admission_refused_provider text,
    ADD CONSTRAINT users_admission_refused_pair
        CHECK ((admission_refused_at IS NULL) = (admission_refused_provider IS NULL));
