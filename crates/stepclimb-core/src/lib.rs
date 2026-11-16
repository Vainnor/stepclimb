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
//! [`sessions`] repository with the browser-session credential resolver, and the
//! [`service_accounts`] repository with the API-key credential resolver; and the [`roles`] and
//! [`role_assignments`] repositories, with the startup sync of built-in role rows
//! ([`roles::sync_builtin`]); the permission evaluator, [`grants::Grants`]; and the
//! [`auth_flows`] repository, the state of a sign-in that spans several requests, and
//! [`oauth_codes`], the authorization codes of the OAuth 2.1 server for Stepclimb's own apps, and
//! [`oauth_refresh_tokens`], its refresh token families and the app access token resolver; and the
//! [`identity_providers`] and [`identities`] repositories, which link a user to an identity
//! provider by the provider's subject; and [`local_sign_in`], the local provider row and its
//! method switches, with [`local_tokens`] (enrollment links), [`passkeys`], [`local_logins`]
//! (passwords), [`local_totp`] (TOTP and recovery codes) and [`local`] (local credentials); and [`admission_settings`], the switches
//! that say who admission lets in; [`admission_webhooks`], the organization's admission webhook; and the entitlement catalogs ([`feature`]) with the
//! [`plans`] and [`org_plans`] repositories, which give every organization the `unlimited`
//! plan, and how a request's plan bounds what it adds ([`org_plans::check_room`]); and
//! [`requirement`], which lets the server's `Require` check a permission or a feature; and
//! [`audit`], the entries the repository functions that change roles, assignments, accounts,
//! organizations and user status write with their change; [`tz`], the IANA time zone names
//! users and organizations are set to; and [`theme`], the server's port of the theme generator,
//! with the [`branding`] repository that stores an organization's generated stylesheet; and
//! [`appearance`], the theme and density a user chose; and [`files`], the stored files of an
//! organization, once per SHA-256. The other
//! repositories arrive with the M0 specs in `docs/specs/m0/`.

pub mod accounts;
pub mod admission_rules;
pub mod admission_settings;
pub mod admission_webhooks;
pub mod appearance;
pub mod audit;
pub mod auth_flows;
pub mod branding;
pub mod clock;
pub mod courses;
pub mod db;
pub mod feature;
pub mod files;
pub mod grants;
pub mod groups;
pub mod id;
pub mod identities;
pub mod identity_providers;
pub mod items;
pub mod local;
pub mod local_logins;
pub mod local_sign_in;
pub mod local_tokens;
pub mod local_totp;
pub mod modules;
mod name;
pub mod oauth_codes;
pub mod oauth_refresh_tokens;
pub mod org_plans;
pub mod organizations;
pub mod passkeys;
pub mod perm;
pub mod plans;
pub mod requirement;
pub mod role_assignments;
pub mod roles;
mod scope;
pub mod service_accounts;
pub mod sessions;
mod settings;
mod source;
pub mod theme;
pub mod tz;
pub mod users;

pub use scope::{OrgScope, ScopeOrigin};
pub use settings::{OrgSettings, OrgSettingsPatch};
pub use source::Source;
