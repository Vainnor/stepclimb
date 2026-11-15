-- SPDX-License-Identifier: AGPL-3.0-or-later
-- The record of a verified email (docs/specs/m0/07-admission.md I4, spec 03 Q2 (a)). Email
-- matching links an unknown identity only to a user whose stored email came from a verified
-- source, so an address a provider merely asserted cannot be claimed first and matched later.
-- `verified_email` is the address a verified source vouched for, lowercased under the "C"
-- collation, which folds only ASCII A to Z whatever the database's locale: an admin, who
-- writes every local user's email, or a sign-in through a provider trusted for matching whose
-- email it marked verified. The check holds the record to the stored address: a write that
-- changes `email` to another address without clearing or renewing the record fails, so no user keeps a record for
-- an address nobody verified.
--
-- Every local user's email was written by an admin (`POST /api/v1/users`, `PATCH
-- /api/v1/users/{id}`, `stepclimb admin create`), so existing local users get the record.
-- Users of any other source get none: nothing recorded whether their email was verified.
ALTER TABLE users
    ADD COLUMN verified_email text,
    ADD CONSTRAINT users_verified_email_is_email
        CHECK (verified_email IS NULL
               OR (email IS NOT NULL AND verified_email = lower(email COLLATE "C")));

UPDATE users SET verified_email = lower(email COLLATE "C")
WHERE source = 'local' AND email IS NOT NULL;
