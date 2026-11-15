// SPDX-License-Identifier: AGPL-3.0-or-later
//! What the `config` of every outside sign-in provider shares, whatever its `kind`
//! ([`crate::oidc_config`], [`crate::oauth2_config`]): the problems validation reports, the
//! client secret and how it is sealed to its row.
//!
//! A validation lists every rule a config breaks as a [`ConfigProblem`]: a JSON Pointer to the
//! member and a stable code, the `errors` of the admin API's 422 `provider.config_invalid`
//! (`docs/specs/m0/06-oauth2-and-vatsim-preset.md` B14). No problem repeats a value.
//!
//! The client secret of a provider created through the admin API is stored only encrypted
//! (`client_secret: {"sealed": "<base64>"}`), under the key ring's current key and bound to the
//! row: the context is `[org_id, b"identity_providers", provider_id, b"client_secret"]`
//! ([`crate::encryption`]), so a sealed secret copied to another provider or organization does
//! not open. A provider declared in `stepclimb.toml` stores no secret at all (`05-oidc.md` Q2
//! (c)): its row carries `source: "config"` instead of `client_secret`, and the server holds the
//! secret it read from the file in [`ConfigSecrets`]. Nothing here prints a secret:
//! [`ClientSecret`], [`SealedSecret`] and [`ConfigSecrets`] leave it out of `Debug`.

pub(crate) mod walk;

use std::collections::HashMap;
use std::fmt;

use serde_json::{Map, Value};
use sha2::{Digest as _, Sha256};

use uuid::Uuid;

use crate::encryption::{DecryptError, EncryptError, KeyRing};

/// The member, in either kind's config, that marks the provider as trusted for matching users
/// by email (`docs/specs/m0/03-users-and-identities.md` Q2 (a), `07-admission.md` B5): a
/// boolean, `false` when absent. Admission reads it from the stored row and counts the provider
/// as trusted only when the member is exactly `true`.
pub const TRUSTED_FOR_EMAIL_MATCHING: &str = "trusted_for_email_matching";

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
    /// A string that is not an absolute `http` or `https` URL, or that has a fragment,
    /// whitespace or a control character, or a query where the member takes none (an OIDC
    /// issuer).
    pub const URL_INVALID: &str = "provider.url_invalid";
    /// An `authorize` URL whose query sets a parameter the sign-in request sets itself.
    pub const URL_PARAMETER_RESERVED: &str = "provider.url_parameter_reserved";
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

/// Why a stored config could not be read. The table holds only configs the config types wrote,
/// so this is corruption or a release mismatch; the read fails closed.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum StoredConfigError {
    #[error("the stored provider config breaks a rule at {pointer} ({code})")]
    Invalid { pointer: String, code: &'static str },
}

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

impl SealedSecret {
    /// The draft's secret sealed to `row` under the ring's current key, or the stored one, which
    /// was sealed to the same row.
    pub(crate) fn from_draft(
        draft: DraftSecret,
        ring: &KeyRing,
        row: ProviderRow,
    ) -> Result<Self, EncryptError> {
        match draft {
            DraftSecret::Kept(sealed) => Ok(sealed),
            DraftSecret::New(plain) => Ok(Self(
                ring.encrypt(plain.0.as_bytes(), &row.secret_context())?,
            )),
        }
    }

    /// Open the secret. It fails when the value was sealed to another row or under a key no
    /// longer in the ring.
    pub(crate) fn open(
        &self,
        ring: &KeyRing,
        row: ProviderRow,
    ) -> Result<ClientSecret, DecryptError> {
        let plain = ring.decrypt(&self.0, &row.secret_context())?;
        String::from_utf8(plain)
            .map(ClientSecret)
            .map_err(|_| DecryptError::Malformed)
    }
}

/// The row a secret is sealed to. The provider id exists before the row is written (UUID v7
/// from the application), so the secret can be sealed for the `INSERT`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
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

/// The member of a stored config that names who owns the row, and its one value: `config`, a
/// provider `stepclimb.toml` declares (`06-oauth2-and-vatsim-preset.md` Q2). A row without it
/// was written through the admin API.
pub const SOURCE: &str = "source";
/// [`SOURCE`]'s value for a provider declared in `stepclimb.toml`.
pub const SOURCE_CONFIG: &str = "config";

/// The member a provider's admission rules live in (`07-admission.md` I3). An admin changes it
/// through the API at any time, so [`Fingerprint`] leaves it out.
const ADMISSION_RULES: &str = "admission_rules";

/// A SHA-256 of a stored config, without its `admission_rules`, over a canonical JSON form
/// (object members sorted by name at every level), so the same config read back from `jsonb`
/// gives the same fingerprint. [`ConfigSecrets`] keeps one beside each secret: a row whose
/// config has changed since this process read `stepclimb.toml` (another replica with another
/// file) gets no secret.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct Fingerprint([u8; 32]);

impl Fingerprint {
    /// The fingerprint of the stored config `config`.
    #[must_use]
    pub fn of(config: &Map<String, Value>) -> Self {
        let mut text = String::new();
        for (name, value) in sorted(config) {
            if name != ADMISSION_RULES {
                canonical(&mut text, name, value);
            }
        }
        Self(Sha256::digest(text.as_bytes()).into())
    }
}

impl fmt::Debug for Fingerprint {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "Fingerprint({:02x}{:02x}{:02x}{:02x}..)",
            self.0[0], self.0[1], self.0[2], self.0[3]
        )
    }
}

fn sorted(map: &Map<String, Value>) -> Vec<(&String, &Value)> {
    let mut members: Vec<_> = map.iter().collect();
    members.sort_by(|a, b| a.0.cmp(b.0));
    members
}

/// `name` and `value` as JSON, members sorted, appended to `out` with a separator.
fn canonical(out: &mut String, name: &str, value: &Value) {
    out.push_str(&Value::String(name.to_owned()).to_string());
    out.push(':');
    canonical_value(out, value);
    out.push(',');
}

fn canonical_value(out: &mut String, value: &Value) {
    match value {
        Value::Object(map) => {
            out.push('{');
            for (name, value) in sorted(map) {
                canonical(out, name, value);
            }
            out.push('}');
        }
        Value::Array(items) => {
            out.push('[');
            for item in items {
                canonical_value(out, item);
                out.push(',');
            }
            out.push(']');
        }
        other => out.push_str(&other.to_string()),
    }
}

/// Where a stored config's client secret is.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum StoredSecret {
    /// Sealed in the row.
    Sealed(SealedSecret),
    /// In `stepclimb.toml`: the row has `source: "config"` and no secret, and the server holds
    /// the secret in [`ConfigSecrets`]. The fingerprint is the stored config's, when it was
    /// read from a row; a config built from the file, never stored, has none and opens no
    /// secret.
    ConfigFile(Option<Fingerprint>),
}

impl StoredSecret {
    /// The secret in the clear: opened from the row, or taken from `config` for the row.
    pub(crate) fn open(
        &self,
        ring: &KeyRing,
        row: ProviderRow,
        config: &ConfigSecrets,
    ) -> Result<ClientSecret, SecretError> {
        match self {
            Self::Sealed(sealed) => Ok(sealed.open(ring, row)?),
            Self::ConfigFile(stored) => {
                let (secret, read) = config.0.get(&row).ok_or(SecretError::NotInConfig)?;
                if *stored != Some(*read) {
                    return Err(SecretError::ConfigChanged);
                }
                Ok(secret.clone())
            }
        }
    }
}

/// Why a stored config's client secret could not be had.
#[derive(Debug, PartialEq, Eq, thiserror::Error)]
pub enum SecretError {
    #[error(transparent)]
    Decrypt(#[from] DecryptError),
    /// The row says its secret is in `stepclimb.toml`, and the file this server read declares
    /// no such provider.
    #[error("the provider's client secret is in stepclimb.toml, which declares no such provider")]
    NotInConfig,
    /// The row's config is not the one this server wrote from its `stepclimb.toml`: another
    /// server with another file has written it since. Replicas must share one file.
    #[error(
        "the provider's stored config is not the one this server's stepclimb.toml declares; \
         every server must read the same file"
    )]
    ConfigChanged,
}

/// The client secrets of the providers `stepclimb.toml` declares, by the row each belongs to,
/// each with the [`Fingerprint`] of the config this server wrote to the row; read when the
/// server starts and never stored. `Debug` shows how many there are, never one.
#[derive(Clone, Default)]
pub struct ConfigSecrets(HashMap<ProviderRow, (ClientSecret, Fingerprint)>);

impl ConfigSecrets {
    /// Hold `secret` for `row`, whose stored config has `fingerprint`, replacing any secret
    /// held for it.
    pub fn insert(&mut self, row: ProviderRow, secret: ClientSecret, fingerprint: Fingerprint) {
        self.0.insert(row, (secret, fingerprint));
    }

    /// The secret held for `row`, whatever the row holds now.
    #[must_use]
    pub fn get(&self, row: ProviderRow) -> Option<&ClientSecret> {
        self.0.get(&row).map(|(secret, _)| secret)
    }

    /// How many secrets are held.
    #[must_use]
    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// Whether none is held.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

impl fmt::Debug for ConfigSecrets {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "ConfigSecrets({} secrets)", self.0.len())
    }
}

impl ClientSecret {
    /// A secret read from `stepclimb.toml`, for [`ConfigSecrets`]. Empty text is refused.
    #[must_use]
    pub fn from_config_file(text: String) -> Option<Self> {
        (!text.is_empty()).then_some(Self(text))
    }
}

/// The secret of a validated config: a new one from the admin, or the one already stored.
#[derive(Clone, Debug)]
pub enum DraftSecret {
    New(ClientSecret),
    Kept(SealedSecret),
}
