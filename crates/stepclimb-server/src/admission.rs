// SPDX-License-Identifier: AGPL-3.0-or-later
//! Admission: after a provider has proved who someone is, whether they may sign in to the
//! organization, and as which user (docs/specs/m0/07-admission.md B1 to B4, B7, B20, B21, Q6).
//!
//! A sign-in through an identity provider ends in [`complete`], with the [`Verified`] its
//! provider proved: the provider row, the subject, the mapped profile fields and attributes.
//! A local sign-in (a passkey, spec 04 B5) ends in [`complete_local`], keyed by the user the
//! credential belongs to: it runs step 1 alone and reads no identity, so signing in with a
//! local credential never depends on, or creates, a local identity. [`complete`] runs
//! [`admit`] and, only for an admitted user, creates the browser session (spec 08 B7) on the
//! same transaction. [`admit`] runs in a savepoint of the caller's transaction (B3): a refusal
//! rolls back every user and identity write it made, even when the caller then commits (a
//! sign-in flow's deletion, say), and a server fault returns an error, so the caller rolls
//! everything back. No path admits on an error. Before step 1, [`admit`] reads the provider
//! row again in the request's organization: a provider of another organization, or one
//! disabled since the caller read it, is a fault.
//!
//! The steps, in the plan's order:
//!
//! 1. **Known identity** (B4). The `identities` row for `(provider, subject)` names the
//!    candidate. Its user's row is locked (`identities::lock_user`, the lock `users::set_status`
//!    takes) and read again, so a suspension that commits first is seen, and one that comes
//!    later waits for this sign-in and then ends its session on the next request (spec 03 B7).
//!    A suspended user is refused with `auth.suspended` before anything is written.
//! 2. **Rules, sync-only mode, the admission webhook** (B8 to B19): not built. A known user
//!    goes on to the writes below, an unknown identity to step 5, and no identity is matched
//!    to an existing user any other way (B5, B6).
//! 3. **Just-in-time creation** (B20, B21). A person no identity matches is created as an `idp`
//!    user, with one identity, when the organization's `admission.signup_allowed` is on, after
//!    the users limit check (spec 10 B10). Off, they are refused with `auth.signup_closed`; a
//!    full plan refuses with `entitlement.limit_reached`. The user and identity are inserted in
//!    a savepoint of their own: when another first sign-in of the same subject commits its
//!    identity first, the insert fails as taken, the savepoint is rolled back, and step 1 runs
//!    again and finds that identity, so both sign-ins are admitted as one user. A local
//!    provider never gets here: its accounts exist before they sign in (spec 04 B3), so a
//!    local subject no identity matches is a server fault.
//!
//! An admitted known user's identity gets the newly mapped attributes (B7). Their profile
//! (names, display name, email) is updated from the provider only when the user is `idp` and
//! the identity is their oldest one, the one just-in-time creation made (Q6 (b)): a user an
//! admin or a sync owns keeps its values. When that identity is unlinked, the next oldest
//! takes over. A profile value the users table cannot store (too
//! long, NUL, an email with an invisible character) is left out as if the provider had sent
//! none, and so is an email another user of the organization has (users' emails are unique,
//! spec 03 B12): the field is left as it is, or a new user is created without it.
//!
//! A refusal answers 403 problem details with its code at a JSON endpoint
//! ([`Refusal::into_error`]) and a 303 to the sign-in page with `?error=<code>` at a browser
//! callback ([`Refusal::redirect`]); neither says more than the code. The server logs the
//! provider's key, the step and the code, never the subject, a profile value or an attribute.

use std::time::SystemTime;

use axum::{
    http::{HeaderMap, HeaderValue, StatusCode, header},
    response::{IntoResponse, Response},
};
use serde_json::{Map, Value};
use sqlx::{Acquire, PgConnection, Postgres, Transaction};
use stepclimb_core::{
    OrgScope, Source,
    audit::Actor,
    feature::Limit,
    id::Uuid,
    identities::{self, NewIdentity},
    identity_providers::{self, IdentityProvider, Kind},
    local_sign_in::PROVIDER_KEY as LOCAL_PROVIDER_KEY,
    org_plans::{self, Room},
    organizations,
    users::{self, MAX_NAME_CHARS, NewUser, Status, User, UserPatch},
};

use crate::{
    audit::ClientAddress,
    entitlements::{Entitlements, limit_reached},
    error::ApiError,
    provider_sign_in::SIGN_IN_PAGE,
    session,
};

/// The refusal codes this module produces (403). A full plan refuses with
/// `entitlement.limit_reached` (spec 10 B10).
pub mod code {
    /// The user is suspended (B4).
    pub const AUTH_SUSPENDED: &str = "auth.suspended";
    /// The person has no user in the organization, and it does not allow sign-up (B21).
    pub const AUTH_SIGNUP_CLOSED: &str = "auth.signup_closed";
}

/// The profile fields a provider mapped (B1); `None` for one it did not send. Never logged.
#[derive(Clone, Default, PartialEq, Eq)]
pub struct Profile {
    pub given_name: Option<String>,
    pub family_name: Option<String>,
    pub email: Option<String>,
}

/// A sign-in a provider has verified (B1). Never logged: it holds personal data.
pub struct Verified<'a> {
    /// The request organization's provider the person signed in with.
    pub provider: &'a IdentityProvider,
    /// The provider's identifier for the person; matched exactly.
    pub subject: &'a str,
    pub profile: Profile,
    /// The mapped attributes; empty for a local sign-in (spec 04 B5).
    pub attributes: Map<String, Value>,
}

/// Where and when the sign-in happens.
#[derive(Clone, Copy, Debug)]
pub struct Context {
    /// The request's organization (spec 08 B5).
    pub scope: OrgScope,
    /// The request's client address, for the audit entry of a just-in-time identity.
    pub client: ClientAddress,
    /// The request's clock reading.
    pub now: SystemTime,
}

/// The step that refused a sign-in, as the log names it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Step {
    /// Step 1: the identity is known (B4).
    KnownIdentity,
    /// Step 5: just-in-time creation (B20, B21).
    JustInTime,
}

impl Step {
    /// The name in log lines.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::KnownIdentity => "known_identity",
            Self::JustInTime => "just_in_time",
        }
    }
}

/// Why admission refused a sign-in: the step, and the 403 the person is answered with.
#[derive(Debug)]
pub struct Refusal {
    step: Step,
    error: ApiError,
}

impl Refusal {
    fn new(step: Step, error: ApiError) -> Self {
        Self { step, error }
    }

    /// The step that refused.
    #[must_use]
    pub fn step(&self) -> Step {
        self.step
    }

    /// The refusal's code.
    #[must_use]
    pub fn code(&self) -> &'static str {
        self.error.code()
    }

    /// The 403 problem details a JSON endpoint answers with (B2).
    #[must_use]
    pub fn into_error(self) -> ApiError {
        self.error
    }

    /// The 303 a browser callback answers with: to [`SIGN_IN_PAGE`] with `?error=<code>` and
    /// nothing else (B2), not cached.
    #[must_use]
    pub fn redirect(&self) -> Response {
        let location = format!("{SIGN_IN_PAGE}?error={}", self.code());
        // Every code is ASCII letters, `.` and `_`, which a header value holds.
        let location = HeaderValue::from_str(&location).expect("a refusal code is a header value");
        (
            StatusCode::SEE_OTHER,
            [
                (header::LOCATION, location),
                (header::CACHE_CONTROL, HeaderValue::from_static("no-store")),
            ],
        )
            .into_response()
    }
}

/// What [`admit`] decided.
#[derive(Debug)]
pub enum Admission {
    /// The sign-in is the user's, as admission left them.
    Admitted(User),
    Refused(Refusal),
}

/// A sign-in [`complete`] admitted: the user and their new session.
#[derive(Debug)]
pub struct Completed {
    pub user: User,
    pub session: session::Started,
}

/// Why [`complete`] made no session.
#[derive(Debug)]
pub enum Completion {
    /// Admission refused; its writes are rolled back and the caller's transaction is usable.
    Refused(Refusal),
    /// A server fault (500): the caller rolls its transaction back.
    Failed(ApiError),
}

/// Complete a local sign-in of the user `user_id` on `tx` (spec 04 B5): step 1 by the user's
/// id (lock the user's row, refuse a suspended user), then the session, ending the sessions
/// `request_headers`' cookies name. It reads and writes no identity. A user the organization
/// does not have is a fault.
pub async fn complete_local(
    tx: &mut Transaction<'_, Postgres>,
    scope: OrgScope,
    user_id: Uuid,
    now: SystemTime,
    request_headers: &HeaderMap,
) -> Result<session::Started, Completion> {
    let failed = |e: &dyn std::error::Error| Completion::Failed(ApiError::internal(e));
    let user = identities::lock_user(tx, scope, user_id)
        .await
        .map_err(|e| failed(&e))?;
    if let Some(refusal) = refused_at_step_1(&user) {
        log_refusal(LOCAL_PROVIDER_KEY, &refusal);
        return Err(Completion::Refused(refusal));
    }
    let session = session::start_on(tx, scope, user.id, request_headers, now)
        .await
        .map_err(|e| failed(&e))?;
    tracing::info!(provider = LOCAL_PROVIDER_KEY, user_id = %user.id, "admission admitted the sign-in");
    Ok(session)
}

/// Step 1's refusal of `user`, read under its row lock: `auth.suspended` for a suspended user.
fn refused_at_step_1(user: &User) -> Option<Refusal> {
    (user.status == Status::Suspended).then(|| Refusal::new(Step::KnownIdentity, suspended()))
}

/// The log line of a refusal (B2): the provider's key, the step and the code, nothing else.
fn log_refusal(provider: &str, refusal: &Refusal) {
    let (step, code) = (refusal.step.as_str(), refusal.code());
    tracing::info!(provider, step, code, "admission refused the sign-in");
}

/// Admit `sign_in` and create the admitted user's browser session on `tx` (B1, spec 08 B7),
/// ending the sessions `request_headers`' cookies name. A refusal or a fault makes no session.
pub async fn complete(
    tx: &mut Transaction<'_, Postgres>,
    sign_in: &Verified<'_>,
    context: &Context,
    request_headers: &HeaderMap,
) -> Result<Completed, Completion> {
    let user = match admit(tx, sign_in, context).await {
        Ok(Admission::Admitted(user)) => user,
        Ok(Admission::Refused(refusal)) => return Err(Completion::Refused(refusal)),
        Err(error) => return Err(Completion::Failed(error)),
    };
    let session = session::start_on(tx, context.scope, user.id, request_headers, context.now)
        .await
        .map_err(|e| Completion::Failed(ApiError::internal(&e)))?;
    Ok(Completed { user, session })
}

/// Run admission for `sign_in` in a savepoint of `tx` (module documentation): the admitted
/// user, or a refusal with every write rolled back. An error is a server fault, and the
/// savepoint is rolled back with it.
pub async fn admit(
    tx: &mut Transaction<'_, Postgres>,
    sign_in: &Verified<'_>,
    context: &Context,
) -> Result<Admission, ApiError> {
    in_scope(tx, sign_in.provider, context.scope).await?;
    identities::validate_subject(sign_in.subject).map_err(|e| ApiError::internal(&e))?;
    let mut sp = tx.begin().await.map_err(|e| ApiError::internal(&e))?;
    let provider = sign_in.provider.key.as_str();
    match steps(&mut sp, sign_in, context).await? {
        Admission::Admitted(user) => {
            sp.commit().await.map_err(|e| ApiError::internal(&e))?;
            tracing::info!(provider, user_id = %user.id, "admission admitted the sign-in");
            Ok(Admission::Admitted(user))
        }
        Admission::Refused(refusal) => {
            sp.rollback().await.map_err(|e| ApiError::internal(&e))?;
            log_refusal(provider, &refusal);
            Ok(Admission::Refused(refusal))
        }
    }
}

/// `Ok` when `provider` is an enabled provider of the scope's organization, read now; a fault
/// otherwise.
async fn in_scope(
    conn: &mut PgConnection,
    provider: &IdentityProvider,
    scope: OrgScope,
) -> Result<(), ApiError> {
    let stored = identity_providers::get_by_key(&mut *conn, scope, &provider.key)
        .await
        .map_err(|e| ApiError::internal(&e))?;
    match stored {
        Some(stored) if stored.id == provider.id && stored.enabled && provider.enabled => Ok(()),
        Some(stored) if stored.id == provider.id => {
            Err(ApiError::internal(&Fault::ProviderDisabled))
        }
        _ => Err(ApiError::internal(&Fault::ProviderNotInScope)),
    }
}

/// The steps of the module documentation, in order, on the savepoint `sp`.
async fn steps(
    sp: &mut Transaction<'_, Postgres>,
    sign_in: &Verified<'_>,
    context: &Context,
) -> Result<Admission, ApiError> {
    let scope = context.scope;
    let linked = identities::find_user(&mut **sp, scope, sign_in.provider.id, sign_in.subject)
        .await
        .map_err(|e| ApiError::internal(&e))?;
    match linked {
        Some(linked) => known(sp, sign_in, context, linked.identity_id, linked.user.id).await,
        None => just_in_time(sp, sign_in, context).await,
    }
}

/// Step 1 for the identity `identity_id` of the user `user_id` (B4), then B7 and Q6's writes.
async fn known(
    conn: &mut PgConnection,
    sign_in: &Verified<'_>,
    context: &Context,
    identity_id: Uuid,
    user_id: Uuid,
) -> Result<Admission, ApiError> {
    let scope = context.scope;
    let user = identities::lock_user(&mut *conn, scope, user_id)
        .await
        .map_err(|e| ApiError::internal(&e))?;
    if let Some(refusal) = refused_at_step_1(&user) {
        return Ok(Admission::Refused(refusal));
    }
    identities::update_attributes(
        &mut *conn,
        scope,
        identity_id,
        &sign_in.attributes,
        context.now,
    )
    .await
    .map_err(|e| ApiError::internal(&e))?;
    let user = update_profile(conn, context, user, identity_id, &sign_in.profile).await?;
    Ok(Admission::Admitted(user))
}

/// Q6 (b): `user` with the provider's profile, when the user is `idp` and `identity_id` is
/// their oldest identity; otherwise `user` as it is. Writes only when a field changes.
async fn update_profile(
    conn: &mut PgConnection,
    context: &Context,
    user: User,
    identity_id: Uuid,
    profile: &Profile,
) -> Result<User, ApiError> {
    if user.source != Source::Idp || *profile == Profile::default() {
        return Ok(user);
    }
    let scope = context.scope;
    let oldest = identities::list_for_user(&mut *conn, scope, user.id)
        .await
        .map_err(|e| ApiError::internal(&e))?
        .first()
        .map(|identity| identity.id);
    if oldest != Some(identity_id) {
        return Ok(user);
    }
    let mut profile = storable(profile);
    profile.email = free_email(&mut *conn, scope, Some(user.id), profile.email).await?;
    let patch = profile_patch(&user, &profile);
    if patch == UserPatch::default() {
        return Ok(user);
    }
    users::update(&mut *conn, scope, user.id, &patch, context.now)
        .await
        .map_err(|e| ApiError::internal(&e))
}

/// Step 5 (B20, B21) for a person no identity matches.
async fn just_in_time(
    sp: &mut Transaction<'_, Postgres>,
    sign_in: &Verified<'_>,
    context: &Context,
) -> Result<Admission, ApiError> {
    if sign_in.provider.kind == Kind::Local {
        return Err(ApiError::internal(&Fault::LocalWithoutIdentity));
    }
    let scope = context.scope;
    let refused = |error| Ok(Admission::Refused(Refusal::new(Step::JustInTime, error)));
    let settings = organizations::settings(&mut **sp, scope)
        .await
        .map_err(|e| ApiError::internal(&e))?;
    if !settings.admission.signup_allowed {
        return refused(signup_closed());
    }
    let entitlements = Entitlements::load(&mut **sp, scope).await?;
    let plan = &entitlements.plan().entitlements;
    let room = org_plans::check_room(sp, scope, plan, Limit::Users, 1)
        .await
        .map_err(|e| ApiError::internal(&e))?;
    if let Room::Full { max } = room {
        return refused(limit_reached(Limit::Users, max));
    }
    let mut profile = storable(&sign_in.profile);
    profile.email = free_email(sp, scope, None, profile.email).await?;
    let new = new_user(&profile, sign_in.subject, settings.default_tz);
    let mut insert = sp.begin().await.map_err(|e| ApiError::internal(&e))?;
    match create(&mut insert, sign_in, context, &new).await {
        Ok(user) => {
            insert.commit().await.map_err(|e| ApiError::internal(&e))?;
            Ok(Admission::Admitted(user))
        }
        Err(identities::Error::Taken) => {
            insert
                .rollback()
                .await
                .map_err(|e| ApiError::internal(&e))?;
            tracing::info!("admission found the identity created by a concurrent first sign-in");
            let linked =
                identities::find_user(&mut **sp, scope, sign_in.provider.id, sign_in.subject)
                    .await
                    .map_err(|e| ApiError::internal(&e))?
                    .ok_or_else(|| ApiError::internal(&Fault::TakenButMissing))?;
            known(sp, sign_in, context, linked.identity_id, linked.user.id).await
        }
        Err(other) => Err(ApiError::internal(&other)),
    }
}

/// Insert the user B20 creates, `new`, and their identity at the sign-in's provider, linked by
/// that user from the request's address. [`identities::Error::Taken`] when another user has the
/// identity.
async fn create(
    conn: &mut PgConnection,
    sign_in: &Verified<'_>,
    context: &Context,
    new: &NewUser,
) -> Result<User, identities::Error> {
    let scope = context.scope;
    let user = users::create(&mut *conn, scope, new, context.now).await?;
    let identity = NewIdentity {
        user_id: user.id,
        provider_id: sign_in.provider.id,
        subject: sign_in.subject.to_owned(),
        attributes: sign_in.attributes.clone(),
    };
    let actor = Actor::User {
        id: user.id,
        ip: context.client.0,
    };
    identities::link(&mut *conn, scope, &identity, context.now, actor).await?;
    Ok(user)
}

/// The user B20 creates from `profile`: `idp`, the mapped names and email, the display name
/// from the names or, without either, the subject.
fn new_user(profile: &Profile, subject: &str, tz: String) -> NewUser {
    let given_name = profile.given_name.clone().unwrap_or_default();
    let family_name = profile.family_name.clone().unwrap_or_default();
    let display_name =
        joined_name(&given_name, &family_name).unwrap_or_else(|| truncated(subject.trim()));
    NewUser {
        display_name,
        given_name,
        family_name,
        email: profile.email.clone(),
        locale: None,
        tz,
        source: Source::Idp,
        external_id: None,
    }
}

/// The changes `profile` makes to `user` (Q6): each field the provider sent that differs, and
/// the display name rebuilt from the names the user will have when the provider sent one.
fn profile_patch(user: &User, profile: &Profile) -> UserPatch {
    let mut patch = UserPatch::default();
    let changed = |sent: &Option<String>, stored: &str| {
        sent.as_ref()
            .filter(|sent| sent.as_str() != stored)
            .cloned()
    };
    patch.given_name = changed(&profile.given_name, &user.given_name);
    patch.family_name = changed(&profile.family_name, &user.family_name);
    if let Some(email) = &profile.email
        && user.email.as_deref() != Some(email.trim())
    {
        patch.email = Some(Some(email.clone()));
    }
    if profile.given_name.is_some() || profile.family_name.is_some() {
        let given = profile.given_name.as_deref().unwrap_or(&user.given_name);
        let family = profile.family_name.as_deref().unwrap_or(&user.family_name);
        patch.display_name = joined_name(given, family).filter(|name| *name != user.display_name);
    }
    patch
}

/// `given family`, each trimmed, without the blank ones, cut to [`MAX_NAME_CHARS`]; `None`
/// when both are blank.
fn joined_name(given: &str, family: &str) -> Option<String> {
    let parts: Vec<&str> = [given.trim(), family.trim()]
        .into_iter()
        .filter(|part| !part.is_empty())
        .collect();
    if parts.is_empty() {
        return None;
    }
    Some(truncated(&parts.join(" ")))
}

/// `text` cut to its first [`MAX_NAME_CHARS`] characters, trimmed again.
fn truncated(text: &str) -> String {
    let cut: String = text.chars().take(MAX_NAME_CHARS).collect();
    cut.trim_end().to_owned()
}

/// `profile` without the values the users table refuses, each left out as if not sent.
fn storable(profile: &Profile) -> Profile {
    let keep = |field: &'static str, value: &Option<String>, patch: fn(String) -> UserPatch| {
        let value = value.clone()?;
        if users::validate_patch(&patch(value.clone())).is_ok() {
            Some(value)
        } else {
            tracing::info!(
                field,
                "admission left out a profile value users cannot store"
            );
            None
        }
    };
    Profile {
        given_name: keep("given_name", &profile.given_name, |given| UserPatch {
            given_name: Some(given),
            ..UserPatch::default()
        }),
        family_name: keep("family_name", &profile.family_name, |family| UserPatch {
            family_name: Some(family),
            ..UserPatch::default()
        }),
        email: keep("email", &profile.email, |email| UserPatch {
            email: Some(Some(email)),
            ..UserPatch::default()
        }),
    }
}

/// `email` unless another user of the organization than `user_id` has it (spec 03 B12);
/// `None` then, logged without the address.
async fn free_email(
    conn: &mut PgConnection,
    scope: OrgScope,
    user_id: Option<Uuid>,
    email: Option<String>,
) -> Result<Option<String>, ApiError> {
    let Some(email) = email else {
        return Ok(None);
    };
    let holder = users::find_by_email(&mut *conn, scope, &email)
        .await
        .map_err(|e| ApiError::internal(&e))?;
    match holder {
        Some(holder) if Some(holder.id) != user_id => {
            tracing::info!("admission left out an email another user of the organization has");
            Ok(None)
        }
        _ => Ok(Some(email)),
    }
}

/// A fault admission refuses to sign in through.
#[derive(Debug, thiserror::Error)]
enum Fault {
    #[error("admission was called with a disabled identity provider")]
    ProviderDisabled,
    #[error("admission was called with a provider that is not the request organization's")]
    ProviderNotInScope,
    #[error("the identity was taken by another sign-in but cannot be read")]
    TakenButMissing,
    #[error("a local sign-in names an account that has no local identity")]
    LocalWithoutIdentity,
}

fn suspended() -> ApiError {
    ApiError::new(
        StatusCode::FORBIDDEN,
        code::AUTH_SUSPENDED,
        "This account is suspended. Contact an administrator of your organization.",
    )
}

fn signup_closed() -> ApiError {
    ApiError::new(
        StatusCode::FORBIDDEN,
        code::AUTH_SIGNUP_CLOSED,
        "This organization has no account for you, and it does not let new people sign up. Ask an administrator of your organization for an account.",
    )
}

#[cfg(test)]
mod tests {
    use std::time::UNIX_EPOCH;

    use http_body_util::BodyExt as _;

    use super::*;

    fn user(source: Source) -> User {
        User {
            id: Uuid::nil(),
            display_name: "Pat Example".into(),
            given_name: "Pat".into(),
            family_name: "Example".into(),
            email: Some("pat@example.test".into()),
            locale: "en".into(),
            tz: "UTC".into(),
            status: Status::Active,
            source,
            external_id: None,
            created_at: UNIX_EPOCH,
            updated_at: UNIX_EPOCH,
        }
    }

    fn profile(given: Option<&str>, family: Option<&str>, email: Option<&str>) -> Profile {
        Profile {
            given_name: given.map(str::to_owned),
            family_name: family.map(str::to_owned),
            email: email.map(str::to_owned),
        }
    }

    #[test]
    fn a_new_user_is_idp_with_the_mapped_names_and_email() {
        let new = new_user(
            &profile(Some("Robin"), Some("Doe"), Some("robin@example.test")),
            "sub-1",
            "Europe/Dublin".into(),
        );
        assert_eq!(new.display_name, "Robin Doe");
        assert_eq!(
            (new.given_name.as_str(), new.family_name.as_str()),
            ("Robin", "Doe")
        );
        assert_eq!(new.email.as_deref(), Some("robin@example.test"));
        assert_eq!(
            (new.source, new.tz.as_str()),
            (Source::Idp, "Europe/Dublin")
        );
        assert_eq!(new.external_id, None);
    }

    #[test]
    fn without_names_the_display_name_is_the_subject() {
        let new = new_user(&profile(None, Some("  "), None), "1000001", "UTC".into());
        assert_eq!(new.display_name, "1000001");
        assert_eq!(
            (new.given_name.as_str(), new.family_name.as_str()),
            ("", "  ")
        );
        let long = "s".repeat(255);
        let new = new_user(&Profile::default(), &long, "UTC".into());
        assert_eq!(new.display_name.chars().count(), MAX_NAME_CHARS);
    }

    #[test]
    fn one_name_alone_is_the_display_name() {
        assert_eq!(joined_name("", " Doe "), Some("Doe".into()));
        assert_eq!(joined_name("Robin", ""), Some("Robin".into()));
        assert_eq!(joined_name(" ", "\t"), None);
        let long = "a".repeat(MAX_NAME_CHARS);
        assert_eq!(
            joined_name(&long, "b").map(|name| name.chars().count()),
            Some(MAX_NAME_CHARS)
        );
    }

    #[test]
    fn a_profile_value_users_cannot_store_is_left_out() {
        let too_long = "a".repeat(MAX_NAME_CHARS + 1);
        let kept = storable(&profile(
            Some(&too_long),
            Some("Doe\0"),
            Some("robin\u{200B}@example.test"),
        ));
        assert!(kept == Profile::default());
        let kept = storable(&profile(Some("Robin"), None, Some(" robin@example.test ")));
        assert!(kept == profile(Some("Robin"), None, Some(" robin@example.test ")));
    }

    #[test]
    fn the_patch_names_only_what_changed() {
        let pat = user(Source::Idp);
        let same = profile(Some("Pat"), Some("Example"), Some(" pat@example.test"));
        assert_eq!(profile_patch(&pat, &same), UserPatch::default());
        assert_eq!(
            profile_patch(&pat, &Profile::default()),
            UserPatch::default()
        );
        let renamed = profile_patch(&pat, &profile(Some("Robin"), None, None));
        assert_eq!(
            renamed,
            UserPatch {
                given_name: Some("Robin".into()),
                display_name: Some("Robin Example".into()),
                ..UserPatch::default()
            }
        );
        let moved = profile_patch(&pat, &profile(None, None, Some("new@example.test")));
        assert_eq!(
            moved,
            UserPatch {
                email: Some(Some("new@example.test".into())),
                ..UserPatch::default()
            }
        );
    }

    #[test]
    fn blank_names_keep_the_display_name() {
        let pat = user(Source::Idp);
        let patch = profile_patch(&pat, &profile(Some(""), Some(""), None));
        assert_eq!(patch.display_name, None);
        assert_eq!(patch.given_name.as_deref(), Some(""));
    }

    #[tokio::test]
    async fn a_refusal_redirects_with_only_its_code() {
        let refusal = Refusal::new(Step::JustInTime, signup_closed());
        let response = refusal.redirect();
        assert_eq!(response.status(), StatusCode::SEE_OTHER);
        assert_eq!(
            response.headers()[header::LOCATION],
            "/sign-in?error=auth.signup_closed"
        );
        assert_eq!(response.headers()[header::CACHE_CONTROL], "no-store");
        assert!(response.headers().get(header::SET_COOKIE).is_none());
        let body = response.into_body().collect().await.unwrap().to_bytes();
        assert!(body.is_empty());
    }

    #[test]
    fn the_refusals_are_403_with_their_codes() {
        for (error, code) in [
            (suspended(), "auth.suspended"),
            (signup_closed(), "auth.signup_closed"),
        ] {
            let refusal = Refusal::new(Step::KnownIdentity, error);
            assert_eq!(refusal.code(), code);
            let problem = refusal.into_error().problem();
            assert_eq!((problem.status, problem.code.as_str()), (403, code));
        }
    }
}
