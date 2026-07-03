// Memory pipeline orchestration
//
// Triggers background ralphx-memory-maintainer and ralphx-memory-capture agents
// after agent run completion based on context type and project settings.

use crate::application::app_state::ResolvedBackgroundAgentRuntime;
use crate::application::harness_runtime_registry::resolve_harness_agent_bootstrap;
use crate::domain::agents::{AgentConfig, AgentRole, DEFAULT_AGENT_HARNESS};
use crate::domain::entities::{
    ChatContextType, ChatConversationId, MemoryActorType, MemoryEvent, ProjectId,
    ProjectMemorySettings,
};
use crate::domain::repositories::{MemoryEventRepository, ProjectMemorySettingsRepository};
use crate::infrastructure::agents::claude::build_spawnable_command_with_mcp_runtime_context;
use crate::infrastructure::agents::mcp_runtime_context::McpRuntimeContext;
use std::path::Path;
use std::sync::Arc;

const MEMORY_MAINTAINER_AGENT: &str = "ralphx:ralphx-memory-maintainer";
const MEMORY_CAPTURE_AGENT: &str = "ralphx:ralphx-memory-capture";

/// Memory category derived from chat context type
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MemoryCategory {
    Planning,
    Execution,
    Review,
    Merge,
    ProjectChat,
}

impl MemoryCategory {
    /// Map ChatContextType to MemoryCategory
    pub fn from_context_type(context_type: ChatContextType) -> Self {
        match context_type {
            ChatContextType::Ideation => MemoryCategory::Planning,
            ChatContextType::Delegation => MemoryCategory::Execution,
            ChatContextType::Task | ChatContextType::TaskExecution => MemoryCategory::Execution,
            ChatContextType::Review => MemoryCategory::Review,
            ChatContextType::Merge => MemoryCategory::Merge,
            ChatContextType::Project => MemoryCategory::ProjectChat,
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            MemoryCategory::Planning => "planning",
            MemoryCategory::Execution => "execution",
            MemoryCategory::Review => "review",
            MemoryCategory::Merge => "merge",
            MemoryCategory::ProjectChat => "project_chat",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MemoryPipelineSkipReason {
    NoProjectId,
    RecursionGuard,
    Disabled,
    NoEnabledCategory,
}

impl MemoryPipelineSkipReason {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::NoProjectId => "no_project_id",
            Self::RecursionGuard => "recursion_guard",
            Self::Disabled => "disabled",
            Self::NoEnabledCategory => "no_enabled_category",
        }
    }
}

/// Determine which memory pipelines should be triggered for a given context.
///
/// Returns (should_maintain, should_capture) based on settings and context.
/// This is the core logic extracted for testability.
pub fn resolve_pipelines(
    context_type: ChatContextType,
    project_id: Option<&ProjectId>,
    agent_name: Option<&str>,
    settings: &ProjectMemorySettings,
) -> Option<(bool, bool)> {
    resolve_pipelines_with_reason(context_type, project_id, agent_name, settings).ok()
}

pub fn resolve_pipelines_with_reason(
    context_type: ChatContextType,
    project_id: Option<&ProjectId>,
    agent_name: Option<&str>,
    settings: &ProjectMemorySettings,
) -> Result<(bool, bool), MemoryPipelineSkipReason> {
    // Guard: If no project ID, skip (memory is project-scoped)
    if project_id.is_none() {
        tracing::debug!("resolve_pipelines: no project_id, skipping");
        return Err(MemoryPipelineSkipReason::NoProjectId);
    }

    // Recursion guard: Skip if current agent is a memory agent
    if let Some(name) = agent_name {
        if name == "ralphx-memory-maintainer" || name == "ralphx-memory-capture" {
            tracing::debug!(
                agent_name = name,
                "resolve_pipelines: recursion guard triggered, skipping"
            );
            return Err(MemoryPipelineSkipReason::RecursionGuard);
        }
    }

    // Early exit if memory disabled
    if !settings.enabled {
        tracing::debug!("resolve_pipelines: memory disabled for project, skipping");
        return Err(MemoryPipelineSkipReason::Disabled);
    }

    // Map context to category
    let category = MemoryCategory::from_context_type(context_type);
    let category_str = category.as_str();

    let should_maintain = settings
        .maintenance_categories
        .contains(&category_str.to_string());
    let should_capture = settings
        .capture_categories
        .contains(&category_str.to_string());

    if !should_maintain && !should_capture {
        tracing::debug!(
            category = category_str,
            "resolve_pipelines: category not in any enabled categories, skipping"
        );
        return Err(MemoryPipelineSkipReason::NoEnabledCategory);
    }

    Ok((should_maintain, should_capture))
}

/// Trigger memory pipelines after agent run completion
///
/// This function orchestrates background memory agents based on:
/// - Project memory settings (enabled/disabled, category filters)
/// - Context type (mapped to memory category)
/// - Recursion guard (skip if current agent is a memory agent)
///
/// Failures are logged but do not block the primary user workflow.
#[allow(clippy::too_many_arguments)]
pub(crate) async fn trigger_memory_pipelines(
    context_type: ChatContextType,
    context_id: &str,
    conversation_id: &ChatConversationId,
    project_id: Option<&ProjectId>,
    agent_name: Option<&str>,
    cli_path: &Path,
    plugin_dir: &Path,
    working_directory: &Path,
    settings: Option<ProjectMemorySettings>,
    memory_event_repo: Option<Arc<dyn MemoryEventRepository>>,
    project_memory_settings_repo: Option<Arc<dyn ProjectMemorySettingsRepository>>,
    memory_agent_runtime: Option<ResolvedBackgroundAgentRuntime>,
) {
    tracing::debug!(
        %context_type,
        context_id = %context_id,
        conversation_id = conversation_id.as_str(),
        "trigger_memory_pipelines: entry"
    );

    let proj_id = match project_id {
        Some(id) => id,
        None => {
            tracing::debug!("trigger_memory_pipelines: no project_id, skipping");
            return;
        }
    };

    let settings = match resolve_project_memory_settings(
        proj_id,
        settings,
        project_memory_settings_repo.as_ref(),
        memory_event_repo.as_ref(),
        conversation_id,
        context_type,
        context_id,
    )
    .await
    {
        Some(settings) => settings,
        None => return,
    };

    let (should_maintain, should_capture) =
        match resolve_pipelines_with_reason(context_type, project_id, agent_name, &settings) {
            Ok(decision) => decision,
            Err(reason) => {
                log_memory_pipeline_skip(
                    memory_event_repo.as_ref(),
                    proj_id,
                    conversation_id,
                    context_type,
                    context_id,
                    reason.as_str(),
                    None,
                )
                .await;
                return;
            }
        };

    let category = MemoryCategory::from_context_type(context_type);

    tracing::info!(
        %context_type,
        category = category.as_str(),
        project_id = proj_id.as_str(),
        "trigger_memory_pipelines: mapped context to category"
    );

    // Spawn memory agents in parallel (fire-and-forget)
    let mut spawn_tasks = vec![];

    if should_maintain {
        let conv_id = conversation_id.clone();
        let ctx = context_type;
        let ctx_id = context_id.to_string();
        let proj = proj_id.clone();
        let cli = cli_path.to_path_buf();
        let plugin = plugin_dir.to_path_buf();
        let wd = working_directory.to_path_buf();
        let event_repo = memory_event_repo.clone();
        let runtime = memory_agent_runtime.clone();

        log_memory_pipeline_spawn_requested(
            memory_event_repo.as_ref(),
            proj_id,
            conversation_id,
            context_type,
            context_id,
            "ralphx-memory-maintainer",
            MemoryActorType::MemoryMaintainer,
        )
        .await;

        spawn_tasks.push(tokio::spawn(async move {
            if let Err(e) =
                spawn_memory_maintainer(&conv_id, ctx, &ctx_id, &proj, &cli, &plugin, &wd, runtime)
                    .await
            {
                tracing::error!(
                    error = %e,
                    conversation_id = conv_id.as_str(),
                    "trigger_memory_pipelines: failed to spawn ralphx-memory-maintainer"
                );
                // Log spawn failure to memory_events table
                if let Some(repo) = event_repo {
                    let event = MemoryEvent::new(
                        proj.clone(),
                        "spawn_failed",
                        MemoryActorType::System,
                        serde_json::json!({
                            "agent": "ralphx-memory-maintainer",
                            "conversation_id": conv_id.as_str(),
                            "context_type": ctx.to_string(),
                            "context_id": &ctx_id,
                            "error": e.to_string(),
                        }),
                    );
                    if let Err(log_err) = repo.create(event).await {
                        tracing::warn!(
                            error = %log_err,
                            "trigger_memory_pipelines: failed to log spawn failure to memory_events"
                        );
                    }
                }
            }
        }));
    }

    if should_capture {
        let conv_id = conversation_id.clone();
        let ctx = context_type;
        let ctx_id = context_id.to_string();
        let proj = proj_id.clone();
        let cli = cli_path.to_path_buf();
        let plugin = plugin_dir.to_path_buf();
        let wd = working_directory.to_path_buf();
        let event_repo = memory_event_repo.clone();
        let runtime = memory_agent_runtime.clone();

        log_memory_pipeline_spawn_requested(
            memory_event_repo.as_ref(),
            proj_id,
            conversation_id,
            context_type,
            context_id,
            "ralphx-memory-capture",
            MemoryActorType::MemoryCapture,
        )
        .await;

        spawn_tasks.push(tokio::spawn(async move {
            if let Err(e) =
                spawn_memory_capture(&conv_id, ctx, &ctx_id, &proj, &cli, &plugin, &wd, runtime)
                    .await
            {
                tracing::error!(
                    error = %e,
                    conversation_id = conv_id.as_str(),
                    "trigger_memory_pipelines: failed to spawn ralphx-memory-capture"
                );
                // Log spawn failure to memory_events table
                if let Some(repo) = event_repo {
                    let event = MemoryEvent::new(
                        proj.clone(),
                        "spawn_failed",
                        MemoryActorType::System,
                        serde_json::json!({
                            "agent": "ralphx-memory-capture",
                            "conversation_id": conv_id.as_str(),
                            "context_type": ctx.to_string(),
                            "context_id": &ctx_id,
                            "error": e.to_string(),
                        }),
                    );
                    if let Err(log_err) = repo.create(event).await {
                        tracing::warn!(
                            error = %log_err,
                            "trigger_memory_pipelines: failed to log spawn failure to memory_events"
                        );
                    }
                }
            }
        }));
    }

    tracing::info!(
        spawning_count = spawn_tasks.len(),
        maintenance = should_maintain,
        capture = should_capture,
        "trigger_memory_pipelines: spawning memory agents"
    );

    // Don't await - fire and forget
    // Tasks will log their own errors
}

async fn log_memory_pipeline_spawn_requested(
    memory_event_repo: Option<&Arc<dyn MemoryEventRepository>>,
    project_id: &ProjectId,
    conversation_id: &ChatConversationId,
    context_type: ChatContextType,
    context_id: &str,
    agent: &str,
    actor_type: MemoryActorType,
) {
    let Some(repo) = memory_event_repo else {
        return;
    };

    let event = MemoryEvent::new(
        project_id.clone(),
        "memory_pipeline_spawn_requested",
        actor_type,
        serde_json::json!({
            "agent": agent,
            "conversation_id": conversation_id.as_str(),
            "context_type": context_type.to_string(),
            "context_id": context_id,
        }),
    );
    if let Err(log_err) = repo.create(event).await {
        tracing::warn!(
            error = %log_err,
            "trigger_memory_pipelines: failed to log memory pipeline spawn request"
        );
    }
}

async fn resolve_project_memory_settings(
    project_id: &ProjectId,
    provided: Option<ProjectMemorySettings>,
    settings_repo: Option<&Arc<dyn ProjectMemorySettingsRepository>>,
    memory_event_repo: Option<&Arc<dyn MemoryEventRepository>>,
    conversation_id: &ChatConversationId,
    context_type: ChatContextType,
    context_id: &str,
) -> Option<ProjectMemorySettings> {
    if let Some(settings) = provided {
        return Some(settings);
    }

    if let Some(repo) = settings_repo {
        match repo.get_for_project(project_id).await {
            Ok(Some(settings)) => return Some(settings),
            Ok(None) => {
                tracing::debug!(
                    project_id = project_id.as_str(),
                    "No project memory settings row found; using project defaults"
                );
            }
            Err(error) => {
                tracing::warn!(
                    project_id = project_id.as_str(),
                    error = %error,
                    "Failed to load project memory settings; skipping memory pipeline"
                );
                log_memory_pipeline_skip(
                    memory_event_repo,
                    project_id,
                    conversation_id,
                    context_type,
                    context_id,
                    "settings_load_failed",
                    Some(error.to_string()),
                )
                .await;
                return None;
            }
        }
    }

    Some(ProjectMemorySettings::default_for_project(
        project_id.clone(),
    ))
}

async fn log_memory_pipeline_skip(
    memory_event_repo: Option<&Arc<dyn MemoryEventRepository>>,
    project_id: &ProjectId,
    conversation_id: &ChatConversationId,
    context_type: ChatContextType,
    context_id: &str,
    reason: &str,
    error: Option<String>,
) {
    let Some(repo) = memory_event_repo else {
        return;
    };

    let mut details = serde_json::json!({
        "reason": reason,
        "conversation_id": conversation_id.as_str(),
        "context_type": context_type.to_string(),
        "context_id": context_id,
    });
    if let Some(error) = error {
        details["error"] = serde_json::json!(error);
    }

    let event = MemoryEvent::new(
        project_id.clone(),
        "memory_pipeline_skipped",
        MemoryActorType::System,
        details,
    );
    if let Err(log_err) = repo.create(event).await {
        tracing::warn!(
            error = %log_err,
            "trigger_memory_pipelines: failed to log memory pipeline skip"
        );
    }
}

/// Spawn ralphx-memory-maintainer agent
///
/// Spawns the ralphx-memory-maintainer agent with appropriate context and environment variables.
async fn spawn_memory_maintainer(
    conversation_id: &ChatConversationId,
    context_type: ChatContextType,
    context_id: &str,
    project_id: &ProjectId,
    cli_path: &Path,
    plugin_dir: &Path,
    working_directory: &Path,
    runtime: Option<ResolvedBackgroundAgentRuntime>,
) -> Result<(), String> {
    tracing::info!(
        conversation_id = conversation_id.as_str(),
        %context_type,
        context_id = %context_id,
        project_id = project_id.as_str(),
        "spawn_memory_maintainer: spawning agent"
    );

    let conv_id_str = conversation_id.as_str();
    let proj_id_str = project_id.as_str();
    let context_type_str = format!("{}", context_type);

    let prompt = format!(
        "Analyze and maintain memory rules for conversation_id='{}' in project_id='{}' (context: {}, {})",
        conv_id_str,
        proj_id_str,
        context_type,
        context_id
    );

    if let Some(runtime) = runtime {
        return spawn_memory_agent_with_runtime(
            MemoryAgentKind::Maintainer,
            runtime,
            prompt,
            conversation_id,
            context_type,
            context_id,
            project_id,
            working_directory,
        )
        .await;
    }

    let runtime_context = memory_agent_runtime_context(
        conversation_id,
        context_type,
        context_id,
        project_id,
        working_directory,
    );

    let mut cmd = build_spawnable_command_with_mcp_runtime_context(
        cli_path,
        plugin_dir,
        &prompt,
        Some("ralphx:ralphx-memory-maintainer"),
        None,
        working_directory,
        None, // effort_override: memory pipelines use default
        None, // model_override: use agent config default
        Some(&runtime_context),
    )?;

    cmd.env("RALPHX_CONVERSATION_ID", &conv_id_str);
    cmd.env("RALPHX_CONTEXT_TYPE", &context_type_str);
    cmd.env("RALPHX_CONTEXT_ID", context_id);
    cmd.env("RALPHX_PROJECT_ID", proj_id_str);

    // Spawn and ignore the child process (fire-and-forget)
    let _child = cmd
        .spawn()
        .await
        .map_err(|e| format!("Failed to spawn ralphx-memory-maintainer: {}", e))?;

    Ok(())
}

/// Spawn ralphx-memory-capture agent
///
/// Spawns the ralphx-memory-capture agent with appropriate context and environment variables.
async fn spawn_memory_capture(
    conversation_id: &ChatConversationId,
    context_type: ChatContextType,
    context_id: &str,
    project_id: &ProjectId,
    cli_path: &Path,
    plugin_dir: &Path,
    working_directory: &Path,
    runtime: Option<ResolvedBackgroundAgentRuntime>,
) -> Result<(), String> {
    tracing::info!(
        conversation_id = conversation_id.as_str(),
        %context_type,
        context_id = %context_id,
        project_id = project_id.as_str(),
        "spawn_memory_capture: spawning agent"
    );

    let conv_id_str = conversation_id.as_str();
    let proj_id_str = project_id.as_str();
    let context_type_str = format!("{}", context_type);

    let prompt = format!(
        "Capture learning from conversation_id='{}' in project_id='{}' (context: {}, {})",
        conv_id_str, proj_id_str, context_type, context_id
    );

    if let Some(runtime) = runtime {
        return spawn_memory_agent_with_runtime(
            MemoryAgentKind::Capture,
            runtime,
            prompt,
            conversation_id,
            context_type,
            context_id,
            project_id,
            working_directory,
        )
        .await;
    }

    let runtime_context = memory_agent_runtime_context(
        conversation_id,
        context_type,
        context_id,
        project_id,
        working_directory,
    );

    let mut cmd = build_spawnable_command_with_mcp_runtime_context(
        cli_path,
        plugin_dir,
        &prompt,
        Some("ralphx:ralphx-memory-capture"),
        None,
        working_directory,
        None, // effort_override: memory pipelines use default
        None, // model_override: use agent config default
        Some(&runtime_context),
    )?;

    cmd.env("RALPHX_CONVERSATION_ID", &conv_id_str);
    cmd.env("RALPHX_CONTEXT_TYPE", &context_type_str);
    cmd.env("RALPHX_CONTEXT_ID", context_id);
    cmd.env("RALPHX_PROJECT_ID", proj_id_str);

    // Spawn and ignore the child process (fire-and-forget)
    let _child = cmd
        .spawn()
        .await
        .map_err(|e| format!("Failed to spawn ralphx-memory-capture: {}", e))?;

    Ok(())
}

#[derive(Clone, Copy)]
enum MemoryAgentKind {
    Maintainer,
    Capture,
}

impl MemoryAgentKind {
    fn agent_name(self) -> &'static str {
        match self {
            Self::Maintainer => MEMORY_MAINTAINER_AGENT,
            Self::Capture => MEMORY_CAPTURE_AGENT,
        }
    }

    fn short_name(self) -> &'static str {
        match self {
            Self::Maintainer => "ralphx-memory-maintainer",
            Self::Capture => "ralphx-memory-capture",
        }
    }
}

async fn spawn_memory_agent_with_runtime(
    kind: MemoryAgentKind,
    runtime: ResolvedBackgroundAgentRuntime,
    prompt: String,
    conversation_id: &ChatConversationId,
    context_type: ChatContextType,
    context_id: &str,
    project_id: &ProjectId,
    working_directory: &Path,
) -> Result<(), String> {
    let config = build_memory_agent_config(
        kind,
        &runtime,
        prompt,
        conversation_id,
        context_type,
        context_id,
        project_id,
        working_directory,
    );
    let client = Arc::clone(&runtime.client);
    let handle = client
        .spawn_agent(config)
        .await
        .map_err(|error| format!("Failed to spawn {}: {}", kind.short_name(), error))?;

    tokio::spawn(async move {
        if let Err(error) = client.wait_for_completion(&handle).await {
            tracing::warn!(
                agent = kind.short_name(),
                error = %error,
                "Memory agent failed after spawn"
            );
        }
    });

    Ok(())
}

fn build_memory_agent_config(
    kind: MemoryAgentKind,
    runtime: &ResolvedBackgroundAgentRuntime,
    prompt: String,
    conversation_id: &ChatConversationId,
    context_type: ChatContextType,
    context_id: &str,
    project_id: &ProjectId,
    working_directory: &Path,
) -> AgentConfig {
    let harness = runtime.harness.unwrap_or(DEFAULT_AGENT_HARNESS);
    let bootstrap = resolve_harness_agent_bootstrap(
        harness,
        kind.agent_name(),
        working_directory.to_path_buf(),
    );
    let mut env = bootstrap.env;
    env.insert(
        "RALPHX_CONVERSATION_ID".to_string(),
        conversation_id.as_str().to_string(),
    );
    env.insert("RALPHX_CONTEXT_TYPE".to_string(), context_type.to_string());
    env.insert("RALPHX_CONTEXT_ID".to_string(), context_id.to_string());
    env.insert(
        "RALPHX_PROJECT_ID".to_string(),
        project_id.as_str().to_string(),
    );

    AgentConfig {
        role: AgentRole::Custom(bootstrap.agent_role),
        prompt,
        working_directory: bootstrap.working_directory,
        plugin_dir: Some(bootstrap.plugin_dir),
        agent: Some(bootstrap.agent_name),
        model: runtime.model.clone(),
        harness: runtime.harness,
        cli_path_override: runtime.cli_path_override.clone(),
        logical_effort: runtime.logical_effort,
        approval_policy: runtime.approval_policy.clone(),
        sandbox_mode: runtime.sandbox_mode.clone(),
        service_tier: runtime.service_tier.clone(),
        max_tokens: None,
        timeout_secs: None,
        env,
    }
}

fn memory_agent_runtime_context(
    conversation_id: &ChatConversationId,
    context_type: ChatContextType,
    context_id: &str,
    project_id: &ProjectId,
    working_directory: &Path,
) -> McpRuntimeContext {
    McpRuntimeContext {
        context_type: Some(context_type.to_string()),
        context_id: Some(context_id.to_string()),
        project_id: Some(project_id.as_str().to_string()),
        working_directory: Some(working_directory.to_path_buf()),
        parent_conversation_id: Some(conversation_id.as_str().to_string()),
        ..Default::default()
    }
}

#[cfg(test)]
#[path = "memory_orchestration_tests.rs"]
mod tests;
