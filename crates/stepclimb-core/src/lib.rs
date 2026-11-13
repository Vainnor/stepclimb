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
//! [`sessions`] repository with the browser-session credential resolver; and the [`roles`] and
//! [`role_assignments`] repositories, with the startup sync of built-in role rows
//! ([`roles::sync_builtin`]); the permission evaluator, [`grants::Grants`]; and the
//! [`auth_flows`] repository, the state of a sign-in that spans several requests; and the
//! [`identity_providers`] and [`identities`] repositories, which link a user to an identity
//! provider by the provider's subject; and the entitlement catalogs ([`feature`]) with the
//! [`plans`] and [`org_plans`] repositories, which give every organization the `unlimited`
//! plan. The other repositories arrive with the M0 specs in
//! `docs/specs/m0/`.

pub mod accounts;
pub mod auth_flows;
pub mod clock;
pub mod db;
pub mod feature;
pub mod grants;
pub mod id;
pub mod identities;
pub mod identity_providers;
mod name;
pub mod org_plans;
pub mod organizations;
pub mod perm;
pub mod plans;
pub mod role_assignments;
pub mod roles;
mod scope;
pub mod sessions;
mod settings;
mod source;
pub mod users;

pub use scope::{OrgScope, ScopeOrigin};
pub use settings::{OrgSettings, OrgSettingsPatch};
pub use source::Source;
