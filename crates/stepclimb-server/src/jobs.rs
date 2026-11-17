// SPDX-License-Identifier: AGPL-3.0-or-later
//! The job kinds this binary runs (docs/specs/m1/01-job-queue-and-outbox.md B11, B13, B18).
//!
//! [`registry`] is every kind `stepclimb serve` registers with its handler; the worker of an
//! `all` or `worker` process serves every queue in it, and its scheduler runs the periodic ones:
//! `jobs.prune` (hourly), `auth.flows_cleanup` (every 10 minutes), `files.cleanup` (hourly,
//! docs/specs/m1/06-files.md B8, B21) and `videos.cleanup` (hourly,
//! docs/specs/m1/08-uploaded-video.md B6). It also knows the progress event topics
//! (docs/specs/m1/05-progress-and-events.md B10), so the outbox dispatcher claims their events.

use stepclimb_jobs::{Registry, RegistryError, prune};

use crate::{files_cleanup, flow, progress, storage::Storage, videos_cleanup};

/// The registry a `stepclimb serve` worker runs with, its file and video upload cleanups on
/// `storage` (`None` when storage is not configured).
pub fn registry(storage: Option<Storage>) -> Result<Registry, RegistryError> {
    let mut registry = Registry::new();
    prune::register(&mut registry)?;
    flow::register_cleanup(&mut registry)?;
    files_cleanup::register(&mut registry, storage.clone())?;
    videos_cleanup::register(&mut registry, storage)?;
    progress::register_topics(&mut registry)?;
    Ok(registry)
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;

    #[test]
    fn the_registry_runs_the_spec_periodic_jobs() {
        let registry = registry(None).expect("the registry builds");
        assert_eq!(
            registry.periodic(),
            [
                ("auth.flows_cleanup", Duration::from_secs(10 * 60)),
                ("files.cleanup", Duration::from_secs(60 * 60)),
                ("jobs.prune", Duration::from_secs(60 * 60)),
                ("videos.cleanup", Duration::from_secs(60 * 60)),
            ]
        );
        assert!(registry.knows(&prune::PRUNE));
        assert!(registry.knows(&flow::CLEANUP));
        assert!(registry.knows(&files_cleanup::CLEANUP));
        assert!(registry.knows(&videos_cleanup::CLEANUP));
        assert_eq!(registry.queues(), ["default"]);
        for topic in [
            progress::ITEM_COMPLETED,
            progress::MODULE_COMPLETED,
            progress::COURSE_COMPLETED,
        ] {
            assert!(registry.knows_topic(&topic), "{}", topic.name);
        }
    }
}
