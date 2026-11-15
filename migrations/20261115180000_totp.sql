-- SPDX-License-Identifier: AGPL-3.0-or-later
-- TOTP as the second step after a local account's password (docs/specs/m0/04-local-accounts.md
-- B15, B19 to B21, Data, Q3, Q11).
--
-- `totp_secret` is the 20-byte secret sealed with the key ring (AES-256-GCM,
-- docs/specs/m0/08-sessions-and-csrf.md B3), bound to the organization, the table, the user and
-- the column; the database never holds it in the clear. It is set when enrollment starts and
-- confirmed, by a valid code, at `totp_confirmed_at`; until then it signs no one in. Only a
-- login with a password can hold one: TOTP is never a way in by itself.
--
-- `totp_last_step` is the 30-second step of the last code accepted for the user (a
-- confirmation or a sign-in). A code is accepted only for a later step, so a code used once is
-- refused when presented again (RFC 6238 section 5.2).
ALTER TABLE local_logins
    ADD COLUMN totp_secret       bytea,
    ADD COLUMN totp_confirmed_at timestamptz,
    ADD COLUMN totp_last_step    bigint,
    ADD CONSTRAINT local_logins_totp_needs_password
        CHECK (totp_secret IS NULL OR password_hash IS NOT NULL),
    ADD CONSTRAINT local_logins_totp_confirmed_has_secret
        CHECK (totp_confirmed_at IS NULL OR totp_secret IS NOT NULL);

-- The ten single-use recovery codes made when TOTP is confirmed (Q11), each stored as the
-- SHA-256 of its text; the codes themselves are shown once. A used code keeps its row with
-- `used_at` set. Confirming again (after disabling) and disabling delete a user's codes.
CREATE TABLE totp_recovery_codes (
    id         uuid        PRIMARY KEY,
    org_id     uuid        NOT NULL REFERENCES organizations (id),
    user_id    uuid        NOT NULL,
    code_hash  bytea       NOT NULL CHECK (octet_length(code_hash) = 32),
    used_at    timestamptz,
    created_at timestamptz NOT NULL DEFAULT now(),
    updated_at timestamptz NOT NULL DEFAULT now(),
    FOREIGN KEY (org_id, user_id) REFERENCES users (org_id, id) ON DELETE CASCADE,
    UNIQUE (org_id, user_id, code_hash)
);
