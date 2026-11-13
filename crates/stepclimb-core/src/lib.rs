// SPDX-License-Identifier: AGPL-3.0-or-later
//! Domain types, repositories (SQL), the permission catalog and the embedded migrations.
//!
//! Today it holds only [`db::MIGRATOR`], the migrations in the repository's `migrations/`
//! folder compiled into the binary, which `stepclimb serve` runs at startup. The domain types,
//! the repositories (every one taking an `OrgScope`) and the permission catalog are **not built
//! yet**; they arrive with the M0 specs in `docs/specs/m0/` (organizations and accounts, users
//! and identities, the permission catalog).

pub mod db;
