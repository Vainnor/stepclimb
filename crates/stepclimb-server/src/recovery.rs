// SPDX-License-Identifier: AGPL-3.0-or-later
//! `stepclimb admin enrollment-link` (docs/specs/m0/04-local-accounts.md B10, coordinator
//! decision 2026-11-14): a new enrollment link for an organization's admin, from the host's
//! shell.
//!
//! Without it an instance whose first admin's link expired unused, or whose `admin create`
//! commit outcome was unknown, has no way in: only an admin can create a link, and no one can
//! sign in. Like `admin create` it needs shell access to the host, the trust boundary a fresh
//! install already has, and it exists only on the command line: no HTTP route, environment
//! variable or setting triggers it.
//!
//! The organization (by slug) and the user (by email or id) are always named. Under the user's
//! row lock it refuses a user who is not an Organization admin at organization scope, unless
//! `--admin-required-override`, and a user who already has a way to sign in (a local credential,
//! or an identity at an enabled provider), unless `--force`. It then makes the link with
//! [`enrollment::create_link`], which deletes the user's earlier unused links and writes an
//! `enrollment_link.create` entry by the instance with `recovery: true`, and commits before the
//! caller prints the link.

use std::time::SystemTime;

use sqlx::PgPool;
use stepclimb_core::{
    audit::Actor,
    clock::Clock,
    id::Uuid,
    identities, local,
    local_tokens::Origin,
    organizations,
    role_assignments::{self, GrantScope},
    users::{self, User},
};

use crate::{
    Config,
    admin::{ORG_ADMIN_ROLE, cli_scope},
    enrollment::{self, LinkError, SetupLink},
    startup,
    web_origin::WebOrigin,
};

/// How the command names the user.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum UserRef {
    Id(Uuid),
    /// Compared in any letter case, as the `users_email` index does.
    Email(String),
}

impl UserRef {
    /// A UUID names the user by id; anything else is an email.
    pub fn parse(value: &str) -> Self {
        match Uuid::try_parse(value.trim()) {
            Ok(id) => Self::Id(id),
            Err(_) => Self::Email(value.to_owned()),
        }
    }
}

/// What `stepclimb admin enrollment-link` is asked to do.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RecoveryRequest {
    pub org_slug: String,
    pub user: UserRef,
    /// Allow a user who is not an Organization admin at organization scope.
    pub admin_required_override: bool,
    /// Allow a user who already has a way to sign in.
    pub force: bool,
}

/// The link the command made, committed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Recovered {
    pub org_id: Uuid,
    pub user: User,
    pub link: SetupLink,
}

/// Why the command made no link. Nothing was written, except after [`RecoveryError::Commit`],
/// whose outcome is unknown.
#[derive(Debug, thiserror::Error)]
pub enum RecoveryError {
    #[error("no organization has this slug")]
    OrganizationNotFound,
    #[error("the organization has no user with this email or id")]
    UserNotFound,
    #[error(
        "the user is not an Organization admin at organization scope; pass \
         --admin-required-override to make the link anyway"
    )]
    NotOrgAdmin,
    #[error(
        "the user can already sign in (a local credential or an identity at an enabled \
         provider); pass --force to make the link anyway"
    )]
    HasSignInMethod,
    #[error("the user has no email address, which local sign-in finds them by")]
    EmailMissing,
    #[error("could not read the organization, the user or their sign-in methods: {0}")]
    Read(String),
    #[error("could not create the enrollment link: {0}")]
    Link(#[source] LinkError),
    #[error("could not start the transaction: {0}")]
    Begin(#[source] sqlx::Error),
    #[error("could not commit the transaction, so whether the link exists is unknown: {0}")]
    Commit(#[source] sqlx::Error),
}

impl RecoveryError {
    /// Whether the command refuses what it was asked (exit 2) rather than failing (exit 1).
    pub fn is_refusal(&self) -> bool {
        matches!(
            self,
            Self::OrganizationNotFound
                | Self::UserNotFound
                | Self::NotOrgAdmin
                | Self::HasSignInMethod
                | Self::EmailMissing
        )
    }
}

/// Why `stepclimb admin enrollment-link` stopped.
#[derive(Debug, thiserror::Error)]
pub enum RecoveryCommandError {
    #[error(transparent)]
    Startup(#[from] startup::StartupError),
    #[error(transparent)]
    Recovery(#[from] RecoveryError),
}

/// `stepclimb admin enrollment-link`: connect, [`startup::prepare`] the database, then
/// [`create_recovery_link`] at the clock's current time.
pub async fn recovery_command(
    config: &Config,
    request: &RecoveryRequest,
) -> Result<Recovered, RecoveryCommandError> {
    startup::log_configuration(config);
    let pool = startup::connect(&config.database_url).await?;
    let clock = Clock::system();
    let result = async {
        startup::prepare(&pool, &clock).await?;
        let now = clock.now();
        Ok(create_recovery_link(&pool, request, &config.public_url, now).await?)
    }
    .await;
    pool.close().await;
    result
}

fn read_error(error: impl std::fmt::Display) -> RecoveryError {
    RecoveryError::Read(error.to_string())
}

/// Make the recovery link `request` asks for, to `public_url`, at `now`, in one transaction
/// (see the module documentation for the refusals).
pub async fn create_recovery_link(
    pool: &PgPool,
    request: &RecoveryRequest,
    public_url: &WebOrigin,
    now: SystemTime,
) -> Result<Recovered, RecoveryError> {
    let mut tx = pool.begin().await.map_err(RecoveryError::Begin)?;
    let org_id = organizations::id_by_slug(&mut *tx, &request.org_slug)
        .await
        .map_err(read_error)?
        .ok_or(RecoveryError::OrganizationNotFound)?;
    let scope = cli_scope(org_id);
    let found = match &request.user {
        UserRef::Id(id) => users::get(&mut *tx, scope, *id).await,
        UserRef::Email(email) => users::find_by_email(&mut *tx, scope, email).await,
    };
    let user = found
        .map_err(read_error)?
        .ok_or(RecoveryError::UserNotFound)?;
    // Until the commit: a removal of an identity or an assignment of this user cannot change
    // what the checks below read.
    identities::lock_user(&mut tx, scope, user.id)
        .await
        .map_err(read_error)?;
    if !request.admin_required_override {
        let grants = role_assignments::for_user_with_paths(&mut *tx, scope, user.id)
            .await
            .map_err(read_error)?;
        let org_admin = grants
            .iter()
            .any(|g| g.role_key == ORG_ADMIN_ROLE && g.scope == GrantScope::Organization);
        if !org_admin {
            return Err(RecoveryError::NotOrgAdmin);
        }
    }
    if !request.force {
        let local = local::has_credential(&mut tx, scope, user.id)
            .await
            .map_err(read_error)?;
        let outside = identities::has_enabled_identity(&mut *tx, scope, user.id)
            .await
            .map_err(read_error)?;
        if local || outside {
            return Err(RecoveryError::HasSignInMethod);
        }
    }
    let link = enrollment::create_link(
        &mut *tx,
        scope,
        user.id,
        Origin::Recovery,
        public_url,
        now,
        Actor::Instance,
    )
    .await
    .map_err(|error| match error {
        LinkError::Store(stepclimb_core::local_tokens::Error::EmailMissing) => {
            RecoveryError::EmailMissing
        }
        other => RecoveryError::Link(other),
    })?;
    tx.commit().await.map_err(RecoveryError::Commit)?;
    Ok(Recovered { org_id, user, link })
}

/// The report the command prints to stdout: who the link is for and until when, then the link.
pub fn report(recovered: &Recovered) -> String {
    let email = recovered.user.email.as_deref().unwrap_or_default();
    format!(
        "stepclimb: created an enrollment link for {email:?} ({user_id}) in organization \
         {org_id}; earlier unused links of the user no longer work\n\
         stepclimb: open this one-time link to set up a passkey or password; it works once and \
         expires at {expires}:\n\
         {url}\n",
        user_id = recovered.user.id,
        org_id = recovered.org_id,
        expires = recovered.link.expires_at_text,
        url = recovered.link.expose_url(),
    )
}

/// The stderr line when the link was committed but the report could not be written.
pub fn report_write_failure(error: &std::io::Error) -> String {
    format!(
        "stepclimb: admin enrollment-link: the enrollment link exists, but it could not be \
         written to stdout: {error}; running the command again replaces it with a new one\n"
    )
}

/// The exit code and the stderr message for a failed run: 2 for a refusal, 1 otherwise.
pub fn failure_report(error: &RecoveryCommandError) -> (u8, String) {
    let prefix = "stepclimb: admin enrollment-link:";
    match error {
        RecoveryCommandError::Startup(error) => {
            (1, format!("{prefix} {error}; no link was made\n"))
        }
        RecoveryCommandError::Recovery(error @ RecoveryError::Commit(_)) => (
            1,
            format!("{prefix} {error}; running the command again replaces any link it made\n"),
        ),
        RecoveryCommandError::Recovery(error) => {
            let code = if error.is_refusal() { 2 } else { 1 };
            (code, format!("{prefix} {error}; no link was made\n"))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_uuid_names_the_user_by_id_and_anything_else_by_email() {
        let id = Uuid::from_u128(42);
        assert_eq!(UserRef::parse(&id.to_string()), UserRef::Id(id));
        assert_eq!(
            UserRef::parse("Sam@Example.test"),
            UserRef::Email("Sam@Example.test".into())
        );
    }

    #[test]
    fn refusals_exit_2_and_failures_exit_1() {
        let refused = RecoveryCommandError::Recovery(RecoveryError::NotOrgAdmin);
        let (code, message) = failure_report(&refused);
        assert_eq!(code, 2);
        assert!(message.contains("--admin-required-override"), "{message}");
        assert!(message.ends_with("no link was made\n"), "{message}");
        let forced = RecoveryCommandError::Recovery(RecoveryError::HasSignInMethod);
        assert!(failure_report(&forced).1.contains("--force"));
        let failed = RecoveryCommandError::Recovery(RecoveryError::Begin(sqlx::Error::PoolClosed));
        assert_eq!(failure_report(&failed).0, 1);
        let commit = RecoveryCommandError::Recovery(RecoveryError::Commit(sqlx::Error::PoolClosed));
        assert!(failure_report(&commit).1.contains("replaces"));
    }

    #[test]
    fn a_failed_write_says_the_link_exists_and_how_to_replace_it() {
        let error = std::io::Error::other("closed");
        let message = report_write_failure(&error);
        assert!(message.contains("the enrollment link exists"), "{message}");
        assert!(
            message.contains("running the command again replaces it"),
            "{message}"
        );
    }
}
