// SPDX-License-Identifier: AGPL-3.0-or-later
//! The job kinds this binary runs (docs/specs/m1/01-job-queue-and-outbox.md B11, B13, B18).
//!
//! [`registry`] is every kind `stepclimb serve` registers with its handler; the worker of an
//! `all` or `worker` process serves every queue in it, and its scheduler runs the periodic ones:
//! `jobs.prune` (hourly), `auth.flows_cleanup` (every 10 minutes) and `files.cleanup` (hourly,
//! docs/specs/m1/06-files.md B8, B21).

use stepclimb_jobs::{Registry, RegistryError, prune};

use crate::{files_cleanup, flow, storage::Storage};

/// The registry a `stepclimb serve` worker runs with, its file cleanup on `storage` (`None` when
/// storage is not configured).
pub fn registry(storage: Option<Storage>) -> Result<Registry, RegistryError> {
    let mut registry = Registry::new();
    prune::register(&mut registry)?;
    flow::register_cleanup(&mut registry)?;
    files_cleanup::register(&mut registry, storage)?;
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
            ]
        );
        assert!(registry.knows(&prune::PRUNE));
        assert!(registry.knows(&flow::CLEANUP));
        assert!(registry.knows(&files_cleanup::CLEANUP));
        assert_eq!(registry.queues(), ["default"]);
    }
}
