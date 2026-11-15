-- SPDX-License-Identifier: AGPL-3.0-or-later
-- A user who has not chosen a theme has none stored (docs/specs/m0/11-app-shell.md B21): their
-- pages take the organization's default appearance, which the branding record holds
-- (docs/specs/m0/12-tokens-and-theme-generator.md). `system` stays a choice: follow the operating
-- system whatever the organization's default.
--
-- Until now `system` was both the column's default and the "Match my device" choice, and the two
-- drew the same page. Rows holding it become "not chosen", since nearly all of them hold the
-- column default; a user who did pick "Match my device" picks it again to keep it over an
-- organization default.
ALTER TABLE users
    ALTER COLUMN theme DROP NOT NULL,
    ALTER COLUMN theme DROP DEFAULT;

UPDATE users SET theme = NULL WHERE theme = 'system';
