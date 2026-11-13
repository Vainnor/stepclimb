-- SPDX-License-Identifier: AGPL-3.0-or-later
-- A fixture migration for tests/migrations.rs: its first statement succeeds, its second fails,
-- so the test can see that the first is rolled back with it.
CREATE TABLE fixture_broken (id integer);
INSERT INTO fixture_missing VALUES (1);
