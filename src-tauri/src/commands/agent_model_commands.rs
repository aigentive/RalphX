use serde::{Deserialize, Serialize};
use tauri::State;

use crate::application::AppState;
use crate::domain::agents::{
    AgentHarnessKind, AgentModelDefinition, AgentModelRegistrySnapshot, AgentModelSource,
    LogicalEffort,
};

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentModelResponse {
    pub provider: String,
    pub model_id: String,
    pub label: String,
    pub menu_label: String,
    pub description: Option<String>,
    pub supported_efforts: Vec<String>,
    pub default_effort: String,
    pub source: String,
    pub enabled: bool,
    pub created_at: Option<String>,
    pub updated_at: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpsertCustomAgentModelInput {
    pub provider: String,
    pub model_id: String,
    pub label: String,
    pub menu_label: Option<String>,
    pub description: Option<String>,
    pub supported_efforts: Vec<String>,
    pub default_effort: String,
    pub enabled: bool,
}

fn parse_provider(value: &str) -> Result<AgentHarnessKind, String> {
    value
        .parse::<AgentHarnessKind>()
        .map_err(|err| format!("Invalid provider: {err}"))
}

fn parse_effort(value: &str) -> Result<LogicalEffort, String> {
    value
        .parse::<LogicalEffort>()
        .map_err(|err| format!("Invalid effort: {err}"))
}

fn parse_efforts(values: &[String]) -> Result<Vec<LogicalEffort>, String> {
    values
        .iter()
        .map(|value| parse_effort(value))
        .collect::<Result<Vec<_>, _>>()
}

fn source_label(source: AgentModelSource) -> &'static str {
    match source {
        AgentModelSource::BuiltIn => "built_in",
        AgentModelSource::Custom => "custom",
    }
}

fn to_response(model: AgentModelDefinition) -> AgentModelResponse {
    AgentModelResponse {
        provider: model.provider.to_string(),
        model_id: model.model_id,
        label: model.label,
        menu_label: model.menu_label,
        description: model.description,
        supported_efforts: model
            .supported_efforts
            .into_iter()
            .map(|effort| effort.to_string())
            .collect(),
        default_effort: model.default_effort.to_string(),
        source: source_label(model.source).to_string(),
        enabled: model.enabled,
        created_at: model.created_at.map(|value| value.to_rfc3339()),
        updated_at: model.updated_at.map(|value| value.to_rfc3339()),
    }
}

fn build_custom_model(input: UpsertCustomAgentModelInput) -> Result<AgentModelDefinition, String> {
    let provider = parse_provider(&input.provider)?;
    let model_id = input.model_id.trim();
    if model_id.is_empty() {
        return Err("Model ID is required".to_string());
    }

    let supported_efforts = parse_efforts(&input.supported_efforts)?;
    if supported_efforts.is_empty() {
        return Err("At least one supported effort is required".to_string());
    }

    let default_effort = parse_effort(&input.default_effort)?;
    if !supported_efforts.contains(&default_effort) {
        return Err("Default effort must be included in supported efforts".to_string());
    }

    let label = if input.label.trim().is_empty() {
        model_id.to_string()
    } else {
        input.label
    };
    let menu_label = input
        .menu_label
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| label.clone());

    Ok(AgentModelDefinition::custom(
        provider,
        model_id,
        label,
        menu_label,
        input.description,
        supported_efforts,
        default_effort,
        input.enabled,
    )
    .normalized())
}

pub(crate) async fn load_agent_model_registry(
    app_state: &AppState,
) -> Result<AgentModelRegistrySnapshot, String> {
    let custom_models = app_state
        .agent_model_registry_repo
        .list_custom_models()
        .await
        .map_err(|error| format!("Failed to fetch custom agent models: {error}"))?;
    Ok(AgentModelRegistrySnapshot::merged(custom_models))
}

#[tauri::command]
pub async fn list_agent_models(
    app_state: State<'_, AppState>,
) -> Result<Vec<AgentModelResponse>, String> {
    let snapshot = load_agent_model_registry(&app_state).await?;
    Ok(snapshot.models.into_iter().map(to_response).collect())
}

#[tauri::command]
pub async fn upsert_custom_agent_model(
    input: UpsertCustomAgentModelInput,
    app_state: State<'_, AppState>,
) -> Result<AgentModelResponse, String> {
    let model = build_custom_model(input)?;
    let saved = app_state
        .agent_model_registry_repo
        .upsert_custom_model(&model)
        .await
        .map_err(|error| format!("Failed to save custom agent model: {error}"))?;
    Ok(to_response(saved))
}

#[tauri::command]
pub async fn delete_custom_agent_model(
    provider: String,
    model_id: String,
    app_state: State<'_, AppState>,
) -> Result<bool, String> {
    let provider = parse_provider(&provider)?;
    let model_id = model_id.trim();
    if model_id.is_empty() {
        return Err("Model ID is required".to_string());
    }

    app_state
        .agent_model_registry_repo
        .delete_custom_model(provider, model_id)
        .await
        .map_err(|error| format!("Failed to delete custom agent model: {error}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_custom_model_trims_and_normalizes_defaults() {
        let model = build_custom_model(UpsertCustomAgentModelInput {
            provider: "codex".to_string(),
            model_id: "  gpt-5.6  ".to_string(),
            label: "  ".to_string(),
            menu_label: Some("  ".to_string()),
            description: Some(" future model ".to_string()),
            supported_efforts: vec![
                "high".to_string(),
                "low".to_string(),
                "high".to_string(),
            ],
            default_effort: "low".to_string(),
            enabled: true,
        })
        .expect("custom model should normalize");

        assert_eq!(model.provider, AgentHarnessKind::Codex);
        assert_eq!(model.model_id, "gpt-5.6");
        assert_eq!(model.label, "gpt-5.6");
        assert_eq!(model.menu_label, "gpt-5.6");
        assert_eq!(model.description.as_deref(), Some("future model"));
        assert_eq!(
            model.supported_efforts,
            vec![LogicalEffort::Low, LogicalEffort::High]
        );
        assert_eq!(model.default_effort, LogicalEffort::Low);
    }

    #[test]
    fn build_custom_model_rejects_invalid_inputs() {
        let invalid_provider = build_custom_model(UpsertCustomAgentModelInput {
            provider: "openai".to_string(),
            model_id: "gpt-5.6".to_string(),
            label: "GPT-5.6".to_string(),
            menu_label: None,
            description: None,
            supported_efforts: vec!["medium".to_string()],
            default_effort: "medium".to_string(),
            enabled: true,
        })
        .expect_err("unknown providers should fail");
        assert!(invalid_provider.contains("Invalid provider"));

        let missing_model = build_custom_model(UpsertCustomAgentModelInput {
            provider: "codex".to_string(),
            model_id: "  ".to_string(),
            label: "GPT-5.6".to_string(),
            menu_label: None,
            description: None,
            supported_efforts: vec!["medium".to_string()],
            default_effort: "medium".to_string(),
            enabled: true,
        })
        .expect_err("empty model ids should fail");
        assert_eq!(missing_model, "Model ID is required");

        let missing_effort = build_custom_model(UpsertCustomAgentModelInput {
            provider: "codex".to_string(),
            model_id: "gpt-5.6".to_string(),
            label: "GPT-5.6".to_string(),
            menu_label: None,
            description: None,
            supported_efforts: Vec::new(),
            default_effort: "medium".to_string(),
            enabled: true,
        })
        .expect_err("empty effort sets should fail");
        assert_eq!(missing_effort, "At least one supported effort is required");

        let mismatched_default = build_custom_model(UpsertCustomAgentModelInput {
            provider: "codex".to_string(),
            model_id: "gpt-5.6".to_string(),
            label: "GPT-5.6".to_string(),
            menu_label: None,
            description: None,
            supported_efforts: vec!["low".to_string()],
            default_effort: "medium".to_string(),
            enabled: true,
        })
        .expect_err("default effort must be supported");
        assert_eq!(
            mismatched_default,
            "Default effort must be included in supported efforts"
        );

        let invalid_effort = build_custom_model(UpsertCustomAgentModelInput {
            provider: "codex".to_string(),
            model_id: "gpt-5.6".to_string(),
            label: "GPT-5.6".to_string(),
            menu_label: None,
            description: None,
            supported_efforts: vec!["warp".to_string()],
            default_effort: "warp".to_string(),
            enabled: true,
        })
        .expect_err("unknown effort should fail");
        assert!(invalid_effort.contains("Invalid effort"));
    }

    #[test]
    fn response_serializes_source_and_timestamps() {
        let mut model = AgentModelDefinition::custom(
            AgentHarnessKind::Claude,
            "claude-opus-5",
            "Claude Opus 5",
            "Claude Opus 5",
            Some("Deep reasoning model".to_string()),
            vec![LogicalEffort::High, LogicalEffort::XHigh, LogicalEffort::Max],
            LogicalEffort::Max,
            false,
        );
        let created = chrono::DateTime::parse_from_rfc3339("2026-05-05T10:11:12Z")
            .unwrap()
            .with_timezone(&chrono::Utc);
        let updated = chrono::DateTime::parse_from_rfc3339("2026-05-05T11:12:13Z")
            .unwrap()
            .with_timezone(&chrono::Utc);
        model.created_at = Some(created);
        model.updated_at = Some(updated);

        let response = to_response(model);

        assert_eq!(response.provider, "claude");
        assert_eq!(response.model_id, "claude-opus-5");
        assert_eq!(response.supported_efforts, vec!["high", "xhigh", "max"]);
        assert_eq!(response.default_effort, "max");
        assert_eq!(response.source, "custom");
        assert!(!response.enabled);
        assert_eq!(
            response.created_at.as_deref(),
            Some("2026-05-05T10:11:12+00:00")
        );
        assert_eq!(
            response.updated_at.as_deref(),
            Some("2026-05-05T11:12:13+00:00")
        );
    }

    #[tokio::test]
    async fn load_registry_merges_custom_models_from_state_repo() {
        let state = AppState::new_test();
        let custom = AgentModelDefinition::custom(
            AgentHarnessKind::Codex,
            "gpt-5.6",
            "GPT-5.6",
            "GPT-5.6",
            Some("Future model".to_string()),
            vec![LogicalEffort::Low, LogicalEffort::Medium],
            LogicalEffort::Medium,
            true,
        );
        state
            .agent_model_registry_repo
            .upsert_custom_model(&custom)
            .await
            .expect("custom model should save");

        let snapshot = load_agent_model_registry(&state)
            .await
            .expect("registry should load");

        let model = snapshot
            .find_enabled(AgentHarnessKind::Codex, "gpt-5.6")
            .expect("custom model should merge into snapshot");
        assert_eq!(model.source, AgentModelSource::Custom);
        assert_eq!(model.default_effort, LogicalEffort::Medium);
    }
}
