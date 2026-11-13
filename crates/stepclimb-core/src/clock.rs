// SPDX-License-Identifier: AGPL-3.0-or-later
//! The controllable clock (`.claude/rules/testing.md` -> Time and randomness).
//!
//! Every deadline, expiry, cooldown and lifetime rule asks a [`Clock`] for the time instead of
//! calling `SystemTime::now()` itself. The server runs on [`Clock::system`]; a test builds a
//! [`ManualClock`], hands its [`ManualClock::clock`] to the code under test, and moves time with
//! [`ManualClock::advance`] or [`ManualClock::set`].
//!
//! The clock speaks `std::time::SystemTime` so it commits the workspace to no date-time crate;
//! callers convert at the database or formatting boundary.

use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, SystemTime};

/// A source of the current time. Cloning is cheap; clones of a manual clock share its time.
#[derive(Clone, Debug)]
pub struct Clock(Source);

#[derive(Clone, Debug)]
enum Source {
    System,
    Manual(Arc<Mutex<SystemTime>>),
}

impl Clock {
    /// The operating system's wall clock, for the running server.
    pub fn system() -> Self {
        Self(Source::System)
    }

    /// The current time.
    pub fn now(&self) -> SystemTime {
        match &self.0 {
            Source::System => SystemTime::now(),
            // A panic while the lock was held cannot leave a `SystemTime` half written, so the
            // value inside a poisoned lock is still a valid time.
            Source::Manual(time) => *time.lock().unwrap_or_else(PoisonError::into_inner),
        }
    }
}

/// A clock that moves only when told to, for tests.
#[derive(Clone, Debug)]
pub struct ManualClock(Arc<Mutex<SystemTime>>);

impl ManualClock {
    /// A clock stopped at `start`.
    pub fn new(start: SystemTime) -> Self {
        Self(Arc::new(Mutex::new(start)))
    }

    /// A [`Clock`] that reads this manual clock's time, now and after every later move.
    pub fn clock(&self) -> Clock {
        Clock(Source::Manual(Arc::clone(&self.0)))
    }

    /// Move the time forward by `by`.
    ///
    /// # Panics
    ///
    /// If the new time does not fit in a `SystemTime`, which only a test asking for a time
    /// billions of years away can reach.
    pub fn advance(&self, by: Duration) {
        let mut time = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        *time = time
            .checked_add(by)
            .expect("a test advanced the clock past the end of SystemTime");
    }

    /// Set the time to `to`, forward or back.
    pub fn set(&self, to: SystemTime) {
        *self.0.lock().unwrap_or_else(PoisonError::into_inner) = to;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(secs: u64) -> SystemTime {
        SystemTime::UNIX_EPOCH + Duration::from_secs(secs)
    }

    #[test]
    fn a_manual_clock_stands_still_until_moved() {
        let manual = ManualClock::new(at(1_000));
        let clock = manual.clock();
        assert_eq!(clock.now(), at(1_000));
        assert_eq!(clock.now(), at(1_000));
    }

    #[test]
    fn advance_moves_every_clone_forward() {
        let manual = ManualClock::new(at(1_000));
        let clock = manual.clock();
        let clone = clock.clone();
        manual.advance(Duration::from_secs(12 * 60 * 60));
        assert_eq!(clock.now(), at(1_000 + 12 * 60 * 60));
        assert_eq!(clone.now(), at(1_000 + 12 * 60 * 60));
    }

    #[test]
    fn set_moves_the_time_back_too() {
        let manual = ManualClock::new(at(5_000));
        let clock = manual.clock();
        manual.set(at(10));
        assert_eq!(clock.now(), at(10));
    }

    #[test]
    fn the_system_clock_follows_the_wall_clock() {
        let before = SystemTime::now();
        let read = Clock::system().now();
        let after = SystemTime::now();
        assert!(before <= read && read <= after);
    }
}
