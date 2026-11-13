-- SPDX-License-Identifier: AGPL-3.0-or-later
-- A fixture migration for tests/migrations.rs, after the failing one: it must never run.
CREATE TABLE fixture_after (id integer);
