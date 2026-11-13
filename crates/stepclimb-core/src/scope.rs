// SPDX-License-Identifier: AGPL-3.0-or-later
//! The organization a repository call acts in (spec 02 B8 to B10).
//!
//! Every repository function takes an [`OrgScope`] and every query it runs filters by
//! `org_id = scope.org_id()`. The scope is the tenancy boundary, so where one can come from is
//! narrow on purpose (B9):
//!
//! 1. **From a credential.** The credential resolvers (spec 08: browser session, app access
//!    token, API key) read the organization from the credential's user or service account row.
//!    They are repository functions in this crate and build the scope with the `pub(crate)`
//!    `OrgScope::from_credential`, which no other crate can call.
//! 2. **By the instance itself.** [`OrgScope::instance`] is public for startup tasks, the
//!    `stepclimb admin` CLI, job workers and the request-organization resolver of
//!    unauthenticated routes. `clippy.toml` lists it under `disallowed-methods`, so every call
//!    site carries `#[expect(clippy::disallowed_methods, reason = "...")]` and `just lint` fails
//!    on any other.
//!
//! An id from the client (a path parameter, a body field, a cursor, a header) never builds or
//! changes a scope: there is no constructor from a bare id that a handler can reach without the
//! lint, no `Deserialize`, and the fields are private.
//!
//! ```compile_fail
//! // A scope cannot be written as a literal outside this crate.
//! use stepclimb_core::{OrgScope, ScopeOrigin, id::new_id};
//! let scope = OrgScope { org_id: new_id(), origin: ScopeOrigin::Session };
//! ```
//!
//! ```compile_fail
//! // Nor built from a credential outside this crate.
//! use stepclimb_core::OrgScope;
//! let build = OrgScope::from_credential;
//! ```

use uuid::Uuid;

/// The organization a repository call acts in, and where that organization came from.
///
/// Cheap to copy; pass it by value.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OrgScope {
    org_id: Uuid,
    origin: ScopeOrigin,
}

/// Where an [`OrgScope`] came from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScopeOrigin {
    /// A browser session's user.
    Session,
    /// An API key's owner.
    ApiKey,
    /// An OAuth access token of one of Stepclimb's own apps.
    AppToken,
    /// The instance acting on its own behalf: startup, the CLI, a job, an unauthenticated
    /// route's organization.
    Instance,
}

/// The kinds of credential a resolver builds a scope from. A separate type from
/// [`ScopeOrigin`] so that the credential path cannot produce an `Instance` scope.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(
    not(test),
    expect(dead_code, reason = "the credential resolvers of spec 08 build these")
)]
pub(crate) enum Credential {
    Session,
    ApiKey,
    AppToken,
}

impl OrgScope {
    /// The scope of an authenticated request, for the credential resolvers in this crate. The
    /// resolver read `org_id` from the credential's own user or service account row.
    #[cfg_attr(
        not(test),
        expect(dead_code, reason = "the credential resolvers of spec 08 call it")
    )]
    pub(crate) fn from_credential(org_id: Uuid, credential: Credential) -> Self {
        let origin = match credential {
            Credential::Session => ScopeOrigin::Session,
            Credential::ApiKey => ScopeOrigin::ApiKey,
            Credential::AppToken => ScopeOrigin::AppToken,
        };
        Self { org_id, origin }
    }

    /// The scope of the instance acting on its own behalf in `org_id`.
    ///
    /// Allowed only in startup tasks, the `stepclimb admin` CLI, job workers and the
    /// request-organization resolver of unauthenticated routes (spec 02 B9), each with
    /// `#[expect(clippy::disallowed_methods, reason = "...")]`. A request handler never calls
    /// it: its scope comes from the credential.
    #[must_use]
    pub fn instance(org_id: Uuid) -> Self {
        Self {
            org_id,
            origin: ScopeOrigin::Instance,
        }
    }

    /// The organization every query run with this scope filters by.
    #[must_use]
    pub fn org_id(self) -> Uuid {
        self.org_id
    }

    /// Where this scope came from.
    #[must_use]
    pub fn origin(self) -> ScopeOrigin {
        self.origin
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::id::new_id;

    #[test]
    fn a_credential_scope_keeps_its_organization_and_origin() {
        let org = new_id();
        let cases = [
            (Credential::Session, ScopeOrigin::Session),
            (Credential::ApiKey, ScopeOrigin::ApiKey),
            (Credential::AppToken, ScopeOrigin::AppToken),
        ];
        for (credential, origin) in cases {
            let scope = OrgScope::from_credential(org, credential);
            assert_eq!(scope.org_id(), org);
            assert_eq!(scope.origin(), origin);
        }
    }

    #[test]
    #[expect(
        clippy::disallowed_methods,
        reason = "tests the instance constructor itself"
    )]
    fn an_instance_scope_has_the_instance_origin() {
        let org = new_id();
        let scope = OrgScope::instance(org);
        assert_eq!(scope.org_id(), org);
        assert_eq!(scope.origin(), ScopeOrigin::Instance);
    }
}
