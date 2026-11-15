// SPDX-License-Identifier: AGPL-3.0-or-later
//! The `config` of an `identity_providers` row with `kind = 'oidc'` (`docs/specs/m0/05-oidc.md`
//! B1, B2, B13, Q2, Q3).
//!
//! An admin's config is checked by [`validate`], which lists every rule it breaks as a
//! [`ConfigProblem`]: a JSON Pointer to the member and a stable code, the `errors` of the admin
//! API's 422 `provider.config_invalid` (`06-oauth2-and-vatsim-preset.md` B14). The members are:
//!
//! | Member | Rule | When absent |
//! | --- | --- | --- |
//! | `issuer` | an absolute `https` URL (`http` with the insecure switch) with no query, fragment or credentials, under the [`UrlPolicy`]; kept exactly as written, since discovery compares it character for character (B4) | refused |
//! | `client_id` | a non-empty string | refused |
//! | `client_secret` | a non-empty string, write-only | the stored secret, or refused when there is none |
//! | `scopes` | scope tokens (RFC 6749 3.3) including `openid` | `["openid", "email", "profile"]` |
//! | `claims` | an object of JSON Pointers: `given_name`, `family_name`, `email`, `email_verified`, `groups`, and `attributes`, an object of names to pointers | B13's defaults for the four profile fields; no `groups`, no attributes |
//! | `userinfo` | a boolean: whether sign-in also reads the provider's userinfo endpoint (Q3) | `true` |
//! | `allow_local_passkeys` | a boolean: whether the provider's users may add a passkey under `/api/v1/me/passkeys` (`04-local-accounts.md` B13) | `false` |
//! | `trusted_for_email_matching` | a boolean: whether, with the organization's `email_matching` on, a sign-in whose verified email equals a user's verified email may be linked to that user (`07-admission.md` B5) | `false` |
//!
//! A provider declared in `stepclimb.toml` is validated the same way, then stored with
//! [`OidcConfig::from_config_file`]: no secret in the row, `source: "config"` in its place
//! (`crate::provider_config`).
//!
//! Any other member is refused, so a misspelled one cannot be silently ignored. The subject is
//! always the ID token's `sub` (B12), so `claims.subject` is refused too.
//!
//! The validated config is sealed with [`OidcConfigDraft::seal`] into an [`OidcConfig`], whose
//! [`OidcConfig::to_json`] is what the row stores. The problems, the client secret and its
//! sealing are those of every provider kind ([`crate::provider_config`], re-exported here).
//! [`OidcConfig`] leaves the secret out of `Debug`.

mod parse;

use std::collections::BTreeMap;
use std::fmt;

use serde_json::{Map, Value};

use crate::claims::ProfileMapping;
use crate::encryption::{EncryptError, KeyRing};
use crate::json_pointer::JsonPointer;
use crate::outbound::UrlPolicy;
use crate::provider_config::walk;
pub use crate::provider_config::{
    ClientSecret, ConfigErrors, ConfigProblem, ConfigSecrets, DraftSecret, ProviderRow,
    SealedSecret, SecretError, StoredConfigError, StoredSecret, TRUSTED_FOR_EMAIL_MATCHING, code,
};

/// The member that lets a provider's users add a passkey (`04-local-accounts.md` B13). The
/// `oauth2` config of `06-oauth2-and-vatsim-preset.md` has the same member with the same rule.
pub const ALLOW_LOCAL_PASSKEYS: &str = "allow_local_passkeys";

/// The scopes of a config that names none (B1).
pub const DEFAULT_SCOPES: [&str; 3] = ["openid", "email", "profile"];

/// Where a config's claims come from: JSON Pointers into the merged ID token and userinfo
/// claims (B13).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ClaimsMapping {
    pub given_name: JsonPointer,
    pub family_name: JsonPointer,
    pub email: JsonPointer,
    pub email_verified: JsonPointer,
    pub groups: Option<JsonPointer>,
    pub attributes: BTreeMap<String, JsonPointer>,
}

impl ClaimsMapping {
    /// The mapping as [`crate::claims::profile`] reads it, the one claim mapping of both
    /// provider kinds (`06-oauth2-and-vatsim-preset.md` B6).
    #[must_use]
    pub fn profile(&self) -> ProfileMapping {
        ProfileMapping {
            given_name: Some(self.given_name.clone()),
            family_name: Some(self.family_name.clone()),
            email: Some(self.email.clone()),
            email_verified: Some(self.email_verified.clone()),
            groups: self.groups.clone(),
            attributes: self.attributes.clone(),
        }
    }
}

/// The members every config has, whatever the state of its secret.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OidcSettings {
    /// The issuer exactly as the admin wrote it.
    pub issuer: String,
    pub client_id: String,
    pub scopes: Vec<String>,
    pub claims: ClaimsMapping,
    pub userinfo: bool,
    /// Whether the provider's users may add a passkey; off unless the admin says so.
    pub allow_local_passkeys: bool,
    /// Whether the provider is trusted for matching users by email; off unless the admin says
    /// so.
    pub trusted_for_email_matching: bool,
}

/// A validated config, before its secret is sealed.
#[derive(Clone, Debug)]
pub struct OidcConfigDraft {
    pub settings: OidcSettings,
    pub secret: DraftSecret,
}

/// A config as stored: its settings and where its secret is.
#[derive(Clone, PartialEq, Eq)]
pub struct OidcConfig {
    pub settings: OidcSettings,
    secret: StoredSecret,
}

impl fmt::Debug for OidcConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("OidcConfig")
            .field("settings", &self.settings)
            .finish_non_exhaustive()
    }
}

/// Check an admin's `config` under `policy`. `stored` is the row's current secret, for a change
/// that leaves `client_secret` out; a new provider passes `None` and must give one.
pub fn validate(
    config: &Value,
    policy: UrlPolicy,
    stored: Option<SealedSecret>,
) -> Result<OidcConfigDraft, ConfigErrors> {
    parse::draft(config, policy, stored)
}

impl OidcConfigDraft {
    /// Seal a new secret to `row` under the ring's current key, or keep the stored one, which
    /// was sealed to the same row.
    pub fn seal(self, ring: &KeyRing, row: ProviderRow) -> Result<OidcConfig, EncryptError> {
        Ok(OidcConfig {
            settings: self.settings,
            secret: StoredSecret::Sealed(SealedSecret::from_draft(self.secret, ring, row)?),
        })
    }
}

impl OidcConfig {
    /// The JSON object the row's `config` stores.
    #[must_use]
    pub fn to_json(&self) -> Map<String, Value> {
        let s = &self.settings;
        let mut claims = Map::new();
        claims.insert("given_name".into(), s.claims.given_name.as_str().into());
        claims.insert("family_name".into(), s.claims.family_name.as_str().into());
        claims.insert("email".into(), s.claims.email.as_str().into());
        claims.insert(
            "email_verified".into(),
            s.claims.email_verified.as_str().into(),
        );
        if let Some(groups) = &s.claims.groups {
            claims.insert("groups".into(), groups.as_str().into());
        }
        claims.insert(
            "attributes".into(),
            walk::attributes_json(&s.claims.attributes),
        );
        let mut config = Map::new();
        config.insert("issuer".into(), s.issuer.as_str().into());
        config.insert("client_id".into(), s.client_id.as_str().into());
        walk::insert_stored_secret(&mut config, &self.secret);
        config.insert("scopes".into(), s.scopes.clone().into());
        config.insert("claims".into(), Value::Object(claims));
        config.insert("userinfo".into(), s.userinfo.into());
        config.insert(ALLOW_LOCAL_PASSKEYS.into(), s.allow_local_passkeys.into());
        config.insert(
            TRUSTED_FOR_EMAIL_MATCHING.into(),
            s.trusted_for_email_matching.into(),
        );
        config
    }

    /// Read a stored config. Members other than the config's (the row's `label`, for one) are
    /// left to their readers. The URL policy is not applied here: the client applies it at
    /// every connection.
    pub fn from_stored(config: &Map<String, Value>) -> Result<Self, StoredConfigError> {
        parse::stored(config)
    }

    /// The config of a provider `stepclimb.toml` declares: `settings` validated by [`validate`],
    /// with the secret left to [`ConfigSecrets`]. Its [`OidcConfig::to_json`] has `source:
    /// "config"` and no `client_secret`.
    #[must_use]
    pub fn from_config_file(settings: OidcSettings) -> Self {
        Self {
            settings,
            secret: StoredSecret::ConfigFile(None),
        }
    }

    /// The sealed secret, for a change that keeps it ([`validate`]'s `stored`); `None` for a
    /// provider `stepclimb.toml` declares.
    #[must_use]
    pub fn sealed_secret(&self) -> Option<SealedSecret> {
        match &self.secret {
            StoredSecret::Sealed(sealed) => Some(sealed.clone()),
            StoredSecret::ConfigFile(_) => None,
        }
    }

    /// The client secret, for the token request: opened from the row, or, for a provider
    /// `stepclimb.toml` declares, taken from `config`. It fails when the value was sealed to
    /// another row or under a key no longer in the ring, or `config` holds none for the row.
    pub fn client_secret(
        &self,
        ring: &KeyRing,
        row: ProviderRow,
        config: &ConfigSecrets,
    ) -> Result<ClientSecret, SecretError> {
        self.secret.open(ring, row, config)
    }
}

#[cfg(test)]
mod tests;
