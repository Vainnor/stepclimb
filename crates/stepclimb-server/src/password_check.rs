// SPDX-License-Identifier: AGPL-3.0-or-later
//! The password check every password route runs (docs/specs/m0/04-local-accounts.md B14, B16 to
//! B18; docs/specs/m0/08-sessions-and-csrf.md B23, B40 to B42).
//!
//! [`check_password`] takes the request's [`RateLimited<SignIn>`] as an argument, so no route
//! can check a password without having taken the per-address budget first. Before the
//! verification it takes the failed-check budgets ([`Budget`]) that apply, each of which counts
//! the check as a failure until it passes and refuses with 429 `auth.rate_limited` once its
//! window is full:
//!
//! - the account's, keyed by its [`LoginEmail`]; unless
//! - the browser presents a device cookie ([`crate::device`]) issued to the login's user for
//!   the password it has now, which replaces the account's budget by the cookie's own (spec 08
//!   B42, OWASP's device-cookie scheme): the owner's browser keeps signing in while an attacker
//!   fills the account's budget, and a stolen cookie gives no more guesses than the account had;
//! - for a signed-in user (a change of password, B17), the user's, by id, always: with or
//!   without an email, with or without a device cookie.
//!
//! A refused budget gives back the ones already taken, since no check ran. A TOTP code is
//! checked against the same budgets ([`take_budgets`], spec 04 B20). Then one argon2id
//! verification runs whatever the case: against the login's hash, or against the dummy hash
//! when there is no login or no password, so a wrong password, an unknown address and an
//! account without a password answer the same 401 `auth.credentials_invalid` after the same
//! work. A right password gives every budget back and, when the stored hash is at older
//! parameters, computes the replacement (B16). No step logs the password, a hash or the email.

use std::time::SystemTime;

use axum::{
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
};
use stepclimb_auth::password::{StoredHash, Verified};
use stepclimb_core::{
    OrgScope,
    id::Uuid,
    local_logins::{Login, LoginEmail},
};

use crate::{
    AppState, device,
    error::ApiError,
    rate_limit::{AccountCheck, RateLimited, SignIn, TooManyRequests},
};

/// The codes this module produces.
pub mod code {
    /// The email and password did not sign in, or the current password is wrong (401).
    pub const AUTH_CREDENTIALS_INVALID: &str = "auth.credentials_invalid";
}

/// Why a password route refused: problem details, or a failed-check budget spent (429 with
/// `Retry-After`).
pub enum Refusal {
    Problem(ApiError),
    Limited(TooManyRequests),
}

impl From<ApiError> for Refusal {
    fn from(error: ApiError) -> Self {
        Self::Problem(error)
    }
}

impl From<TooManyRequests> for Refusal {
    fn from(limited: TooManyRequests) -> Self {
        Self::Limited(limited)
    }
}

impl IntoResponse for Refusal {
    fn into_response(self) -> Response {
        match self {
            Self::Problem(error) => error.into_response(),
            Self::Limited(limited) => limited.into_response(),
        }
    }
}

/// The request a password is checked for.
pub(crate) struct Attempt<'a> {
    pub state: &'a AppState,
    pub scope: OrgScope,
    pub headers: &'a HeaderMap,
    pub now: SystemTime,
}

/// Whose password is checked.
pub(crate) struct Subject<'a> {
    /// The address the login was found by, or the signed-in user's; `None` for a user without
    /// an email.
    pub email: Option<&'a LoginEmail>,
    /// The login whose hash is checked; `None` when the address matched none.
    pub login: Option<&'a Login>,
    /// The signed-in user making the check, whose own budget then applies too.
    pub signed_in: Option<Uuid>,
}

/// A password that matched: the user, the hash it matched, the password's generation, and the
/// hash at the current parameters to store instead when that one is older.
pub(crate) struct Passed {
    pub user_id: Uuid,
    pub verified: StoredHash,
    pub generation: i64,
    pub rehash: Option<StoredHash>,
}

/// A failed-check budget a check counts against.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Budget<'a> {
    Email(&'a LoginEmail),
    Device(Uuid),
    User(Uuid),
}

/// The budgets of `subject`'s check (see the module documentation), given the device cookie
/// the request presents.
fn budgets<'a>(subject: &Subject<'a>, device: Option<device::Device>) -> Vec<Budget<'a>> {
    let mut budgets = Vec::new();
    if let Some(user_id) = subject.signed_in {
        budgets.push(Budget::User(user_id));
    }
    let exempting = subject.login.and_then(|login| {
        device.filter(|device| {
            login.password_hash.is_some()
                && device.user_id == login.user_id
                && device.generation == login.password_generation
        })
    });
    match (exempting, subject.email) {
        (Some(device), _) => budgets.push(Budget::Device(device.device_id)),
        (None, Some(email)) => budgets.push(Budget::Email(email)),
        (None, None) => {}
    }
    budgets
}

/// Take every budget of `wanted`, or none: when one is spent, the ones taken are given back and
/// its refusal is returned.
fn take(
    attempt: &Attempt<'_>,
    wanted: &[Budget<'_>],
) -> Result<Vec<AccountCheck>, TooManyRequests> {
    let limiter = &attempt.state.rate_limits;
    let (scope, now) = (attempt.scope, attempt.now);
    let mut taken = Vec::with_capacity(wanted.len());
    for budget in wanted {
        let check = match *budget {
            Budget::Email(email) => limiter.account_check(scope, email, now),
            Budget::Device(device_id) => limiter.device_check(scope, device_id, now),
            Budget::User(user_id) => limiter.user_check(scope, user_id, now),
        };
        match check {
            Ok(check) => taken.push(check),
            Err(refused) => {
                taken.into_iter().for_each(AccountCheck::passed);
                return Err(refused);
            }
        }
    }
    Ok(taken)
}

/// Take every failed-check budget of `subject`'s check (see the module documentation), given
/// the device cookie the request presents, for a password or a TOTP code. Each counts the check
/// as a failure until [`AccountCheck::passed`]; when one is spent, none is taken. It takes the
/// request's per-address budget as an argument, so no check runs without it.
pub(crate) fn take_budgets(
    _address_budget: &RateLimited<SignIn>,
    attempt: &Attempt<'_>,
    subject: &Subject<'_>,
) -> Result<Vec<AccountCheck>, TooManyRequests> {
    let keys = &attempt.state.keys;
    let device = device::presented(keys, attempt.scope, attempt.headers, attempt.now);
    take(attempt, &budgets(subject, device))
}

/// Check `password` for `subject` (see the module documentation). A refusal is 401
/// `auth.credentials_invalid`, 429 `auth.rate_limited`, or a fault.
pub(crate) async fn check_password(
    address_budget: &RateLimited<SignIn>,
    attempt: &Attempt<'_>,
    subject: Subject<'_>,
    password: &str,
) -> Result<Passed, Refusal> {
    let state = attempt.state;
    let taken = take_budgets(address_budget, attempt, &subject)?;
    let login = subject.login;
    let stored = login
        .and_then(|login| login.password_hash.clone())
        .map(StoredHash::new);
    let verified = state
        .passwords
        .verify(password.to_owned(), stored.clone())
        .await
        .map_err(internal)?;
    let (Verified::Match { rehash }, Some(login), Some(stored)) = (verified, login, stored) else {
        // The budgets taken keep this failure counted.
        drop(taken);
        tracing::info!(user_id = ?login.map(|login| login.user_id), "password check failed");
        return Err(credentials_invalid().into());
    };
    taken.into_iter().for_each(AccountCheck::passed);
    let rehash = if rehash {
        let new = state.passwords.hash(password.to_owned()).await;
        Some(new.map_err(internal)?)
    } else {
        None
    };
    Ok(Passed {
        user_id: login.user_id,
        verified: stored,
        generation: login.password_generation,
        rehash,
    })
}

/// 401 `auth.credentials_invalid`.
pub(crate) fn credentials_invalid() -> ApiError {
    ApiError::new(
        StatusCode::UNAUTHORIZED,
        code::AUTH_CREDENTIALS_INVALID,
        "The email or password is wrong. Check both and try again.",
    )
}

pub(crate) fn internal<E: std::error::Error>(error: E) -> ApiError {
    ApiError::internal(&error)
}

#[cfg(test)]
mod tests {
    use stepclimb_core::local_logins::LoginEmail;

    use super::*;

    fn login(user: u128, generation: i64) -> Login {
        Login {
            user_id: Uuid::from_u128(user),
            password_hash: Some("$argon2id$".into()),
            email: None,
            password_generation: generation,
        }
    }

    fn device(user: u128, generation: i64) -> device::Device {
        device::Device {
            user_id: Uuid::from_u128(user),
            device_id: Uuid::from_u128(99),
            generation,
        }
    }

    #[test]
    fn a_device_cookie_for_the_login_and_its_password_replaces_the_account_budget() {
        let email = LoginEmail::for_test("ada@example.test");
        let login = login(1, 3);
        let subject = Subject {
            email: Some(&email),
            login: Some(&login),
            signed_in: None,
        };
        assert_eq!(budgets(&subject, None), [Budget::Email(&email)]);
        let own = budgets(&subject, Some(device(1, 3)));
        assert_eq!(own, [Budget::Device(Uuid::from_u128(99))]);
        // Another user's cookie, or one from before the password changed, exempts nothing.
        assert_eq!(
            budgets(&subject, Some(device(2, 3))),
            [Budget::Email(&email)]
        );
        assert_eq!(
            budgets(&subject, Some(device(1, 2))),
            [Budget::Email(&email)]
        );
    }

    #[test]
    fn a_signed_in_user_always_has_a_budget_of_their_own() {
        let login = login(1, 3);
        let user = Uuid::from_u128(1);
        let no_email = Subject {
            email: None,
            login: Some(&login),
            signed_in: Some(user),
        };
        assert_eq!(budgets(&no_email, None), [Budget::User(user)]);
        let with_device = budgets(&no_email, Some(device(1, 3)));
        assert_eq!(
            with_device,
            [Budget::User(user), Budget::Device(Uuid::from_u128(99))]
        );
    }

    #[test]
    fn wrong_credentials_are_401() {
        let problem = credentials_invalid().problem();
        assert_eq!(
            (problem.status, problem.code.as_str()),
            (401, "auth.credentials_invalid")
        );
    }
}
