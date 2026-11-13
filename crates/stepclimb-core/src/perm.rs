// SPDX-License-Identifier: AGPL-3.0-or-later
//! The permission catalog: every permission Stepclimb checks, as a unit struct.
//!
//! The catalog changes only in code. One `catalog!` invocation below defines each struct, its
//! [`Permission`] impl and its entry in [`ALL`], so a permission cannot exist without being
//! listed. [`Permission`] is sealed with a trait private to this module: an `impl Permission`
//! anywhere else does not compile, so the macro is the only way to add one.
//!
//! ```compile_fail,E0277
//! use stepclimb_core::perm::{Permission, ScopeType};
//!
//! struct Unlisted;
//!
//! impl Permission for Unlisted {
//!     const KEY: &'static str = "unlisted.manage";
//!     const GRANTABLE: &'static [ScopeType] = &[ScopeType::Organization];
//! }
//! ```

/// The kind of scope a role assignment grants at.
///
/// Scopes nest: an organization contains its accounts, an account contains its descendant
/// accounts and their courses, and a course contains nothing else.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ScopeType {
    /// The organization itself.
    Organization,
    /// One account and its whole subtree.
    Account,
    /// One course.
    Course,
}

/// A permission in the catalog.
///
/// Handlers name the type in `Require<P>`; the key is what roles store and the API shows.
pub trait Permission: sealed::Sealed {
    /// The stored and API name, `area.action` in lowercase snake case.
    const KEY: &'static str;
    /// The scope types at which a grant of this permission takes effect. A role assignment at
    /// any other scope type does not grant it.
    const GRANTABLE: &'static [ScopeType];
}

/// One catalog entry as data, for code that walks [`ALL`]. Its fields are private, so only the
/// catalog in this module can build one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PermissionInfo {
    key: &'static str,
    grantable: &'static [ScopeType],
}

impl PermissionInfo {
    /// [`Permission::KEY`].
    pub fn key(&self) -> &'static str {
        self.key
    }

    /// [`Permission::GRANTABLE`].
    pub fn grantable(&self) -> &'static [ScopeType] {
        self.grantable
    }
}

mod sealed {
    /// Private to `perm`, so only this module can implement [`super::Permission`].
    pub trait Sealed {}
}

/// Defines each permission struct, seals it, implements [`Permission`] and lists it in [`ALL`].
/// The `+` repetition makes an empty grantable list a compile error.
macro_rules! catalog {
    ($(
        $(#[$attr:meta])*
        $name:ident => $key:literal, [$($scope:ident),+ $(,)?];
    )+) => {
        $(
            $(#[$attr])*
            #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
            pub struct $name;

            impl sealed::Sealed for $name {}

            impl Permission for $name {
                const KEY: &'static str = $key;
                const GRANTABLE: &'static [ScopeType] = &[$(ScopeType::$scope),+];
            }
        )+

        /// Every permission in the catalog, in definition order.
        pub static ALL: &[PermissionInfo] = &[$(
            PermissionInfo { key: <$name as Permission>::KEY, grantable: <$name as Permission>::GRANTABLE },
        )+];
    };
}

catalog! {
    /// Change the organization and its settings, and view its entitlements.
    OrgManage => "organization.manage", [Organization];
    /// View accounts.
    AccountView => "account.view", [Organization, Account];
    /// Create, rename, move and delete accounts.
    AccountManage => "account.manage", [Organization, Account];
    /// View users.
    UserView => "user.view", [Organization, Account];
    /// Create, edit and suspend users.
    UserManage => "user.manage", [Organization];
    /// View roles and role assignments.
    RoleView => "role.view", [Organization, Account];
    /// Define, change and delete the organization's custom roles.
    RoleManage => "role.manage", [Organization];
    /// Create and remove role assignments.
    RoleAssign => "role.assign", [Organization, Account];
    /// Configure sign-in providers and local sign-in methods.
    ProviderManage => "provider.manage", [Organization];
    /// Configure admission rules and the admission webhook.
    AdmissionManage => "admission.manage", [Organization];
    /// Create and manage service accounts and their API keys.
    ServiceAccountManage => "service_account.manage", [Organization];
    /// Change the organization's branding.
    BrandingManage => "branding.manage", [Organization];
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    /// `area.action`, each part a lowercase snake case word starting with a letter (spec 09 Q1).
    fn is_key_format(key: &str) -> bool {
        let part = |s: &str| {
            let mut chars = s.chars();
            chars.next().is_some_and(|c| c.is_ascii_lowercase())
                && chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
        };
        matches!(key.split_once('.'), Some((area, action)) if part(area) && part(action))
    }

    #[test]
    fn key_format_accepts_and_refuses() {
        for good in ["account.manage", "service_account.manage", "a.b2"] {
            assert!(is_key_format(good), "{good}");
        }
        for bad in [
            "",
            "account",
            "account:manage",
            "Account.manage",
            "account.Manage",
            "account.manage.all",
            ".manage",
            "account.",
            "1account.manage",
            "account._manage",
            "account-x.manage",
            "AccountManage",
        ] {
            assert!(!is_key_format(bad), "{bad}");
        }
    }

    #[test]
    fn every_key_is_unique() {
        let mut seen = HashSet::new();
        for p in ALL {
            assert!(seen.insert(p.key), "duplicate key {}", p.key);
        }
    }

    #[test]
    fn every_key_matches_the_format() {
        for p in ALL {
            assert!(is_key_format(p.key), "{} is not area.action", p.key);
        }
    }

    #[test]
    fn every_entry_is_grantable_somewhere_without_repeats() {
        for p in ALL {
            assert!(!p.grantable.is_empty(), "{} has no grantable scope", p.key);
            let distinct: HashSet<_> = p.grantable.iter().collect();
            assert_eq!(
                distinct.len(),
                p.grantable.len(),
                "{} repeats a scope",
                p.key
            );
        }
    }

    #[test]
    fn all_is_the_m0_catalog() {
        use ScopeType::{Account as A, Organization as O};
        let expected: &[(&str, &[ScopeType])] = &[
            ("organization.manage", &[O]),
            ("account.view", &[O, A]),
            ("account.manage", &[O, A]),
            ("user.view", &[O, A]),
            ("user.manage", &[O]),
            ("role.view", &[O, A]),
            ("role.manage", &[O]),
            ("role.assign", &[O, A]),
            ("provider.manage", &[O]),
            ("admission.manage", &[O]),
            ("service_account.manage", &[O]),
            ("branding.manage", &[O]),
        ];
        let actual: Vec<_> = ALL.iter().map(|p| (p.key, p.grantable)).collect();
        assert_eq!(actual, expected);
    }
}
