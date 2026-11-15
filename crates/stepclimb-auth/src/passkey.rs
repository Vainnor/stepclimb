// SPDX-License-Identifier: AGPL-3.0-or-later
//! WebAuthn passkeys (docs/specs/m0/04-local-accounts.md B8, Q12), on `webauthn-rs`.
//!
//! A [`RelyingParty`] is built from the organization's public origin
//! (docs/specs/m0/08-sessions-and-csrf.md Q1: `STEPCLIMB_PUBLIC_URL` in M0). Its relying party ID
//! is that origin's host exactly, so a passkey registered at `lms.example.org` works there and
//! nowhere else, and the origin a browser reports must be that origin exactly: no subdomain, no
//! other port.
//!
//! Registration takes two requests. [`RelyingParty::start_registration`] returns the credential
//! creation options for the browser and the ceremony's state, which the caller keeps on the
//! server (sealed in a sign-in flow) and never sends to the client. The options follow Q12:
//! user verification required, no attestation, and the user's existing passkeys excluded. A
//! resident (discoverable) key is not required. [`RelyingParty::finish_registration`] checks the
//! browser's response against that state (challenge, origin, relying party ID, user
//! verification) and returns the credential to store, with its id.
//!
//! The types of `webauthn-rs` stay inside this module: callers pass and receive JSON and bytes.

use serde::Deserialize;
use serde_json::Value;
use webauthn_rs::prelude::{
    PasskeyRegistration, RegisterPublicKeyCredential, Url, Uuid, Webauthn, WebauthnBuilder,
    WebauthnError,
};

/// Why a public origin cannot be a relying party.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum ConfigError {
    /// The origin does not parse as a URL.
    #[error("the public URL is not a URL")]
    NotAUrl,
    /// The origin's host is an IP address; a WebAuthn relying party ID is a domain name.
    #[error("the public URL's host must be a domain name for passkeys, not an IP address")]
    NotADomain,
    /// `webauthn-rs` refused the relying party.
    #[error("the public URL cannot be a WebAuthn relying party")]
    Refused,
}

/// Why a registration could not start.
#[derive(Debug, thiserror::Error)]
pub enum StartError {
    #[error("could not create the passkey registration options: {0}")]
    Webauthn(#[from] WebauthnError),
    #[error("could not serialize the passkey registration: {0}")]
    Serialize(#[from] serde_json::Error),
}

/// Why a registration response was not accepted.
#[derive(Debug, thiserror::Error)]
pub enum FinishError {
    /// The response is not a WebAuthn registration response. The client's to fix.
    #[error("the response is not a passkey registration")]
    Malformed(#[source] serde_json::Error),
    /// The response failed a check: another challenge, origin or relying party ID, no user
    /// verification, or a bad signature. The client's, or an attacker's.
    #[error("the passkey registration was refused: {0}")]
    Refused(#[source] WebauthnError),
    /// The stored state did not parse: a server fault, since only [`RelyingParty`] writes it.
    #[error("the stored passkey registration state is unreadable: {0}")]
    State(#[source] serde_json::Error),
    /// The verified credential did not serialize.
    #[error("could not serialize the passkey: {0}")]
    Serialize(#[source] serde_json::Error),
}

/// The user a passkey is registered for, as the authenticator may show and store them.
#[derive(Clone, Copy, Debug)]
pub struct Registrant<'a> {
    /// The WebAuthn user handle: the user's id, never their email.
    pub user_id: Uuid,
    /// The account name, such as the user's email.
    pub name: &'a str,
    /// The name shown beside it.
    pub display_name: &'a str,
}

/// A registration [`RelyingParty::start_registration`] started.
pub struct Started {
    /// The `CredentialCreationOptions` for `navigator.credentials.create`, as JSON
    /// (`{"publicKey": {...}}`).
    pub options: Value,
    /// The ceremony's state, holding the challenge. Keep it on the server, sealed; never send
    /// it to the client or log it.
    pub state: String,
}

/// Leaves the state out.
impl std::fmt::Debug for Started {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Started").finish_non_exhaustive()
    }
}

/// A credential [`RelyingParty::finish_registration`] verified.
#[derive(Clone, Debug, PartialEq)]
pub struct Registered {
    /// The credential id the authenticator chose; at most one passkey per organization has it.
    pub credential_id: Vec<u8>,
    /// The credential as `webauthn-rs` serializes it: the public key, the sign counter, the
    /// backup flags. No secret.
    pub credential: Value,
}

/// One organization's WebAuthn relying party.
#[derive(Clone, Debug)]
pub struct RelyingParty(Webauthn);

impl RelyingParty {
    /// The relying party for `origin` (`https://lms.example.org`), shown to users as `name`. Its
    /// ID is `origin`'s host, and `origin` is the only origin it accepts.
    pub fn new(origin: &str, name: &str) -> Result<Self, ConfigError> {
        let url = Url::parse(origin).map_err(|_| ConfigError::NotAUrl)?;
        let host = url.domain().ok_or(ConfigError::NotADomain)?.to_owned();
        let webauthn = WebauthnBuilder::new(&host, &url)
            .map_err(|_| ConfigError::Refused)?
            .rp_name(name)
            .build()
            .map_err(|_| ConfigError::Refused)?;
        Ok(Self(webauthn))
    }

    /// Start registering a passkey for `user`, excluding the credentials of `existing` (the
    /// user's passkeys) so an authenticator cannot be registered twice.
    pub fn start_registration(
        &self,
        user: Registrant<'_>,
        existing: Vec<Vec<u8>>,
    ) -> Result<Started, StartError> {
        let exclude = (!existing.is_empty()).then_some(existing);
        let (options, state) = self.0.start_passkey_registration(
            user.user_id,
            user.name,
            user.display_name,
            exclude,
        )?;
        Ok(Started {
            options: serde_json::to_value(options)?,
            state: serde_json::to_string(&state)?,
        })
    }

    /// Check the browser's registration `response` against `state`, which
    /// [`RelyingParty::start_registration`] returned, and return the credential to store.
    pub fn finish_registration(
        &self,
        response: &Value,
        state: &str,
    ) -> Result<Registered, FinishError> {
        let state: PasskeyRegistration = serde_json::from_str(state).map_err(FinishError::State)?;
        let response =
            RegisterPublicKeyCredential::deserialize(response).map_err(FinishError::Malformed)?;
        let passkey = self
            .0
            .finish_passkey_registration(&response, &state)
            .map_err(FinishError::Refused)?;
        Ok(Registered {
            credential_id: passkey.cred_id().clone(),
            credential: serde_json::to_value(&passkey).map_err(FinishError::Serialize)?,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use webauthn_authenticator_rs::{WebauthnAuthenticator, softpasskey::SoftPasskey};
    use webauthn_rs::prelude::CreationChallengeResponse;

    const ORIGIN: &str = "https://lms.example.test";

    fn party() -> RelyingParty {
        RelyingParty::new(ORIGIN, "Example Org").expect("a relying party")
    }

    fn registrant() -> Registrant<'static> {
        Registrant {
            user_id: Uuid::from_u128(0x0190_0000_0000_7000_8000_0000_0000_0001),
            name: "sam@example.test",
            display_name: "Sam Example",
        }
    }

    /// What a browser at `origin` answers to `options` after `edit` changed them, with a soft
    /// passkey that verifies its user when asked to.
    fn respond_to(options: &Value, origin: &str, edit: impl FnOnce(&mut Value)) -> Value {
        let mut options = options.clone();
        edit(&mut options);
        let options: CreationChallengeResponse =
            serde_json::from_value(options).expect("creation options");
        let mut authenticator = SoftPasskey::new(true);
        let response = authenticator
            .do_registration(Url::parse(origin).expect("a URL"), options)
            .expect("the soft passkey registers");
        serde_json::to_value(response).expect("JSON")
    }

    fn respond(options: &Value, origin: &str) -> Value {
        respond_to(options, origin, |_| {})
    }

    #[test]
    fn the_relying_party_id_is_the_public_hosts_name() {
        let started = party()
            .start_registration(registrant(), Vec::new())
            .unwrap();
        let public_key = &started.options["publicKey"];
        assert_eq!(public_key["rp"]["id"], "lms.example.test");
        assert_eq!(public_key["rp"]["name"], "Example Org");
        assert_eq!(public_key["user"]["name"], "sam@example.test");
        assert_eq!(public_key["user"]["displayName"], "Sam Example");
    }

    #[test]
    fn the_options_follow_the_policy() {
        let existing = vec![vec![7_u8; 16]];
        let started = party().start_registration(registrant(), existing).unwrap();
        let public_key = &started.options["publicKey"];
        assert_eq!(public_key["attestation"], "none");
        let selection = &public_key["authenticatorSelection"];
        assert_eq!(selection["userVerification"], "required");
        assert_eq!(selection["requireResidentKey"], false);
        assert_eq!(
            public_key["excludeCredentials"][0]["id"],
            "BwcHBwcHBwcHBwcHBwcHBw"
        );
    }

    #[test]
    fn an_ip_address_cannot_be_a_relying_party() {
        let refused = RelyingParty::new("https://192.0.2.10", "Example Org").unwrap_err();
        assert_eq!(refused, ConfigError::NotADomain);
        assert_eq!(
            RelyingParty::new("not a url", "Example Org").unwrap_err(),
            ConfigError::NotAUrl
        );
    }

    #[test]
    fn a_soft_passkey_registers() {
        let party = party();
        let started = party.start_registration(registrant(), Vec::new()).unwrap();
        let response = respond(&started.options, ORIGIN);
        let registered = party
            .finish_registration(&response, &started.state)
            .unwrap();
        assert!(!registered.credential_id.is_empty());
        let printed = registered.credential.to_string();
        assert!(printed.contains("counter"), "{printed}");
    }

    /// The origin must be the public origin exactly: another port or a subdomain of it is
    /// refused, though an authenticator accepts both for the relying party ID.
    #[test]
    fn another_origin_is_refused() {
        let party = party();
        for origin in [
            "https://lms.example.test:8443",
            "https://sub.lms.example.test",
        ] {
            let started = party.start_registration(registrant(), Vec::new()).unwrap();
            let response = respond(&started.options, origin);
            let refused = party.finish_registration(&response, &started.state);
            assert!(
                matches!(
                    refused,
                    Err(FinishError::Refused(WebauthnError::InvalidRPOrigin))
                ),
                "{origin}: {refused:?}"
            );
        }
    }

    /// Our challenge and origin, but signed for the parent domain's relying party ID.
    #[test]
    fn another_relying_party_id_is_refused() {
        let party = party();
        let started = party.start_registration(registrant(), Vec::new()).unwrap();
        let response = respond_to(&started.options, ORIGIN, |options| {
            options["publicKey"]["rp"]["id"] = "example.test".into();
        });
        let refused = party.finish_registration(&response, &started.state);
        assert!(
            matches!(
                refused,
                Err(FinishError::Refused(WebauthnError::InvalidRPIDHash))
            ),
            "{refused:?}"
        );
    }

    #[test]
    fn another_challenge_is_refused() {
        let party = party();
        let first = party.start_registration(registrant(), Vec::new()).unwrap();
        let second = party.start_registration(registrant(), Vec::new()).unwrap();
        let response = respond(&first.options, ORIGIN);
        let refused = party.finish_registration(&response, &second.state);
        assert!(
            matches!(
                refused,
                Err(FinishError::Refused(WebauthnError::MismatchedChallenge))
            ),
            "{refused:?}"
        );
    }

    /// A response without user verification is refused (Q12), even when the browser was told
    /// it could skip it.
    #[test]
    fn a_passkey_that_does_not_verify_its_user_is_refused() {
        let party = party();
        let started = party.start_registration(registrant(), Vec::new()).unwrap();
        let response = respond_to(&started.options, ORIGIN, |options| {
            options["publicKey"]["authenticatorSelection"]["userVerification"] =
                "discouraged".into();
        });
        let refused = party.finish_registration(&response, &started.state);
        assert!(
            matches!(
                refused,
                Err(FinishError::Refused(WebauthnError::UserNotVerified))
            ),
            "{refused:?}"
        );
    }

    #[test]
    fn a_response_that_is_not_a_registration_is_malformed() {
        let party = party();
        let started = party.start_registration(registrant(), Vec::new()).unwrap();
        let refused = party.finish_registration(&serde_json::json!({"id": 1}), &started.state);
        assert!(
            matches!(refused, Err(FinishError::Malformed(_))),
            "{refused:?}"
        );
    }

    #[test]
    fn debug_output_leaves_the_state_out() {
        let started = party()
            .start_registration(registrant(), Vec::new())
            .unwrap();
        let printed = format!("{started:?}");
        assert!(!printed.contains("challenge"), "{printed}");
        assert_eq!(printed, "Started { .. }");
    }
}
