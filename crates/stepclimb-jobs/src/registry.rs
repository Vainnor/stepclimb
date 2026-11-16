// SPDX-License-Identifier: AGPL-3.0-or-later
//! The handlers this binary runs, by kind (docs/specs/m1/01-job-queue-and-outbox.md B11).

use std::collections::{BTreeSet, HashMap};
use std::fmt;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use std::time::Duration;

use crate::backoff::longest_span;
use crate::kind::{Kind, KindError};
use crate::outbox::{EVENT_RETENTION, Topic, TopicError};
use crate::worker::{JobContext, Outcome};

/// What a handler returns: a future the worker runs on a task of its own.
pub type HandlerFuture = Pin<Box<dyn Future<Output = Outcome> + Send>>;

pub(crate) type Handler = Arc<dyn Fn(JobContext) -> HandlerFuture + Send + Sync>;

/// Why a registration was refused.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum RegistryError {
    #[error(transparent)]
    Kind(#[from] KindError),
    #[error("the job kind `{0}` is registered twice")]
    Duplicate(&'static str),
    #[error(transparent)]
    Topic(#[from] TopicError),
    #[error("the event topic `{0}` is registered twice")]
    DuplicateTopic(&'static str),
    #[error("the event topic `{0}` is not registered with this schema version")]
    UnknownTopic(&'static str),
    #[error(
        "the subscriber kind `{0}` can retry for longer than the 30 days an event is kept after \
         dispatch"
    )]
    RetrySpan(&'static str),
    #[error(
        "the periodic job kind `{0}` needs an interval of whole seconds, at least 10 s and below \
         2^31 s"
    )]
    Interval(&'static str),
}

/// The shortest interval of a periodic kind, as `job_schedules.interval_s` allows (B17).
pub const MIN_INTERVAL: Duration = Duration::from_secs(10);

/// Every kind this binary knows, with its settings and handler. Built once at startup.
#[derive(Clone, Default)]
pub struct Registry {
    entries: HashMap<&'static str, (Kind, Handler)>,
    /// The interval of each periodic kind, by name (B17).
    periodic: HashMap<&'static str, Duration>,
    /// Every event topic this binary emits or subscribes to, by name (B20).
    topics: HashMap<&'static str, Topic>,
    /// The kinds of each topic's subscribers, by topic name, in registration order (B21).
    subscribers: HashMap<&'static str, Vec<Kind>>,
}

impl Registry {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Register `handler` for `kind`. The worker runs each claimed job of `kind` by calling
    /// `handler` with the job's [`JobContext`].
    pub fn register<F, Fut>(&mut self, kind: Kind, handler: F) -> Result<(), RegistryError>
    where
        F: Fn(JobContext) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Outcome> + Send + 'static,
    {
        kind.validate()?;
        if self.entries.contains_key(kind.name) {
            return Err(RegistryError::Duplicate(kind.name));
        }
        let handler: Handler = Arc::new(move |context| Box::pin(handler(context)));
        self.entries.insert(kind.name, (kind, handler));
        Ok(())
    }

    /// Register `handler` for `kind`, and run `kind` every `interval` as a periodic job (B17):
    /// the scheduler enqueues one instance job of it per interval, with no payload.
    pub fn register_periodic<F, Fut>(
        &mut self,
        kind: Kind,
        interval: Duration,
        handler: F,
    ) -> Result<(), RegistryError>
    where
        F: Fn(JobContext) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Outcome> + Send + 'static,
    {
        let whole = interval.subsec_nanos() == 0;
        let fits = i32::try_from(interval.as_secs()).is_ok();
        if !(whole && fits && interval >= MIN_INTERVAL) {
            return Err(RegistryError::Interval(kind.name));
        }
        self.register(kind, handler)?;
        self.periodic.insert(kind.name, interval);
        Ok(())
    }

    /// Register an event topic with its schema version (B20). The dispatcher claims only events
    /// of registered topics, so an event of a topic this binary does not know, a newer
    /// release's, waits for a process that knows it instead of being marked dispatched without
    /// that release's subscribers.
    pub fn register_topic(&mut self, topic: Topic) -> Result<(), RegistryError> {
        topic.validate()?;
        if self.topics.contains_key(topic.name) {
            return Err(RegistryError::DuplicateTopic(topic.name));
        }
        self.topics.insert(topic.name, topic);
        Ok(())
    }

    /// Register `handler` for `kind`, and make `kind` a subscriber of `topic` (B21): the
    /// dispatcher enqueues one job of `kind` for each event of `topic`, with the event's
    /// organization and the payload `{ "event_id": <id> }` ([`crate::outbox::event`] reads it).
    /// `topic` must be registered first, with the same schema version, and `kind`'s
    /// [`longest_span`] must fit within [`EVENT_RETENTION`], so a subscriber's last attempt still
    /// finds its event before `jobs.prune` deletes it.
    pub fn subscribe<F, Fut>(
        &mut self,
        topic: &Topic,
        kind: Kind,
        handler: F,
    ) -> Result<(), RegistryError>
    where
        F: Fn(JobContext) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Outcome> + Send + 'static,
    {
        if !self.knows_topic(topic) {
            return Err(RegistryError::UnknownTopic(topic.name));
        }
        if longest_span(&kind) > EVENT_RETENTION {
            return Err(RegistryError::RetrySpan(kind.name));
        }
        self.register(kind, handler)?;
        self.subscribers.entry(topic.name).or_default().push(kind);
        Ok(())
    }

    /// Whether `topic` is registered with exactly this schema version. Each crate that emits
    /// events has a test that asserts this for every topic constant it passes to
    /// [`crate::outbox::emit`].
    #[must_use]
    pub fn knows_topic(&self, topic: &Topic) -> bool {
        self.topics.get(topic.name) == Some(topic)
    }

    /// Every registered topic with the kinds of its subscribers, sorted by topic name.
    #[must_use]
    pub fn topics(&self) -> Vec<(Topic, &[Kind])> {
        let mut topics: Vec<_> = self
            .topics
            .values()
            .map(|topic| {
                let kinds = self
                    .subscribers
                    .get(topic.name)
                    .map_or(&[][..], Vec::as_slice);
                (*topic, kinds)
            })
            .collect();
        topics.sort_unstable_by_key(|(topic, _)| topic.name);
        topics
    }

    /// Every periodic kind with its interval, sorted by name.
    #[must_use]
    pub fn periodic(&self) -> Vec<(&'static str, Duration)> {
        let mut periodic: Vec<_> = self.periodic.iter().map(|(n, i)| (*n, *i)).collect();
        periodic.sort_unstable();
        periodic
    }

    /// The registered kind named `name`.
    #[must_use]
    pub fn kind(&self, name: &str) -> Option<&Kind> {
        self.entries.get(name).map(|(kind, _)| kind)
    }

    /// Whether `kind` is registered with exactly these settings. Each crate that enqueues jobs
    /// has a test that asserts this for every kind constant it passes to [`crate::enqueue`].
    #[must_use]
    pub fn knows(&self, kind: &Kind) -> bool {
        self.kind(kind.name) == Some(kind)
    }

    /// Every queue a registered kind runs on, sorted: what a worker of the `all` and `worker`
    /// roles serves (B13).
    #[must_use]
    pub fn queues(&self) -> Vec<&'static str> {
        let queues: BTreeSet<_> = self.entries.values().map(|(kind, _)| kind.queue).collect();
        queues.into_iter().collect()
    }

    /// Every registered kind with its lease, for [`crate::queue::claim`].
    pub(crate) fn leases(&self) -> Vec<(&'static str, std::time::Duration)> {
        self.entries
            .values()
            .map(|(kind, _)| (kind.name, kind.lease))
            .collect()
    }

    pub(crate) fn get(&self, name: &str) -> Option<(&Kind, &Handler)> {
        self.entries
            .get(name)
            .map(|(kind, handler)| (kind, handler))
    }
}

impl fmt::Debug for Registry {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut kinds: Vec<_> = self.entries.keys().collect();
        kinds.sort();
        let mut topics: Vec<_> = self.topics.keys().collect();
        topics.sort();
        f.debug_struct("Registry")
            .field("kinds", &kinds)
            .field("topics", &topics)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn done(_: JobContext) -> Outcome {
        Outcome::Done
    }

    const SEND: Kind = Kind::new("email.send", "default");
    const TRANSCODE: Kind = Kind::new("media.transcode", "transcode");

    #[test]
    fn a_registered_kind_is_known_with_its_settings() {
        let mut registry = Registry::new();
        registry.register(SEND, done).unwrap();
        registry.register(TRANSCODE, done).unwrap();
        assert!(registry.knows(&SEND));
        assert!(!registry.knows(&SEND.with_lease(std::time::Duration::from_secs(9))));
        assert!(registry.knows(&TRANSCODE));
        assert_eq!(registry.queues(), ["default", "transcode"]);
        assert!(Registry::new().queues().is_empty());
        assert!(!registry.knows(&Kind::new("email.other", "default")));
    }

    #[test]
    fn a_periodic_kind_is_registered_with_its_interval() {
        let mut registry = Registry::new();
        registry
            .register_periodic(SEND, Duration::from_secs(3600), done)
            .unwrap();
        registry.register(TRANSCODE, done).unwrap();
        assert!(registry.knows(&SEND));
        assert_eq!(
            registry.periodic(),
            [("email.send", Duration::from_secs(3600))]
        );
    }

    #[test]
    fn a_periodic_interval_must_be_whole_seconds_of_at_least_10() {
        for interval in [
            Duration::from_secs(9),
            Duration::from_millis(10_500),
            Duration::from_secs(1 << 31),
        ] {
            let mut registry = Registry::new();
            assert_eq!(
                registry.register_periodic(SEND, interval, done),
                Err(RegistryError::Interval("email.send")),
                "{interval:?}"
            );
            assert!(!registry.knows(&SEND), "nothing registered");
        }
        let mut registry = Registry::new();
        assert!(registry.register_periodic(SEND, MIN_INTERVAL, done).is_ok());
    }

    #[test]
    fn a_kind_is_registered_once() {
        let mut registry = Registry::new();
        registry.register(SEND, done).unwrap();
        assert_eq!(
            registry.register(SEND, done),
            Err(RegistryError::Duplicate("email.send"))
        );
    }

    const HAPPENED: Topic = Topic::new("test.happened", 1);

    #[test]
    fn a_subscriber_is_a_registered_kind_of_a_registered_topic() {
        let mut registry = Registry::new();
        assert_eq!(
            registry.subscribe(&HAPPENED, SEND, done),
            Err(RegistryError::UnknownTopic("test.happened")),
            "the topic is not registered yet"
        );
        assert!(!registry.knows(&SEND), "nothing registered");
        registry.register_topic(HAPPENED).unwrap();
        registry
            .register_topic(Topic::new("test.ignored", 1))
            .unwrap();
        assert_eq!(
            registry.subscribe(&Topic::new("test.happened", 2), SEND, done),
            Err(RegistryError::UnknownTopic("test.happened")),
            "another schema version"
        );
        registry.subscribe(&HAPPENED, SEND, done).unwrap();
        registry.subscribe(&HAPPENED, TRANSCODE, done).unwrap();
        assert!(registry.knows(&SEND));
        assert!(registry.knows_topic(&HAPPENED));
        assert!(!registry.knows_topic(&Topic::new("test.happened", 2)));
        assert_eq!(
            registry.topics(),
            [
                (HAPPENED, &[SEND, TRANSCODE][..]),
                (Topic::new("test.ignored", 1), &[][..]),
            ]
        );
    }

    #[test]
    fn a_topic_is_registered_once_and_valid() {
        let mut registry = Registry::new();
        registry.register_topic(HAPPENED).unwrap();
        assert_eq!(
            registry.register_topic(Topic::new("test.happened", 2)),
            Err(RegistryError::DuplicateTopic("test.happened"))
        );
        assert_eq!(
            registry.register_topic(Topic::new("happened", 1)),
            Err(RegistryError::Topic(TopicError::Name("happened")))
        );
    }

    #[test]
    fn a_kind_subscribes_once() {
        let mut registry = Registry::new();
        registry.register_topic(HAPPENED).unwrap();
        registry.subscribe(&HAPPENED, SEND, done).unwrap();
        assert_eq!(
            registry.subscribe(&HAPPENED, SEND, done),
            Err(RegistryError::Duplicate("email.send"))
        );
        assert_eq!(registry.topics(), [(HAPPENED, &[SEND][..])]);
    }

    #[test]
    fn a_subscriber_must_finish_retrying_within_the_event_retention() {
        let mut registry = Registry::new();
        registry.register_topic(HAPPENED).unwrap();
        // With the default 5 s base, 1 h cap and 5 min lease, m attempts span at most
        // 5,115 s (the first 10 retries) + (m - 11) * 3,600 s + m * 300 s = 3,900 m - 34,485 s.
        // 673 attempts: 2,590,215 s, within 30 days (2,592,000 s); 674: 2,594,115 s, past it.
        let with_attempts = |name, max_attempts| {
            Kind::new(name, "default").with_retry(crate::kind::Retry {
                max_attempts,
                ..crate::kind::Retry::DEFAULT
            })
        };
        let long = with_attempts("test.long", 674);
        assert_eq!(longest_span(&long), Duration::from_secs(2_594_115));
        assert_eq!(
            registry.subscribe(&HAPPENED, long, done),
            Err(RegistryError::RetrySpan("test.long"))
        );
        assert!(!registry.knows(&long), "nothing registered");
        let fits = with_attempts("test.fits", 673);
        assert_eq!(longest_span(&fits), Duration::from_secs(2_590_215));
        registry.subscribe(&HAPPENED, fits, done).unwrap();
    }

    #[test]
    fn an_invalid_kind_is_refused() {
        let mut registry = Registry::new();
        let bad = Kind::new("send", "default");
        assert_eq!(
            registry.register(bad, done),
            Err(RegistryError::Kind(KindError::Name("send")))
        );
    }
}
