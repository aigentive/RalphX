use std::path::Path;
use std::str::FromStr;
use std::sync::Arc;
use std::time::Duration;

use crate::application::harness_runtime_registry::resolve_harness_agent_bootstrap;
use crate::application::AppState;
use crate::domain::agents::{AgentConfig, AgentHarnessKind, AgentRole, DEFAULT_AGENT_HARNESS};
use crate::domain::entities::{Automation, ChatContextType};
use crate::error::{AppError, AppResult};
use crate::utils::path_safety::validate_absolute_non_root_path;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AutomationUtilityAgentOutput {
    pub raw_output: String,
    pub model_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AutomationUtilityModelPolicy {
    LockedDefault,
    Override(Option<String>),
}

pub async fn invoke_automation_utility_agent(
    state: &AppState,
    automation: &Automation,
    agent_name: &'static str,
    purpose: &str,
    prompt: String,
    timeout: Duration,
    model_policy: AutomationUtilityModelPolicy,
) -> AppResult<AutomationUtilityAgentOutput> {
    let harness = AgentHarnessKind::from_str(automation.provider_harness.trim())
        .map_err(AppError::Validation)?;
    let project = state
        .project_repo
        .get_by_id(&automation.project_id)
        .await?
        .ok_or_else(|| {
            AppError::NotFound(format!(
                "{purpose} project {} not found",
                automation.project_id.as_str()
            ))
        })?;
    let project_working_directory = validate_absolute_non_root_path(
        Path::new(&project.working_directory),
        &format!("{purpose} project checkout"),
    )?;
    let role = crate::application::agent_lane_resolution::routing_role_for_chat_launch(
        agent_name,
        ChatContextType::Project,
        None,
        None,
        false,
    );
    let runtime = state
        .resolve_manual_role_background_agent_runtime(
            Some(automation.project_id.as_str()),
            Some(project_working_directory.as_path()),
            role,
            None,
            agent_name,
            purpose,
            Some(harness),
        )
        .await?;
    let mut runtime = match model_policy {
        AutomationUtilityModelPolicy::LockedDefault => runtime,
        AutomationUtilityModelPolicy::Override(model) => {
            let mut runtime = runtime;
            runtime.model = model;
            runtime
        }
    };
    let helper_harness = runtime.harness.unwrap_or(DEFAULT_AGENT_HARNESS);
    let bootstrap =
        resolve_harness_agent_bootstrap(helper_harness, agent_name, project_working_directory);
    let env = runtime.env_with_overrides(bootstrap.env);
    let client = Arc::clone(&runtime.client);
    let model_id = runtime.model.clone();
    let handle = client
        .spawn_agent(AgentConfig {
            role: AgentRole::Custom(bootstrap.agent_role.clone()),
            prompt,
            working_directory: bootstrap.working_directory,
            plugin_dir: Some(bootstrap.plugin_dir),
            agent: Some(bootstrap.agent_name),
            model: runtime.model.take(),
            harness: runtime.harness,
            cli_path_override: runtime.cli_path_override,
            logical_effort: runtime.logical_effort,
            approval_policy: runtime.approval_policy,
            sandbox_mode: runtime.sandbox_mode,
            service_tier: runtime.service_tier,
            max_tokens: None,
            timeout_secs: Some(timeout.as_secs().max(1)),
            env,
            mcp_launch_policy: Default::default(),
        })
        .await
        .map_err(|error| AppError::Infrastructure(format!("failed to spawn {purpose}: {error}")))?;
    let output = client
        .wait_for_completion(&handle)
        .await
        .map_err(|error| AppError::Infrastructure(format!("{purpose} failed: {error}")))?;
    if !output.success {
        return Err(AppError::Infrastructure(format!(
            "{purpose} exited unsuccessfully: {}",
            output.content.trim()
        )));
    }
    Ok(AutomationUtilityAgentOutput {
        raw_output: output.content,
        model_id,
    })
}
