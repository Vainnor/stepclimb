// SPDX-License-Identifier: AGPL-3.0-or-later
//! The entitlement catalogs: every feature a plan can switch on or off, as a unit struct, and
//! every limit a plan can set, as a variant of [`Limit`]
//! (docs/specs/m0/10-entitlements-framework.md B1, B2).
//!
//! The `catalog!` invocation below defines each feature struct, its [`Feature`] impl and its
//! entry in [`ALL`]. Unlike [`crate::perm::Permission`], [`Feature`] is not sealed: a test crate
//! defines a feature of its own to check that the `unlimited` plan enables a feature the catalog
//! does not list (B4). Such a feature is not in [`ALL`], so it is never written to the
//! `unlimited` row or shown by the API; on any other plan it reads from the plan's row like any
//! other key.
//!
//! Keys are one lowercase snake case word (`custom_roles`, `users`): the part format of spec 09
//! Q1, without the `area.action` dot, because the spec names the limit keys and the plan's API
//! example shows them as single words.

/// A feature in the catalog.
///
/// Handlers name the type in `Require<F>` (spec 10 B9); the key is what plan rows store and the
/// API shows.
pub trait Feature {
    /// The stored and API name, one lowercase snake case word.
    const KEY: &'static str;
}

/// Defines each feature struct, implements [`Feature`] and lists its key in [`ALL`].
macro_rules! catalog {
    ($(
        $(#[$attr:meta])*
        $name:ident => $key:literal;
    )+) => {
        $(
            $(#[$attr])*
            #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
            pub struct $name;

            impl Feature for $name {
                const KEY: &'static str = $key;
            }
        )+

        /// The key of every feature in the catalog, in definition order.
        pub static ALL: &[&str] = &[$(<$name as Feature>::KEY),+];
    };
}

catalog! {
    /// Define, change and delete custom roles (spec 09's role-writing routes).
    CustomRoles => "custom_roles";
    /// Configure OpenID Connect identity providers (spec 05).
    OidcProviders => "oidc_providers";
    /// Configure OAuth 2.0 identity providers (spec 06).
    Oauth2Providers => "oauth2_providers";
    /// Configure the admission webhook (spec 07).
    AdmissionWebhook => "admission_webhook";
    /// Create service accounts and API keys (spec 08).
    ApiKeys => "api_keys";
    /// Change the organization's branding (spec 12).
    CustomBranding => "custom_branding";
}

/// A limit a plan sets on what an organization can add.
///
/// Each later limit (storage, video storage, transcode minutes, API rate, connectors) is added
/// by the spec that builds what it counts (B2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Limit {
    /// How many users the organization can have.
    Users,
}

impl Limit {
    /// Every limit, in definition order.
    pub const ALL: [Self; 1] = [Self::Users];

    /// The stored and API name, one lowercase snake case word.
    #[must_use]
    pub fn key(self) -> &'static str {
        match self {
            Self::Users => "users",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    /// One lowercase snake case word starting with a letter: a part of spec 09 Q1's format.
    fn is_key_format(key: &str) -> bool {
        let mut chars = key.chars();
        chars.next().is_some_and(|c| c.is_ascii_lowercase())
            && chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
    }

    #[test]
    fn key_format_accepts_and_refuses() {
        for good in ["users", "custom_roles", "oauth2_providers", "a"] {
            assert!(is_key_format(good), "{good}");
        }
        for bad in [
            "",
            "Users",
            "custom-roles",
            "custom.roles",
            "1users",
            "_users",
            "custom roles",
            "CustomRoles",
        ] {
            assert!(!is_key_format(bad), "{bad}");
        }
    }

    #[test]
    fn every_feature_and_limit_key_is_unique_and_well_formed() {
        let mut seen = HashSet::new();
        let limits = Limit::ALL.iter().map(|l| l.key());
        for key in ALL.iter().copied().chain(limits) {
            assert!(is_key_format(key), "{key} is not a snake case word");
            assert!(seen.insert(key), "duplicate key {key}");
        }
    }

    #[test]
    fn all_is_the_m0_feature_catalog() {
        assert_eq!(
            ALL,
            [
                "custom_roles",
                "oidc_providers",
                "oauth2_providers",
                "admission_webhook",
                "api_keys",
                "custom_branding",
            ]
        );
    }

    #[test]
    fn the_m0_limits_are_users_only() {
        let keys: Vec<_> = Limit::ALL.iter().map(|l| l.key()).collect();
        assert_eq!(keys, ["users"]);
    }
}
