use async_trait::async_trait;

use crate::ideation::IdeationModelSettings;

/// Repository for ideation model settings.
///
/// Follows the `ideation_effort_settings` pattern:
/// - global row: `project_id IS NULL` in DB
/// - per-project override row: `project_id = Some(s)`
#[async_trait]
pub trait IdeationModelSettingsRepository: Send + Sync {
    /// Fetch the global settings row (project_id IS NULL).
    /// Returns `Ok(None)` if no global row exists.
    async fn get_global(&self)
        -> Result<Option<IdeationModelSettings>, Box<dyn std::error::Error>>;

    /// Fetch the per-project override row for the given project_id.
    /// Returns `Ok(None)` if no row exists for that project_id.
    async fn get_for_project(
        &self,
        project_id: &str,
    ) -> Result<Option<IdeationModelSettings>, Box<dyn std::error::Error>>;

    /// Upsert (insert-or-replace) the primary_model, verifier_model, verifier_subagent_model,
    /// and ideation_subagent_model for the global row.
    /// Returns the resulting row after the upsert.
    async fn upsert_global(
        &self,
        primary_model: &str,
        verifier_model: &str,
        verifier_subagent_model: &str,
        ideation_subagent_model: &str,
    ) -> Result<IdeationModelSettings, Box<dyn std::error::Error>>;

    /// Upsert (insert-or-replace) the primary_model, verifier_model, verifier_subagent_model,
    /// and ideation_subagent_model for a project override row.
    /// Returns the resulting row after the upsert.
    async fn upsert_for_project(
        &self,
        project_id: &str,
        primary_model: &str,
        verifier_model: &str,
        verifier_subagent_model: &str,
        ideation_subagent_model: &str,
    ) -> Result<IdeationModelSettings, Box<dyn std::error::Error>>;
}
