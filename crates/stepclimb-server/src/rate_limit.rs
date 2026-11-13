// SPDX-License-Identifier: AGPL-3.0-or-later
//! Rate limits on the sign-in and token routes (docs/specs/m0/08-sessions-and-csrf.md B22, B23,
//! Q7).
//!
//! Three budgets, each a fixed window that opens at the first request it counts and closes a
//! window length later; numbers come from configuration, with Q7's as defaults ([`Limits`]):
//!
//! - every request to a sign-in route, per client address: 20 a minute;
//! - every request to `/oauth/token` and `/oauth/revoke`, per client address: 60 a minute;
//! - failed password and TOTP checks, per account identifier in an organization: 10 per 15
//!   minutes. A throttle, not a lockout: the account works again when its window closes.
//!
//! A route declares its address budget by taking [`RateLimited<SignIn>`] or
//! [`RateLimited<Token>`] as its first extractor, so a refused request runs no other extractor
//! and no handler code. A handler that checks a password or TOTP code calls
//! [`RateLimiter::account_check`] before it and [`AccountCheck::passed`] after a success.
//!
//! The client address is [`TrustedProxies::client_ip`] of the TCP peer, so only a configured
//! proxy can name another address. An IPv6 client is counted by its /64, the block one host is
//! usually given, so a client cannot get a new budget by changing the low 64 bits.
//!
//! The counters live in this process (Q7): with several server replicas each has its own, and a
//! restart clears them. Windows read the [`Clock`] in `AppState`, so tests move time. An account
//! is stored as the SHA-256 of its organization and identifier, 32 bytes whatever the identifier's
//! length. Closed windows are swept out by the first request a minute or more after the last
//! sweep, and by any request that finds the table doubled since then (see [`Table::sweep`]).

use std::collections::HashMap;
use std::marker::PhantomData;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::num::NonZeroU32;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::{Duration, SystemTime};

use axum::{
    extract::{ConnectInfo, FromRef, FromRequestParts},
    http::{HeaderMap, HeaderValue, StatusCode, header, request::Parts},
    response::{IntoResponse, Response},
};
use sha2::{Digest, Sha256};
use stepclimb_core::OrgScope;

use crate::{AppState, client_ip::TrustedProxies, error::ApiError};

/// The codes this module produces.
pub mod code {
    /// A rate limit refused the request; `Retry-After` says when to try again (429).
    pub const AUTH_RATE_LIMITED: &str = "auth.rate_limited";
}

/// How long a sign-in route's per-address window lasts.
pub const SIGN_IN_WINDOW: Duration = Duration::from_secs(60);
/// How long the token routes' per-address window lasts.
pub const TOKEN_WINDOW: Duration = Duration::from_secs(60);
/// How long an account's failed-check window lasts.
pub const FAILED_CHECK_WINDOW: Duration = Duration::from_secs(15 * 60);

/// The table is swept for closed windows once it holds at least this many, whatever the time.
const MIN_SWEEP: usize = 1024;
/// The table is swept at most this long after the last sweep, when a request comes: the
/// shortest window, so no closed window outlives the next sweep by more than its own length.
const SWEEP_EVERY: Duration = SIGN_IN_WINDOW;

/// How many requests or failures each window allows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Limits {
    /// Requests to sign-in routes per client address per [`SIGN_IN_WINDOW`].
    pub sign_in: NonZeroU32,
    /// Requests to the token routes per client address per [`TOKEN_WINDOW`].
    pub token: NonZeroU32,
    /// Failed password or TOTP checks per account per [`FAILED_CHECK_WINDOW`].
    pub failed_checks: NonZeroU32,
}

impl Limits {
    /// Spec 08 Q7 (a), the defaults Q7 (c) makes configurable.
    pub const DEFAULT: Self = Self {
        sign_in: NonZeroU32::new(20).unwrap(),
        token: NonZeroU32::new(60).unwrap(),
        failed_checks: NonZeroU32::new(10).unwrap(),
    };
}

/// A group of routes that shares one budget per client address.
pub trait RouteClass: sealed::Sealed + Send + Sync + 'static {
    #[doc(hidden)]
    const CLASS: Class;
}

/// The sign-in routes: `/auth/...` start and finish, password and TOTP checks.
pub enum SignIn {}
/// `POST /oauth/token` and `POST /oauth/revoke`.
pub enum Token {}

impl RouteClass for SignIn {
    const CLASS: Class = Class::SignIn;
}
impl RouteClass for Token {
    const CLASS: Class = Class::Token;
}

mod sealed {
    pub trait Sealed {}
    impl Sealed for super::SignIn {}
    impl Sealed for super::Token {}
}

#[doc(hidden)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Class {
    SignIn,
    Token,
}

/// A client address as counted: an IPv4 address, or an IPv6 address's first 64 bits.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum AddressKey {
    V4(Ipv4Addr),
    V6Prefix(u64),
}

impl From<IpAddr> for AddressKey {
    fn from(address: IpAddr) -> Self {
        match address.to_canonical() {
            IpAddr::V4(v4) => Self::V4(v4),
            IpAddr::V6(v6) => Self::V6Prefix((u128::from(v6) >> 64) as u64),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum Key {
    Address(Class, AddressKey),
    /// SHA-256 of the organization id's 16 bytes, then the identifier's bytes.
    Account([u8; 32]),
}

impl Key {
    fn window(&self) -> Duration {
        match self {
            Self::Address(Class::SignIn, _) => SIGN_IN_WINDOW,
            Self::Address(Class::Token, _) => TOKEN_WINDOW,
            Self::Account(_) => FAILED_CHECK_WINDOW,
        }
    }

    /// The account key: a fixed 32 bytes, so an identifier of any length costs the table the
    /// same. The organization id has a fixed length, so no two pairs hash the same input.
    fn account(scope: OrgScope, identifier: &str) -> Self {
        let digest = Sha256::new()
            .chain_update(scope.org_id().as_bytes())
            .chain_update(identifier.as_bytes())
            .finalize();
        Self::Account(digest.into())
    }
}

#[derive(Clone, Copy, Debug)]
struct Window {
    opened: SystemTime,
    count: u32,
}

impl Window {
    /// Whether the window is over at `now`. A window that opened after `now` (the wall clock
    /// went back) is over too, so a clock step cannot hold a budget shut.
    fn closed(&self, length: Duration, now: SystemTime) -> bool {
        now.duration_since(self.opened)
            .map_or(true, |elapsed| elapsed >= length)
    }
}

struct Table {
    windows: HashMap<Key, Window>,
    /// The size that triggers a sweep before [`SWEEP_EVERY`] has passed.
    sweep_at: usize,
    /// When the last sweep ran; `None` until the first request.
    swept: Option<SystemTime>,
}

/// The counters, shared by every clone. Built once at startup from configuration.
#[derive(Clone)]
pub struct RateLimiter(Arc<Inner>);

struct Inner {
    limits: Limits,
    trusted: TrustedProxies,
    table: Mutex<Table>,
}

/// Q7's default numbers and no trusted proxy, as an unconfigured server runs.
impl Default for RateLimiter {
    fn default() -> Self {
        Self::new(Limits::DEFAULT, TrustedProxies::none())
    }
}

impl RateLimiter {
    pub fn new(limits: Limits, trusted: TrustedProxies) -> Self {
        Self(Arc::new(Inner {
            limits,
            trusted,
            table: Mutex::new(Table {
                windows: HashMap::new(),
                sweep_at: MIN_SWEEP,
                swept: None,
            }),
        }))
    }

    /// The client address of a request from `peer` with `headers` (see [`crate::client_ip`]).
    pub fn client_ip(&self, peer: IpAddr, headers: &HeaderMap) -> IpAddr {
        self.0.trusted.client_ip(peer, headers)
    }

    /// Count one request of `class` from `client`, or refuse it when the window's budget is spent.
    pub fn hit_address(
        &self,
        class: Class,
        client: IpAddr,
        now: SystemTime,
    ) -> Result<(), TooManyRequests> {
        let limit = match class {
            Class::SignIn => self.0.limits.sign_in,
            Class::Token => self.0.limits.token,
        };
        self.take(Key::Address(class, client.into()), limit, now)
            .map(|_| ())
    }

    /// Start a password or TOTP check for `identifier` in `scope`'s organization. The check
    /// counts as a failure from now on, so parallel guesses cannot all pass before the first
    /// failure is recorded; [`AccountCheck::passed`] takes it back after a success. Refused
    /// when the account's failures in the window have reached the limit: run no check then,
    /// right or wrong. `identifier` is what the sign-in method matches the account by, already
    /// normalized the way that match is.
    pub fn account_check(
        &self,
        scope: OrgScope,
        identifier: &str,
        now: SystemTime,
    ) -> Result<AccountCheck, TooManyRequests> {
        let key = Key::account(scope, identifier);
        let opened = self.take(key, self.0.limits.failed_checks, now)?;
        Ok(AccountCheck {
            limiter: self.clone(),
            key,
            opened,
        })
    }

    /// Add one to `key`'s window, opening a new one if it has closed. Returns when the window
    /// opened, or how long until it closes when it is full.
    fn take(
        &self,
        key: Key,
        limit: NonZeroU32,
        now: SystemTime,
    ) -> Result<SystemTime, TooManyRequests> {
        let length = key.window();
        let mut table = self.table();
        table.sweep(now);
        let window = table.windows.entry(key).or_insert(Window {
            opened: now,
            count: 0,
        });
        if window.closed(length, now) {
            *window = Window {
                opened: now,
                count: 0,
            };
        }
        if window.count >= limit.get() {
            return Err(TooManyRequests::until(window.opened + length, now));
        }
        window.count += 1;
        Ok(window.opened)
    }

    fn give_back(&self, key: &Key, opened: SystemTime) {
        let mut table = self.table();
        if let Some(window) = table.windows.get_mut(key)
            && window.opened == opened
        {
            window.count = window.count.saturating_sub(1);
        }
    }

    fn table(&self) -> MutexGuard<'_, Table> {
        // Nothing panics while the lock is held but an allocation failure, which aborts; a
        // poisoned table still holds whole counts.
        self.0.table.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

impl Table {
    /// Drop the closed windows when [`SWEEP_EVERY`] has passed since the last sweep (or the
    /// clock went back past it), or when the table has reached `sweep_at`, twice what the last
    /// sweep left (at least [`MIN_SWEEP`]). The time trigger bounds how long a closed window is
    /// kept; the size trigger bounds the work, which stays constant per request on average. When
    /// the table's capacity is more than four times the new `sweep_at`, as after a burst has
    /// passed, the spare capacity is released down to `sweep_at`.
    fn sweep(&mut self, now: SystemTime) {
        let last = *self.swept.get_or_insert(now);
        let due = now
            .duration_since(last)
            .map_or(true, |elapsed| elapsed >= SWEEP_EVERY);
        if !due && self.windows.len() < self.sweep_at {
            return;
        }
        self.windows
            .retain(|key, window| !window.closed(key.window(), now));
        self.swept = Some(now);
        self.sweep_at = (self.windows.len() * 2).max(MIN_SWEEP);
        if self.windows.capacity() > 4 * self.sweep_at {
            self.windows.shrink_to(self.sweep_at);
        }
    }
}

/// A password or TOTP check in progress, counted as a failure until [`AccountCheck::passed`].
#[must_use = "a check counts as a failure unless `passed` is called after a success"]
pub struct AccountCheck {
    limiter: RateLimiter,
    key: Key,
    opened: SystemTime,
}

impl AccountCheck {
    /// The check succeeded: it no longer counts against the account. A window that has closed
    /// and reopened since the check began is left alone.
    pub fn passed(self) {
        self.limiter.give_back(&self.key, self.opened);
    }
}

/// 429 `auth.rate_limited` with `Retry-After` in whole seconds, at least 1.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TooManyRequests {
    retry_after: u64,
}

impl TooManyRequests {
    fn until(closes: SystemTime, now: SystemTime) -> Self {
        let left = closes.duration_since(now).unwrap_or_default();
        let seconds = left.as_secs() + u64::from(left.subsec_nanos() > 0);
        Self {
            retry_after: seconds.max(1),
        }
    }

    /// The seconds the client is told to wait.
    pub fn retry_after(&self) -> u64 {
        self.retry_after
    }
}

impl IntoResponse for TooManyRequests {
    fn into_response(self) -> Response {
        let mut response = ApiError::new(
            StatusCode::TOO_MANY_REQUESTS,
            code::AUTH_RATE_LIMITED,
            "Too many attempts. Wait the number of seconds in the Retry-After header, then try again.",
        )
        .into_response();
        response
            .headers_mut()
            .insert(header::RETRY_AFTER, HeaderValue::from(self.retry_after));
        response
    }
}

/// The request was counted against its client address's budget for `C` and allowed. Take it
/// first among a handler's extractors.
#[derive(Debug, Clone, Copy)]
pub struct RateLimited<C> {
    /// The client address the request was counted against.
    pub client: IpAddr,
    class: PhantomData<C>,
}

/// The server was started without the TCP peer's address (`ConnectInfo`), so no request can be
/// counted. `startup::serve_on` provides it; anything else is a wiring bug.
#[derive(Debug, thiserror::Error)]
#[error("the request has no peer address; serve the router with connect info")]
struct NoPeerAddress;

impl<C, S> FromRequestParts<S> for RateLimited<C>
where
    C: RouteClass,
    S: Send + Sync,
    AppState: FromRef<S>,
{
    type Rejection = Response;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Response> {
        let state = AppState::from_ref(state);
        let Some(ConnectInfo(peer)) = parts.extensions.get::<ConnectInfo<SocketAddr>>() else {
            return Err(ApiError::internal(&NoPeerAddress).into_response());
        };
        let client = state.rate_limits.client_ip(peer.ip(), &parts.headers);
        state
            .rate_limits
            .hit_address(C::CLASS, client, state.clock.now())
            .map_err(IntoResponse::into_response)?;
        Ok(Self {
            client,
            class: PhantomData,
        })
    }
}

#[cfg(test)]
mod tests {
    use stepclimb_core::id::Uuid;

    use super::*;

    fn at(secs: u64) -> SystemTime {
        SystemTime::UNIX_EPOCH + Duration::from_secs(1_763_000_000 + secs)
    }

    fn ip(text: &str) -> IpAddr {
        text.parse().unwrap()
    }

    fn limits(sign_in: u32, token: u32, failed: u32) -> Limits {
        Limits {
            sign_in: NonZeroU32::new(sign_in).unwrap(),
            token: NonZeroU32::new(token).unwrap(),
            failed_checks: NonZeroU32::new(failed).unwrap(),
        }
    }

    fn limiter(sign_in: u32, token: u32, failed: u32) -> RateLimiter {
        RateLimiter::new(limits(sign_in, token, failed), TrustedProxies::none())
    }

    #[expect(
        clippy::disallowed_methods,
        reason = "a test stands in for the request-organization resolver"
    )]
    fn scope(n: u128) -> OrgScope {
        OrgScope::instance(Uuid::from_u128(n))
    }

    #[test]
    fn the_defaults_are_q7s_numbers() {
        assert_eq!(Limits::DEFAULT, limits(20, 60, 10));
        assert_eq!(SIGN_IN_WINDOW, Duration::from_secs(60));
        assert_eq!(TOKEN_WINDOW, Duration::from_secs(60));
        assert_eq!(FAILED_CHECK_WINDOW, Duration::from_secs(900));
    }

    #[test]
    fn the_request_after_the_limit_is_refused_until_the_window_closes() {
        let limiter = limiter(3, 60, 10);
        let client = ip("203.0.113.9");
        for second in 0..3 {
            assert!(
                limiter
                    .hit_address(Class::SignIn, client, at(second))
                    .is_ok()
            );
        }
        let refused = limiter
            .hit_address(Class::SignIn, client, at(10))
            .unwrap_err();
        assert_eq!(refused.retry_after(), 50);
        let refused = limiter
            .hit_address(Class::SignIn, client, at(59))
            .unwrap_err();
        assert_eq!(refused.retry_after(), 1);
        assert!(limiter.hit_address(Class::SignIn, client, at(60)).is_ok());
    }

    #[test]
    fn retry_after_rounds_up_and_is_at_least_one_second() {
        let opened = at(0);
        let half = opened + Duration::from_millis(59_500);
        assert_eq!(
            TooManyRequests::until(opened + SIGN_IN_WINDOW, half).retry_after(),
            1
        );
        let early = opened + Duration::from_millis(500);
        assert_eq!(
            TooManyRequests::until(opened + SIGN_IN_WINDOW, early).retry_after(),
            60
        );
        assert_eq!(TooManyRequests::until(opened, at(5)).retry_after(), 1);
    }

    #[test]
    fn classes_and_addresses_have_their_own_budgets() {
        let limiter = limiter(1, 2, 10);
        let client = ip("203.0.113.9");
        assert!(limiter.hit_address(Class::SignIn, client, at(0)).is_ok());
        assert!(limiter.hit_address(Class::SignIn, client, at(0)).is_err());
        assert!(limiter.hit_address(Class::Token, client, at(0)).is_ok());
        assert!(limiter.hit_address(Class::Token, client, at(0)).is_ok());
        assert!(limiter.hit_address(Class::Token, client, at(0)).is_err());
        assert!(
            limiter
                .hit_address(Class::SignIn, ip("203.0.113.10"), at(0))
                .is_ok()
        );
    }

    #[test]
    fn an_ipv6_client_is_counted_by_its_slash_64() {
        let limiter = limiter(1, 60, 10);
        assert!(
            limiter
                .hit_address(Class::SignIn, ip("2001:db8:1:2::1"), at(0))
                .is_ok()
        );
        assert!(
            limiter
                .hit_address(Class::SignIn, ip("2001:db8:1:2:ffff::9"), at(0))
                .is_err()
        );
        assert!(
            limiter
                .hit_address(Class::SignIn, ip("2001:db8:1:3::1"), at(0))
                .is_ok()
        );
        // An IPv4-mapped address is its IPv4 address.
        assert!(
            limiter
                .hit_address(Class::SignIn, ip("198.51.100.1"), at(0))
                .is_ok()
        );
        assert!(
            limiter
                .hit_address(Class::SignIn, ip("::ffff:198.51.100.1"), at(0))
                .is_err()
        );
    }

    #[test]
    fn a_clock_that_went_back_opens_a_new_window() {
        let limiter = limiter(1, 60, 10);
        let client = ip("203.0.113.9");
        assert!(limiter.hit_address(Class::SignIn, client, at(100)).is_ok());
        assert!(limiter.hit_address(Class::SignIn, client, at(100)).is_err());
        assert!(limiter.hit_address(Class::SignIn, client, at(10)).is_ok());
    }

    #[test]
    fn failed_checks_share_the_account_budget_across_addresses() {
        let limiter = limiter(20, 60, 2);
        for _ in 0..2 {
            drop(limiter.account_check(scope(1), "ada", at(0)).unwrap());
        }
        let refused = limiter.account_check(scope(1), "ada", at(1)).err().unwrap();
        assert_eq!(refused.retry_after(), 899);
        // Another account, or the same name in another organization, is untouched.
        assert!(limiter.account_check(scope(1), "grace", at(1)).is_ok());
        assert!(limiter.account_check(scope(2), "ada", at(1)).is_ok());
        // The window closes 15 minutes after the first failure.
        assert!(limiter.account_check(scope(1), "ada", at(900)).is_ok());
    }

    #[test]
    fn a_passed_check_does_not_count() {
        let limiter = limiter(20, 60, 2);
        for _ in 0..5 {
            limiter
                .account_check(scope(1), "ada", at(0))
                .unwrap()
                .passed();
        }
        drop(limiter.account_check(scope(1), "ada", at(0)).unwrap());
        drop(limiter.account_check(scope(1), "ada", at(0)).unwrap());
        assert!(limiter.account_check(scope(1), "ada", at(0)).is_err());
    }

    #[test]
    fn checks_in_flight_count_before_they_finish() {
        let limiter = limiter(20, 60, 2);
        let first = limiter.account_check(scope(1), "ada", at(0)).unwrap();
        let second = limiter.account_check(scope(1), "ada", at(0)).unwrap();
        assert!(limiter.account_check(scope(1), "ada", at(0)).is_err());
        first.passed();
        assert!(limiter.account_check(scope(1), "ada", at(0)).is_ok());
        drop(second);
    }

    #[test]
    fn a_pass_after_the_window_reopened_leaves_the_new_window_alone() {
        let limiter = limiter(20, 60, 1);
        let old = limiter.account_check(scope(1), "ada", at(0)).unwrap();
        drop(limiter.account_check(scope(1), "ada", at(900)).unwrap());
        old.passed();
        assert!(limiter.account_check(scope(1), "ada", at(901)).is_err());
    }

    #[test]
    fn closed_windows_are_swept_once_the_table_doubles() {
        let limiter = limiter(20, 60, 10);
        drop(limiter.account_check(scope(1), "ada", at(0)).unwrap());
        for n in 1..MIN_SWEEP as u32 {
            let client = IpAddr::V4(Ipv4Addr::from(n));
            limiter.hit_address(Class::SignIn, client, at(0)).unwrap();
        }
        assert_eq!(limiter.table().windows.len(), MIN_SWEEP);
        // At 61 s every address window has closed and the account's 15 minutes have not: the
        // sweep keeps the account and drops the rest before counting the new request.
        limiter
            .hit_address(Class::SignIn, ip("203.0.113.9"), at(61))
            .unwrap();
        let table = limiter.table();
        assert_eq!(table.windows.len(), 2);
        assert_eq!(table.sweep_at, MIN_SWEEP);
    }

    #[test]
    fn a_closed_window_is_swept_a_minute_later_without_the_table_doubling() {
        let limiter = limiter(20, 60, 10);
        for n in 0..10u32 {
            let client = IpAddr::V4(Ipv4Addr::from(n));
            limiter.hit_address(Class::SignIn, client, at(0)).unwrap();
        }
        limiter
            .hit_address(Class::SignIn, ip("203.0.113.9"), at(59))
            .unwrap();
        assert_eq!(limiter.table().windows.len(), 11);
        limiter
            .hit_address(Class::SignIn, ip("203.0.113.10"), at(60))
            .unwrap();
        // The ten closed at 60 s; the one from 59 s is still open.
        assert_eq!(limiter.table().windows.len(), 2);
    }

    #[test]
    fn capacity_left_by_a_burst_is_released_once_it_has_passed() {
        let limiter = limiter(20, 60, 10);
        for n in 0..5_000u32 {
            let client = IpAddr::V4(Ipv4Addr::from(n));
            limiter.hit_address(Class::SignIn, client, at(0)).unwrap();
        }
        let peak = limiter.table().windows.capacity();
        assert!(peak >= 5_000, "{peak}");
        limiter
            .hit_address(Class::SignIn, ip("203.0.113.9"), at(61))
            .unwrap();
        let table = limiter.table();
        assert_eq!(table.windows.len(), 1);
        assert_eq!(table.sweep_at, MIN_SWEEP);
        assert!(
            table.windows.capacity() <= 4 * MIN_SWEEP,
            "{} after a peak of {peak}",
            table.windows.capacity()
        );
    }

    #[test]
    fn an_account_key_is_32_bytes_whatever_the_identifier() {
        let one = "a".repeat(1 << 20);
        let mut other = one.clone();
        other.push('b');
        let (first, second) = (Key::account(scope(1), &one), Key::account(scope(1), &other));
        for key in [first, second] {
            assert!(matches!(key, Key::Account(digest) if digest.len() == 32));
        }
        assert_ne!(first, second);
        assert_ne!(first, Key::account(scope(2), &one));
        // `Key` is `Copy`, so it owns no heap memory, and its size is fixed.
        assert!(std::mem::size_of::<Key>() <= 40);
        // The two long identifiers keep separate budgets.
        let limiter = limiter(20, 60, 1);
        drop(limiter.account_check(scope(1), &one, at(0)).unwrap());
        assert!(limiter.account_check(scope(1), &one, at(0)).is_err());
        assert!(limiter.account_check(scope(1), &other, at(0)).is_ok());
    }
}
