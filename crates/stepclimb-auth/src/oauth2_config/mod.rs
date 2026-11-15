// SPDX-License-Identifier: AGPL-3.0-or-later
//! The `config` of an `identity_providers` row with `kind = 'oauth2'`
//! (`docs/specs/m0/06-oauth2-and-vatsim-preset.md` B4, B6, Q3; `05-oidc.md` Q2, Q4).
//!
//! An admin's config is checked by [`validate`], which lists every rule it breaks as a
//! [`ConfigProblem`], as [`crate::oidc_config`] does. The members are:
//!
//! | Member | Rule | When absent |
//! | --- | --- | --- |
//! | `authorize`, `token`, `userinfo` | an absolute `https` URL (`http` with the insecure switch) with no fragment or credentials, under the [`UrlPolicy`]; a query is allowed (RFC 6749 3.1, 3.2), except that the `authorize` query may not set a parameter the authorization request sets (`response_type`, `client_id`, `redirect_uri`, `scope`, `state`, `code_challenge`, `code_challenge_method`, `nonce`); kept exactly as written | refused |
//! | `client_id` | a non-empty string | refused |
//! | `client_secret` | a non-empty string, write-only | the stored secret, or refused when there is none |
//! | `scopes` | scope tokens (RFC 6749 3.3) | `[]` |
//! | `pkce` | a boolean: whether sign-in sends S256 PKCE (Q3); `false` removes the protection against an injected authorization code (a code issued for someone else, put into an attacker's own callback), so turn it off only for a provider that refuses PKCE | `true` |
//! | `claims` | an object with `subject`, a JSON Pointer, and optional `given_name`, `family_name`, `email`, `email_verified`, `groups` pointers and `attributes`, an object of names to pointers | refused |
//! | `allow_local_passkeys` | a boolean: whether users who sign in through this provider may also add passkeys (`04-local-accounts.md` B13) | `false` |
//!
//! A provider declared in `stepclimb.toml` is validated the same way, then stored with
//! [`Oauth2Config::from_config_file`]: no secret in the row, `source: "config"` in its place
//! (`crate::provider_config`).
//!
//! Any other member is refused, so a misspelled one cannot be silently ignored. A profile field
//! with no pointer is not mapped: unlike OIDC, a userinfo body has no standard shape, so there
//! are no defaults to fall back on. Mapping a userinfo body is [`Oauth2Claims::map`], over the
//! shared [`crate::claims`].
//!
//! The validated config is sealed with [`Oauth2ConfigDraft::seal`] into an [`Oauth2Config`],
//! whose [`Oauth2Config::to_json`] is what the row stores, the client secret sealed to the row as
//! [`crate::provider_config`] says.

mod parse;

pub(crate) use parse::RESERVED_PARAMETERS;

use std::fmt;

use serde_json::{Map, Value};

pub use crate::claims::MappedSignIn;
use crate::claims::{self, ProfileMapping, SubjectMissing};
use crate::encryption::{EncryptError, KeyRing};
use crate::json_pointer::JsonPointer;
use crate::outbound::UrlPolicy;
use crate::provider_config::walk;
pub use crate::provider_config::{
    ClientSecret, ConfigErrors, ConfigProblem, ConfigSecrets, DraftSecret, ProviderRow,
    SealedSecret, SecretError, StoredConfigError, StoredSecret, code,
};

/// Where a config's claims come from: JSON Pointers into the userinfo body (B6).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Oauth2Claims {
    pub subject: JsonPointer,
    pub profile: ProfileMapping,
}

impl Oauth2Claims {
    /// The subject and profile of a userinfo body, or [`SubjectMissing`] when it has no usable
    /// subject. When the mapping names an `email_verified` pointer and the body's value there is
    /// not exactly `true`, the email is left out (spec 06 B10): the admin said the provider
    /// vouches for its emails only when that flag is set, so admission never sees an address
    /// the provider did not vouch for.
    pub fn map(&self, userinfo: &Value) -> Result<MappedSignIn, SubjectMissing> {
        let mut profile = claims::profile(userinfo, &self.profile);
        if self.profile.email_verified.is_some() && profile.email_verified != Some(true) {
            profile.email = None;
        }
        Ok(MappedSignIn {
            subject: claims::subject(userinfo, &self.subject)?,
            profile,
        })
    }
}

/// The members every config has, whatever the state of its secret.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Oauth2Settings {
    /// The authorization endpoint exactly as the admin wrote it.
    pub authorize: String,
    /// The token endpoint exactly as the admin wrote it.
    pub token: String,
    /// The userinfo endpoint exactly as the admin wrote it.
    pub userinfo: String,
    pub client_id: String,
    pub scopes: Vec<String>,
    pub pkce: bool,
    pub claims: Oauth2Claims,
    pub allow_local_passkeys: bool,
}

/// A validated config, before its secret is sealed.
#[derive(Clone, Debug)]
pub struct Oauth2ConfigDraft {
    pub settings: Oauth2Settings,
    pub secret: DraftSecret,
}

/// A config as stored: its settings and where its secret is.
#[derive(Clone, PartialEq, Eq)]
pub struct Oauth2Config {
    pub settings: Oauth2Settings,
    secret: StoredSecret,
}

impl fmt::Debug for Oauth2Config {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Oauth2Config")
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
) -> Result<Oauth2ConfigDraft, ConfigErrors> {
    parse::draft(config, policy, stored)
}

impl Oauth2ConfigDraft {
    /// Seal a new secret to `row` under the ring's current key, or keep the stored one, which
    /// was sealed to the same row.
    pub fn seal(self, ring: &KeyRing, row: ProviderRow) -> Result<Oauth2Config, EncryptError> {
        Ok(Oauth2Config {
            settings: self.settings,
            secret: StoredSecret::Sealed(SealedSecret::from_draft(self.secret, ring, row)?),
        })
    }
}

/// `pointer` under `name` in `map`, when there is one.
fn insert_pointer(map: &mut Map<String, Value>, name: &str, pointer: Option<&JsonPointer>) {
    if let Some(pointer) = pointer {
        map.insert(name.into(), pointer.as_str().into());
    }
}

impl Oauth2Config {
    /// The JSON object the row's `config` stores.
    #[must_use]
    pub fn to_json(&self) -> Map<String, Value> {
        let s = &self.settings;
        let p = &s.claims.profile;
        let mut claims = Map::new();
        claims.insert("subject".into(), s.claims.subject.as_str().into());
        insert_pointer(&mut claims, "given_name", p.given_name.as_ref());
        insert_pointer(&mut claims, "family_name", p.family_name.as_ref());
        insert_pointer(&mut claims, "email", p.email.as_ref());
        insert_pointer(&mut claims, "email_verified", p.email_verified.as_ref());
        insert_pointer(&mut claims, "groups", p.groups.as_ref());
        claims.insert("attributes".into(), walk::attributes_json(&p.attributes));
        let mut config = Map::new();
        config.insert("authorize".into(), s.authorize.as_str().into());
        config.insert("token".into(), s.token.as_str().into());
        config.insert("userinfo".into(), s.userinfo.as_str().into());
        config.insert("client_id".into(), s.client_id.as_str().into());
        walk::insert_stored_secret(&mut config, &self.secret);
        config.insert("scopes".into(), s.scopes.clone().into());
        config.insert("pkce".into(), s.pkce.into());
        config.insert("claims".into(), Value::Object(claims));
        config.insert("allow_local_passkeys".into(), s.allow_local_passkeys.into());
        config
    }

    /// Read a stored config. Members other than the config's (the row's `label`, for one) are
    /// left to their readers. Neither the URL policy nor the `authorize` query rule is applied
    /// here: the outbound client checks the `token` and `userinfo` URLs at every connection, but
    /// nothing fetches `authorize`, which the browser is sent to, so the sign-in start must check
    /// it against the policy again before redirecting (as `05-oidc.md` does for an
    /// authorization endpoint from discovery).
    pub fn from_stored(config: &Map<String, Value>) -> Result<Self, StoredConfigError> {
        parse::stored(config)
    }

    /// The config of a provider `stepclimb.toml` declares: `settings` validated by [`validate`],
    /// with the secret left to [`ConfigSecrets`]. Its [`Oauth2Config::to_json`] has `source:
    /// "config"` and no `client_secret`.
    #[must_use]
    pub fn from_config_file(settings: Oauth2Settings) -> Self {
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
