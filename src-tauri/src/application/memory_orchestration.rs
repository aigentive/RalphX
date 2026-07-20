// Memory pipeline orchestration
//
// Triggers background ralphx-memory-maintainer and ralphx-memory-capture agents
// after agent run completion based on context type and project settings.

use crate::domain::entities::{
    ChatContextType, ChatConversationId, MemoryActorType, MemoryEvent, ProjectId,
};
use crate::domain::repositories::MemoryEventRepository;
use crate::infrastructure::agents::claude::build_spawnable_command;
use std::path::Path;
use std::sync::Arc;

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
            ChatContextType::BranchUpdate => MemoryCategory::Execution,
            ChatContextType::Review => MemoryCategory::Review,
            ChatContextType::Merge => MemoryCategory::Merge,
            ChatContextType::Project | ChatContextType::Standalone => MemoryCategory::ProjectChat,
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

/// Project memory settings (stub - will be replaced with repository-backed implementation)
#[derive(Debug, Clone)]
pub struct ProjectMemorySettings {
    pub enabled: bool,
    pub maintenance_categories: Vec<String>,
    pub capture_categories: Vec<String>,
}

impl Default for ProjectMemorySettings {
    fn default() -> Self {
        Self {
            enabled: false,
            maintenance_categories: vec![
                "execution".to_string(),
                "review".to_string(),
                "merge".to_string(),
            ],
            capture_categories: vec![
                "planning".to_string(),
                "execution".to_string(),
                "review".to_string(),
            ],
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
    // Guard: If no project ID, skip (memory is project-scoped)
    if project_id.is_none() {
        tracing::debug!("resolve_pipelines: no project_id, skipping");
        return None;
    }

    // Recursion guard: Skip if current agent is a memory agent
    if let Some(name) = agent_name {
        if name == "ralphx-memory-maintainer" || name == "ralphx-memory-capture" {
            tracing::debug!(
                agent_name = name,
                "resolve_pipelines: recursion guard triggered, skipping"
            );
            return None;
        }
    }

    // Early exit if memory disabled
    if !settings.enabled {
        tracing::debug!("resolve_pipelines: memory disabled for project, skipping");
        return None;
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
        return None;
    }

    Some((should_maintain, should_capture))
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
pub async fn trigger_memory_pipelines(
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

    // Use provided settings or defaults
    let settings = settings.unwrap_or_default();

    let Some((should_maintain, should_capture)) =
        resolve_pipelines(context_type, project_id, agent_name, &settings)
    else {
        return;
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

        spawn_tasks.push(tokio::spawn(async move {
            if let Err(e) =
                spawn_memory_maintainer(&conv_id, ctx, &ctx_id, &proj, &cli, &plugin, &wd).await
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

        spawn_tasks.push(tokio::spawn(async move {
            if let Err(e) =
                spawn_memory_capture(&conv_id, ctx, &ctx_id, &proj, &cli, &plugin, &wd).await
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

    let mut cmd = build_spawnable_command(
        cli_path,
        plugin_dir,
        &prompt,
        Some("ralphx:ralphx-memory-maintainer"),
        None,
        working_directory,
        None, // effort_override: memory pipelines use default
        None, // model_override: use agent config default
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

    let mut cmd = build_spawnable_command(
        cli_path,
        plugin_dir,
        &prompt,
        Some("ralphx:ralphx-memory-capture"),
        None,
        working_directory,
        None, // effort_override: memory pipelines use default
        None, // model_override: use agent config default
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

#[cfg(test)]
#[path = "memory_orchestration_tests.rs"]
mod tests;
