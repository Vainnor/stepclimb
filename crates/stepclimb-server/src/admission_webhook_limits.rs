// SPDX-License-Identifier: AGPL-3.0-or-later
//! How many admission webhook calls may be waiting at once (docs/specs/m0/07-admission.md I6).
//!
//! A webhook call runs inside the sign-in's transaction, so each one in flight holds a pooled
//! database connection, and the row locks the earlier steps took, for up to the 3-second limit.
//! [`WebhookLimits`] caps those calls on the whole instance at half the pool
//! ([`instance_limit`] of `crate::startup::POOL_MAX_CONNECTIONS`, at least 1) and per
//! organization at [`PER_ORGANIZATION`], so one organization's slow webhook cannot take every
//! connection. A call only tries to take a [`Permit`]: it never waits for one. A sign-in that
//! finds either limit full is refused with `auth.admission_unavailable`, as for any webhook that
//! cannot decide.

use std::collections::HashMap;
use std::fmt;
use std::num::NonZeroUsize;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use stepclimb_core::id::Uuid;

/// The most webhook calls one organization may have in flight.
pub const PER_ORGANIZATION: NonZeroUsize = NonZeroUsize::new(3).unwrap();

/// The most webhook calls the instance may have in flight with a pool of `pool_max`
/// connections: half of them, at least 1.
#[must_use]
pub const fn instance_limit(pool_max: u32) -> NonZeroUsize {
    match NonZeroUsize::new(pool_max as usize / 2) {
        Some(limit) => limit,
        None => NonZeroUsize::MIN,
    }
}

/// The two limits and the calls in flight, shared by every clone.
#[derive(Clone)]
pub struct WebhookLimits(Arc<Inner>);

struct Inner {
    instance: usize,
    per_org: usize,
    held: Mutex<Held>,
}

#[derive(Default)]
struct Held {
    total: usize,
    by_org: HashMap<Uuid, usize>,
}

/// One call's place under both limits, given back when it is dropped.
#[must_use = "the place is given back as soon as the permit is dropped"]
pub struct Permit {
    limits: Arc<Inner>,
    org: Uuid,
}

impl WebhookLimits {
    /// At most `instance` calls in flight, and at most `per_org` of one organization's.
    pub fn new(instance: NonZeroUsize, per_org: NonZeroUsize) -> Self {
        Self(Arc::new(Inner {
            instance: instance.get(),
            per_org: per_org.get(),
            held: Mutex::default(),
        }))
    }

    /// A place for a call of `org`'s, or `None` when the instance or the organization already
    /// has its limit in flight. It never waits.
    pub fn try_acquire(&self, org: Uuid) -> Option<Permit> {
        let mut held = self.0.lock();
        let of_org = held.by_org.get(&org).copied().unwrap_or(0);
        if held.total >= self.0.instance || of_org >= self.0.per_org {
            return None;
        }
        held.total += 1;
        held.by_org.insert(org, of_org + 1);
        Some(Permit {
            limits: Arc::clone(&self.0),
            org,
        })
    }
}

impl Inner {
    /// The counts. Every update under the lock is a few integer writes that cannot panic
    /// halfway, so a lock poisoned by an unrelated panic still holds consistent counts.
    fn lock(&self) -> MutexGuard<'_, Held> {
        self.held.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

impl Drop for Permit {
    fn drop(&mut self) {
        let mut held = self.limits.lock();
        held.total -= 1;
        if let Some(of_org) = held.by_org.get_mut(&self.org) {
            *of_org -= 1;
            if *of_org == 0 {
                held.by_org.remove(&self.org);
            }
        }
    }
}

impl fmt::Debug for WebhookLimits {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("WebhookLimits")
            .field("instance", &self.0.instance)
            .field("per_org", &self.0.per_org)
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use stepclimb_core::id::new_id;

    use super::*;

    fn limits(instance: usize, per_org: usize) -> WebhookLimits {
        WebhookLimits::new(
            NonZeroUsize::new(instance).unwrap(),
            NonZeroUsize::new(per_org).unwrap(),
        )
    }

    #[test]
    fn the_instance_limit_is_half_the_pool_and_at_least_one() {
        assert_eq!(instance_limit(10).get(), 5);
        assert_eq!(instance_limit(11).get(), 5);
        assert_eq!(instance_limit(2).get(), 1);
        assert_eq!(instance_limit(1).get(), 1);
        assert_eq!(instance_limit(0).get(), 1);
    }

    #[test]
    fn an_organization_gets_at_most_its_limit_while_another_still_gets_in() {
        let limits = limits(5, 3);
        let (a, b) = (new_id(), new_id());
        let held: Vec<Permit> = (0..3).map(|_| limits.try_acquire(a).unwrap()).collect();
        assert!(limits.try_acquire(a).is_none(), "a fourth call of a");
        let of_b = limits.try_acquire(b).expect("b still gets in");
        drop(held);
        assert!(limits.try_acquire(a).is_some(), "a's places came back");
        drop(of_b);
    }

    #[test]
    fn the_instance_limit_holds_across_organizations() {
        let limits = limits(2, 3);
        let first = limits.try_acquire(new_id()).unwrap();
        let _second = limits.try_acquire(new_id()).unwrap();
        assert!(limits.try_acquire(new_id()).is_none());
        drop(first);
        assert!(limits.try_acquire(new_id()).is_some());
    }

    #[test]
    fn a_dropped_permit_leaves_no_count_behind() {
        let limits = limits(5, 3);
        let org = new_id();
        drop(limits.try_acquire(org).unwrap());
        let held = limits.0.lock();
        assert_eq!(held.total, 0);
        assert!(held.by_org.is_empty());
    }
}
