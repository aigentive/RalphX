use std::sync::Arc;

use crate::domain::repositories::IdeationEffortSettingsRepository;

/// Result of running the ideation effort settings bootstrap.
#[derive(Debug, Clone)]
pub struct IdeationEffortBootstrapResult {
    /// Whether the global inherit row was seeded on this run.
    pub seeded_global: bool,
}

/// Seed the global ideation effort settings row if none exists.
///
/// This is idempotent: if a global row already exists (even with non-inherit values),
/// it is left unchanged. This ensures user-configured values survive app restarts.
pub async fn seed_ideation_effort_defaults(
    repo: Arc<dyn IdeationEffortSettingsRepository>,
) -> Result<IdeationEffortBootstrapResult, String> {
    let existing = repo
        .get_by_project_id(None)
        .await
        .map_err(|e| e.to_string())?;

    if existing.is_some() {
        return Ok(IdeationEffortBootstrapResult {
            seeded_global: false,
        });
    }

    // No global row — seed with inherit/inherit to preserve current YAML behavior
    repo.upsert(None, "inherit", "inherit")
        .await
        .map_err(|e| e.to_string())?;

    tracing::info!("ideation_effort_bootstrap: seeded global inherit/inherit row");

    Ok(IdeationEffortBootstrapResult { seeded_global: true })
}

#[cfg(test)]
#[path = "ideation_effort_bootstrap_tests.rs"]
mod tests;
