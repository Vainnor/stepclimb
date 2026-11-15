// SPDX-License-Identifier: AGPL-3.0-or-later
//! The admission webhook repository (docs/specs/m0/07-admission.md B24, Data, Q2): the one
//! `admission_webhooks` row an organization may have.
//!
//! This module never sees the signing secret in the clear. The server generates it and seals it
//! with the key ring, bound to the row's id, before calling [`save`] or [`rotate_secret`]; the
//! repository stores the sealed bytes and returns them only in [`AdmissionWebhook`], whose
//! `Debug` output leaves them out. The URL is checked by the server against the outbound URL
//! policy before it gets here.
//!
//! Every write records its audit entry in its transaction (spec 09 B22), with the URL and the
//! `enabled` switch, never the secret: `admission_webhook.create`, `admission_webhook.update`
//! (only what changed), `admission_webhook.rotate_secret` (no data) and
//! `admission_webhook.delete`. A [`save`] that changes nothing writes nothing. Every query filters
//! by the scope's organization.

use std::{fmt, time::SystemTime};

use serde_json::{Map, Value, json};
use sqlx::{Acquire, PgConnection, PgExecutor, Postgres};
use uuid::Uuid;

use crate::{
    OrgScope,
    audit::{self, Action, Actor},
    db::{TimeOutOfRange, epoch_micros, from_epoch_micros},
};

/// The stored webhook.
#[derive(Clone, PartialEq, Eq)]
pub struct AdmissionWebhook {
    pub id: Uuid,
    pub url: String,
    pub enabled: bool,
    /// The signing secret as the key ring sealed it. Never log it.
    pub sealed_secret: Vec<u8>,
    pub created_at: SystemTime,
    pub updated_at: SystemTime,
}

impl fmt::Debug for AdmissionWebhook {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AdmissionWebhook")
            .field("id", &self.id)
            .field("enabled", &self.enabled)
            .field("created_at", &self.created_at)
            .field("updated_at", &self.updated_at)
            .finish_non_exhaustive()
    }
}

/// A change to the webhook: `None` leaves a member as it is.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Change<'a> {
    pub url: Option<&'a str>,
    pub enabled: Option<bool>,
}

/// The row [`save`] inserts when the organization has no webhook: a new id the caller
/// generated, and a new secret sealed for that id.
pub struct Fresh<'a> {
    pub id: Uuid,
    pub sealed_secret: &'a [u8],
}

/// What [`save`] did.
#[derive(Debug, PartialEq, Eq)]
pub enum Saved {
    /// The organization had no webhook; this one was created with the [`Fresh`] row's secret.
    Created(AdmissionWebhook),
    /// The stored webhook was changed; its secret is the one it had.
    Updated(AdmissionWebhook),
    /// The change named only stored values, or nothing; nothing was written.
    Unchanged(AdmissionWebhook),
}

/// Why an admission webhook repository call failed.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// The scope's organization has no row.
    #[error("the organization does not exist")]
    OrgNotFound,
    /// The organization has no webhook.
    #[error("the organization has no admission webhook")]
    NotFound,
    /// Creating a webhook needs a URL.
    #[error("a new admission webhook needs a URL")]
    UrlRequired,
    /// The webhook's secret is no longer the one the caller read: another rotation committed
    /// in between.
    #[error("the admission webhook's secret changed since it was read")]
    SecretChanged,
    #[error(transparent)]
    Time(#[from] TimeOutOfRange),
    #[error(transparent)]
    Database(#[from] sqlx::Error),
}

/// An `admission_webhooks` row as the queries select it.
struct Row {
    id: Uuid,
    url: String,
    enabled: bool,
    secret: Vec<u8>,
    created_at_us: i64,
    updated_at_us: i64,
}

impl TryFrom<Row> for AdmissionWebhook {
    type Error = Error;

    fn try_from(row: Row) -> Result<Self, Error> {
        Ok(Self {
            id: row.id,
            url: row.url,
            enabled: row.enabled,
            sealed_secret: row.secret,
            created_at: from_epoch_micros(row.created_at_us)?,
            updated_at: from_epoch_micros(row.updated_at_us)?,
        })
    }
}

/// The scope's organization's webhook, if it has one.
pub async fn get(
    db: impl PgExecutor<'_>,
    scope: OrgScope,
) -> Result<Option<AdmissionWebhook>, Error> {
    let row = sqlx::query_as!(
        Row,
        r#"SELECT id, url, enabled, secret,
                  (extract(epoch FROM created_at) * 1000000)::bigint AS "created_at_us!",
                  (extract(epoch FROM updated_at) * 1000000)::bigint AS "updated_at_us!"
           FROM admission_webhooks
           WHERE org_id = $1"#,
        scope.org_id(),
    )
    .fetch_optional(db)
    .await?;
    row.map(AdmissionWebhook::try_from).transpose()
}

/// [`get`] under the organization row's lock (`FOR NO KEY UPDATE`), which every [`save`] and
/// [`rotate_secret`] takes, so two writes of one organization run one after the other and a
/// webhook is created once.
async fn get_for_update(
    conn: &mut PgConnection,
    scope: OrgScope,
) -> Result<Option<AdmissionWebhook>, Error> {
    sqlx::query_scalar!(
        "SELECT id FROM organizations WHERE id = $1 FOR NO KEY UPDATE",
        scope.org_id(),
    )
    .fetch_optional(&mut *conn)
    .await?
    .ok_or(Error::OrgNotFound)?;
    get(&mut *conn, scope).await
}

/// Apply `change` to the scope's organization's webhook, by `actor` at `now`.
///
/// With no webhook stored, it creates one from `change` (a URL is required:
/// [`Error::UrlRequired`]; `enabled` defaults to `false`, so the receiver can be given the secret
/// before any sign-in calls it) with `fresh`'s id and sealed secret,
/// and writes `admission_webhook.create`. With one stored, `fresh` is unused: a change naming
/// only stored values writes nothing, and any other sets the members it names, moves
/// `updated_at` and writes `admission_webhook.update` with each changed member before and
/// after. Runs in one transaction (a savepoint when `db` is already one).
pub async fn save<'c>(
    db: impl Acquire<'c, Database = Postgres>,
    scope: OrgScope,
    change: Change<'_>,
    fresh: &Fresh<'_>,
    now: SystemTime,
    actor: Actor,
) -> Result<Saved, Error> {
    let now_us = epoch_micros(now)?;
    let mut tx = db.begin().await?;
    let saved = match get_for_update(&mut tx, scope).await? {
        None => {
            let created = create(&mut tx, scope, change, fresh, now_us, actor).await?;
            Saved::Created(created)
        }
        Some(current) => match update(&mut tx, scope, &current, change, now_us, actor).await? {
            Some(updated) => Saved::Updated(updated),
            None => return Ok(Saved::Unchanged(current)),
        },
    };
    tx.commit().await?;
    Ok(saved)
}

async fn create(
    conn: &mut PgConnection,
    scope: OrgScope,
    change: Change<'_>,
    fresh: &Fresh<'_>,
    now_us: i64,
    actor: Actor,
) -> Result<AdmissionWebhook, Error> {
    let url = change.url.ok_or(Error::UrlRequired)?;
    let enabled = change.enabled.unwrap_or(false);
    let row = sqlx::query_as!(
        Row,
        r#"INSERT INTO admission_webhooks (id, org_id, url, secret, enabled, created_at, updated_at)
           VALUES ($1, $2, $3, $4, $5,
                   timestamptz 'epoch' + $6::bigint * interval '1 microsecond',
                   timestamptz 'epoch' + $6::bigint * interval '1 microsecond')
           RETURNING id, url, enabled, secret,
                     (extract(epoch FROM created_at) * 1000000)::bigint AS "created_at_us!",
                     (extract(epoch FROM updated_at) * 1000000)::bigint AS "updated_at_us!""#,
        fresh.id,
        scope.org_id(),
        url,
        fresh.sealed_secret,
        enabled,
        now_us,
    )
    .fetch_one(&mut *conn)
    .await?;
    let data = json!({ "after": { "url": url, "enabled": enabled } });
    let action = Action::AdmissionWebhookCreate;
    audit::record(conn, scope, actor, action, fresh.id, &data, now_us).await?;
    row.try_into()
}

/// `current` with `change` applied, or `None` when that is `current` itself.
async fn update(
    conn: &mut PgConnection,
    scope: OrgScope,
    current: &AdmissionWebhook,
    change: Change<'_>,
    now_us: i64,
    actor: Actor,
) -> Result<Option<AdmissionWebhook>, Error> {
    let url = change.url.unwrap_or(&current.url);
    let enabled = change.enabled.unwrap_or(current.enabled);
    let (mut before, mut after) = (Map::new(), Map::new());
    if url != current.url {
        before.insert("url".into(), Value::from(current.url.as_str()));
        after.insert("url".into(), Value::from(url));
    }
    if enabled != current.enabled {
        before.insert("enabled".into(), Value::from(current.enabled));
        after.insert("enabled".into(), Value::from(enabled));
    }
    if after.is_empty() {
        return Ok(None);
    }
    let row = sqlx::query_as!(
        Row,
        r#"UPDATE admission_webhooks
           SET url = $3, enabled = $4,
               updated_at = timestamptz 'epoch' + $5::bigint * interval '1 microsecond'
           WHERE org_id = $1 AND id = $2
           RETURNING id, url, enabled, secret,
                     (extract(epoch FROM created_at) * 1000000)::bigint AS "created_at_us!",
                     (extract(epoch FROM updated_at) * 1000000)::bigint AS "updated_at_us!""#,
        scope.org_id(),
        current.id,
        url,
        enabled,
        now_us,
    )
    .fetch_one(&mut *conn)
    .await?;
    let data = json!({ "before": before, "after": after });
    let action = Action::AdmissionWebhookUpdate;
    audit::record(conn, scope, actor, action, current.id, &data, now_us).await?;
    row.try_into().map(Some)
}

/// Replace the secret of the scope's webhook `id` with `sealed_secret`, a new secret sealed for
/// `id`, by `actor` at `now`, and write `admission_webhook.rotate_secret`. The old secret is
/// gone once this commits.
///
/// `read_secret` is the sealed secret the caller read with the webhook. Under the organization
/// row's lock, the stored one must still be it, or the call is [`Error::SecretChanged`] and
/// writes nothing: of two rotations that read the same secret, only the first replaces it, so
/// no caller is handed a secret that another rotation has already replaced.
/// [`Error::NotFound`] when the organization has no webhook with that id, as when it was
/// deleted, or deleted and created again, since the caller read it. Runs in one transaction (a
/// savepoint when `db` is already one).
pub async fn rotate_secret<'c>(
    db: impl Acquire<'c, Database = Postgres>,
    scope: OrgScope,
    id: Uuid,
    read_secret: &[u8],
    sealed_secret: &[u8],
    now: SystemTime,
    actor: Actor,
) -> Result<(), Error> {
    let now_us = epoch_micros(now)?;
    let mut tx = db.begin().await?;
    let current = get_for_update(&mut tx, scope)
        .await?
        .filter(|webhook| webhook.id == id)
        .ok_or(Error::NotFound)?;
    if current.sealed_secret != read_secret {
        return Err(Error::SecretChanged);
    }
    sqlx::query!(
        "UPDATE admission_webhooks
         SET secret = $3,
             updated_at = timestamptz 'epoch' + $4::bigint * interval '1 microsecond'
         WHERE org_id = $1 AND id = $2",
        scope.org_id(),
        id,
        sealed_secret,
        now_us,
    )
    .execute(&mut *tx)
    .await?;
    let action = Action::AdmissionWebhookRotateSecret;
    audit::record(&mut tx, scope, actor, action, id, &json!({}), now_us).await?;
    tx.commit().await?;
    Ok(())
}

/// Delete the scope's organization's webhook, by `actor` at `now`, and write
/// `admission_webhook.delete` with its URL and switch. [`Error::NotFound`] when it has none.
/// Runs in one transaction (a savepoint when `db` is already one).
pub async fn delete<'c>(
    db: impl Acquire<'c, Database = Postgres>,
    scope: OrgScope,
    now: SystemTime,
    actor: Actor,
) -> Result<(), Error> {
    let now_us = epoch_micros(now)?;
    let mut tx = db.begin().await?;
    let deleted = sqlx::query!(
        "DELETE FROM admission_webhooks WHERE org_id = $1 RETURNING id, url, enabled",
        scope.org_id(),
    )
    .fetch_optional(&mut *tx)
    .await?
    .ok_or(Error::NotFound)?;
    let data = json!({ "before": { "url": deleted.url, "enabled": deleted.enabled } });
    let action = Action::AdmissionWebhookDelete;
    audit::record(&mut tx, scope, actor, action, deleted.id, &data, now_us).await?;
    tx.commit().await?;
    Ok(())
}
