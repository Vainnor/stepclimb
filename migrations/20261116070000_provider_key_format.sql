-- SPDX-License-Identifier: AGPL-3.0-or-later
-- The provider key format (docs/specs/m0/06-oauth2-and-vatsim-preset.md Q5 (a)): 1 to 32
-- lowercase ASCII letters, digits and hyphens, starting with a letter or digit. A key is the
-- `{provider}` segment of `/auth/{provider}/...` and of the redirect URI registered at the
-- provider, so the table refuses any other key whichever code writes it: `.` and `..` (path
-- segments a client or proxy resolves), `/`, `%`, upper case and non-ASCII letters among them.
-- `COLLATE "C"` makes the ranges plain byte ranges, whatever the database's collation. The
-- `local` row's key passes, and `identity_providers_local_key` already keeps every other kind
-- off it.
ALTER TABLE identity_providers ADD CONSTRAINT identity_providers_key_format
    CHECK ((key COLLATE "C") ~ '^[a-z0-9][a-z0-9-]{0,31}$');
