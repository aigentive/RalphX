// Diagnostic commands — agent health and harness availability inspection

use serde::{Deserialize, Serialize};
use tauri::State;

use crate::application::harness_runtime_registry::{
    probe_codex_harness_with_capabilities, HarnessRuntimeProbe,
};
use crate::application::AppState;
use crate::infrastructure::agents::CodexCliCapabilities;

const MAX_FRONTEND_ERROR_MESSAGE_LENGTH: usize = 4_096;
const MAX_FRONTEND_COMPONENT_STACK_LENGTH: usize = 16_384;
const MAX_FRONTEND_ERROR_SOURCE_LENGTH: usize = 256;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FrontendErrorLogInput {
    pub message: String,
    pub component_stack: Option<String>,
    pub source: Option<String>,
}

#[doc(hidden)]
pub(crate) fn truncate_frontend_error_field(value: &str, max_length: usize) -> String {
    value.chars().take(max_length).collect()
}

/// Persist a frontend error through the application's existing tracing pipeline.
#[tauri::command]
pub fn log_frontend_error(input: FrontendErrorLogInput) {
    let message = truncate_frontend_error_field(&input.message, MAX_FRONTEND_ERROR_MESSAGE_LENGTH);
    let component_stack = input
        .component_stack
        .as_deref()
        .map(|value| truncate_frontend_error_field(value, MAX_FRONTEND_COMPONENT_STACK_LENGTH));
    let source = input
        .source
        .as_deref()
        .map(|value| truncate_frontend_error_field(value, MAX_FRONTEND_ERROR_SOURCE_LENGTH));

    tracing::error!(%message, component_stack = ?component_stack, source = ?source, "frontend_error");
}

#[derive(Debug, Clone)]
pub struct CodexCliProbeStatus {
    pub binary_path: Option<String>,
    pub binary_found: bool,
    pub probe_succeeded: bool,
    pub available: bool,
    pub missing_core_exec_features: Vec<String>,
    pub error: Option<String>,
}

impl From<HarnessRuntimeProbe> for CodexCliProbeStatus {
    fn from(value: HarnessRuntimeProbe) -> Self {
        Self {
            binary_path: value.binary_path,
            binary_found: value.binary_found,
            probe_succeeded: value.probe_succeeded,
            available: value.available,
            missing_core_exec_features: value.missing_core_exec_features,
            error: value.error,
        }
    }
}

/// Serializable IPR entry for agent health report
#[derive(Debug, Clone, Serialize)]
pub struct IprEntryResponse {
    pub context_type: String,
    pub context_id: String,
}

/// Serializable running agent entry for agent health report
#[derive(Debug, Clone, Serialize)]
pub struct RunningAgentResponse {
    pub context_type: String,
    pub context_id: String,
    pub pid: u32,
    pub conversation_id: String,
    pub agent_run_id: String,
    pub started_at: String,
    pub worktree_path: Option<String>,
    pub last_active_at: Option<String>,
}

/// Full agent health report returned by get_agent_health
#[derive(Debug, Clone, Serialize)]
pub struct AgentHealthReport {
    /// Interactive process registry entries (open stdin handles)
    pub ipr_entries: Vec<IprEntryResponse>,
    /// All agents currently tracked in the running agent registry
    pub running_agents: Vec<RunningAgentResponse>,
}

/// Codex CLI diagnostics for backend availability and feature support.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CodexCliDiagnosticsResponse {
    pub binary_path: Option<String>,
    pub binary_found: bool,
    pub probe_succeeded: bool,
    pub version: Option<String>,
    pub has_core_exec_support: bool,
    pub missing_core_exec_features: Vec<String>,
    pub supports_search_flag: bool,
    pub supports_resume_subcommand: bool,
    pub supports_mcp_subcommand: bool,
    pub supports_fast_mode: bool,
    pub fast_mode_supported_models: Vec<String>,
    pub error: Option<String>,
}

/// Get agent health — IPR entries + running agents for runtime inspection.
///
/// # Errors
/// Returns an error string if registry access fails.
#[tauri::command]
pub async fn get_agent_health(state: State<'_, AppState>) -> Result<AgentHealthReport, String> {
    let ipr_keys = state.interactive_process_registry.dump_state().await;
    let ipr_entries = ipr_keys
        .into_iter()
        .map(|k| IprEntryResponse {
            context_type: k.context_type,
            context_id: k.context_id,
        })
        .collect();

    let all_agents = state.running_agent_registry.list_all().await;
    let running_agents = all_agents
        .into_iter()
        .map(|(key, info)| RunningAgentResponse {
            context_type: key.context_type,
            context_id: key.context_id,
            pid: info.pid,
            conversation_id: info.conversation_id,
            agent_run_id: info.agent_run_id,
            started_at: info.started_at.to_rfc3339(),
            worktree_path: info.worktree_path,
            last_active_at: info.last_active_at.map(|t| t.to_rfc3339()),
        })
        .collect();

    Ok(AgentHealthReport {
        ipr_entries,
        running_agents,
    })
}

#[doc(hidden)]
pub fn build_codex_cli_diagnostics_response(
    probe: CodexCliProbeStatus,
    capabilities: Option<CodexCliCapabilities>,
) -> CodexCliDiagnosticsResponse {
    match capabilities {
        Some(capabilities) => CodexCliDiagnosticsResponse {
            binary_path: probe.binary_path,
            binary_found: probe.binary_found,
            probe_succeeded: probe.probe_succeeded,
            version: capabilities.version.clone(),
            has_core_exec_support: capabilities.has_core_exec_support(),
            missing_core_exec_features: capabilities
                .missing_core_exec_features()
                .into_iter()
                .map(str::to_string)
                .collect(),
            supports_search_flag: capabilities.supports_search_flag,
            supports_resume_subcommand: capabilities.supports_resume_subcommand,
            supports_mcp_subcommand: capabilities.supports_mcp_subcommand,
            supports_fast_mode: capabilities.supports_fast_mode(),
            fast_mode_supported_models: capabilities.fast_mode_supported_models(),
            error: probe.error,
        },
        None => CodexCliDiagnosticsResponse {
            binary_path: probe.binary_path,
            binary_found: probe.binary_found,
            probe_succeeded: probe.probe_succeeded,
            version: None,
            has_core_exec_support: probe.available,
            missing_core_exec_features: probe.missing_core_exec_features,
            supports_search_flag: false,
            supports_resume_subcommand: false,
            supports_mcp_subcommand: false,
            supports_fast_mode: false,
            fast_mode_supported_models: Vec::new(),
            error: probe.error,
        },
    }
}

/// Get Codex CLI diagnostics without requiring the frontend to shell out locally.
#[tauri::command]
pub fn get_codex_cli_diagnostics() -> Result<CodexCliDiagnosticsResponse, String> {
    let (probe, capabilities) = probe_codex_harness_with_capabilities();
    Ok(build_codex_cli_diagnostics_response(
        probe.into(),
        capabilities,
    ))
}
