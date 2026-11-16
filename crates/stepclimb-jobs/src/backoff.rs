// SPDX-License-Identifier: AGPL-3.0-or-later
//! The delay before a retry (docs/specs/m1/01-job-queue-and-outbox.md B7): exponential with a
//! cap and equal jitter, drawn from a random source passed in so tests fix it.

use std::fmt;
use std::sync::Arc;
use std::time::Duration;

use crate::kind::Retry;

/// A source of random 64-bit values for jitter. Cloning is cheap.
#[derive(Clone)]
pub struct Random(Arc<dyn Fn() -> u64 + Send + Sync>);

impl Random {
    /// The operating system's generator, for the running server.
    ///
    /// Jitter only spreads retries out, so a generator that fails does not stop a retry: the
    /// draw is logged at `warn` and counts as 0, the shortest delay of the range.
    #[must_use]
    pub fn system() -> Self {
        Self(Arc::new(|| {
            getrandom::u64().unwrap_or_else(|error| {
                tracing::warn!(%error, "the random generator failed; retrying with the shortest backoff");
                0
            })
        }))
    }

    /// A source that calls `draw` for every value, for tests.
    #[must_use]
    pub fn from_fn(draw: impl Fn() -> u64 + Send + Sync + 'static) -> Self {
        Self(Arc::new(draw))
    }

    fn draw(&self) -> u64 {
        (self.0)()
    }
}

impl fmt::Debug for Random {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Random")
    }
}

/// The delay before the retry that follows claim number `attempts` (1 for the first run):
/// `min(base * 2^(attempts - 1), cap)`, then a uniform draw between half that value and the
/// value, in whole microseconds.
#[must_use]
pub fn backoff(retry: &Retry, attempts: u32, random: &Random) -> Duration {
    let base = retry.base.as_micros();
    let cap = retry.cap.as_micros();
    let doublings = attempts.saturating_sub(1).min(64);
    let full = base.saturating_mul(1u128 << doublings).min(cap);
    let half = full / 2;
    let span = full - half + 1;
    let jitter = u128::from(random.draw()) % span;
    // At most `cap`, and a registered kind's cap is at most 7 days (`Kind::validate`), so the
    // value fits; saturating keeps an unchecked `Retry` in range rather than failing.
    let micros = u64::try_from(half + jitter).unwrap_or(u64::MAX);
    Duration::from_micros(micros)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixed(value: u64) -> Random {
        Random::from_fn(move || value)
    }

    fn secs(s: u64) -> Duration {
        Duration::from_secs(s)
    }

    #[test]
    fn the_lowest_draw_gives_half_the_doubled_delay() {
        let retry = Retry::DEFAULT;
        let delays: Vec<_> = (1..=7).map(|n| backoff(&retry, n, &fixed(0))).collect();
        let halves = [2_500, 5_000, 10_000, 20_000, 40_000, 80_000, 160_000];
        let expected: Vec<_> = halves.iter().map(|&ms| Duration::from_millis(ms)).collect();
        assert_eq!(delays, expected);
    }

    #[test]
    fn the_highest_draw_gives_the_doubled_delay() {
        let retry = Retry::DEFAULT;
        // 5 s is 5,000,000 µs; half is 2,500,000, so the span is 2,500,001 values.
        assert_eq!(backoff(&retry, 1, &fixed(2_500_000)), secs(5));
        assert_eq!(backoff(&retry, 1, &fixed(2_500_001)), secs(5) / 2);
        assert_eq!(backoff(&retry, 3, &fixed(10_000_000)), secs(20));
    }

    #[test]
    fn every_draw_stays_within_the_bounds() {
        let retry = Retry::DEFAULT;
        for n in 1..=12 {
            let full = secs(5).saturating_mul(1 << (n - 1)).min(secs(3600));
            for draw in [0, 1, 7, 999_999_937, u64::MAX / 3, u64::MAX] {
                let delay = backoff(&retry, n, &fixed(draw));
                assert!(
                    full / 2 <= delay && delay <= full,
                    "n={n} draw={draw}: {delay:?}"
                );
            }
        }
    }

    #[test]
    fn the_cap_bounds_the_delay() {
        let retry = Retry::DEFAULT;
        assert_eq!(backoff(&retry, 11, &fixed(0)), secs(1800));
        assert_eq!(
            backoff(&retry, 1000, &fixed(u64::MAX)),
            backoff(&retry, 11, &fixed(u64::MAX))
        );
        assert!(backoff(&retry, u32::MAX, &fixed(u64::MAX)) <= secs(3600));
    }

    #[test]
    fn the_system_source_draws_from_the_os_generator() {
        let random = Random::system();
        let draws: Vec<u64> = (0..4).map(|_| random.draw()).collect();
        // Four equal 64-bit draws from a working generator have a chance of 2^-192.
        assert!(draws.windows(2).any(|pair| pair[0] != pair[1]), "{draws:?}");
        let delay = backoff(&Retry::DEFAULT, 1, &random);
        assert!(secs(5) / 2 <= delay && delay <= secs(5), "{delay:?}");
    }

    #[test]
    fn a_kind_sets_its_own_base_and_cap() {
        let retry = Retry {
            base: secs(60),
            cap: secs(600),
            max_attempts: 20,
        };
        assert_eq!(backoff(&retry, 2, &fixed(0)), secs(60));
        assert_eq!(backoff(&retry, 9, &fixed(0)), secs(300));
    }
}
