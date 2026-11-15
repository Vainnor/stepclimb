// SPDX-License-Identifier: AGPL-3.0-or-later
//! Who made a request's change, for its audit entry (docs/specs/m0/09-permissions-and-roles.md
//! Q5; `stepclimb_core::audit`).
//!
//! A handler that changes roles, assignments, accounts or the organization takes [`ClientAddress`]
//! after its `Require` and passes [`ClientAddress::actor`] to the repository function, which
//! writes the entry with the change. The address is the one the rate limits count by
//! ([`crate::client_ip`]): the TCP peer, or with `STEPCLIMB_TRUSTED_PROXIES` the right-most
//! `X-Forwarded-For` entry that is not a trusted proxy.

use std::net::IpAddr;

use axum::{
    extract::{FromRef, FromRequestParts},
    http::request::Parts,
};
use stepclimb_core::audit::Actor;

use crate::{
    AppState,
    error::ApiError,
    rate_limit::client_address,
    session::{Authenticated, Via},
};

/// The client address of the request. Without the TCP peer's address (`ConnectInfo`, which
/// `startup::serve_on` provides) the request is refused with 500: a change with no address
/// would be one an entry cannot place.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ClientAddress(pub IpAddr);

impl ClientAddress {
    /// The audit actor of a change `principal` makes from this address.
    /// A change an API key makes is the key's (spec 08 I6), recorded by its service account's
    /// id; the key's owner is on the service account.
    pub fn actor(self, principal: Authenticated) -> Actor {
        match principal.via() {
            Via::Session => Actor::User {
                id: principal.user_id,
                ip: self.0,
            },
            Via::ApiKey { service_account_id } => Actor::ApiKey {
                id: service_account_id,
                ip: self.0,
            },
        }
    }
}

impl<S> FromRequestParts<S> for ClientAddress
where
    S: Send + Sync,
    AppState: FromRef<S>,
{
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, ApiError> {
        client_address(parts, &AppState::from_ref(state)).map(Self)
    }
}
