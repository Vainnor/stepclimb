// SPDX-License-Identifier: AGPL-3.0-or-later
//! Domain types, repositories (SQL), the permission catalog and the embedded migrations.
//!
//! Today it holds [`db::MIGRATOR`], the migrations in the repository's `migrations/` folder
//! compiled into the binary, which `stepclimb serve` runs at startup; [`id::new_id`], the UUID
//! v7 generator for every row key; the permission catalog ([`perm`]); and the built-in role
//! lists ([`roles::BUILTIN`]). The domain types and the repositories (every one taking an
//! `OrgScope`) are **not built yet**; they arrive with the M0 specs in `docs/specs/m0/`
//! (organizations and accounts, users and identities, roles).

pub mod db;
pub mod id;
pub mod perm;
pub mod roles;
