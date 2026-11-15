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
//! Sign-in takes two requests too (B11, B12). [`RelyingParty::start_authentication`] returns
//! the credential request options, listing the credentials it is given (none for an address
//! with no passkey, spec 04 Q9), and the ceremony's state. [`Assertion::parse`] reads the
//! browser's answer, whose [`Assertion::credential_id`] names the stored credential to check it
//! against, and [`RelyingParty::finish_authentication`] verifies it: the state's challenge,
//! origin and relying party ID, user verification (Q12: required), the signature with the
//! public key, a user handle naming the expected user, and the sign counter against the stored
//! one. It returns the credential with its counter and backup state brought up to date.
//!
//! The types of `webauthn-rs` stay inside this module: callers pass and receive JSON and bytes.

use serde::Deserialize;
use serde_json::Value;
use webauthn_rs::prelude::{
    Passkey, PasskeyAuthentication, PasskeyRegistration, PublicKeyCredential,
    RegisterPublicKeyCredential, Url, Uuid, Webauthn, WebauthnBuilder, WebauthnError,
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

/// Why a registration or a sign-in could not start.
#[derive(Debug, thiserror::Error)]
pub enum StartError {
    #[error("could not create the passkey options: {0}")]
    Webauthn(#[from] WebauthnError),
    #[error("could not serialize the passkey ceremony: {0}")]
    Serialize(#[from] serde_json::Error),
    /// A stored credential did not parse: a server fault, since only [`RelyingParty`] writes it.
    #[error("a stored passkey is unreadable: {0}")]
    Stored(#[source] serde_json::Error),
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

/// A sign-in [`RelyingParty::start_authentication`] started.
pub struct SignInStarted {
    /// The `CredentialRequestOptions` for `navigator.credentials.get`, as JSON
    /// (`{"publicKey": {...}}`).
    pub options: Value,
    /// The ceremony's state, holding the challenge and the allowed credentials. Keep it on the
    /// server, sealed; never send it to the client or log it.
    pub state: String,
}

/// Leaves the state out.
impl std::fmt::Debug for SignInStarted {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SignInStarted").finish_non_exhaustive()
    }
}

/// A browser's answer to sign-in request options: a WebAuthn assertion, not yet verified.
pub struct Assertion(PublicKeyCredential);

/// Leaves the signature and the client data out.
impl std::fmt::Debug for Assertion {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Assertion").finish_non_exhaustive()
    }
}

impl Assertion {
    /// Read `response` (`PublicKeyCredential.toJSON()` of `navigator.credentials.get`).
    pub fn parse(response: &Value) -> Result<Self, serde_json::Error> {
        PublicKeyCredential::deserialize(response).map(Self)
    }

    /// The id of the credential the authenticator signed with (`rawId`): which stored passkey
    /// to verify the assertion against. The client chose it; it proves nothing until
    /// [`RelyingParty::finish_authentication`] has verified the assertion.
    pub fn credential_id(&self) -> &[u8] {
        self.0.get_credential_id()
    }
}

/// Why an assertion did not sign in.
#[derive(Debug, thiserror::Error)]
pub enum SignInError {
    /// `webauthn-rs` refused it: another challenge, origin or relying party ID, no user
    /// verification, a bad signature, a credential the ceremony did not allow, or a sign counter
    /// that did not go up against the one the ceremony started with. The client's, or an
    /// attacker's.
    #[error("the passkey sign-in was refused: {0}")]
    Refused(#[source] WebauthnError),
    /// The assertion names a user handle other than the expected user's id.
    #[error("the passkey's user handle names another user")]
    UserHandleMismatch,
    /// The stored credential is not the one the assertion was signed with.
    #[error("the assertion is for another credential")]
    WrongCredential,
    /// The sign counter did not go up against the stored one (spec 04 Q12): two copies of the
    /// passkey may exist. Only a counter of 0 on both sides, a passkey without a counter, passes.
    #[error("the passkey's sign counter went from {stored} to {presented}")]
    CounterNotIncreased { stored: u32, presented: u32 },
    /// The stored state did not parse: a server fault, since only [`RelyingParty`] writes it.
    #[error("the stored passkey sign-in state is unreadable: {0}")]
    State(#[source] serde_json::Error),
    /// The stored credential did not parse: a server fault.
    #[error("the stored passkey is unreadable: {0}")]
    Stored(#[source] serde_json::Error),
    /// The updated credential did not serialize.
    #[error("could not serialize the passkey: {0}")]
    Serialize(#[source] serde_json::Error),
}

/// A verified sign-in.
#[derive(Clone, Debug, PartialEq)]
pub struct Verified {
    /// The stored credential with the assertion's sign counter and backup state: store it in
    /// place of the old one.
    pub credential: Value,
    /// The sign counter the authenticator reported; 0 for a passkey without one.
    pub counter: u32,
    /// Whether the passkey is backed up (synced) now.
    pub backup_state: bool,
}

/// The sign counter of a stored credential, as `webauthn-rs` serializes a [`Passkey`]
/// (`{"cred": {"counter": n, ...}}`): `Passkey` has no accessor for it.
#[derive(Deserialize)]
struct StoredCounter {
    cred: CounterOnly,
}

#[derive(Deserialize)]
struct CounterOnly {
    counter: u32,
}

/// Whether a sign counter of `presented` may follow the stored `stored` (W3C Web Authentication
/// Level 3, verifying an assertion, the signature counter step; spec 04 Q12): it must go up,
/// unless both are 0, which a passkey without a counter (most synced passkeys) always reports.
fn counter_increased(stored: u32, presented: u32) -> bool {
    presented > stored || (presented == 0 && stored == 0)
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

impl RelyingParty {
    /// Start a sign-in that accepts the stored credentials `credentials` (the user's passkeys,
    /// as [`RelyingParty::finish_registration`] or [`RelyingParty::finish_authentication`]
    /// returned them), with user verification required (Q12). With none the options allow no
    /// credential, and no assertion can finish the ceremony.
    pub fn start_authentication(&self, credentials: &[Value]) -> Result<SignInStarted, StartError> {
        let passkeys = credentials
            .iter()
            .map(Passkey::deserialize)
            .collect::<Result<Vec<_>, _>>()
            .map_err(StartError::Stored)?;
        let (options, state) = self.0.start_passkey_authentication(&passkeys)?;
        Ok(SignInStarted {
            options: serde_json::to_value(options)?,
            state: serde_json::to_string(&state)?,
        })
    }

    /// Verify `assertion` against `state`, which [`RelyingParty::start_authentication`]
    /// returned, for the user `user_id` and their stored credential `stored`, read for this
    /// check (the state holds the credentials as they were when the ceremony started).
    ///
    /// `webauthn-rs` checks the challenge, origin, relying party ID, user verification, that the
    /// credential is one the ceremony allowed, the signature, and the counter against the
    /// state's copy. This adds the user handle, when the authenticator sent one, and the counter
    /// against `stored`, so an assertion cannot pass on a counter another sign-in has used since
    /// the ceremony started. A counter must go up unless it is 0 both stored and presented.
    pub fn finish_authentication(
        &self,
        assertion: &Assertion,
        state: &str,
        user_id: Uuid,
        stored: &Value,
    ) -> Result<Verified, SignInError> {
        let state: PasskeyAuthentication =
            serde_json::from_str(state).map_err(SignInError::State)?;
        let mut passkey = Passkey::deserialize(stored).map_err(SignInError::Stored)?;
        let stored_counter = StoredCounter::deserialize(stored)
            .map_err(SignInError::Stored)?
            .cred
            .counter;
        let result = self
            .0
            .finish_passkey_authentication(&assertion.0, &state)
            .map_err(SignInError::Refused)?;
        if let Some(handle) = assertion.0.get_user_unique_id()
            && handle != user_id.as_bytes()
        {
            return Err(SignInError::UserHandleMismatch);
        }
        let presented = result.counter();
        if !counter_increased(stored_counter, presented) {
            return Err(SignInError::CounterNotIncreased {
                stored: stored_counter,
                presented,
            });
        }
        if passkey.update_credential(&result).is_none() {
            return Err(SignInError::WrongCredential);
        }
        Ok(Verified {
            credential: serde_json::to_value(&passkey).map_err(SignInError::Serialize)?,
            counter: presented,
            backup_state: result.backup_state(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use webauthn_authenticator_rs::{WebauthnAuthenticator, softpasskey::SoftPasskey};
    use webauthn_rs::prelude::{CreationChallengeResponse, RequestChallengeResponse};

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

    /// A soft passkey registered with `party` for [`registrant`], and its stored credential.
    fn registered(party: &RelyingParty) -> (SoftPasskey, Registered) {
        let started = party.start_registration(registrant(), Vec::new()).unwrap();
        let options: CreationChallengeResponse =
            serde_json::from_value(started.options).expect("creation options");
        let mut authenticator = SoftPasskey::new(true);
        let response = authenticator
            .do_registration(Url::parse(ORIGIN).unwrap(), options)
            .expect("the soft passkey registers");
        let response = serde_json::to_value(response).unwrap();
        let registered = party
            .finish_registration(&response, &started.state)
            .unwrap();
        (authenticator, registered)
    }

    /// What `authenticator` at `origin` answers to the request `options` after `edit` changed
    /// them.
    fn assert_with(
        authenticator: &mut SoftPasskey,
        options: &Value,
        origin: &str,
        edit: impl FnOnce(&mut Value),
    ) -> Value {
        let mut options = options.clone();
        edit(&mut options);
        let options: RequestChallengeResponse =
            serde_json::from_value(options).expect("request options");
        let response = authenticator
            .do_authentication(Url::parse(origin).unwrap(), options)
            .expect("the soft passkey signs");
        serde_json::to_value(response).unwrap()
    }

    fn sign_in(
        party: &RelyingParty,
        authenticator: &mut SoftPasskey,
        stored: &Value,
        edit_response: impl FnOnce(&mut Value),
    ) -> Result<Verified, SignInError> {
        let started = party
            .start_authentication(std::slice::from_ref(stored))
            .unwrap();
        let mut response = assert_with(authenticator, &started.options, ORIGIN, |_| {});
        edit_response(&mut response);
        let assertion = Assertion::parse(&response).expect("an assertion");
        party.finish_authentication(&assertion, &started.state, registrant().user_id, stored)
    }

    fn with_counter(credential: &Value, counter: u32) -> Value {
        let mut credential = credential.clone();
        credential["cred"]["counter"] = counter.into();
        credential
    }

    #[test]
    fn sign_in_options_allow_the_given_credentials_and_require_user_verification() {
        let party = party();
        let (_, first) = registered(&party);
        let (_, second) = registered(&party);
        let started = party
            .start_authentication(&[first.credential.clone(), second.credential.clone()])
            .unwrap();
        let public_key = &started.options["publicKey"];
        assert_eq!(public_key["rpId"], "lms.example.test");
        assert_eq!(public_key["userVerification"], "required");
        let allowed: Vec<&Value> = public_key["allowCredentials"]
            .as_array()
            .expect("a list")
            .iter()
            .map(|c| &c["id"])
            .collect();
        let ids: Vec<Value> = [&first, &second]
            .iter()
            .map(|r| {
                use base64::Engine as _;
                base64::engine::general_purpose::URL_SAFE_NO_PAD
                    .encode(&r.credential_id)
                    .into()
            })
            .collect();
        assert_eq!(allowed, ids.iter().collect::<Vec<_>>());
    }

    #[test]
    fn without_credentials_the_options_allow_none() {
        let started = party().start_authentication(&[]).unwrap();
        assert_eq!(started.options["publicKey"]["allowCredentials"], json!([]));
        assert_eq!(started.options["publicKey"]["userVerification"], "required");
    }

    #[test]
    fn a_registered_passkey_signs_in_and_its_counter_goes_up() {
        let party = party();
        let (mut authenticator, registered) = registered(&party);
        let first = sign_in(&party, &mut authenticator, &registered.credential, |_| {}).unwrap();
        assert_eq!(first.counter, 1);
        assert_eq!(first.credential["cred"]["counter"], 1);
        let second = sign_in(&party, &mut authenticator, &first.credential, |_| {}).unwrap();
        assert_eq!(second.counter, 2);
        assert_eq!(second.credential["cred"]["counter"], 2);
    }

    #[test]
    fn the_counter_rule_lets_only_a_rise_or_two_zeros_pass() {
        assert!(counter_increased(0, 0));
        assert!(counter_increased(0, 1));
        assert!(counter_increased(4, 5));
        assert!(!counter_increased(5, 5));
        assert!(!counter_increased(5, 4));
        assert!(!counter_increased(5, 0));
    }

    /// The stored counter is past what the authenticator reports, as when a copy of the
    /// passkey signed in since the ceremony started: refused, though the ceremony's own copy
    /// of the credential still has the old counter.
    #[test]
    fn a_counter_that_did_not_go_up_against_the_stored_one_is_refused() {
        let party = party();
        let (mut authenticator, registered) = registered(&party);
        let started = party
            .start_authentication(std::slice::from_ref(&registered.credential))
            .unwrap();
        let response = assert_with(&mut authenticator, &started.options, ORIGIN, |_| {});
        let assertion = Assertion::parse(&response).unwrap();
        let since = with_counter(&registered.credential, 7);
        let refused =
            party.finish_authentication(&assertion, &started.state, registrant().user_id, &since);
        assert!(
            matches!(
                refused,
                Err(SignInError::CounterNotIncreased {
                    stored: 7,
                    presented: 1
                })
            ),
            "{refused:?}"
        );
    }

    #[test]
    fn a_counter_behind_the_ceremonys_copy_is_refused_by_webauthn_rs() {
        let party = party();
        let (mut authenticator, registered) = registered(&party);
        let ahead = with_counter(&registered.credential, 7);
        let refused = sign_in(&party, &mut authenticator, &ahead, |_| {});
        assert!(
            matches!(
                refused,
                Err(SignInError::Refused(
                    WebauthnError::CredentialPossibleCompromise
                ))
            ),
            "{refused:?}"
        );
    }

    #[test]
    fn another_challenge_origin_or_unverified_user_is_refused() {
        let party = party();
        let (mut authenticator, registered) = registered(&party);
        let stored = std::slice::from_ref(&registered.credential);
        let user = registrant().user_id;
        let first = party.start_authentication(stored).unwrap();
        let second = party.start_authentication(stored).unwrap();

        let response = assert_with(&mut authenticator, &first.options, ORIGIN, |_| {});
        let assertion = Assertion::parse(&response).unwrap();
        let refused =
            party.finish_authentication(&assertion, &second.state, user, &registered.credential);
        assert!(
            matches!(
                refused,
                Err(SignInError::Refused(WebauthnError::MismatchedChallenge))
            ),
            "{refused:?}"
        );

        let response = assert_with(
            &mut authenticator,
            &first.options,
            "https://sub.lms.example.test",
            |_| {},
        );
        let assertion = Assertion::parse(&response).unwrap();
        let refused =
            party.finish_authentication(&assertion, &first.state, user, &registered.credential);
        assert!(
            matches!(
                refused,
                Err(SignInError::Refused(WebauthnError::InvalidRPOrigin))
            ),
            "{refused:?}"
        );

        let response = assert_with(&mut authenticator, &first.options, ORIGIN, |options| {
            options["publicKey"]["userVerification"] = "discouraged".into();
        });
        let assertion = Assertion::parse(&response).unwrap();
        let refused =
            party.finish_authentication(&assertion, &first.state, user, &registered.credential);
        assert!(
            matches!(
                refused,
                Err(SignInError::Refused(WebauthnError::UserNotVerified))
            ),
            "{refused:?}"
        );
    }

    #[test]
    fn a_bad_signature_is_refused() {
        let party = party();
        let (mut authenticator, registered) = registered(&party);
        let refused = sign_in(
            &party,
            &mut authenticator,
            &registered.credential,
            |response| {
                use base64::Engine as _;
                let engine = base64::engine::general_purpose::URL_SAFE_NO_PAD;
                let signature = response["response"]["signature"].as_str().unwrap();
                let mut bytes = engine.decode(signature).unwrap();
                let last = bytes.len() - 1;
                bytes[last] ^= 1;
                response["response"]["signature"] = engine.encode(bytes).into();
            },
        );
        assert!(
            matches!(refused, Err(SignInError::Refused(_))),
            "{refused:?}"
        );
    }

    /// A passkey the ceremony did not allow: another authenticator's, answering options that
    /// were edited to allow it.
    #[test]
    fn a_credential_the_ceremony_did_not_allow_is_refused() {
        let party = party();
        let (_, allowed) = registered(&party);
        let (mut other, other_registered) = registered(&party);
        let started = party
            .start_authentication(std::slice::from_ref(&allowed.credential))
            .unwrap();
        let other_options = party
            .start_authentication(std::slice::from_ref(&other_registered.credential))
            .unwrap()
            .options;
        let response = assert_with(&mut other, &started.options, ORIGIN, |options| {
            options["publicKey"]["allowCredentials"] =
                other_options["publicKey"]["allowCredentials"].clone();
        });
        let assertion = Assertion::parse(&response).unwrap();
        assert_eq!(assertion.credential_id(), other_registered.credential_id);
        let refused = party.finish_authentication(
            &assertion,
            &started.state,
            registrant().user_id,
            &other_registered.credential,
        );
        assert!(
            matches!(
                refused,
                Err(SignInError::Refused(WebauthnError::CredentialNotFound))
            ),
            "{refused:?}"
        );
    }

    #[test]
    fn a_stored_credential_other_than_the_signed_one_is_refused() {
        let party = party();
        let (mut authenticator, signed) = registered(&party);
        let (_, other) = registered(&party);
        let started = party
            .start_authentication(&[signed.credential.clone(), other.credential.clone()])
            .unwrap();
        let response = assert_with(&mut authenticator, &started.options, ORIGIN, |_| {});
        let assertion = Assertion::parse(&response).unwrap();
        let refused = party.finish_authentication(
            &assertion,
            &started.state,
            registrant().user_id,
            &other.credential,
        );
        assert!(
            matches!(refused, Err(SignInError::WrongCredential)),
            "{refused:?}"
        );
    }

    /// The user handle is not signed, so a client can send any: one naming another user is
    /// refused, the expected user's is accepted.
    #[test]
    fn a_user_handle_must_name_the_expected_user() {
        use base64::Engine as _;
        let engine = base64::engine::general_purpose::URL_SAFE_NO_PAD;
        let party = party();
        let (mut authenticator, registered) = registered(&party);
        let other = Uuid::from_u128(0x0190_0000_0000_7000_8000_0000_0000_0002);
        let refused = sign_in(&party, &mut authenticator, &registered.credential, |r| {
            r["response"]["userHandle"] = engine.encode(other.as_bytes()).into();
        });
        assert!(
            matches!(refused, Err(SignInError::UserHandleMismatch)),
            "{refused:?}"
        );
        let own = registrant().user_id;
        let accepted = sign_in(&party, &mut authenticator, &registered.credential, |r| {
            r["response"]["userHandle"] = engine.encode(own.as_bytes()).into();
        });
        assert!(accepted.is_ok(), "{accepted:?}");
    }

    #[test]
    fn debug_output_leaves_the_sign_in_state_and_assertion_out() {
        let party = party();
        let (mut authenticator, registered) = registered(&party);
        let started = party
            .start_authentication(std::slice::from_ref(&registered.credential))
            .unwrap();
        assert_eq!(format!("{started:?}"), "SignInStarted { .. }");
        let response = assert_with(&mut authenticator, &started.options, ORIGIN, |_| {});
        let assertion = Assertion::parse(&response).unwrap();
        assert_eq!(format!("{assertion:?}"), "Assertion { .. }");
    }
}
