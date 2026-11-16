// SPDX-License-Identifier: AGPL-3.0-or-later
//! The job kinds this binary runs (docs/specs/m1/01-job-queue-and-outbox.md B11, B13, B18).
//!
//! [`registry`] is every kind `stepclimb serve` registers with its handler; the worker of an
//! `all` or `worker` process serves every queue in it, and its scheduler runs the periodic ones:
//! `jobs.prune` (hourly) and `auth.flows_cleanup` (every 10 minutes).

use stepclimb_jobs::{Registry, RegistryError, prune};

use crate::flow;

/// The registry a `stepclimb serve` worker runs with.
pub fn registry() -> Result<Registry, RegistryError> {
    let mut registry = Registry::new();
    prune::register(&mut registry)?;
    flow::register_cleanup(&mut registry)?;
    Ok(registry)
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;

    #[test]
    fn the_registry_runs_the_spec_periodic_jobs() {
        let registry = registry().expect("the registry builds");
        assert_eq!(
            registry.periodic(),
            [
                ("auth.flows_cleanup", Duration::from_secs(10 * 60)),
                ("jobs.prune", Duration::from_secs(60 * 60)),
            ]
        );
        assert!(registry.knows(&prune::PRUNE));
        assert!(registry.knows(&flow::CLEANUP));
        assert_eq!(registry.queues(), ["default"]);
    }
}
