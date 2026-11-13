// SPDX-License-Identifier: AGPL-3.0-or-later
//! Domain types, repositories (SQL), the permission catalog and the embedded migrations.
//!
//! Today it holds [`db::MIGRATOR`], the migrations in the repository's `migrations/` folder
//! compiled into the binary, which `stepclimb serve` runs at startup; [`id::new_id`], the UUID
//! v7 generator for every row key; the permission catalog ([`perm`]); the built-in role lists
//! ([`roles::BUILTIN`]); [`clock::Clock`], the controllable clock every time rule reads;
//! [`OrgScope`], the organization every repository function acts in; the [`organizations`]
//! repository with its typed [`OrgSettings`]; the [`accounts`] repository, the tree inside an
//! organization; the [`users`] repository and [`Source`], where a record came from; and the
//! [`sessions`] repository with the browser-session credential resolver. The other repositories
//! arrive with the M0 specs in `docs/specs/m0/` (identities, roles).

pub mod accounts;
pub mod clock;
pub mod db;
pub mod id;
mod name;
pub mod organizations;
pub mod perm;
pub mod roles;
mod scope;
pub mod sessions;
mod settings;
mod source;
pub mod users;

pub use scope::{OrgScope, ScopeOrigin};
pub use settings::{OrgSettings, OrgSettingsPatch};
pub use source::Source;
