// SPDX-License-Identifier: AGPL-3.0-or-later
//! `stepclimb admin create` (spec 02 B27, Q1 option A): create an organization and its first
//! Organization admin from the host's shell, the trust boundary a fresh install already has.
//!
//! The command brings the database to what this binary serves first ([`startup::prepare`]:
//! migrations, the built-in roles, the `unlimited` plan), so the Organization admin role exists
//! even when `stepclimb serve` has never run. It then creates the organization (with its root
//! account and plan row, B26), the user and the user's organization-scope Organization admin
//! assignment in one transaction: when any step fails, none of them is kept.

use std::time::SystemTime;

use sqlx::PgPool;
use stepclimb_core::{
    OrgScope, Source,
    clock::Clock,
    id::Uuid,
    organizations::{self, Organization},
    role_assignments::{self, NewAssignment, Scope},
    roles,
    users::{self, NewUser, User},
};

use crate::{Config, startup};

/// The built-in role the first user gets (spec 09 B4).
pub const ORG_ADMIN_ROLE: &str = "org_admin";

/// The first user's time zone: the default of the organization's default time zone (spec 03 B3,
/// Q4), which is `UTC` until that setting exists.
pub const FIRST_ADMIN_TZ: &str = "UTC";

/// What `stepclimb admin create` is asked to create.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NewOrganizationAdmin {
    /// The organization's slug (spec 02 Q3), fixed after creation.
    pub org_slug: String,
    pub org_name: String,
    /// The admin's email, which local sign-in finds the user by (spec 04 B4).
    pub email: String,
    /// The admin's display name.
    pub name: String,
}

/// What `stepclimb admin create` created.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Created {
    pub organization: Organization,
    pub user: User,
    pub assignment_id: Uuid,
}

/// Why creating the organization and its admin failed. Nothing was written.
#[derive(Debug, thiserror::Error)]
pub enum CreateError {
    #[error(transparent)]
    Organization(organizations::Error),
    #[error(transparent)]
    User(users::Error),
    #[error("could not read the Organization admin role: {0}")]
    Role(#[source] roles::Error),
    /// The built-in row is written by [`startup::prepare`], which runs first; it can be missing
    /// only when something removed it in between.
    #[error("the built-in Organization admin role does not exist")]
    RoleMissing,
    #[error("could not assign the Organization admin role: {0}")]
    Assignment(#[source] role_assignments::Error),
    #[error("the database failed: {0}")]
    Database(#[source] sqlx::Error),
}

impl CreateError {
    /// The command-line flag whose value was refused, or `None` when the failure is not the
    /// input's (the database, or a row the command expected).
    pub fn refused_argument(&self) -> Option<&'static str> {
        use organizations::Error as O;
        use users::Error as U;
        match self {
            Self::Organization(O::SlugInvalid | O::SlugReserved | O::SlugTaken) => {
                Some("--org-slug")
            }
            Self::Organization(O::NameEmpty | O::NameTooLong | O::NameContainsNul) => {
                Some("--org-name")
            }
            Self::User(
                U::DisplayNameEmpty
                | U::ContainsNul {
                    field: "display_name",
                },
            ) => Some("--name"),
            Self::User(
                U::EmailEmpty | U::EmailInvalidCharacter | U::ContainsNul { field: "email" },
            ) => Some("--email"),
            _ => None,
        }
    }
}

/// Why `stepclimb admin create` stopped.
#[derive(Debug, thiserror::Error)]
pub enum CommandError {
    #[error(transparent)]
    Startup(#[from] startup::StartupError),
    #[error(transparent)]
    Create(#[from] CreateError),
}

/// `stepclimb admin create`: connect, [`startup::prepare`] the database, then
/// [`create_organization_with_admin`] at the clock's current time.
pub async fn create_command(
    config: &Config,
    input: &NewOrganizationAdmin,
) -> Result<Created, CommandError> {
    startup::log_configuration(config);
    let pool = startup::connect(&config.database_url).await?;
    let clock = Clock::system();
    let result = async {
        startup::prepare(&pool, &clock).await?;
        Ok(create_organization_with_admin(&pool, input, clock.now()).await?)
    }
    .await;
    pool.close().await;
    result
}

/// Create the organization `input` names, with its root account and plan row
/// ([`organizations::create`]), a `local`, `active` user with `input`'s email and name, and that
/// user's Organization admin assignment at organization scope, all in one transaction stamped
/// `now`. The built-in roles must have been synced ([`roles::sync_builtin`]).
pub async fn create_organization_with_admin(
    pool: &PgPool,
    input: &NewOrganizationAdmin,
    now: SystemTime,
) -> Result<Created, CreateError> {
    let mut tx = pool.begin().await.map_err(CreateError::Database)?;
    let organization = organizations::create(&mut *tx, &input.org_slug, &input.org_name, now)
        .await
        .map_err(CreateError::Organization)?;
    let scope = cli_scope(organization.id);
    let user = users::create(&mut *tx, scope, &first_admin(input), now)
        .await
        .map_err(CreateError::User)?;
    let role_id = roles::builtin_id(&mut *tx, ORG_ADMIN_ROLE)
        .await
        .map_err(CreateError::Role)?
        .ok_or(CreateError::RoleMissing)?;
    let assignment = NewAssignment {
        user_id: user.id,
        role_id,
        scope: Scope::Organization,
        source: Source::Local,
        external_id: None,
    };
    let assignment = role_assignments::create(&mut *tx, scope, &assignment, now)
        .await
        .map_err(CreateError::Assignment)?;
    tx.commit().await.map_err(CreateError::Database)?;
    Ok(Created {
        organization,
        user,
        assignment_id: assignment.id,
    })
}

fn first_admin(input: &NewOrganizationAdmin) -> NewUser {
    NewUser {
        display_name: input.name.clone(),
        given_name: String::new(),
        family_name: String::new(),
        email: Some(input.email.clone()),
        locale: None,
        tz: FIRST_ADMIN_TZ.to_owned(),
        source: Source::Local,
        external_id: None,
    }
}

/// The scope of the organization the command has just created, which no credential can belong
/// to yet.
#[expect(
    clippy::disallowed_methods,
    reason = "spec 02 B9 allows the instance scope in the `stepclimb admin` CLI (B27)"
)]
fn cli_scope(org_id: Uuid) -> OrgScope {
    OrgScope::instance(org_id)
}

/// The report `stepclimb admin create` prints on success. It holds ids, the slug and the email,
/// and no secret.
pub fn report(created: &Created) -> String {
    let email = created.user.email.as_deref().unwrap_or_default();
    format!(
        "stepclimb: created organization {slug:?} ({org_id})\n\
         stepclimb: created user {email:?} ({user_id}) with the Organization admin role at \
         organization scope\n",
        slug = created.organization.slug,
        org_id = created.organization.id,
        user_id = created.user.id,
    )
}
