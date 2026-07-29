use std::sync::Arc;

use crate::domain::repositories::IdeationModelSettingsRepository;

/// Result of running the ideation model settings bootstrap.
#[derive(Debug, Clone)]
pub struct IdeationModelBootstrapResult {
    /// Whether the global inherit row was seeded on this run.
    pub seeded_global: bool,
}

/// Seed the global ideation model settings row if none exists.
///
/// This is idempotent: if a global row already exists (even with non-inherit values),
/// it is left unchanged. This ensures user-configured values survive app restarts.
pub async fn seed_ideation_model_settings(
    repo: Arc<dyn IdeationModelSettingsRepository>,
) -> Result<IdeationModelBootstrapResult, String> {
    let existing = repo
        .get_global()
        .await
        .map_err(|e| e.to_string())?;

    if existing.is_some() {
        return Ok(IdeationModelBootstrapResult {
            seeded_global: false,
        });
    }

    // No global row — seed with inherit/inherit/inherit to preserve current YAML behavior
    repo.upsert_global("inherit", "inherit", "inherit", "inherit")
        .await
        .map_err(|e| e.to_string())?;

    tracing::info!("ideation_model_bootstrap: seeded global inherit/inherit row");

    Ok(IdeationModelBootstrapResult { seeded_global: true })
}

#[cfg(test)]
#[path = "ideation_model_bootstrap_tests.rs"]
mod tests;
