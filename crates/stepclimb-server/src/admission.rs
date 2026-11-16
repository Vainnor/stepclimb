// SPDX-License-Identifier: AGPL-3.0-or-later
//! Admission: after a provider has proved who someone is, whether they may sign in to the
//! organization, and as which user (docs/specs/m0/07-admission.md B1 to B17, B19 to B21, Q3,
//! Q4, Q6, Q7).
//!
//! A sign-in through an identity provider ends in [`complete`], with the [`Verified`] its
//! provider proved: the provider row, the subject, the mapped profile fields and attributes.
//! A local sign-in (a passkey, spec 04 B5) ends in [`complete_local`], keyed by the user the
//! credential belongs to: it runs step 1 alone and reads no identity, so signing in with a
//! local credential never depends on, or creates, a local identity. It runs no rules (step 2
//! reads mapped attributes, which a local sign-in has none of), but it refuses a user carrying
//! the refusal marker step 2 records. [`complete`] runs
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
//!
//!    With no identity, the person may still be an existing user (B5). A user an admin
//!    pre-provisioned an identity for is already a known identity (Q4 (b)). Otherwise, with the
//!    organization's `email_matching` on, the candidate is the user [`users::find_email_match`]
//!    finds for the sign-in's email, when all of these hold (spec 03 Q2 (a)): the provider
//!    marked the address verified ([`Verified::email_verified`]); the provider, as stored now,
//!    sets `trusted_for_email_matching` to exactly `true`; the user's stored email is recorded
//!    as verified (an admin's, or a verified sign-in through a trusted provider); the user is
//!    `local` or `idp`, never one an outside source owns, and has no identity at this provider
//!    yet; and both addresses are ASCII only, compared with ASCII letter case folded. The match locks the user's row; a
//!    suspended match is refused with `auth.suspended`, so a suspended person cannot get a
//!    second account by signing up. The identity is linked to the matched user, by that user
//!    from the request's address, only after the later steps admit them, in the savepoint, so
//!    a refusal leaves no link (B3).
//! 2. **Rules** (B8 to B10, Q3 (a)). Every sign-in that passed step 1, known identity or new
//!    person, must pass every rule of the organization's list and of the provider's list
//!    (`stepclimb_core::admission_rules`), read in this transaction. A rule that fails, or that
//!    cannot be evaluated (an attribute of the wrong type), refuses with
//!    `auth.admission_refused`; an absent attribute fails its condition (B9). A stored list
//!    that does not parse is a fault. With no rules, step 2 admits. A refusal of a known user
//!    records the refusal marker on the user after the savepoint is rolled back (the caller
//!    commits it), and a known user who passes the rules has it cleared. A local sign-in
//!    refuses a user who carries the marker, so signing in with a passkey or a password does
//!    not get round a rule the user's provider sign-in failed. A user matched by email counts as
//!    known here: a refusal marks them.
//! 3. **Sync-only mode** (B11, B12). A person with no candidate is refused with
//!    `auth.not_provisioned` when the organization's `admission.sync_only` is on, whether or not
//!    sign-up is allowed.
//! 4. **Admission webhook** (B13 to B17, B19, Q3 (a)). Every sign-in through an outside
//!    provider that got this far, known user or new person, is sent to the organization's
//!    enabled webhook (`crate::admission_webhook_call`), whatever the plan's features. A reply
//!    of `allow: false` refuses with `auth.admission_refused` and, for a known user, records the
//!    refusal marker as step 2 does. Any failure (the secret, the URL policy, the 3-second
//!    limit, the connection, the status, the reply's shape) refuses with
//!    `auth.admission_unavailable` and records no marker: the webhook decided nothing, and the
//!    savepoint's rollback leaves any marker as it was. `allow: true` admits, and the reply's
//!    `attributes` are merged over the mapped ones (its keys win) for the writes below. A local
//!    sign-in never calls it.
//! 5. **Just-in-time creation** (B20, B21). A person no identity matches is created as an `idp`
//!    user, with one identity, when the organization's `admission.signup_allowed` is on, after
//!    the users limit check (spec 10 B10). Off, they are refused with `auth.signup_closed`; a
//!    full plan refuses with `entitlement.limit_reached`. The user and identity are inserted in
//!    a savepoint of their own: when another first sign-in of the same subject commits its
//!    identity first, the insert fails as taken, the savepoint is rolled back, and step 1 runs
//!    again and finds that identity, so both sign-ins are admitted as one user; a link by email
//!    that loses the same race does the same. A local provider never gets past step 1 without a
//!    candidate: its accounts exist before they sign in (spec 04 B3), so a local subject no
//!    identity matches is a server fault. When the provider is trusted for matching and marked
//!    the email verified, the new user's email is recorded as verified.
//!
//! An admitted known user's identity gets the newly mapped attributes, with the webhook's merged
//! over them (B7, B19). Their profile
//! (names, display name, email) is updated from the provider only when the user is `idp` and
//! the identity is their oldest one, the one just-in-time creation made (Q6 (b)): a user an
//! admin or a sync owns keeps its values. An email it changes loses the record of a verified
//! email, and the email is recorded as verified again when the provider is trusted for matching
//! and marked it verified. When that identity is unlinked, the next oldest
//! takes over. A profile value the users table cannot store (too
//! long, NUL, an email with an invisible character) is left out as if the provider had sent
//! none, and so is an email another user of the organization has (users' emails are unique,
//! spec 03 B12): the field is left as it is, or a new user is created without it.
//!
//! A refusal answers 403 problem details with its code at a JSON endpoint
//! ([`Refusal::into_error`]) and a 303 to the sign-in page with `?error=<code>` at a browser
//! callback ([`Refusal::redirect`]); neither says more than the code. The server logs the
//! provider's key, the step and the code, and for a rule's refusal the rule's id (its pointer
//! in the admin API, `/rules/organization/0`) and whether it failed or could not be evaluated;
//! never the subject, a profile value or an attribute. Each step logs at debug level as it
//! starts, which is how a test sees the order the steps ran in (T21).

use std::time::SystemTime;

use axum::{
    http::{HeaderMap, HeaderValue, StatusCode, header},
    response::{IntoResponse, Response},
};
use serde_json::{Map, Value};
use sqlx::{Acquire, PgConnection, Postgres, Transaction};
use stepclimb_auth::encryption::KeyRing;
use stepclimb_core::{
    OrgScope, OrgSettings, Source,
    admission_rules::{self, Outcome, RuleId},
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
    admission_webhook_call::{self as webhook_call, Call, Decision},
    audit::ClientAddress,
    entitlements::{Entitlements, limit_reached},
    error::ApiError,
    provider_sign_in::SIGN_IN_PAGE,
    providers::Providers,
    session,
};
use stepclimb_auth::provider_config::TRUSTED_FOR_EMAIL_MATCHING;

/// The refusal codes this module produces (403). A full plan refuses with
/// `entitlement.limit_reached` (spec 10 B10).
pub mod code {
    /// The user is suspended (B4).
    pub const AUTH_SUSPENDED: &str = "auth.suspended";
    /// The person has no user in the organization, and it does not allow sign-up (B21).
    pub const AUTH_SIGNUP_CLOSED: &str = "auth.signup_closed";
    /// A rule of the organization's or the provider's list does not hold (B8, B9).
    pub const AUTH_ADMISSION_REFUSED: &str = "auth.admission_refused";
    /// The person has no user in the organization, and it admits only provisioned users (B11).
    pub const AUTH_NOT_PROVISIONED: &str = "auth.not_provisioned";
    /// The admission webhook could not decide the sign-in (B14).
    pub const AUTH_ADMISSION_UNAVAILABLE: &str = "auth.admission_unavailable";
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
    /// Whether the provider vouched for `profile.email`: the OIDC claims say `email_verified`
    /// is `true` (spec 05 B13), or the OAuth 2 mapping's verified flag does (spec 06 B10).
    /// `false` for an OAuth 2 provider that maps no verified flag, and with no email.
    pub email_verified: bool,
    /// The mapped attributes; empty for a local sign-in (spec 04 B5).
    pub attributes: Map<String, Value>,
}

/// Where and when the sign-in happens, and what step 4 calls the webhook with.
#[derive(Clone, Copy, Debug)]
pub struct Context<'a> {
    /// The request's organization (spec 08 B5).
    pub scope: OrgScope,
    /// The request's client address, for the audit entry of a just-in-time identity.
    pub client: ClientAddress,
    /// The request's clock reading.
    pub now: SystemTime,
    /// The key ring the webhook's signing secret is sealed with.
    pub keys: &'a KeyRing,
    /// The outbound clients; step 4 picks the webhook's by the instance's tenancy.
    pub providers: &'a Providers,
}

/// The step that refused a sign-in, as the log names it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Step {
    /// Step 1: the identity is known (B4).
    KnownIdentity,
    /// Step 2: the declarative rules (B8 to B10).
    Rules,
    /// Step 3: sync-only mode (B11).
    SyncOnly,
    /// Step 4: the admission webhook (B13 to B17).
    Webhook,
    /// Step 5: just-in-time creation (B20, B21).
    JustInTime,
}

impl Step {
    /// The name in log lines.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::KnownIdentity => "known_identity",
            Self::Rules => "rules",
            Self::SyncOnly => "sync_only",
            Self::Webhook => "webhook",
            Self::JustInTime => "just_in_time",
        }
    }
}

/// Why admission refused a sign-in: the step, and the 403 the person is answered with.
#[derive(Debug)]
pub struct Refusal {
    step: Step,
    error: ApiError,
    /// The rule that refused at step 2, and whether it failed or could not be evaluated; boxed
    /// to keep the refusal small on the paths that have none.
    rule: Option<Box<(RuleId, Outcome)>>,
    /// The known user whose refusal at step 2, or by the webhook at step 4, [`admit`] records
    /// as the refusal marker, after it rolled the sign-in's writes back.
    marks: Option<Uuid>,
}

impl Refusal {
    fn new(step: Step, error: ApiError) -> Self {
        Self {
            step,
            error,
            rule: None,
            marks: None,
        }
    }

    /// A refusal at step 2 by `rule`; `known` is the user of a known identity, whom the
    /// refusal marks.
    fn by_rule(rule: RuleId, outcome: Outcome, known: Option<Uuid>) -> Self {
        Self {
            step: Step::Rules,
            error: admission_refused(),
            rule: Some(Box::new((rule, outcome))),
            marks: known,
        }
    }

    /// A refusal by the webhook's `allow: false` (B15); `known` is the candidate user, whom
    /// the refusal marks.
    fn by_webhook(known: Option<Uuid>) -> Self {
        Self {
            step: Step::Webhook,
            error: admission_refused(),
            rule: None,
            marks: known,
        }
    }

    /// A local sign-in's refusal of a user who carries the refusal marker.
    fn by_marker() -> Self {
        Self::new(Step::Rules, admission_refused())
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
/// id (lock the user's row, refuse a suspended user), then the refusal marker (a user whose
/// last refusal by the rules has not been cleared is refused with `auth.admission_refused`),
/// then the session, ending the sessions `request_headers`' cookies name. It reads and writes
/// no identity. A user the organization does not have is a fault.
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
    let refusal = match refused_at_step_1(&user) {
        Some(refusal) => Some(refusal),
        None => refused_by_marker(tx, scope, user.id)
            .await
            .map_err(Completion::Failed)?,
    };
    if let Some(refusal) = refusal {
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

/// The refusal of a local sign-in by the user `user_id`, who carries the refusal marker, or
/// `None`.
async fn refused_by_marker(
    conn: &mut PgConnection,
    scope: OrgScope,
    user_id: Uuid,
) -> Result<Option<Refusal>, ApiError> {
    let marked = admission_rules::refused_by(&mut *conn, scope, user_id)
        .await
        .map_err(|e| ApiError::internal(&e))?;
    Ok(marked.map(|_| Refusal::by_marker()))
}

/// The log line of a refusal (B2): the provider's key, the step and the code, and a rule's id
/// and outcome; nothing else.
fn log_refusal(provider: &str, refusal: &Refusal) {
    let (step, code) = (refusal.step.as_str(), refusal.code());
    match refusal.rule.as_deref() {
        Some((rule, outcome)) => {
            let (rule, outcome) = (rule.to_string(), outcome_name(*outcome));
            tracing::info!(
                provider,
                step,
                code,
                rule,
                outcome,
                "admission refused the sign-in"
            );
        }
        None => tracing::info!(provider, step, code, "admission refused the sign-in"),
    }
}

/// How a log line names a rule's outcome.
fn outcome_name(outcome: Outcome) -> &'static str {
    match outcome {
        Outcome::Holds => "holds",
        Outcome::Fails => "fails",
        Outcome::Unevaluable => "unevaluable",
    }
}

/// The debug line each step logs as it starts (T21).
fn log_step(step: Step) {
    tracing::debug!(step = step.as_str(), "admission runs a step");
}

/// Admit `sign_in` and create the admitted user's browser session on `tx` (B1, spec 08 B7),
/// ending the sessions `request_headers`' cookies name. A refusal or a fault makes no session.
pub async fn complete(
    tx: &mut Transaction<'_, Postgres>,
    sign_in: &Verified<'_>,
    context: &Context<'_>,
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
/// user, or a refusal with every write rolled back. A refusal of a known user's sign-in by the
/// rules or by the webhook's `allow: false` then records the refusal marker on `tx` itself,
/// which the caller commits. An error
/// is a server fault, and the savepoint is rolled back with it.
pub async fn admit(
    tx: &mut Transaction<'_, Postgres>,
    sign_in: &Verified<'_>,
    context: &Context<'_>,
) -> Result<Admission, ApiError> {
    let stored = in_scope(tx, sign_in.provider, context.scope).await?;
    identities::validate_subject(sign_in.subject).map_err(|e| ApiError::internal(&e))?;
    // The steps read the provider as stored now: its rules and its trust for matching.
    let sign_in = Verified {
        provider: &stored,
        subject: sign_in.subject,
        profile: sign_in.profile.clone(),
        email_verified: sign_in.email_verified,
        attributes: sign_in.attributes.clone(),
    };
    let mut sp = tx.begin().await.map_err(|e| ApiError::internal(&e))?;
    let provider = stored.key.as_str();
    match steps(&mut sp, &sign_in, context).await? {
        Admission::Admitted(user) => {
            sp.commit().await.map_err(|e| ApiError::internal(&e))?;
            tracing::info!(provider, user_id = %user.id, "admission admitted the sign-in");
            Ok(Admission::Admitted(user))
        }
        Admission::Refused(refusal) => {
            sp.rollback().await.map_err(|e| ApiError::internal(&e))?;
            if let Some(user_id) = refusal.marks {
                admission_rules::mark_refused(
                    &mut **tx,
                    context.scope,
                    user_id,
                    provider,
                    context.now,
                )
                .await
                .map_err(|e| ApiError::internal(&e))?;
            }
            log_refusal(provider, &refusal);
            Ok(Admission::Refused(refusal))
        }
    }
}

/// The row of `provider` as stored now, when it is an enabled provider of the scope's
/// organization; a fault otherwise. Step 2 reads the provider's rules from this row, and B5
/// its trust for matching.
async fn in_scope(
    conn: &mut PgConnection,
    provider: &IdentityProvider,
    scope: OrgScope,
) -> Result<IdentityProvider, ApiError> {
    let stored = identity_providers::get_by_key(&mut *conn, scope, &provider.key)
        .await
        .map_err(|e| ApiError::internal(&e))?;
    match stored {
        Some(stored) if stored.id == provider.id && stored.enabled && provider.enabled => {
            Ok(stored)
        }
        Some(stored) if stored.id == provider.id => {
            Err(ApiError::internal(&Fault::ProviderDisabled))
        }
        _ => Err(ApiError::internal(&Fault::ProviderNotInScope)),
    }
}

/// The user a sign-in is for after step 1: the user of its known identity, or the user it was
/// matched to by email (B5), whose identity is not linked yet.
enum Candidate {
    Linked { identity_id: Uuid, user: User },
    Matched(User),
}

impl Candidate {
    fn user_id(&self) -> Uuid {
        match self {
            Self::Linked { user, .. } | Self::Matched(user) => user.id,
        }
    }
}

/// The steps of the module documentation, in order, on the savepoint `sp`. The sign-in's
/// provider is the row [`in_scope`] read.
async fn steps(
    sp: &mut Transaction<'_, Postgres>,
    sign_in: &Verified<'_>,
    context: &Context<'_>,
) -> Result<Admission, ApiError> {
    let scope = context.scope;
    let provider = sign_in.provider;
    log_step(Step::KnownIdentity);
    let (candidate, settings) = match candidate(sp, sign_in, scope).await? {
        Ok(found) => found,
        Err(refusal) => return Ok(Admission::Refused(refusal)),
    };
    let known = candidate.as_ref().map(Candidate::user_id);
    if let Some(refusal) = rules(sp, provider, &sign_in.attributes, scope, known).await? {
        return Ok(Admission::Refused(refusal));
    }
    if candidate.is_none()
        && let Some(refusal) = settings.as_ref().and_then(sync_only)
    {
        return Ok(Admission::Refused(refusal));
    }
    let attributes = match webhook(sp, sign_in, context, known).await? {
        Ok(attributes) => attributes,
        Err(refusal) => return Ok(Admission::Refused(refusal)),
    };
    let sign_in = &Verified {
        provider,
        subject: sign_in.subject,
        profile: sign_in.profile.clone(),
        email_verified: sign_in.email_verified,
        attributes,
    };
    match (candidate, settings) {
        (Some(Candidate::Linked { identity_id, user }), _) => {
            admit_known(sp, sign_in, context, identity_id, user).await
        }
        (Some(Candidate::Matched(user)), _) => admit_matched(sp, sign_in, context, user).await,
        (None, Some(settings)) => just_in_time(sp, sign_in, context, settings).await,
        (None, None) => Err(ApiError::internal(&Fault::LocalWithoutIdentity)),
    }
}

/// Step 1 (B4, B5): the candidate, or the refusal of a suspended one. For a sign-in no
/// identity matches, also the organization's settings, read once for the steps that follow; a
/// local sign-in has none and reads none, since it is matched by its identity alone.
async fn candidate(
    sp: &mut Transaction<'_, Postgres>,
    sign_in: &Verified<'_>,
    scope: OrgScope,
) -> Result<Result<(Option<Candidate>, Option<OrgSettings>), Refusal>, ApiError> {
    let linked = identities::find_user(&mut **sp, scope, sign_in.provider.id, sign_in.subject)
        .await
        .map_err(|e| ApiError::internal(&e))?;
    if let Some(linked) = linked {
        let identity_id = linked.identity_id;
        return Ok(known_user(sp, scope, linked.user.id)
            .await?
            .map(|user| (Some(Candidate::Linked { identity_id, user }), None)));
    }
    if sign_in.provider.kind == Kind::Local {
        return Ok(Ok((None, None)));
    }
    let settings = organizations::settings(&mut **sp, scope)
        .await
        .map_err(|e| ApiError::internal(&e))?;
    let matched = match email_match(sp, sign_in, scope, &settings).await? {
        Some(Ok(user)) => Some(Candidate::Matched(user)),
        Some(Err(refusal)) => return Ok(Err(refusal)),
        None => None,
    };
    Ok(Ok((matched, Some(settings))))
}

/// B5's email match for a sign-in no identity matches (module documentation): the matched
/// user, read under its row lock, or the refusal of a suspended one; `None` when matching is
/// off, the conditions do not hold, or no user qualifies.
async fn email_match(
    conn: &mut PgConnection,
    sign_in: &Verified<'_>,
    scope: OrgScope,
    settings: &OrgSettings,
) -> Result<Option<Result<User, Refusal>>, ApiError> {
    if !settings.email_matching {
        return Ok(None);
    }
    let Some(email) = vouched_email(sign_in) else {
        return Ok(None);
    };
    let matched = users::find_email_match(&mut *conn, scope, &email, sign_in.provider.id)
        .await
        .map_err(|e| ApiError::internal(&e))?;
    Ok(matched.map(|user| match refused_at_step_1(&user) {
        Some(refusal) => Err(refusal),
        None => Ok(user),
    }))
}

/// The sign-in's email when it can be matched or recorded as verified (spec 03 Q2 (a)): the
/// provider marked it verified, the provider is trusted for matching, the users table can store
/// it, and it is ASCII only, so no Unicode case rule takes part in a match. `None` otherwise.
fn vouched_email(sign_in: &Verified<'_>) -> Option<String> {
    if !sign_in.email_verified || !trusted_for_matching(sign_in.provider) {
        return None;
    }
    storable(&sign_in.profile)
        .email
        .filter(|email| email.is_ascii())
}

/// Whether `provider`'s config sets `trusted_for_email_matching` to exactly `true`. A local
/// provider never is: its sign-ins are matched by their identity alone.
fn trusted_for_matching(provider: &IdentityProvider) -> bool {
    provider.kind != Kind::Local
        && provider.config.get(TRUSTED_FOR_EMAIL_MATCHING) == Some(&Value::Bool(true))
}

/// Step 3 (B11) for a person with no candidate, under the organization's `settings`: the
/// refusal in sync-only mode, or `None`.
fn sync_only(settings: &OrgSettings) -> Option<Refusal> {
    log_step(Step::SyncOnly);
    settings
        .admission
        .sync_only
        .then(|| Refusal::new(Step::SyncOnly, not_provisioned()))
}

/// Step 4 (B13 to B17, B19): the attributes the later writes store, the mapped ones with the
/// webhook's merged over them, or the webhook's refusal. `known` is the candidate user, whom an
/// `allow: false` marks. A local sign-in calls no webhook and keeps its attributes.
async fn webhook(
    conn: &mut PgConnection,
    sign_in: &Verified<'_>,
    context: &Context<'_>,
    known: Option<Uuid>,
) -> Result<Result<Map<String, Value>, Refusal>, ApiError> {
    let provider = sign_in.provider;
    if provider.kind == Kind::Local {
        return Ok(Ok(sign_in.attributes.clone()));
    }
    log_step(Step::Webhook);
    let call = Call {
        provider: &provider.key,
        subject: sign_in.subject,
        attributes: &sign_in.attributes,
    };
    let (keys, providers) = (context.keys, context.providers);
    let decision =
        webhook_call::decide(conn, context.scope, context.now, keys, providers, &call).await?;
    Ok(match decision {
        Decision::Skipped => Ok(sign_in.attributes.clone()),
        Decision::Allowed(extra) => {
            let mut merged = sign_in.attributes.clone();
            merged.extend(extra);
            Ok(merged)
        }
        Decision::Refused => Err(Refusal::by_webhook(known)),
        Decision::Unavailable(reason) => {
            let provider = provider.key.as_str();
            tracing::warn!(provider, %reason, "the admission webhook could not decide the sign-in");
            Err(Refusal::new(Step::Webhook, admission_unavailable()))
        }
    })
}

/// Step 1 for the user `user_id` of a known identity (B4): the user read under its row lock,
/// or the refusal of a suspended one.
async fn known_user(
    conn: &mut PgConnection,
    scope: OrgScope,
    user_id: Uuid,
) -> Result<Result<User, Refusal>, ApiError> {
    let user = identities::lock_user(&mut *conn, scope, user_id)
        .await
        .map_err(|e| ApiError::internal(&e))?;
    Ok(match refused_at_step_1(&user) {
        Some(refusal) => Err(refusal),
        None => Ok(user),
    })
}

/// Step 2 (B8 to B10): the refusal of the first rule of the organization's or `provider`'s
/// list that does not hold for `attributes`, or `None`. `known` is the user of a known
/// identity: a refusal marks them, and passing the rules clears their marker. A local provider
/// has no rules, since its sign-ins carry no mapped attributes; its known user is checked for
/// the refusal marker instead, as [`complete_local`] does.
async fn rules(
    conn: &mut PgConnection,
    provider: &IdentityProvider,
    attributes: &Map<String, Value>,
    scope: OrgScope,
    known: Option<Uuid>,
) -> Result<Option<Refusal>, ApiError> {
    if provider.kind == Kind::Local {
        return match known {
            Some(user_id) => refused_by_marker(conn, scope, user_id).await,
            None => Ok(None),
        };
    }
    log_step(Step::Rules);
    let rules = admission_rules::for_sign_in(&mut *conn, scope, provider)
        .await
        .map_err(|e| ApiError::internal(&e))?;
    if let Some((rule, outcome)) = rules.refusal(attributes) {
        return Ok(Some(Refusal::by_rule(rule, outcome, known)));
    }
    if let Some(user_id) = known {
        admission_rules::clear_refused(&mut *conn, scope, user_id)
            .await
            .map_err(|e| ApiError::internal(&e))?;
    }
    Ok(None)
}

/// B7 and Q6's writes for the admitted user `user` of the identity `identity_id`.
async fn admit_known(
    conn: &mut PgConnection,
    sign_in: &Verified<'_>,
    context: &Context<'_>,
    identity_id: Uuid,
    user: User,
) -> Result<Admission, ApiError> {
    identities::update_attributes(
        &mut *conn,
        context.scope,
        identity_id,
        &sign_in.attributes,
        context.now,
    )
    .await
    .map_err(|e| ApiError::internal(&e))?;
    let user = update_profile(conn, sign_in, context, user, identity_id).await?;
    Ok(Admission::Admitted(user))
}

/// B5's link of the identity to `user`, the user it was matched to by email, by that user from
/// the request's address, then Q6's profile writes. When another sign-in of the same subject
/// linked the identity first, the link fails as taken and the sign-in is admitted as the user
/// that identity belongs to, as step 5 does.
async fn admit_matched(
    sp: &mut Transaction<'_, Postgres>,
    sign_in: &Verified<'_>,
    context: &Context<'_>,
    user: User,
) -> Result<Admission, ApiError> {
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
    let mut insert = sp.begin().await.map_err(|e| ApiError::internal(&e))?;
    match identities::link(&mut insert, context.scope, &identity, context.now, actor).await {
        Ok(linked) => {
            insert.commit().await.map_err(|e| ApiError::internal(&e))?;
            let provider = sign_in.provider.key.as_str();
            tracing::info!(provider, user_id = %user.id, "admission linked the identity by email");
            let user = update_profile(sp, sign_in, context, user, linked.id).await?;
            Ok(Admission::Admitted(user))
        }
        Err(identities::Error::Taken) => {
            insert
                .rollback()
                .await
                .map_err(|e| ApiError::internal(&e))?;
            admit_linked_meanwhile(sp, sign_in, context).await
        }
        Err(other) => Err(ApiError::internal(&other)),
    }
}

/// The admission of a sign-in whose identity another, concurrent sign-in linked first: step 1
/// again, for the user that identity belongs to.
async fn admit_linked_meanwhile(
    sp: &mut Transaction<'_, Postgres>,
    sign_in: &Verified<'_>,
    context: &Context<'_>,
) -> Result<Admission, ApiError> {
    tracing::info!("admission found the identity created by a concurrent first sign-in");
    let scope = context.scope;
    let linked = identities::find_user(&mut **sp, scope, sign_in.provider.id, sign_in.subject)
        .await
        .map_err(|e| ApiError::internal(&e))?
        .ok_or_else(|| ApiError::internal(&Fault::TakenButMissing))?;
    match known_user(sp, scope, linked.user.id).await? {
        Ok(user) => admit_known(sp, sign_in, context, linked.identity_id, user).await,
        Err(refusal) => Ok(Admission::Refused(refusal)),
    }
}

/// Q6 (b): `user` with the provider's profile, when the user is `idp` and `identity_id` is
/// their oldest identity; otherwise `user` as it is. Writes only when a field changes, then
/// records the email as verified when [`vouched_email`] vouches for the one the user now has.
async fn update_profile(
    conn: &mut PgConnection,
    sign_in: &Verified<'_>,
    context: &Context<'_>,
    user: User,
    identity_id: Uuid,
) -> Result<User, ApiError> {
    if user.source != Source::Idp || sign_in.profile == Profile::default() {
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
    let mut profile = storable(&sign_in.profile);
    profile.email = free_email(&mut *conn, scope, Some(user.id), profile.email).await?;
    let patch = profile_patch(&user, &profile);
    let user = if patch == UserPatch::default() {
        user
    } else {
        users::update(&mut *conn, scope, user.id, &patch, context.now)
            .await
            .map_err(|e| ApiError::internal(&e))?
    };
    if profile.email.is_some() {
        record_vouched_email(conn, sign_in, scope, user.id).await?;
    }
    Ok(user)
}

/// Record the sign-in's email as the verified address of the user `user_id` when
/// [`vouched_email`] vouches for it; [`users::record_verified_email`] records only when it is
/// the user's stored email.
async fn record_vouched_email(
    conn: &mut PgConnection,
    sign_in: &Verified<'_>,
    scope: OrgScope,
    user_id: Uuid,
) -> Result<(), ApiError> {
    let Some(email) = vouched_email(sign_in) else {
        return Ok(());
    };
    users::record_verified_email(&mut *conn, scope, user_id, &email)
        .await
        .map_err(|e| ApiError::internal(&e))?;
    Ok(())
}

/// Step 5 (B20, B21) for a person with no candidate, under the organization's `settings`.
async fn just_in_time(
    sp: &mut Transaction<'_, Postgres>,
    sign_in: &Verified<'_>,
    context: &Context<'_>,
    settings: OrgSettings,
) -> Result<Admission, ApiError> {
    log_step(Step::JustInTime);
    let scope = context.scope;
    let refused = |error| Ok(Admission::Refused(Refusal::new(Step::JustInTime, error)));
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
            if user.email.is_some() {
                record_vouched_email(sp, sign_in, scope, user.id).await?;
            }
            Ok(Admission::Admitted(user))
        }
        Err(identities::Error::Taken) => {
            insert
                .rollback()
                .await
                .map_err(|e| ApiError::internal(&e))?;
            admit_linked_meanwhile(sp, sign_in, context).await
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
    context: &Context<'_>,
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

fn admission_refused() -> ApiError {
    ApiError::new(
        StatusCode::FORBIDDEN,
        code::AUTH_ADMISSION_REFUSED,
        "This organization's sign-in rules do not let you in. Contact an administrator of your organization.",
    )
}

fn not_provisioned() -> ApiError {
    ApiError::new(
        StatusCode::FORBIDDEN,
        code::AUTH_NOT_PROVISIONED,
        "This organization lets in only people it already has an account for, and it has none for you. Ask an administrator of your organization for an account.",
    )
}

fn admission_unavailable() -> ApiError {
    ApiError::new(
        StatusCode::FORBIDDEN,
        code::AUTH_ADMISSION_UNAVAILABLE,
        "This organization could not check your sign-in just now. Try again in a few minutes, or contact an administrator of your organization.",
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
            (admission_refused(), "auth.admission_refused"),
            (not_provisioned(), "auth.not_provisioned"),
            (admission_unavailable(), "auth.admission_unavailable"),
        ] {
            let refusal = Refusal::new(Step::KnownIdentity, error);
            assert_eq!(refusal.code(), code);
            let problem = refusal.into_error().problem();
            assert_eq!((problem.status, problem.code.as_str()), (403, code));
        }
    }
}
