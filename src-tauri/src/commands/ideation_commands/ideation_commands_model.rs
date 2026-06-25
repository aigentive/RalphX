// Tauri commands for getting and updating ideation model settings.
//
// Resolution chain (4 levels, first non-inherit wins):
//   1. Per-project DB row (if project_id is Some and row exists and value != inherit)
//   2. Global DB row (if row exists and value != inherit)
//   3. YAML agent-specific model (if AgentConfig.model is Some)
//   4. Shared Claude model resolver default ("sonnet")

use serde::{Deserialize, Serialize};
use std::str::FromStr;
use tauri::State;

use crate::application::AppState;
use crate::domain::ideation::ModelLevel;
use crate::infrastructure::agents::claude::{
    resolve_ideation_subagent_model_with_source,
    resolve_model_with_source as resolve_claude_model_with_source,
    resolve_verifier_subagent_model_with_source,
};

// Representative agents for each bucket — used to resolve YAML model values.
const PRIMARY_REPR_AGENT: &str = "ralphx-ideation";
const VERIFIER_REPR_AGENT: &str = "ralphx-plan-verifier";
// Same as ralphx-plan-verifier, separate const for future flexibility.
const VERIFIER_SUBAGENT_REPR_AGENT: &str = VERIFIER_REPR_AGENT;

// ============================================================================
// Response type
// ============================================================================

/// Response returned by both get and update commands.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct IdeationModelResponse {
    /// Stored value for the primary bucket (may be "inherit").
    pub primary_model: String,
    /// Stored value for the verifier bucket (may be "inherit").
    pub verifier_model: String,
    /// Resolved effective model for the primary bucket (never "inherit").
    pub effective_primary_model: String,
    /// Resolved effective model for the verifier bucket (never "inherit").
    pub effective_verifier_model: String,
    /// Source label for how effective_primary_model was determined.
    /// One of: "user" | "global" | "yaml" | "yaml_default"
    pub primary_model_source: String,
    /// Source label for how effective_verifier_model was determined.
    /// One of: "user" | "global" | "yaml" | "yaml_default"
    pub verifier_model_source: String,
    /// Stored value for the verifier subagent bucket (may be "inherit").
    pub verifier_subagent_model: String,
    /// Resolved effective model for the verifier subagent bucket (never "inherit").
    pub effective_verifier_subagent_model: String,
    /// Source label for how effective_verifier_subagent_model was determined.
    /// One of: "user" | "global" | "default"
    pub verifier_subagent_model_source: String,
    /// Stored value for the ideation subagent bucket (may be "inherit").
    pub ideation_subagent_model: String,
    /// Resolved effective model for the ideation subagent bucket (never "inherit").
    pub effective_ideation_subagent_model: String,
    /// Source label for how effective_ideation_subagent_model was determined.
    /// One of: "user" | "global" | "default"
    pub ideation_subagent_model_source: String,
}

// ============================================================================
// Input type
// ============================================================================

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateIdeationModelInput {
    /// None = update global row; Some = update per-project row.
    pub project_id: Option<String>,
    /// New primary model level. Only updated if provided.
    pub primary_model: Option<String>,
    /// New verifier model level. Only updated if provided.
    pub verifier_model: Option<String>,
    /// New verifier subagent model level. Only updated if provided.
    pub verifier_subagent_model: Option<String>,
    /// New ideation subagent model level. Only updated if provided.
    pub ideation_subagent_model: Option<String>,
}

// ============================================================================
// Resolution helpers
// ============================================================================

/// Resolve the effective model string and its source label for one bucket.
///
/// The 4-level chain:
///   1. project_value (if Some and != inherit) → source "user"
///   2. global_value  (if Some and != inherit) → source "global"
///   3. YAML agent-specific model              → source "yaml"
///   4. Hardcoded default ("sonnet")            → source "yaml_default"
fn resolve_bucket_model_with_source(
    project_value: Option<&ModelLevel>,
    global_value: Option<&ModelLevel>,
    repr_agent: &str,
) -> (String, String) {
    // Level 1 — per-project row
    if let Some(level) = project_value {
        if *level != ModelLevel::Inherit {
            return (level.to_string(), "user".to_string());
        }
    }

    // Level 2 — global row
    if let Some(level) = global_value {
        if *level != ModelLevel::Inherit {
            return (level.to_string(), "global".to_string());
        }
    }

    let (model, source) = resolve_claude_model_with_source(Some(repr_agent));
    let source = if source == "default" {
        "yaml_default".to_string()
    } else {
        source
    };
    (model, source)
}

// ============================================================================
// Commands
// ============================================================================

/// Get ideation model settings for a project (or global if project_id is None).
///
/// Returns both the stored values and the resolved effective values with source labels.
///
/// # Errors
///
/// Returns a string error if the repository lookup fails.
#[tauri::command]
pub async fn get_ideation_model_settings(
    project_id: Option<String>,
    app_state: State<'_, AppState>,
) -> Result<IdeationModelResponse, String> {
    // Fetch per-project row (if project_id is Some)
    let project_row = if let Some(ref pid) = project_id {
        app_state
            .ideation_model_settings_repo
            .get_for_project(pid)
            .await
            .map_err(|e| format!("Failed to fetch per-project model settings: {e}"))?
    } else {
        None
    };

    // Fetch global row
    let global_row = app_state
        .ideation_model_settings_repo
        .get_global()
        .await
        .map_err(|e| format!("Failed to fetch global model settings: {e}"))?;

    // Stored values come from the project row when present; otherwise global row; otherwise inherit.
    let stored_primary = project_row
        .as_ref()
        .map(|r| r.primary_model.to_string())
        .or_else(|| global_row.as_ref().map(|r| r.primary_model.to_string()))
        .unwrap_or_else(|| ModelLevel::Inherit.to_string());

    let stored_verifier = project_row
        .as_ref()
        .map(|r| r.verifier_model.to_string())
        .or_else(|| global_row.as_ref().map(|r| r.verifier_model.to_string()))
        .unwrap_or_else(|| ModelLevel::Inherit.to_string());

    let stored_verifier_subagent = project_row
        .as_ref()
        .map(|r| r.verifier_subagent_model.to_string())
        .or_else(|| {
            global_row
                .as_ref()
                .map(|r| r.verifier_subagent_model.to_string())
        })
        .unwrap_or_else(|| ModelLevel::Inherit.to_string());

    let stored_ideation_subagent = project_row
        .as_ref()
        .map(|r| r.ideation_subagent_model.to_string())
        .or_else(|| {
            global_row
                .as_ref()
                .map(|r| r.ideation_subagent_model.to_string())
        })
        .unwrap_or_else(|| ModelLevel::Inherit.to_string());

    // Resolve effective values
    let project_primary = project_row.as_ref().map(|r| &r.primary_model);
    let project_verifier = project_row.as_ref().map(|r| &r.verifier_model);
    let project_verifier_subagent = project_row.as_ref().map(|r| &r.verifier_subagent_model);
    let project_ideation_subagent = project_row.as_ref().map(|r| &r.ideation_subagent_model);
    let global_primary = global_row.as_ref().map(|r| &r.primary_model);
    let global_verifier = global_row.as_ref().map(|r| &r.verifier_model);
    let global_verifier_subagent = global_row.as_ref().map(|r| &r.verifier_subagent_model);
    let global_ideation_subagent = global_row.as_ref().map(|r| &r.ideation_subagent_model);

    let (effective_primary, primary_source) =
        resolve_bucket_model_with_source(project_primary, global_primary, PRIMARY_REPR_AGENT);
    let (effective_verifier, verifier_source) =
        resolve_bucket_model_with_source(project_verifier, global_verifier, VERIFIER_REPR_AGENT);
    let (effective_verifier_subagent, verifier_subagent_source) =
        resolve_verifier_subagent_model_with_source(
            project_verifier_subagent,
            global_verifier_subagent,
        );
    let (effective_ideation_subagent, ideation_subagent_source) =
        resolve_ideation_subagent_model_with_source(
            project_ideation_subagent,
            global_ideation_subagent,
        );
    // VERIFIER_SUBAGENT_REPR_AGENT is reserved for future YAML-level resolution.
    let _ = VERIFIER_SUBAGENT_REPR_AGENT;

    Ok(IdeationModelResponse {
        primary_model: stored_primary,
        verifier_model: stored_verifier,
        effective_primary_model: effective_primary,
        effective_verifier_model: effective_verifier,
        primary_model_source: primary_source,
        verifier_model_source: verifier_source,
        verifier_subagent_model: stored_verifier_subagent,
        effective_verifier_subagent_model: effective_verifier_subagent,
        verifier_subagent_model_source: verifier_subagent_source,
        ideation_subagent_model: stored_ideation_subagent,
        effective_ideation_subagent_model: effective_ideation_subagent,
        ideation_subagent_model_source: ideation_subagent_source,
    })
}

/// Update ideation model settings for a project (or global if project_id is None).
///
/// Only provided fields are updated; omitted fields fall back to the current stored value
/// (or "inherit" if no row exists yet).
///
/// # Errors
///
/// Returns a string error if any provided model value is invalid or the upsert fails.
#[tauri::command]
pub async fn update_ideation_model_settings(
    input: UpdateIdeationModelInput,
    app_state: State<'_, AppState>,
) -> Result<IdeationModelResponse, String> {
    // Validate any provided values upfront.
    if let Some(ref v) = input.primary_model {
        ModelLevel::from_str(v).map_err(|e| format!("Invalid primaryModel: {e}"))?;
    }
    if let Some(ref v) = input.verifier_model {
        ModelLevel::from_str(v).map_err(|e| format!("Invalid verifierModel: {e}"))?;
    }
    if let Some(ref v) = input.verifier_subagent_model {
        ModelLevel::from_str(v).map_err(|e| format!("Invalid verifierSubagentModel: {e}"))?;
    }
    if let Some(ref v) = input.ideation_subagent_model {
        ModelLevel::from_str(v).map_err(|e| format!("Invalid ideationSubagentModel: {e}"))?;
    }

    // Fetch the existing row so we can merge (keep old values for unspecified fields).
    let existing = if let Some(ref pid) = input.project_id {
        app_state
            .ideation_model_settings_repo
            .get_for_project(pid)
            .await
            .map_err(|e| format!("Failed to fetch current model settings: {e}"))?
    } else {
        app_state
            .ideation_model_settings_repo
            .get_global()
            .await
            .map_err(|e| format!("Failed to fetch current model settings: {e}"))?
    };

    let current_primary = existing
        .as_ref()
        .map(|r| r.primary_model.to_string())
        .unwrap_or_else(|| ModelLevel::Inherit.to_string());

    let current_verifier = existing
        .as_ref()
        .map(|r| r.verifier_model.to_string())
        .unwrap_or_else(|| ModelLevel::Inherit.to_string());

    let current_verifier_subagent = existing
        .as_ref()
        .map(|r| r.verifier_subagent_model.to_string())
        .unwrap_or_else(|| ModelLevel::Inherit.to_string());

    let current_ideation_subagent = existing
        .as_ref()
        .map(|r| r.ideation_subagent_model.to_string())
        .unwrap_or_else(|| ModelLevel::Inherit.to_string());

    let new_primary = input.primary_model.as_deref().unwrap_or(&current_primary);
    let new_verifier = input.verifier_model.as_deref().unwrap_or(&current_verifier);
    let new_verifier_subagent = input
        .verifier_subagent_model
        .as_deref()
        .unwrap_or(&current_verifier_subagent);
    let new_ideation_subagent = input
        .ideation_subagent_model
        .as_deref()
        .unwrap_or(&current_ideation_subagent);

    // Upsert the row.
    if let Some(ref pid) = input.project_id {
        app_state
            .ideation_model_settings_repo
            .upsert_for_project(
                pid,
                new_primary,
                new_verifier,
                new_verifier_subagent,
                new_ideation_subagent,
            )
            .await
            .map_err(|e| format!("Failed to save model settings: {e}"))?;
    } else {
        app_state
            .ideation_model_settings_repo
            .upsert_global(
                new_primary,
                new_verifier,
                new_verifier_subagent,
                new_ideation_subagent,
            )
            .await
            .map_err(|e| format!("Failed to save model settings: {e}"))?;
    }

    // Return freshly-resolved values by delegating to the get command logic.
    get_ideation_model_settings(input.project_id, app_state).await
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::ideation::{IdeationModelSettings, ModelBucket};
    use chrono::Utc;

    fn make_settings(primary: ModelLevel, verifier: ModelLevel) -> IdeationModelSettings {
        IdeationModelSettings {
            id: 1,
            project_id: None,
            primary_model: primary,
            verifier_model: verifier,
            verifier_subagent_model: ModelLevel::Inherit,
            ideation_subagent_model: ModelLevel::Inherit,
            updated_at: Utc::now(),
        }
    }

    #[test]
    fn resolve_model_project_row_wins() {
        let project = make_settings(ModelLevel::Sonnet, ModelLevel::Opus);
        let global = make_settings(ModelLevel::Haiku, ModelLevel::Haiku);

        let (eff, src) = resolve_bucket_model_with_source(
            Some(&project.primary_model),
            Some(&global.primary_model),
            PRIMARY_REPR_AGENT,
        );
        assert_eq!(eff, "sonnet");
        assert_eq!(src, "user");
    }

    #[test]
    fn resolve_model_falls_through_to_global() {
        let project = make_settings(ModelLevel::Inherit, ModelLevel::Inherit);
        let global = make_settings(ModelLevel::Opus, ModelLevel::Sonnet);

        let (eff, src) = resolve_bucket_model_with_source(
            Some(&project.primary_model),
            Some(&global.primary_model),
            PRIMARY_REPR_AGENT,
        );
        assert_eq!(eff, "opus");
        assert_eq!(src, "global");
    }

    #[test]
    fn resolve_model_no_rows_uses_yaml_fallback() {
        // Both None → should reach yaml or yaml_default level.
        let (_, src) = resolve_bucket_model_with_source(None, None, PRIMARY_REPR_AGENT);
        assert!(
            src == "yaml" || src == "yaml_default",
            "Expected yaml or yaml_default, got: {src}"
        );
    }

    #[test]
    fn resolve_model_inherit_only_falls_through() {
        let settings = make_settings(ModelLevel::Inherit, ModelLevel::Inherit);

        // Inherit-only project row, no global row → yaml fallback
        let (_, src) = resolve_bucket_model_with_source(
            Some(&settings.primary_model),
            None,
            PRIMARY_REPR_AGENT,
        );
        assert!(
            src == "yaml" || src == "yaml_default",
            "Expected yaml/yaml_default after inherit, got: {src}"
        );
    }

    #[test]
    fn model_bucket_for_primary_agent() {
        use crate::domain::ideation::model_settings::model_bucket_for_agent;
        assert_eq!(
            model_bucket_for_agent("ralphx-ideation"),
            Some(ModelBucket::Primary)
        );
    }

    #[test]
    fn model_bucket_for_verifier_agent() {
        use crate::domain::ideation::model_settings::model_bucket_for_agent;
        assert_eq!(
            model_bucket_for_agent("ralphx-plan-verifier"),
            Some(ModelBucket::Verifier)
        );
    }

    #[test]
    fn model_bucket_for_non_ideation_agent() {
        use crate::domain::ideation::model_settings::model_bucket_for_agent;
        assert_eq!(model_bucket_for_agent("ralphx-execution-worker"), None);
    }

    #[test]
    fn model_level_roundtrip() {
        for (s, lvl) in [
            ("inherit", ModelLevel::Inherit),
            ("sonnet", ModelLevel::Sonnet),
            ("opus", ModelLevel::Opus),
            ("haiku", ModelLevel::Haiku),
            ("fable", ModelLevel::Fable),
        ] {
            assert_eq!(ModelLevel::from_str(s).unwrap(), lvl);
            assert_eq!(lvl.to_string(), s);
        }
    }
}
