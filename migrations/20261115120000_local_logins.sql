-- SPDX-License-Identifier: AGPL-3.0-or-later
-- Local accounts' sign-in by email and password (docs/specs/m0/04-local-accounts.md B4, B9, B14,
-- B16, Data, Q3, Q5). One row per user who has set a password through an enrollment link.
--
-- `email_normalized` is `lower(users.email)`, the comparison the `users_email` unique index
-- makes, so the unique pair below can never refuse a row the users table allows, and password
-- sign-in finds the account the way `users::find_by_email` does. `lower()` folds case by the
-- database's LC_CTYPE: under the `C` locale it folds ASCII letters only. The server derives the
-- per-account rate limit key with the same `lower()` (spec 08 B41). The column is written from
-- `users` on insert and kept equal by the trigger below whenever `users.email` changes; it is
-- NULL while the user has no email, and such an account cannot sign in by password until an
-- email is set again (B4).
--
-- `password_hash` is an argon2id PHC string (Q8). `password_generation` counts the passwords
-- set: each new one (an enrollment link's or a change) adds 1, a rehash of the same password
-- does not. A device cookie seals the generation it was issued for, so a new password revokes
-- older cookies (docs/specs/m0/08-sessions-and-csrf.md B42). TOTP's columns arrive with issue I7.
CREATE TABLE local_logins (
    user_id             uuid        PRIMARY KEY,
    org_id              uuid        NOT NULL REFERENCES organizations (id),
    email_normalized    text,
    password_hash       text        CHECK (password_hash LIKE '$argon2id$%'),
    password_changed_at timestamptz,
    password_generation bigint      NOT NULL DEFAULT 1 CHECK (password_generation > 0),
    created_at          timestamptz NOT NULL DEFAULT now(),
    updated_at          timestamptz NOT NULL DEFAULT now(),
    FOREIGN KEY (org_id, user_id) REFERENCES users (org_id, id) ON DELETE CASCADE,
    UNIQUE (org_id, email_normalized),
    CHECK ((password_hash IS NULL) = (password_changed_at IS NULL))
);

CREATE FUNCTION local_logins_follow_email() RETURNS trigger
LANGUAGE plpgsql AS $$
BEGIN
    UPDATE local_logins
    SET email_normalized = lower(NEW.email),
        updated_at = NEW.updated_at
    WHERE org_id = NEW.org_id AND user_id = NEW.id;
    RETURN NULL;
END;
$$;

CREATE TRIGGER users_email_to_local_logins
    AFTER UPDATE OF email ON users
    FOR EACH ROW
    WHEN (OLD.email IS DISTINCT FROM NEW.email)
    EXECUTE FUNCTION local_logins_follow_email();
