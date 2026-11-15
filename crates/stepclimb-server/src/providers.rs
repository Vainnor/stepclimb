// SPDX-License-Identifier: AGPL-3.0-or-later
//! The outbound HTTP the server uses for identity providers (docs/specs/m0/05-oidc.md B2, Q4,
//! Q5).
//!
//! Every request to a provider goes through a `stepclimb_auth::outbound::OutboundClient` under a
//! [`UrlPolicy`] built from two facts:
//!
//! - the instance's insecure-providers switch (`STEPCLIMB_ALLOW_INSECURE_PROVIDERS`), which
//!   allows `http` and loopback addresses for tests and local development;
//! - how many organizations the instance has, read for each request: private network addresses
//!   are allowed only on a single-organization instance (Q4), whose one admin is the operator's.
//!   With two or more organizations, or none, they are refused, so one organization's admin
//!   cannot make the server reach the operator's network.
//!
//! [`Providers`] holds one client per policy the instance can be under and picks one by the
//! organization count ([`Providers::client`]); a client's policy is fixed when it is built, and
//! its connection pool is kept across requests. It also holds the caches of discovery documents
//! and signing keys.

use stepclimb_auth::{
    oidc::{DiscoveryCache, JwksCache},
    outbound::{BuildError, OutboundClient, UrlPolicy},
};
use stepclimb_core::organizations::Tenancy;

/// The provider clients and the discovery cache, shared by every clone.
#[derive(Clone, Debug)]
pub struct Providers {
    single_org: OutboundClient,
    multi_org: OutboundClient,
    discovery: DiscoveryCache,
    jwks: JwksCache,
}

impl Providers {
    /// Build both clients under the insecure switch `allow_insecure`. It fails only when the TLS
    /// setup or the system's trust store cannot be read.
    pub fn new(allow_insecure: bool) -> Result<Self, BuildError> {
        Ok(Self {
            single_org: OutboundClient::for_providers(policy(allow_insecure, true))?,
            multi_org: OutboundClient::for_providers(policy(allow_insecure, false))?,
            discovery: DiscoveryCache::default(),
            jwks: JwksCache::default(),
        })
    }

    /// The client for an instance with `tenancy`: private networks allowed only for
    /// [`Tenancy::Single`].
    pub fn client(&self, tenancy: Tenancy) -> &OutboundClient {
        match tenancy {
            Tenancy::Single(_) => &self.single_org,
            Tenancy::Multiple | Tenancy::Empty => &self.multi_org,
        }
    }

    /// The discovery documents, per provider (Q5).
    pub fn discovery(&self) -> &DiscoveryCache {
        &self.discovery
    }

    /// The providers' signing keys (Q5).
    pub fn jwks(&self) -> &JwksCache {
        &self.jwks
    }
}

const fn policy(allow_insecure: bool, allow_private: bool) -> UrlPolicy {
    UrlPolicy::new(allow_insecure, allow_private)
}

#[cfg(test)]
mod tests {
    use stepclimb_core::id::new_id;

    use super::*;

    #[test]
    fn private_networks_are_allowed_only_with_exactly_one_organization() {
        for insecure in [false, true] {
            let providers = Providers::new(insecure).unwrap();
            assert_eq!(
                providers.client(Tenancy::Single(new_id())).policy(),
                UrlPolicy::new(insecure, true)
            );
            assert_eq!(
                providers.client(Tenancy::Multiple).policy(),
                UrlPolicy::new(insecure, false)
            );
            assert_eq!(
                providers.client(Tenancy::Empty).policy(),
                UrlPolicy::new(insecure, false)
            );
        }
    }
}
