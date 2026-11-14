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
//!
//! Any other member is refused, so a misspelled one cannot be silently ignored. The subject is
//! always the ID token's `sub` (B12), so `claims.subject` is refused too.
//!
//! The validated config is sealed with [`OidcConfigDraft::seal`] into an [`OidcConfig`], whose
//! [`OidcConfig::to_json`] is what the row stores. The client secret is stored only encrypted
//! (`client_secret: {"sealed": "<base64>"}`), under the key ring's current key and bound to the
//! row: the context is `[org_id, b"identity_providers", provider_id, b"client_secret"]`
//! ([`crate::encryption`]). Nothing here prints a secret: [`ClientSecret`] and [`OidcConfig`]
//! leave it out of `Debug`, and no problem repeats a value.

mod parse;

use std::collections::BTreeMap;
use std::fmt;

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use serde_json::{Map, Value};
use uuid::Uuid;

use crate::encryption::{DecryptError, EncryptError, KeyRing};
use crate::json_pointer::JsonPointer;
use crate::outbound::UrlPolicy;

/// The scopes of a config that names none (B1).
pub const DEFAULT_SCOPES: [&str; 3] = ["openid", "email", "profile"];

/// The codes of [`ConfigProblem`]s.
pub mod code {
    /// A required member is missing.
    pub const FIELD_REQUIRED: &str = "provider.field_required";
    /// A member the config does not have.
    pub const FIELD_UNKNOWN: &str = "provider.field_unknown";
    /// A member of the wrong JSON type.
    pub const FIELD_TYPE: &str = "provider.field_type";
    /// An empty string where one is required.
    pub const FIELD_EMPTY: &str = "provider.field_empty";
    /// A string that is not an absolute URL, or has a query, fragment or other scheme.
    pub const URL_INVALID: &str = "provider.url_invalid";
    /// A URL the outbound URL policy refuses (`05-oidc.md` Q4).
    pub const URL_REFUSED: &str = "provider.url_refused";
    /// `scopes` without `openid`.
    pub const SCOPES_MISSING_OPENID: &str = "provider.scopes_missing_openid";
    /// A scope that is not an RFC 6749 scope token.
    pub const SCOPE_INVALID: &str = "provider.scope_invalid";
    /// A claim mapping that is not a JSON Pointer naming a member.
    pub const POINTER_INVALID: &str = "provider.pointer_invalid";
    /// A string holding NUL, which a `jsonb` column cannot store.
    pub const TEXT_CONTAINS_NUL: &str = "text.contains_nul";
}

/// One rule a config broke.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConfigProblem {
    /// A JSON Pointer to the member, relative to the config: `/issuer`, `/claims/given_name`.
    pub pointer: String,
    /// One of [`code`].
    pub code: &'static str,
}

/// Every rule a config broke, in the order they were checked.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
#[error("the provider config breaks {} rule(s)", .0.len())]
pub struct ConfigErrors(pub Vec<ConfigProblem>);

/// A client secret in the clear. `Debug` never shows it; it leaves the type only through
/// [`ClientSecret::expose_secret`], for the token request. No `PartialEq`: comparing a secret
/// with `==` is not constant time.
#[derive(Clone)]
pub struct ClientSecret(String);

impl ClientSecret {
    /// The secret's text.
    #[must_use]
    pub fn expose_secret(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for ClientSecret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("ClientSecret(<redacted>)")
    }
}

/// A client secret as stored: the key ring's sealed bytes.
#[derive(Clone, PartialEq, Eq)]
pub struct SealedSecret(Vec<u8>);

impl fmt::Debug for SealedSecret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "SealedSecret({} bytes)", self.0.len())
    }
}

/// The row a secret is sealed to. The provider id exists before the row is written (UUID v7
/// from the application), so the secret can be sealed for the `INSERT`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ProviderRow {
    pub org_id: Uuid,
    pub provider_id: Uuid,
}

impl ProviderRow {
    fn secret_context(&self) -> [&[u8]; 4] {
        [
            self.org_id.as_bytes(),
            b"identity_providers",
            self.provider_id.as_bytes(),
            b"client_secret",
        ]
    }
}

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

/// The members every config has, whatever the state of its secret.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OidcSettings {
    /// The issuer exactly as the admin wrote it.
    pub issuer: String,
    pub client_id: String,
    pub scopes: Vec<String>,
    pub claims: ClaimsMapping,
    pub userinfo: bool,
}

/// The secret of a validated config: a new one from the admin, or the one already stored.
#[derive(Clone, Debug)]
pub enum DraftSecret {
    New(ClientSecret),
    Kept(SealedSecret),
}

/// A validated config, before its secret is sealed.
#[derive(Clone, Debug)]
pub struct OidcConfigDraft {
    pub settings: OidcSettings,
    pub secret: DraftSecret,
}

/// A config as stored: its settings and its sealed secret.
#[derive(Clone, PartialEq, Eq)]
pub struct OidcConfig {
    pub settings: OidcSettings,
    secret: SealedSecret,
}

impl fmt::Debug for OidcConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("OidcConfig")
            .field("settings", &self.settings)
            .finish_non_exhaustive()
    }
}

/// Why a stored config could not be read. The table holds only configs [`OidcConfig::to_json`]
/// wrote, so this is corruption or a release mismatch; the read fails closed.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum StoredConfigError {
    #[error("the stored OIDC config breaks a rule at {pointer} ({code})")]
    Invalid { pointer: String, code: &'static str },
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
        let secret = match self.secret {
            DraftSecret::Kept(sealed) => sealed,
            DraftSecret::New(plain) => {
                SealedSecret(ring.encrypt(plain.0.as_bytes(), &row.secret_context())?)
            }
        };
        Ok(OidcConfig {
            settings: self.settings,
            secret,
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
        let attributes: Map<String, Value> = s
            .claims
            .attributes
            .iter()
            .map(|(name, pointer)| (name.clone(), pointer.as_str().into()))
            .collect();
        claims.insert("attributes".into(), Value::Object(attributes));
        let mut secret = Map::new();
        secret.insert("sealed".into(), STANDARD.encode(&self.secret.0).into());
        let mut config = Map::new();
        config.insert("issuer".into(), s.issuer.as_str().into());
        config.insert("client_id".into(), s.client_id.as_str().into());
        config.insert("client_secret".into(), Value::Object(secret));
        config.insert("scopes".into(), s.scopes.clone().into());
        config.insert("claims".into(), Value::Object(claims));
        config.insert("userinfo".into(), s.userinfo.into());
        config
    }

    /// Read a stored config. Members other than the config's (the row's `label`, for one) are
    /// left to their readers. The URL policy is not applied here: the client applies it at
    /// every connection.
    pub fn from_stored(config: &Map<String, Value>) -> Result<Self, StoredConfigError> {
        parse::stored(config)
    }

    /// The sealed secret, for a change that keeps it ([`validate`]'s `stored`).
    #[must_use]
    pub fn sealed_secret(&self) -> SealedSecret {
        self.secret.clone()
    }

    /// Open the client secret, for the token request. It fails when the value was sealed to
    /// another row or under a key no longer in the ring.
    pub fn client_secret(
        &self,
        ring: &KeyRing,
        row: ProviderRow,
    ) -> Result<ClientSecret, DecryptError> {
        let plain = ring.decrypt(&self.secret.0, &row.secret_context())?;
        String::from_utf8(plain)
            .map(ClientSecret)
            .map_err(|_| DecryptError::Malformed)
    }
}

#[cfg(test)]
mod tests;
