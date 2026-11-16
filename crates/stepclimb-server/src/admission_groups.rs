// SPDX-License-Identifier: AGPL-3.0-or-later
//! The group memberships an admitted sign-in gives its user (docs/specs/m0/07-admission.md B18,
//! B22, Q8 (a), I7).
//!
//! Two sources answer at sign-in, and each replaces only its own memberships
//! (`stepclimb_core::groups::replace_memberships`):
//!
//! - **Claims** (`idp`), when the provider's mapping names a groups claim. The mapped list is the
//!   user's claim memberships; a claim that is missing or not an array of strings is no groups
//!   (`stepclimb_auth::claims`). A provider that maps no groups claim says nothing about groups,
//!   and the user's claim memberships stay as they are. Claim memberships do not record which
//!   provider gave them, so with two providers that both map groups the last sign-in's list
//!   wins. A key that is not usable (empty, over 255 characters, a control character) is
//!   dropped, and keys past the first 1,000 usable ones are dropped; both are logged, without
//!   the keys.
//! - **The admission webhook** (`admission_webhook`), when it was called and allowed the
//!   sign-in. Its reply's `groups` (none when absent) are the user's webhook memberships; the
//!   reply was already refused as malformed if a key is not usable or there are more than
//!   1,000 (`crate::admission_webhook_call`). A sign-in that does not call the webhook (none
//!   configured or enabled, a local sign-in) leaves them as they are.
//!
//! It runs inside admission's savepoint after the user is admitted, so a refusal or a fault
//! changes no membership (B3). No line logs a group key: keys can name people's roles.

use std::collections::HashSet;
use std::time::SystemTime;

use sqlx::PgConnection;
use stepclimb_core::{
    OrgScope,
    groups::{self, GroupSource, MAX_KEYS, Replaced},
    id::Uuid,
};

use crate::error::ApiError;

/// The groups each source gave an admitted sign-in; `None` for a source that said nothing.
#[derive(Clone, Copy, Debug, Default)]
pub struct Given<'a> {
    /// The provider's mapped group claim.
    pub claims: Option<&'a [String]>,
    /// The webhook's reply.
    pub webhook: Option<&'a [String]>,
}

/// Replace `user_id`'s memberships from each source that answered in `given` (module
/// documentation). `provider` is the provider key, for the log.
pub async fn apply(
    conn: &mut PgConnection,
    scope: OrgScope,
    user_id: Uuid,
    provider: &str,
    given: Given<'_>,
    now: SystemTime,
) -> Result<(), ApiError> {
    if let Some(claimed) = given.claims {
        let keys = usable_claims(provider, claimed);
        replace(conn, scope, user_id, GroupSource::Idp, &keys, now).await?;
    }
    if let Some(keys) = given.webhook {
        replace(
            conn,
            scope,
            user_id,
            GroupSource::AdmissionWebhook,
            keys,
            now,
        )
        .await?;
    }
    Ok(())
}

async fn replace(
    conn: &mut PgConnection,
    scope: OrgScope,
    user_id: Uuid,
    source: GroupSource,
    keys: &[String],
    now: SystemTime,
) -> Result<(), ApiError> {
    let Replaced { added, removed } =
        groups::replace_memberships(&mut *conn, scope, user_id, source, keys, now)
            .await
            .map_err(|e| ApiError::internal(&e))?;
    let source = source.as_str();
    tracing::debug!(
        source,
        added,
        removed,
        "admission replaced group memberships"
    );
    Ok(())
}

/// The claimed keys a membership can be made from: the usable ones, at most [`MAX_KEYS`]
/// distinct ones, in the order the claim gave them.
fn usable_claims(provider: &str, claimed: &[String]) -> Vec<String> {
    let mut seen: HashSet<&str> = HashSet::new();
    let (mut unusable, mut dropped) = (0_usize, 0_usize);
    for key in claimed {
        if !groups::usable_key(key) {
            unusable += 1;
        } else if seen.len() == MAX_KEYS && !seen.contains(key.as_str()) {
            dropped += 1;
        } else {
            seen.insert(key);
        }
    }
    if unusable > 0 {
        tracing::info!(
            provider,
            unusable,
            "admission dropped unusable group keys from the claims"
        );
    }
    let keys = claimed
        .iter()
        .filter(|key| seen.remove(key.as_str()))
        .cloned()
        .collect();
    if dropped > 0 {
        tracing::warn!(
            provider,
            dropped,
            "admission dropped group keys past the limit from the claims"
        );
    }
    keys
}

#[cfg(test)]
mod tests {
    use super::*;

    fn strings(keys: &[&str]) -> Vec<String> {
        keys.iter().map(|&key| key.to_owned()).collect()
    }

    #[test]
    fn unusable_and_repeated_claimed_keys_are_dropped() {
        let claimed = strings(&["staff", "", "home", "a\nb", "staff", &"k".repeat(256)]);
        assert_eq!(usable_claims("sso", &claimed), strings(&["staff", "home"]));
        assert!(usable_claims("sso", &[]).is_empty());
    }

    #[test]
    fn claimed_keys_past_the_limit_are_dropped() {
        let claimed: Vec<String> = (0..=MAX_KEYS).map(|n| format!("g{n}")).collect();
        let kept = usable_claims("sso", &claimed);
        assert_eq!(kept.len(), MAX_KEYS);
        assert_eq!(kept, claimed[..MAX_KEYS]);
    }
}
