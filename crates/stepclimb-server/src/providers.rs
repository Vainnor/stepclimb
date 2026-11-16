// SPDX-License-Identifier: AGPL-3.0-or-later
//! The outbound HTTP the server uses for identity providers and the admission webhook
//! (docs/specs/m0/05-oidc.md B2, Q4, Q5; 07-admission.md B14, B17).
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
//! its connection pool is kept across requests. The admission webhook has its own pair under
//! the same two policies, with its 3-second timeout and 64 KiB reply limit
//! ([`Providers::admission_webhook`]), and the limits on how many webhook calls may be in flight
//! ([`Providers::admission_webhook_limits`]). It also holds the caches of discovery documents
//! and signing keys, and the client secrets of the providers `stepclimb.toml` declares, which
//! are never stored ([`Providers::config_secrets`]).

use std::sync::Arc;

use stepclimb_auth::{
    oidc::{DiscoveryCache, JwksCache},
    outbound::{BuildError, OutboundClient, UrlPolicy},
    provider_config::ConfigSecrets,
};
use stepclimb_core::organizations::Tenancy;

use crate::admission_webhook_limits::{self as limits, WebhookLimits};
use crate::startup::POOL_MAX_CONNECTIONS;

/// The provider clients and the discovery cache, shared by every clone.
#[derive(Clone, Debug)]
pub struct Providers {
    single_org: OutboundClient,
    multi_org: OutboundClient,
    webhook_single_org: OutboundClient,
    webhook_multi_org: OutboundClient,
    webhook_limits: WebhookLimits,
    discovery: DiscoveryCache,
    jwks: JwksCache,
    config_secrets: Arc<ConfigSecrets>,
}

impl Providers {
    /// Build the clients under the insecure switch `allow_insecure`. It fails only when the TLS
    /// setup or the system's trust store cannot be read.
    pub fn new(allow_insecure: bool) -> Result<Self, BuildError> {
        let (single, multi) = (policy(allow_insecure, true), policy(allow_insecure, false));
        Ok(Self {
            single_org: OutboundClient::for_providers(single)?,
            multi_org: OutboundClient::for_providers(multi)?,
            webhook_single_org: OutboundClient::for_admission_webhook(single)?,
            webhook_multi_org: OutboundClient::for_admission_webhook(multi)?,
            webhook_limits: WebhookLimits::new(
                limits::instance_limit(POOL_MAX_CONNECTIONS),
                limits::PER_ORGANIZATION,
            ),
            discovery: DiscoveryCache::default(),
            jwks: JwksCache::default(),
            config_secrets: Arc::default(),
        })
    }

    /// These clients with the secrets `stepclimb serve` read for the providers `stepclimb.toml`
    /// declares (`crate::config_providers`).
    #[must_use]
    pub fn with_config_secrets(mut self, secrets: ConfigSecrets) -> Self {
        self.config_secrets = Arc::new(secrets);
        self
    }

    /// These clients with other webhook call limits than the pool's, for tests.
    #[must_use]
    pub fn with_admission_webhook_limits(mut self, limits: WebhookLimits) -> Self {
        self.webhook_limits = limits;
        self
    }

    /// How many admission webhook calls may be in flight, shared by every clone.
    pub fn admission_webhook_limits(&self) -> &WebhookLimits {
        &self.webhook_limits
    }

    /// The client secrets of the providers `stepclimb.toml` declares, by row.
    pub fn config_secrets(&self) -> &ConfigSecrets {
        &self.config_secrets
    }

    /// The client for an instance with `tenancy`: private networks allowed only for
    /// [`Tenancy::Single`].
    pub fn client(&self, tenancy: Tenancy) -> &OutboundClient {
        if allows_private(tenancy) {
            &self.single_org
        } else {
            &self.multi_org
        }
    }

    /// The admission webhook's client for an instance with `tenancy`, under the policy
    /// [`Providers::client`] picks for it.
    pub fn admission_webhook(&self, tenancy: Tenancy) -> &OutboundClient {
        if allows_private(tenancy) {
            &self.webhook_single_org
        } else {
            &self.webhook_multi_org
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

/// Whether an instance with `tenancy` may reach private network addresses: only with exactly one
/// organization (Q4).
fn allows_private(tenancy: Tenancy) -> bool {
    matches!(tenancy, Tenancy::Single(_))
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
            for tenancy in [Tenancy::Single(new_id()), Tenancy::Multiple, Tenancy::Empty] {
                assert_eq!(
                    providers.admission_webhook(tenancy).policy(),
                    providers.client(tenancy).policy(),
                    "{tenancy:?}"
                );
            }
        }
    }
}
