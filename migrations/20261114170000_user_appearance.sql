-- SPDX-License-Identifier: AGPL-3.0-or-later
-- A user's appearance choice (docs/specs/m0/11-app-shell.md B21, Q6): the theme and the density
-- the server writes into the page as `data-sc-theme` and `data-sc-density`. `system` follows the
-- operating system's color and contrast settings, `comfortable` is the default density; both
-- leave their attribute unset.
ALTER TABLE users
    ADD COLUMN theme text NOT NULL DEFAULT 'system'
        CHECK (theme IN ('system', 'light', 'dark', 'high-contrast')),
    ADD COLUMN density text NOT NULL DEFAULT 'comfortable'
        CHECK (density IN ('comfortable', 'compact'));
