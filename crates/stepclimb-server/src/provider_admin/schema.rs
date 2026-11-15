// SPDX-License-Identifier: AGPL-3.0-or-later
//! The document's form of a provider's config as answers show it: the stored config of
//! `stepclimb_auth::oidc_config::OidcConfig::to_json` or
//! `stepclimb_auth::oauth2_config::Oauth2Config::to_json`, without the client secret
//! (`identity_providers::public_config`). Answers serialize the stored JSON; these types only
//! describe it, and `tests/providers.rs` checks answers of both kinds against them.

use std::collections::BTreeMap;

use utoipa::ToSchema;

/// A provider's configuration, by its `kind`.
#[derive(ToSchema)]
#[serde(untagged)]
pub enum ProviderConfig {
    Oidc(OidcProviderConfig),
    Oauth2(Oauth2ProviderConfig),
}

/// An `oidc` provider's configuration (`05-oidc.md` B1).
#[derive(ToSchema)]
pub struct OidcProviderConfig {
    /// The issuer, exactly as written; discovery is read from it.
    pub issuer: String,
    pub client_id: String,
    /// Always includes `openid`.
    pub scopes: Vec<String>,
    pub claims: OidcProviderClaims,
    /// Whether sign-in also reads the provider's userinfo endpoint.
    pub userinfo: bool,
    /// Whether users who sign in through the provider may also add passkeys.
    pub allow_local_passkeys: bool,
    /// Whether, with the organization's email matching on, a sign-in whose verified email
    /// equals a user's verified email may be linked to that user.
    pub trusted_for_email_matching: bool,
}

/// JSON Pointers into the merged ID token and userinfo claims.
#[derive(ToSchema)]
pub struct OidcProviderClaims {
    pub given_name: String,
    pub family_name: String,
    pub email: String,
    pub email_verified: String,
    pub groups: Option<String>,
    /// Attribute names to pointers.
    pub attributes: BTreeMap<String, String>,
}

/// An `oauth2` provider's configuration (B4).
#[derive(ToSchema)]
pub struct Oauth2ProviderConfig {
    pub authorize: String,
    pub token: String,
    pub userinfo: String,
    pub client_id: String,
    pub scopes: Vec<String>,
    /// Whether sign-in sends S256 PKCE.
    pub pkce: bool,
    pub claims: Oauth2ProviderClaims,
    /// Whether users who sign in through the provider may also add passkeys.
    pub allow_local_passkeys: bool,
    /// Whether, with the organization's email matching on, a sign-in whose verified email
    /// equals a user's verified email may be linked to that user.
    pub trusted_for_email_matching: bool,
}

/// JSON Pointers into the userinfo body; a profile field without one is not mapped.
#[derive(ToSchema)]
pub struct Oauth2ProviderClaims {
    pub subject: String,
    pub given_name: Option<String>,
    pub family_name: Option<String>,
    pub email: Option<String>,
    pub email_verified: Option<String>,
    pub groups: Option<String>,
    /// Attribute names to pointers.
    pub attributes: BTreeMap<String, String>,
}
