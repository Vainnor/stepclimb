// SPDX-License-Identifier: AGPL-3.0-or-later
//! What a test sets on a running mock and reads back from it. Every setting lasts until it is
//! set again.

use std::sync::Arc;
use std::time::SystemTime;

use serde_json::Value;

use crate::MockIdp;
use crate::keys::{Key, KeyError};
use crate::state::Signing;
use crate::types::{
    AuthorizeError, ClientAuth, Endpoint, Fault, IdTokenSigning, IssuedTokens, MockUser,
    RecordedRequest,
};

impl MockIdp {
    /// The user the authorize endpoints approve from now on. Codes already issued keep theirs.
    pub fn set_user(&self, user: MockUser) {
        self.shared.lock().user = user;
    }

    /// The body the OAuth 2 userinfo endpoint answers, any JSON. Without one it answers the
    /// user's `sub` and claims.
    pub fn set_oauth2_userinfo(&self, body: Option<Value>) {
        self.shared.lock().oauth2_userinfo = body;
    }

    /// Make both authorize endpoints redirect with this error instead of a code; `None` turns
    /// it off.
    pub fn set_authorize_error(&self, error: Option<AuthorizeError>) {
        self.shared.lock().authorize_error = error;
    }

    /// Make `endpoint` misbehave as `fault` says.
    pub fn set_fault(&self, endpoint: Endpoint, fault: Fault) {
        self.shared.lock().faults.insert(endpoint, fault);
    }

    /// Make `endpoint` answer normally again.
    pub fn clear_fault(&self, endpoint: Endpoint) {
        self.shared.lock().faults.remove(&endpoint);
    }

    /// Set (`Some`) or remove (`None`) a top-level field of the discovery document.
    pub fn set_discovery_field(&self, name: &str, value: Option<Value>) {
        let mut state = self.shared.lock();
        state.discovery_overrides.insert(name.to_owned(), value);
    }

    /// Set or remove a field of both token endpoints' success response, such as `id_token`,
    /// `access_token` or `token_type`.
    pub fn set_token_response_field(&self, name: &str, value: Option<Value>) {
        let mut state = self.shared.lock();
        state.token_overrides.insert(name.to_owned(), value);
    }

    /// Set or remove a claim of the ID tokens issued from now on, after the mock has filled in
    /// `iss`, `sub`, `aud`, `iat`, `exp`, `nonce` and `at_hash`. Times are seconds since the
    /// Unix epoch.
    pub fn set_id_token_claim(&self, name: &str, value: Option<Value>) {
        let mut state = self.shared.lock();
        state.id_token_overrides.insert(name.to_owned(), value);
    }

    /// Set or remove a claim of the OIDC userinfo response, `sub` included.
    pub fn set_userinfo_claim(&self, name: &str, value: Option<Value>) {
        let mut state = self.shared.lock();
        state.userinfo_overrides.insert(name.to_owned(), value);
    }

    /// Forget every override set with the four field setters above.
    pub fn clear_overrides(&self) {
        let mut state = self.shared.lock();
        state.discovery_overrides.clear();
        state.token_overrides.clear();
        state.id_token_overrides.clear();
        state.userinfo_overrides.clear();
    }

    /// How ID tokens are signed from now on. [`IdTokenSigning::UnpublishedKey`] generates a key
    /// the JWKS never lists, a new one each call.
    pub fn set_id_token_signing(&self, signing: IdTokenSigning) -> Result<(), KeyError> {
        let signing = match signing {
            IdTokenSigning::Current => Signing::Current,
            IdTokenSigning::AlgNone => Signing::AlgNone,
            IdTokenSigning::UnpublishedKey => {
                Signing::Unpublished(Arc::new(Key::generate("mock-key-unpublished".to_owned())?))
            }
        };
        self.shared.lock().signing = signing;
        Ok(())
    }

    /// Generate a key, publish it in the JWKS beside the others and sign with it from now on.
    /// Returns its `kid`.
    pub fn rotate_signing_key(&self) -> Result<String, KeyError> {
        let serial = {
            let mut state = self.shared.lock();
            state.keys.generated += 1;
            state.keys.generated
        };
        let key = Arc::new(Key::generate(format!("mock-key-{serial}"))?);
        let kid = key.kid().to_owned();
        let mut state = self.shared.lock();
        state.keys.published.push(Arc::clone(&key));
        state.keys.current = key;
        Ok(kid)
    }

    /// Remove the key `kid` from the JWKS; returns whether it was there. Withdrawing the signing
    /// key leaves it signing, so its tokens carry a `kid` the JWKS no longer lists.
    pub fn withdraw_key(&self, kid: &str) -> bool {
        let mut state = self.shared.lock();
        let before = state.keys.published.len();
        state.keys.published.retain(|key| key.kid() != kid);
        state.keys.published.len() != before
    }

    /// The `kid` of the key that signs ID tokens.
    #[must_use]
    pub fn signing_kid(&self) -> String {
        self.shared.lock().keys.current.kid().to_owned()
    }

    /// The client authentication methods the token endpoints accept, and the discovery document
    /// lists, from now on.
    pub fn set_token_auth_methods(&self, methods: &[ClientAuth]) {
        self.shared.lock().auth_methods = methods.to_vec();
    }

    /// Whether the authorize endpoints refuse a request without a PKCE challenge. On by
    /// default; a challenge that is sent is always checked.
    pub fn set_require_pkce(&self, required: bool) {
        self.shared.lock().require_pkce = required;
    }

    /// Accept only these redirect URIs at authorize, compared as exact strings; any other
    /// answers 400 with no redirect. `None`, the default, accepts any absolute `http` or `https`
    /// URL.
    pub fn set_redirect_uris(&self, uris: Option<&[&str]>) {
        self.shared.lock().redirect_uris =
            uris.map(|uris| uris.iter().map(|uri| (*uri).to_owned()).collect());
    }

    /// Set the registered client's secret, replacing the generated one.
    pub fn set_client_secret(&self, secret: &str) {
        self.shared.lock().client_secret = secret.to_owned();
    }

    /// Where ID tokens' `iat` and `exp` come from, for a test that runs the server on a manual
    /// clock: `mock.set_time_source(move || clock.now())`. The system clock by default.
    pub fn set_time_source(&self, now: impl Fn() -> SystemTime + Send + Sync + 'static) {
        self.shared.lock().now = Arc::new(now);
    }

    /// Every request received so far, in order.
    #[must_use]
    pub fn requests(&self) -> Vec<RecordedRequest> {
        self.shared.lock().requests.clone()
    }

    /// The requests received so far at `endpoint`, in order.
    #[must_use]
    pub fn requests_to(&self, endpoint: Endpoint) -> Vec<RecordedRequest> {
        let state = self.shared.lock();
        state
            .requests
            .iter()
            .filter(|request| request.endpoint == Some(endpoint))
            .cloned()
            .collect()
    }

    /// The tokens each successful token request generated, in order. An override set with
    /// [`MockIdp::set_token_response_field`] can change what was sent.
    #[must_use]
    pub fn issued_tokens(&self) -> Vec<IssuedTokens> {
        self.shared.lock().issued.clone()
    }
}
