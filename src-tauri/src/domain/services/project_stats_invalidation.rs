//! Port for evicting cached project statistics when a task changes state.
//!
//! The stats caches themselves are owned by the command layer, but the state
//! machine is what knows a task moved. Rather than have `domain` import
//! `commands`, the domain owns this slot and the command layer registers an
//! implementation during startup.
//!
//! Formalized as a first-class `StatsCacheInvalidator` port in phase 10.

use std::sync::{Arc, OnceLock};

/// Evicts every cached statistic derived from the given project.
pub type ProjectStatsInvalidator = Arc<dyn Fn(&str) + Send + Sync>;

static INVALIDATOR: OnceLock<ProjectStatsInvalidator> = OnceLock::new();

/// Register the process-wide invalidator.
///
/// Called once from the composition root before any transition can run.
/// Subsequent calls are ignored so a late registration cannot swap the
/// implementation out from under an in-flight transition.
pub fn register_project_stats_invalidator(invalidator: ProjectStatsInvalidator) {
    if INVALIDATOR.set(invalidator).is_err() {
        tracing::debug!("project stats invalidator already registered; keeping the first one");
    }
}

/// Evict cached statistics for a project.
///
/// A no-op until an invalidator is registered. That is the correct behaviour
/// for tests and for any build without the command-layer caches: the caches
/// only exist where something registered them.
pub fn invalidate_project_stats(project_id: &str) {
    if let Some(invalidate) = INVALIDATOR.get() {
        invalidate(project_id);
    }
}
