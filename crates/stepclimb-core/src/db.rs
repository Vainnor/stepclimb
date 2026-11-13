// SPDX-License-Identifier: AGPL-3.0-or-later
//! The database schema, as embedded migrations.

use sqlx::migrate::Migrator;

/// Every migration in `migrations/` at the repository root, compiled in. Migrations are
/// forward-only: an applied migration is never edited, and a change is a new migration.
pub static MIGRATOR: Migrator = sqlx::migrate!("../../migrations");
