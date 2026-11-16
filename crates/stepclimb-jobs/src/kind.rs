// SPDX-License-Identifier: AGPL-3.0-or-later
//! What a job is: its kind, the queue it runs on, its lease and its retry settings
//! (docs/specs/m1/01-job-queue-and-outbox.md B5, B7, B11).

use std::time::Duration;

/// The lease a claim takes unless the kind sets its own (B5).
pub const DEFAULT_LEASE: Duration = Duration::from_secs(5 * 60);

/// The longest lease a kind may set.
pub const MAX_LEASE: Duration = Duration::from_secs(24 * 60 * 60);

/// The longest backoff cap a kind may set.
pub const MAX_CAP: Duration = Duration::from_secs(7 * 24 * 60 * 60);

/// The longest kind name, as the `jobs.kind` check constraint allows.
const MAX_KIND_LEN: usize = 100;

/// The longest queue name, as the `jobs.queue` check constraint allows.
const MAX_QUEUE_LEN: usize = 32;

/// How a kind's failed runs are retried (B7, Q4).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Retry {
    /// The delay before the first retry, before jitter.
    pub base: Duration,
    /// The longest delay, before jitter.
    pub cap: Duration,
    /// How many times a job may be claimed before a retry outcome makes it dead.
    pub max_attempts: u32,
}

impl Retry {
    /// Base 5 s, cap 1 h, 8 attempts.
    pub const DEFAULT: Self = Self {
        base: Duration::from_secs(5),
        cap: Duration::from_secs(60 * 60),
        max_attempts: 8,
    };
}

/// A kind of job: the name its rows carry, the queue it runs on and how it is retried.
///
/// Each kind is a `const` beside its handler, and the same constant is passed to
/// [`crate::enqueue`] and to [`crate::Registry::register`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Kind {
    /// `area.action`, such as `email.send`: lowercase ASCII letters, digits and `_`, each part
    /// starting with a letter, 3 to 100 bytes.
    pub name: &'static str,
    /// A lowercase ASCII letter, then up to 31 lowercase letters, digits and `_`.
    pub queue: &'static str,
    /// How long a claim of this kind holds the job before another worker may take it (B5).
    pub lease: Duration,
    pub retry: Retry,
}

/// Why a [`Kind`] cannot be stored or registered.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum KindError {
    #[error("the job kind `{0}` is not of the form area.action")]
    Name(&'static str),
    #[error("the queue `{queue}` of job kind `{kind}` is not a valid queue name")]
    Queue {
        kind: &'static str,
        queue: &'static str,
    },
    #[error(
        "job kind `{0}` needs a lease of 1 s to 24 h, a base of at least 1 ms, a cap from its base to 7 days and 1 to 1000 attempts"
    )]
    Settings(&'static str),
}

impl Kind {
    /// A kind on `queue` with the default lease and retry settings.
    #[must_use]
    pub const fn new(name: &'static str, queue: &'static str) -> Self {
        Self {
            name,
            queue,
            lease: DEFAULT_LEASE,
            retry: Retry::DEFAULT,
        }
    }

    /// The same kind with its own lease.
    #[must_use]
    pub const fn with_lease(self, lease: Duration) -> Self {
        Self { lease, ..self }
    }

    /// The same kind with its own retry settings.
    #[must_use]
    pub const fn with_retry(self, retry: Retry) -> Self {
        Self { retry, ..self }
    }

    /// Checks the name, the queue and the settings.
    pub fn validate(&self) -> Result<(), KindError> {
        if !valid_name(self.name) {
            return Err(KindError::Name(self.name));
        }
        if !valid_queue(self.queue) {
            return Err(KindError::Queue {
                kind: self.name,
                queue: self.queue,
            });
        }
        let Retry {
            base,
            cap,
            max_attempts,
        } = self.retry;
        let settings_ok = (Duration::from_secs(1)..=MAX_LEASE).contains(&self.lease)
            && base >= Duration::from_millis(1)
            && (base..=MAX_CAP).contains(&cap)
            && (1..=1000).contains(&max_attempts);
        if !settings_ok {
            return Err(KindError::Settings(self.name));
        }
        Ok(())
    }
}

/// One lowercase ASCII letter, then lowercase letters, digits and `_`.
fn valid_word(word: &str) -> bool {
    let mut bytes = word.bytes();
    bytes.next().is_some_and(|b| b.is_ascii_lowercase())
        && bytes.all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
}

fn valid_name(name: &str) -> bool {
    let Some((area, action)) = name.split_once('.') else {
        return false;
    };
    (3..=MAX_KIND_LEN).contains(&name.len()) && valid_word(area) && valid_word(action)
}

fn valid_queue(queue: &str) -> bool {
    queue.len() <= MAX_QUEUE_LEN && valid_word(queue)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_of_the_area_action_form_are_valid() {
        for name in [
            "email.send",
            "progress.recompute_course",
            "a.b",
            "jobs.prune2",
        ] {
            assert_eq!(Kind::new(name, "default").validate(), Ok(()), "{name}");
        }
    }

    #[test]
    fn other_names_are_refused() {
        let long = format!("a.{}", "b".repeat(99));
        for name in [
            "email",
            "email.",
            ".send",
            "Email.send",
            "email.send.now",
            "email.2send",
            "email.se-nd",
            "e.",
            long.as_str(),
        ] {
            let name: &'static str = Box::leak(name.to_owned().into_boxed_str());
            assert_eq!(
                Kind::new(name, "default").validate(),
                Err(KindError::Name(name)),
                "{name}"
            );
        }
    }

    #[test]
    fn queue_names_follow_the_column_check() {
        assert!(Kind::new("a.b", "transcode").validate().is_ok());
        assert!(Kind::new("a.b", "q_1").validate().is_ok());
        let longest = Box::leak("q".repeat(32).into_boxed_str());
        assert!(Kind::new("a.b", longest).validate().is_ok());
        let too_long = Box::leak("q".repeat(33).into_boxed_str());
        for queue in ["", "1q", "Q", "q-1", &*too_long] {
            let queue: &'static str = Box::leak(queue.to_owned().into_boxed_str());
            assert!(
                matches!(
                    Kind::new("a.b", queue).validate(),
                    Err(KindError::Queue { .. })
                ),
                "{queue}"
            );
        }
    }

    #[test]
    fn settings_out_of_range_are_refused() {
        let kind = Kind::new("a.b", "default");
        let retry = Retry::DEFAULT;
        let bad = [
            kind.with_lease(Duration::from_millis(999)),
            kind.with_lease(MAX_LEASE + Duration::from_micros(1)),
            kind.with_retry(Retry {
                cap: MAX_CAP + Duration::from_micros(1),
                ..retry
            }),
            kind.with_retry(Retry {
                max_attempts: 0,
                ..retry
            }),
            kind.with_retry(Retry {
                max_attempts: 1001,
                ..retry
            }),
            kind.with_retry(Retry {
                base: Duration::ZERO,
                ..retry
            }),
            kind.with_retry(Retry {
                cap: Duration::from_secs(4),
                ..retry
            }),
        ];
        for kind in bad {
            assert_eq!(kind.validate(), Err(KindError::Settings("a.b")), "{kind:?}");
        }
    }

    #[test]
    fn the_longest_lease_and_cap_are_allowed() {
        let kind = Kind::new("a.b", "default")
            .with_lease(MAX_LEASE)
            .with_retry(Retry {
                cap: MAX_CAP,
                ..Retry::DEFAULT
            });
        assert_eq!(kind.validate(), Ok(()));
    }

    #[test]
    fn the_defaults_are_the_specs() {
        let kind = Kind::new("a.b", "default");
        assert_eq!(kind.lease, Duration::from_secs(300));
        assert_eq!(kind.retry.base, Duration::from_secs(5));
        assert_eq!(kind.retry.cap, Duration::from_secs(3600));
        assert_eq!(kind.retry.max_attempts, 8);
    }
}
