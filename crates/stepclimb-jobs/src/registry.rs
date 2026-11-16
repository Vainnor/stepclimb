// SPDX-License-Identifier: AGPL-3.0-or-later
//! The handlers this binary runs, by kind (docs/specs/m1/01-job-queue-and-outbox.md B11).

use std::collections::{BTreeSet, HashMap};
use std::fmt;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use crate::kind::{Kind, KindError};
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
}

/// Every kind this binary knows, with its settings and handler. Built once at startup.
#[derive(Clone, Default)]
pub struct Registry {
    entries: HashMap<&'static str, (Kind, Handler)>,
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
        f.debug_struct("Registry").field("kinds", &kinds).finish()
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
    fn a_kind_is_registered_once() {
        let mut registry = Registry::new();
        registry.register(SEND, done).unwrap();
        assert_eq!(
            registry.register(SEND, done),
            Err(RegistryError::Duplicate("email.send"))
        );
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
