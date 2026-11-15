// SPDX-License-Identifier: AGPL-3.0-or-later
//! Password hashing off the async runtime, a few at a time (docs/specs/m0/04-local-accounts.md
//! B16, Q8).
//!
//! One argon2id computation (`stepclimb_auth::password`) takes tens of milliseconds of CPU and
//! 19 MiB. [`PasswordHasher`] runs each on tokio's blocking pool, holding one of [`CAP`] permits
//! of a semaphore for it, so a burst of sign-ins queues instead of growing memory: at most 4 at
//! once, 76 MiB at Q8's parameters, within the 1 GB budget of `docs/plan/technical.md` ->
//! Hosting and operations. A request that waits for a permit still holds its place under the
//! per-address rate limit.
//!
//! It counts the computations it ran and the most that ran at once, so the tests can show that
//! every password check costs one verification (spec 04 T10) and that the cap holds (T12).

use std::num::NonZeroUsize;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};

use stepclimb_auth::password::{self, HashError, StoredHash, Verified, VerifyError};
use tokio::sync::Semaphore;

/// How many hashes run at once (Q8).
pub const CAP: NonZeroUsize = NonZeroUsize::new(4).unwrap();

/// The shared hasher. Clones share the permits and the counts.
#[derive(Clone)]
pub struct PasswordHasher(Arc<Inner>);

struct Inner {
    permits: Semaphore,
    computations: AtomicU64,
    running: AtomicUsize,
    peak: AtomicUsize,
}

/// Why a hash or a verification did not run to an answer.
#[derive(Debug, thiserror::Error)]
pub enum HasherError {
    #[error(transparent)]
    Hash(#[from] HashError),
    #[error(transparent)]
    Verify(#[from] VerifyError),
    #[error("the password computation did not finish: {0}")]
    Task(#[from] tokio::task::JoinError),
}

/// [`CAP`] permits.
impl Default for PasswordHasher {
    fn default() -> Self {
        Self::new(CAP)
    }
}

impl PasswordHasher {
    /// A hasher that runs at most `cap` computations at once.
    pub fn new(cap: NonZeroUsize) -> Self {
        Self(Arc::new(Inner {
            permits: Semaphore::new(cap.get()),
            computations: AtomicU64::new(0),
            running: AtomicUsize::new(0),
            peak: AtomicUsize::new(0),
        }))
    }

    /// The argon2id hash of `password` with a new salt ([`password::hash`]).
    pub async fn hash(&self, password: String) -> Result<StoredHash, HasherError> {
        Ok(self.run(move || password::hash(&password)).await??)
    }

    /// Check `password` against `stored` ([`password::verify`]), or, when there is no stored
    /// hash, do the same work against the dummy hash and answer [`Verified::Mismatch`]
    /// ([`password::verify_dummy`]). One computation either way.
    pub async fn verify(
        &self,
        password: String,
        stored: Option<StoredHash>,
    ) -> Result<Verified, HasherError> {
        let verified = self
            .run(move || match stored {
                Some(stored) => password::verify(&password, &stored),
                None => Ok(password::verify_dummy(&password)),
            })
            .await??;
        Ok(verified)
    }

    /// How many computations have run.
    pub fn computations(&self) -> u64 {
        self.0.computations.load(Ordering::SeqCst)
    }

    /// The most computations that ran at once.
    pub fn peak(&self) -> usize {
        self.0.peak.load(Ordering::SeqCst)
    }

    /// Run `work` on the blocking pool under a permit.
    async fn run<T: Send + 'static>(
        &self,
        work: impl FnOnce() -> T + Send + 'static,
    ) -> Result<T, tokio::task::JoinError> {
        // Nothing closes the semaphore, so acquiring only waits.
        let permit = self
            .0
            .permits
            .acquire()
            .await
            .expect("the hasher's semaphore is never closed");
        let inner = self.0.clone();
        let result = tokio::task::spawn_blocking(move || {
            let running = inner.running.fetch_add(1, Ordering::SeqCst) + 1;
            inner.peak.fetch_max(running, Ordering::SeqCst);
            inner.computations.fetch_add(1, Ordering::SeqCst);
            let value = work();
            inner.running.fetch_sub(1, Ordering::SeqCst);
            value
        })
        .await;
        drop(permit);
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_cap_is_q8s_four() {
        assert_eq!(CAP.get(), 4);
    }

    // T12: with the cap at N, N + 1 and more parallel checks run at most N at a time.
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn parallel_checks_run_at_most_the_cap_at_a_time() {
        let hasher = PasswordHasher::new(NonZeroUsize::new(2).unwrap());
        let tasks: Vec<_> = (0..6)
            .map(|_| {
                let hasher = hasher.clone();
                tokio::spawn(async move {
                    hasher
                        .verify("correct horse battery staple".to_owned(), None)
                        .await
                })
            })
            .collect();
        for task in tasks {
            assert_eq!(task.await.unwrap().unwrap(), Verified::Mismatch);
        }
        assert_eq!(hasher.computations(), 6);
        assert!(
            (1..=2).contains(&hasher.peak()),
            "peak {} with a cap of 2",
            hasher.peak()
        );
    }

    #[tokio::test]
    async fn a_hash_verifies_through_the_hasher() {
        let hasher = PasswordHasher::default();
        let stored = hasher
            .hash("correct horse battery staple".to_owned())
            .await
            .unwrap();
        let verified = hasher
            .verify("correct horse battery staple".to_owned(), Some(stored))
            .await
            .unwrap();
        assert_eq!(verified, Verified::Match { rehash: false });
        assert_eq!(hasher.computations(), 2);
    }
}
