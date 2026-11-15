// SPDX-License-Identifier: AGPL-3.0-or-later
//! The WebAuthn credential creation and request options the API answers, as typed members, so
//! the OpenAPI document and the generated client describe them (W3C Web Authentication Level 3,
//! `PublicKeyCredentialCreationOptionsJSON` and `PublicKeyCredentialRequestOptionsJSON`).
//!
//! `stepclimb_auth::passkey` produces them as JSON; [`PasskeyCreationOptions::from_json`] and
//! [`PasskeyRequestOptions::from_json`] read that JSON into these types and refuse any member
//! they do not declare, so a `webauthn-rs` release that adds one fails every passkey test
//! instead of reaching clients undocumented.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use utoipa::ToSchema;

/// `CredentialCreationOptions` as JSON: pass `publicKey` to
/// `PublicKeyCredential.parseCreationOptionsFromJSON`, then to `navigator.credentials.create`.
#[derive(Debug, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct PasskeyCreationOptions {
    #[serde(rename = "publicKey")]
    pub public_key: PublicKeyCreationOptions,
}

impl PasskeyCreationOptions {
    /// Read the options `stepclimb_auth::passkey` made.
    pub fn from_json(options: Value) -> Result<Self, serde_json::Error> {
        serde_json::from_value(options)
    }
}

/// `PublicKeyCredentialCreationOptionsJSON`.
#[derive(Debug, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PublicKeyCreationOptions {
    pub rp: RelyingPartyInfo,
    pub user: UserInfo,
    /// base64url, unpadded.
    pub challenge: String,
    pub pub_key_cred_params: Vec<CredentialParameter>,
    /// Milliseconds.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schema(nullable = false)]
    pub timeout: Option<u32>,
    /// The user's registered passkeys, which the authenticator must not register again.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schema(nullable = false)]
    pub exclude_credentials: Option<Vec<CredentialDescriptor>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schema(nullable = false)]
    pub authenticator_selection: Option<AuthenticatorSelection>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schema(nullable = false)]
    pub hints: Option<Vec<String>>,
    /// `none`: the server does not ask which device this is.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schema(nullable = false)]
    pub attestation: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schema(nullable = false)]
    pub attestation_formats: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schema(nullable = false)]
    pub extensions: Option<RegistrationExtensions>,
}

/// `PublicKeyCredentialRpEntity`.
#[derive(Debug, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct RelyingPartyInfo {
    /// The relying party ID: the instance's host name.
    pub id: String,
    /// The organization's name.
    pub name: String,
}

/// `PublicKeyCredentialUserEntityJSON`.
#[derive(Debug, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct UserInfo {
    /// The user handle, base64url, unpadded: the user's id.
    pub id: String,
    /// The user's email.
    pub name: String,
    pub display_name: String,
}

/// `PublicKeyCredentialParameters`.
#[derive(Debug, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct CredentialParameter {
    /// A COSE algorithm identifier, such as -7 (ES256).
    pub alg: i32,
    #[serde(rename = "type")]
    pub credential_type: String,
}

/// `PublicKeyCredentialDescriptorJSON`.
#[derive(Debug, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct CredentialDescriptor {
    /// The credential id, base64url, unpadded.
    pub id: String,
    #[serde(rename = "type")]
    pub credential_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schema(nullable = false)]
    pub transports: Option<Vec<String>>,
}

/// `AuthenticatorSelectionCriteria`.
#[derive(Debug, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AuthenticatorSelection {
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schema(nullable = false)]
    pub authenticator_attachment: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schema(nullable = false)]
    pub resident_key: Option<String>,
    pub require_resident_key: bool,
    /// `required`: the authenticator checks a PIN or biometric.
    pub user_verification: String,
}

/// `AuthenticationExtensionsClientInputs` for a registration.
#[derive(Debug, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RegistrationExtensions {
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schema(nullable = false)]
    pub credential_protection_policy: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schema(nullable = false)]
    pub enforce_credential_protection_policy: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schema(nullable = false)]
    pub uvm: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schema(nullable = false)]
    pub cred_props: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schema(nullable = false)]
    pub min_pin_length: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schema(nullable = false)]
    pub hmac_create_secret: Option<bool>,
}

/// `CredentialRequestOptions` as JSON: pass `publicKey` to
/// `PublicKeyCredential.parseRequestOptionsFromJSON`, then to `navigator.credentials.get`.
#[derive(Debug, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct PasskeyRequestOptions {
    #[serde(rename = "publicKey")]
    pub public_key: PublicKeyRequestOptions,
}

impl PasskeyRequestOptions {
    /// Read the options `stepclimb_auth::passkey` made.
    pub fn from_json(options: Value) -> Result<Self, serde_json::Error> {
        serde_json::from_value(options)
    }
}

/// `PublicKeyCredentialRequestOptionsJSON`.
#[derive(Debug, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PublicKeyRequestOptions {
    /// base64url, unpadded.
    pub challenge: String,
    /// Milliseconds.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schema(nullable = false)]
    pub timeout: Option<u32>,
    /// The relying party ID: the instance's host name.
    pub rp_id: String,
    /// The passkeys the account may sign in with. Empty when the address has no account or
    /// the account has no passkey: the answer looks the same either way.
    pub allow_credentials: Vec<CredentialDescriptor>,
    /// `required`: the authenticator checks a PIN or biometric.
    pub user_verification: String,
}
