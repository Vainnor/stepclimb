// SPDX-License-Identifier: AGPL-3.0-or-later
//! The mock's shared state: what the test configured, what it issued and what it received.

use std::collections::{BTreeMap, HashMap};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::SystemTime;

use serde_json::Value;

use crate::keys::Key;
use crate::types::{
    AuthorizeError, ClientAuth, Endpoint, Fault, IssuedTokens, MockUser, RecordedRequest,
};

/// Which of the two providers an authorize, token or userinfo request went to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Flavor {
    Oidc,
    OAuth2,
}

/// The time source for `iat` and `exp`.
pub(crate) type TimeSource = Arc<dyn Fn() -> SystemTime + Send + Sync>;

/// Field overrides: `Some` sets the field to the value, `None` removes it.
pub(crate) type Overrides = BTreeMap<String, Option<Value>>;

/// An authorization code and what the authorize request bound to it.
pub(crate) struct Grant {
    pub(crate) flavor: Flavor,
    pub(crate) client_id: String,
    /// The authorize request's `redirect_uri` as sent; the token request must repeat it exactly.
    pub(crate) redirect_uri: String,
    pub(crate) code_challenge: Option<String>,
    pub(crate) nonce: Option<String>,
    pub(crate) user: MockUser,
}

/// The signing keys: the published ones (the JWKS, in order) and the one that signs.
pub(crate) struct Keys {
    pub(crate) published: Vec<Arc<Key>>,
    pub(crate) current: Arc<Key>,
    /// How many keys the mock has generated, for the next `kid`.
    pub(crate) generated: usize,
}

/// How ID tokens are signed: [`crate::IdTokenSigning`] with its unpublished key in hand.
pub(crate) enum Signing {
    Current,
    AlgNone,
    Unpublished(Arc<Key>),
}

pub(crate) struct State {
    pub(crate) base_url: String,
    pub(crate) client_id: String,
    pub(crate) client_secret: String,
    pub(crate) auth_methods: Vec<ClientAuth>,
    pub(crate) require_pkce: bool,
    /// The redirect URIs authorize accepts, compared as exact strings; `None` accepts any.
    pub(crate) redirect_uris: Option<Vec<String>>,
    pub(crate) user: MockUser,
    pub(crate) oauth2_userinfo: Option<Value>,
    pub(crate) authorize_error: Option<AuthorizeError>,
    pub(crate) faults: HashMap<Endpoint, Fault>,
    pub(crate) discovery_overrides: Overrides,
    pub(crate) token_overrides: Overrides,
    pub(crate) id_token_overrides: Overrides,
    pub(crate) userinfo_overrides: Overrides,
    pub(crate) signing: Signing,
    pub(crate) keys: Keys,
    pub(crate) grants: HashMap<String, Grant>,
    /// Access token to the provider that issued it and the user it stands for.
    pub(crate) access_tokens: HashMap<String, (Flavor, MockUser)>,
    pub(crate) issued: Vec<IssuedTokens>,
    pub(crate) requests: Vec<RecordedRequest>,
    pub(crate) now: TimeSource,
}

impl State {
    pub(crate) fn issuer(&self) -> String {
        format!("{}/oidc", self.base_url)
    }

    pub(crate) fn url(&self, endpoint: Endpoint) -> String {
        format!("{}{}", self.base_url, endpoint.path())
    }
}

/// The state behind a lock that is never held across an `.await`.
#[derive(Clone)]
pub(crate) struct Shared(Arc<Mutex<State>>);

impl Shared {
    pub(crate) fn new(state: State) -> Self {
        Self(Arc::new(Mutex::new(state)))
    }

    /// The state. A panic while the lock was held (a failed assertion in a test closure) does
    /// not leave the state half-written: every write under the lock is a single assignment or
    /// collection insert, so a poisoned lock is still read.
    pub(crate) fn lock(&self) -> MutexGuard<'_, State> {
        self.0.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

/// Apply `overrides` to `object` in place.
pub(crate) fn apply(object: &mut serde_json::Map<String, Value>, overrides: &Overrides) {
    for (name, value) in overrides {
        match value {
            Some(value) => {
                object.insert(name.clone(), value.clone());
            }
            None => {
                object.remove(name);
            }
        }
    }
}
