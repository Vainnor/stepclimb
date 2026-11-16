// SPDX-License-Identifier: AGPL-3.0-or-later
//! The transactional outbox (docs/specs/m1/01-job-queue-and-outbox.md B19 to B24).
//!
//! [`emit`] writes an event, one `outbox` row, in the caller's transaction, so the event exists
//! exactly when the change it reports commits. Its payload is the event envelope
//! `{ "type", "timestamp", "data" }` (B20) and its id is the row's id. [`dispatch`] turns each
//! event into one job for every subscriber of its topic ([`crate::Registry::subscribe`]), with
//! the payload `{ "event_id": <id> }`; a subscriber's handler reads the event with [`event`].
//! [`dispatch::run`] is the loop every worker process runs. `jobs.prune` deletes dispatched
//! events 30 days after dispatch ([`prune_dispatched`]).
//!
//! Each event records its topic's schema version, and the dispatcher claims only the topic and
//! version pairs its registry knows: an event of a newer release's topic or version waits for a
//! process that knows it. A topic and version therefore stay registered for as long as events
//! of them can exist, undispatched; [`warn_unknown_topics`] logs the ones that are waiting, at
//! startup and in every `jobs.prune` run.
//!
//! Delivery is at least once and in no guaranteed order (B12, B22): a subscriber's job retries
//! on its own, and two transactions can commit their events out of order. Subscribers use the
//! envelope's `timestamp` and are idempotent per event id.
//!
//! Every query here filters by the event's organization, except the dispatcher's claim, which is
//! instance-wide by design (B26) and says so beside it.

pub mod dispatch;

use std::time::{Duration, SystemTime};

use serde::{Deserialize, Serialize};
use serde_json::Value;
use sqlx::{PgExecutor, Postgres, Transaction};
use stepclimb_core::{
    OrgScope,
    db::{TimeOutOfRange, epoch_micros},
    id::{Uuid, new_id},
};

use crate::registry::Registry;
use crate::worker::{JobContext, Outcome};

pub use dispatch::{BATCH, Dispatched, dispatch, dispatch_in};

/// How long a dispatched event is kept after `dispatched_at` (B23, Q3).
pub const EVENT_RETENTION: Duration = Duration::from_secs(30 * 24 * 60 * 60);

/// An event topic: its name, one of the plan's event names such as `item.completed`, and the
/// schema version of its `data` (B20). Within one version `data` only gains fields.
///
/// Each topic is a `const` beside the code that emits it, and the same constant is passed to
/// [`emit`], to [`crate::Registry::register_topic`] and to [`crate::Registry::subscribe`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Topic {
    /// Two or more dot-separated words of lowercase ASCII letters and `_`, each starting with a
    /// letter, as the `outbox.topic` check constraint allows.
    pub name: &'static str,
    /// 1 to `i32::MAX`, as the `outbox.version` column holds.
    pub version: u32,
}

/// Why a [`Topic`] cannot be emitted or registered.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum TopicError {
    #[error("the event topic `{0}` is not of the form area.event")]
    Name(&'static str),
    #[error("the event topic `{0}` needs a schema version from 1 to 2^31 - 1")]
    Version(&'static str),
}

impl Topic {
    #[must_use]
    pub const fn new(name: &'static str, version: u32) -> Self {
        Self { name, version }
    }

    /// Checks the name and the version.
    pub fn validate(&self) -> Result<(), TopicError> {
        let mut words = self.name.split('.');
        let first = words.next().is_some_and(valid_word);
        let rest: Vec<&str> = words.collect();
        if !(first && !rest.is_empty() && rest.iter().all(|w| valid_word(w))) {
            return Err(TopicError::Name(self.name));
        }
        if self.version == 0 || i32::try_from(self.version).is_err() {
            return Err(TopicError::Version(self.name));
        }
        Ok(())
    }
}

/// One lowercase ASCII letter, then lowercase letters and `_`.
fn valid_word(word: &str) -> bool {
    let mut bytes = word.bytes();
    bytes.next().is_some_and(|b| b.is_ascii_lowercase())
        && bytes.all(|b| b.is_ascii_lowercase() || b == b'_')
}

/// Why an outbox call failed.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error(transparent)]
    Topic(#[from] TopicError),
    #[error("the event data does not serialize")]
    Data(#[source] serde_json::Error),
    /// The event's `data` serialized to something other than a JSON object (B20).
    #[error("the event data is not a JSON object")]
    DataNotObject,
    /// A stored payload that is not JSON, which the column's type rules out.
    #[error("the stored event payload does not parse")]
    StoredPayload(#[source] serde_json::Error),
    #[error(transparent)]
    Queue(#[from] crate::queue::Error),
    #[error(transparent)]
    Time(#[from] TimeOutOfRange),
    #[error(transparent)]
    Database(#[from] sqlx::Error),
}

/// Emit an event of `topic` for `scope`'s organization in `tx`, the caller's transaction (B19),
/// and return its id. `data` must serialize to a JSON object; it carries ids and the facts of
/// the change, never a secret (B20). `now` is the change's clock reading: the envelope's
/// `timestamp` and the row's `created_at`.
pub async fn emit(
    tx: &mut Transaction<'_, Postgres>,
    scope: OrgScope,
    topic: &Topic,
    data: &impl Serialize,
    now: SystemTime,
) -> Result<Uuid, Error> {
    topic.validate()?;
    let data = serde_json::to_value(data).map_err(Error::Data)?;
    if !data.is_object() {
        return Err(Error::DataNotObject);
    }
    // `validate` keeps the version within `i32`.
    let version = i32::try_from(topic.version).map_err(|_| TopicError::Version(topic.name))?;
    let id = new_id();
    // The envelope's `timestamp` is RFC 3339 in UTC with microseconds, as the API writes times.
    sqlx::query!(
        r#"INSERT INTO outbox (id, org_id, topic, version, payload, created_at, updated_at)
           SELECT $1, $2, $3, $6,
                  jsonb_build_object(
                      'type', $3::text,
                      'timestamp', to_char(now.t AT TIME ZONE 'UTC',
                                           'YYYY-MM-DD"T"HH24:MI:SS.US"Z"'),
                      'data', $4::text::jsonb
                  ),
                  now.t, now.t
           FROM (SELECT timestamptz 'epoch' + $5::bigint * interval '1 microsecond' AS t) now"#,
        id,
        scope.org_id(),
        topic.name,
        data.to_string(),
        epoch_micros(now)?,
        version,
    )
    .execute(&mut **tx)
    .await?;
    Ok(id)
}

/// An event as stored.
#[derive(Clone, Debug, PartialEq)]
pub struct Event {
    /// The event id.
    pub id: Uuid,
    pub org_id: Uuid,
    pub topic: String,
    /// The schema version of the topic's `data` this event was written with.
    pub version: u32,
    /// The envelope: `{ "type", "timestamp", "data" }`.
    pub payload: Value,
    /// Whether the dispatcher has enqueued its subscribers' jobs.
    pub dispatched: bool,
}

/// The event `id` of `scope`'s organization, or `None` when there is none: another
/// organization's event, or one already pruned.
pub async fn get(
    db: impl PgExecutor<'_>,
    scope: OrgScope,
    id: Uuid,
) -> Result<Option<Event>, Error> {
    let row = sqlx::query!(
        r#"SELECT id, org_id, topic, version, payload::text AS "payload!",
                  dispatched_at IS NOT NULL AS "dispatched!"
           FROM outbox WHERE id = $1 AND org_id = $2"#,
        id,
        scope.org_id(),
    )
    .fetch_optional(db)
    .await?;
    row.map(|row| {
        Ok(Event {
            id: row.id,
            org_id: row.org_id,
            topic: row.topic,
            // `outbox.version` checks `version >= 1`.
            version: u32::try_from(row.version).unwrap_or(0),
            payload: serde_json::from_str(&row.payload).map_err(Error::StoredPayload)?,
            dispatched: row.dispatched,
        })
    })
    .transpose()
}

/// Undispatched events of one topic and version the registry does not know.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UnknownTopic {
    pub topic: String,
    pub version: i32,
    pub events: i64,
}

/// The undispatched events, across every organization, of each topic and version `registry`
/// does not know, sorted by topic and version. The dispatcher leaves them for a release that
/// knows them; a count that stays means none does.
pub async fn unknown_topics(
    db: impl PgExecutor<'_>,
    registry: &Registry,
) -> Result<Vec<UnknownTopic>, Error> {
    let (names, versions) = known_pairs(registry);
    // Instance-wide by design (B26): an operator's count of waiting events per topic, across
    // every organization. It reads no organization's ids or contents. `outbox_undispatched`
    // covers it.
    let rows = sqlx::query_as!(
        UnknownTopic,
        r#"SELECT o.topic, o.version, count(*) AS "events!"
           FROM outbox o
           WHERE o.dispatched_at IS NULL
             AND NOT EXISTS (
                 SELECT 1 FROM unnest($1::text[], $2::integer[]) AS k (topic, version)
                 WHERE k.topic = o.topic AND k.version = o.version)
           GROUP BY o.topic, o.version
           ORDER BY o.topic, o.version"#,
        &names,
        &versions,
    )
    .fetch_all(db)
    .await?;
    Ok(rows)
}

/// Log at `warn` each topic and version with undispatched events `registry` does not know, with
/// their count ([`unknown_topics`]). Returns how many such topics and versions it found.
pub async fn warn_unknown_topics(
    db: impl PgExecutor<'_>,
    registry: &Registry,
) -> Result<usize, Error> {
    let unknown = unknown_topics(db, registry).await?;
    for row in &unknown {
        tracing::warn!(
            topic = row.topic,
            version = row.version,
            events = row.events,
            "undispatched events of a topic this release does not know wait for one that does"
        );
    }
    Ok(unknown.len())
}

/// The registry's topics as parallel name and version arrays, for `unnest`.
pub(crate) fn known_pairs(registry: &Registry) -> (Vec<String>, Vec<i32>) {
    registry
        .topics()
        .iter()
        // `Topic::validate`, run by `register_topic`, keeps the version within `i32`.
        .map(|(topic, _)| {
            (
                topic.name.to_owned(),
                i32::try_from(topic.version).unwrap_or(0),
            )
        })
        .unzip()
}

/// The payload of a subscriber's job (B21).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct EventJob {
    pub event_id: Uuid,
}

/// The event a subscriber's job names, read with the job's organization scope. A payload that
/// is not an [`EventJob`], an instance job and an event that no longer exists are a fail
/// outcome (B6); a database error is a retry.
pub async fn event(ctx: &JobContext) -> Result<Event, Outcome> {
    let Some(scope) = ctx.scope() else {
        return Err(Outcome::fail("an event job runs for an organization"));
    };
    let Ok(job) = EventJob::deserialize(ctx.payload()) else {
        return Err(Outcome::fail("the job's payload does not name an event"));
    };
    match get(ctx.pool(), scope, job.event_id).await {
        Ok(Some(event)) => Ok(event),
        Ok(None) => Err(Outcome::fail("the job's event does not exist")),
        Err(error) => Err(Outcome::retry(format_args!(
            "could not read the job's event: {error}"
        ))),
    }
}

/// Delete up to `limit` events of `scope`'s organization dispatched at or before `cutoff` (B23:
/// `jobs.prune`). An undispatched event is never deleted. Returns how many it deleted; fewer
/// than `limit` means none is left.
pub async fn prune_dispatched(
    db: impl PgExecutor<'_>,
    scope: OrgScope,
    cutoff: SystemTime,
    limit: u32,
) -> Result<u64, Error> {
    // `outbox_dispatched` serves the inner select: `org_id = $1`, `dispatched_at <= $2`.
    let done = sqlx::query!(
        "DELETE FROM outbox
         WHERE id IN (
             SELECT id FROM outbox
             WHERE org_id = $1 AND dispatched_at IS NOT NULL
               AND dispatched_at <= timestamptz 'epoch' + $2::bigint * interval '1 microsecond'
             LIMIT $3
         )",
        scope.org_id(),
        epoch_micros(cutoff)?,
        i64::from(limit),
    )
    .execute(db)
    .await?;
    Ok(done.rows_affected())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn topics_of_two_or_more_words_are_valid() {
        for name in [
            "item.completed",
            "activity.session.completed",
            "a.b",
            "user_x.y_z",
        ] {
            assert_eq!(Topic::new(name, 1).validate(), Ok(()), "{name}");
        }
    }

    #[test]
    fn other_topic_names_are_refused() {
        for name in [
            "item",
            "item.",
            ".completed",
            "Item.completed",
            "item.completed2",
            "item.2completed",
            "item..completed",
            "item.com-pleted",
            "_item.completed",
            "",
        ] {
            assert_eq!(
                Topic::new(name, 1).validate(),
                Err(TopicError::Name(name)),
                "{name}"
            );
        }
    }

    #[test]
    fn a_topic_needs_a_version_the_column_holds() {
        for version in [0, 1 << 31, u32::MAX] {
            assert_eq!(
                Topic::new("item.completed", version).validate(),
                Err(TopicError::Version("item.completed")),
                "{version}"
            );
        }
        assert_eq!(Topic::new("a.b", i32::MAX as u32).validate(), Ok(()));
    }

    #[test]
    fn the_retention_is_30_days() {
        assert_eq!(EVENT_RETENTION, Duration::from_secs(2_592_000));
    }
}
