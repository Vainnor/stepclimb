-- SPDX-License-Identifier: AGPL-3.0-or-later
-- One-time local sign-in tokens (docs/specs/m0/04-local-accounts.md B6, B7, Data, Q3): today the
-- enrollment links an admin creates and hands over, later the magic links of issue I8.
-- `token_hash` is the SHA-256 of the token (docs/specs/m0/08-sessions-and-csrf.md Q2), never the
-- token itself: the length check refuses the 43-character token text. The composite foreign key
-- keeps a token's user in the token's organization.
--
-- Who made a link decides whether it still works (B6): a link an admin made works only while
-- that admin could make it again, which the server re-checks at use. `created_by_instance` marks
-- a link the instance made (`stepclimb admin create` and `stepclimb admin enrollment-link`),
-- whose `created_by` is NULL. A link that is not the instance's and has a NULL `created_by` lost
-- its creator (`ON DELETE SET NULL`) and is refused.
CREATE TABLE local_tokens (
    id          uuid        PRIMARY KEY,
    org_id      uuid        NOT NULL REFERENCES organizations (id),
    user_id     uuid        NOT NULL,
    kind        text        NOT NULL CHECK (kind IN ('enrollment', 'magic_link')),
    token_hash  bytea       NOT NULL UNIQUE CHECK (octet_length(token_hash) = 32),
    created_by  uuid        REFERENCES users (id) ON DELETE SET NULL,
    created_by_instance boolean NOT NULL,
    expires_at  timestamptz NOT NULL,
    used_at     timestamptz,
    created_at  timestamptz NOT NULL DEFAULT now(),
    updated_at  timestamptz NOT NULL DEFAULT now(),
    FOREIGN KEY (org_id, user_id) REFERENCES users (org_id, id) ON DELETE CASCADE,
    CONSTRAINT local_tokens_instance_has_no_creator
        CHECK (NOT created_by_instance OR created_by IS NULL)
);
-- B6: creating a link removes the user's earlier unused links of the same kind.
CREATE INDEX local_tokens_by_user ON local_tokens (org_id, user_id, kind);
