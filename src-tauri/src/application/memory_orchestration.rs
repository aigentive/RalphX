// Memory pipeline orchestration
//
// Triggers background ralphx-memory-maintainer and ralphx-memory-capture agents
// after agent run completion based on context type and project settings.

use crate::application::app_state::ResolvedBackgroundAgentRuntime;
use crate::application::harness_runtime_registry::resolve_harness_agent_bootstrap;
use crate::application::project_skill_distillation_service::{
    claim_outcome_ids, PreparedProjectSkillDistillation, ProjectSkillDistillationSelection,
    ProjectSkillDistillationService, ProjectSkillDistillationTrigger, SKILL_DISTILLER_PROFILE,
};
use crate::domain::agents::{AgentConfig, AgentRole, DEFAULT_AGENT_HARNESS};
use crate::domain::entities::{
    ChatContextType, ChatConversationId, MemoryActorType, MemoryEvent, ProjectId,
    ProjectMemorySettings, TaskOutcomeId,
};
use crate::domain::repositories::{
    MemoryEventRepository, ProjectMemorySettingsRepository, ProjectSkillEvidenceBatchRepository,
    ProjectSkillRepository, ProjectSkillSettingsRepository, TaskOutcomeRepository,
};
use crate::infrastructure::agents::claude::build_spawnable_command_with_mcp_runtime_context;
use crate::infrastructure::agents::claude::build_spawnable_command_with_mcp_runtime_context_and_profile;
use crate::infrastructure::agents::claude::SpawnableCommand;
use crate::infrastructure::agents::mcp_runtime_context::McpRuntimeContext;
use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;

const MEMORY_MAINTAINER_AGENT: &str = "ralphx:ralphx-memory-maintainer";
const MEMORY_CAPTURE_AGENT: &str = "ralphx:ralphx-memory-capture";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProjectSkillDistillationScheduleStatus {
    Started,
    Queued,
    Skipped,
    Unavailable,
    Failed,
}

impl ProjectSkillDistillationScheduleStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Started => "started",
            Self::Queued => "queued",
            Self::Skipped => "skipped",
            Self::Unavailable => "unavailable",
            Self::Failed => "failed",
        }
    }
}

#[derive(Debug, Clone)]
pub struct ProjectSkillDistillationScheduleResult {
    pub status: ProjectSkillDistillationScheduleStatus,
    pub selected_outcomes: usize,
    pub batch_count: usize,
    pub started_batches: usize,
    pub message: Option<String>,
}

#[derive(Clone)]
pub(crate) struct ProjectSkillDistillationDependencies {
    pub(crate) outcome_repo: Arc<dyn TaskOutcomeRepository>,
    pub(crate) batch_repo: Arc<dyn ProjectSkillEvidenceBatchRepository>,
    pub(crate) settings_repo: Arc<dyn ProjectSkillSettingsRepository>,
    pub(crate) skill_repo: Arc<dyn ProjectSkillRepository>,
}

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

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MemoryPipelineSkipReason {
    NoProjectId,
    RecursionGuard,
    Disabled,
    NoEnabledCategory,
    SettingsLoadFailed,
}

impl MemoryPipelineSkipReason {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::NoProjectId => "no_project_id",
            Self::RecursionGuard => "recursion_guard",
            Self::Disabled => "disabled",
            Self::NoEnabledCategory => "no_enabled_category",
            Self::SettingsLoadFailed => "settings_load_failed",
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
        let normalized_name = name.strip_prefix("ralphx:").unwrap_or(name);
        if normalized_name == "ralphx-memory-maintainer"
            || normalized_name == "ralphx-memory-capture"
        {
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
    skill_distillation: Option<ProjectSkillDistillationDependencies>,
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

    if !is_memory_agent(agent_name) {
        if let Some(dependencies) = skill_distillation {
            trigger_project_skill_distillation(
                proj_id,
                context_type,
                context_id,
                conversation_id,
                cli_path,
                plugin_dir,
                working_directory,
                memory_agent_runtime.clone(),
                memory_event_repo.clone(),
                dependencies,
            )
            .await;
        }
    }

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

fn is_memory_agent(agent_name: Option<&str>) -> bool {
    agent_name
        .map(|name| name.strip_prefix("ralphx:").unwrap_or(name))
        .is_some_and(|name| matches!(name, "ralphx-memory-maintainer" | "ralphx-memory-capture"))
}

#[allow(clippy::too_many_arguments)]
pub(crate) async fn schedule_explicit_project_skill_distillation(
    state: &crate::application::AppState,
    project_id: &ProjectId,
    selection: ProjectSkillDistillationSelection,
    conversation_id: Option<&ChatConversationId>,
    context_type: ChatContextType,
    context_id: &str,
) -> ProjectSkillDistillationScheduleResult {
    let service = ProjectSkillDistillationService::new(
        Arc::clone(&state.task_outcome_repo),
        Arc::clone(&state.project_skill_evidence_batch_repo),
        Arc::clone(&state.project_skill_settings_repo),
        Arc::clone(&state.project_skill_repo),
        Arc::clone(&state.memory_event_repo),
    );
    let stale_after_secs =
        crate::infrastructure::agents::claude::limits_config().skill_distiller_claim_stale_secs;
    let preparation = match service
        .prepare_explicit_claims(project_id, selection, stale_after_secs)
        .await
    {
        Ok(preparation) => preparation,
        Err(error) => {
            log_skill_distillation_event(
                &state.memory_event_repo,
                project_id,
                "skill_distillation_failed",
                serde_json::json!({ "phase": "prepare_explicit", "error": error.to_string() }),
            )
            .await;
            return ProjectSkillDistillationScheduleResult {
                status: ProjectSkillDistillationScheduleStatus::Failed,
                selected_outcomes: 0,
                batch_count: 0,
                started_batches: 0,
                message: Some("Evidence could not be queued for distillation.".to_string()),
            };
        }
    };
    if !preparation.enabled {
        return ProjectSkillDistillationScheduleResult {
            status: ProjectSkillDistillationScheduleStatus::Skipped,
            selected_outcomes: 0,
            batch_count: 0,
            started_batches: 0,
            message: Some("Project skills are disabled for this project.".to_string()),
        };
    }
    if preparation.selected_outcomes == 0 {
        return ProjectSkillDistillationScheduleResult {
            status: ProjectSkillDistillationScheduleStatus::Skipped,
            selected_outcomes: 0,
            batch_count: 0,
            started_batches: 0,
            message: Some("No eligible evidence was available to queue.".to_string()),
        };
    }
    if preparation.prepared.is_empty() {
        return ProjectSkillDistillationScheduleResult {
            status: ProjectSkillDistillationScheduleStatus::Queued,
            selected_outcomes: preparation.selected_outcomes,
            batch_count: preparation.batch_count,
            started_batches: 0,
            message: Some(
                "Evidence is already queued or being processed by the distiller.".to_string(),
            ),
        };
    }

    let project = match state.project_repo.get_by_id(project_id).await {
        Ok(Some(project)) => project,
        Ok(None) => {
            release_explicit_claims(state, &preparation.prepared).await;
            return ProjectSkillDistillationScheduleResult {
                status: ProjectSkillDistillationScheduleStatus::Unavailable,
                selected_outcomes: preparation.selected_outcomes,
                batch_count: preparation.batch_count,
                started_batches: 0,
                message: Some("The project is no longer available.".to_string()),
            };
        }
        Err(error) => {
            release_explicit_claims(state, &preparation.prepared).await;
            return ProjectSkillDistillationScheduleResult {
                status: ProjectSkillDistillationScheduleStatus::Failed,
                selected_outcomes: preparation.selected_outcomes,
                batch_count: preparation.batch_count,
                started_batches: 0,
                message: Some(format!("Project lookup failed: {error}")),
            };
        }
    };
    let working_directory = match crate::utils::path_safety::validate_absolute_non_root_path(
        Path::new(&project.working_directory),
        "project root",
    ) {
        Ok(path) if path.is_dir() => path,
        _ => {
            release_explicit_claims(state, &preparation.prepared).await;
            return ProjectSkillDistillationScheduleResult {
                status: ProjectSkillDistillationScheduleStatus::Unavailable,
                selected_outcomes: preparation.selected_outcomes,
                batch_count: preparation.batch_count,
                started_batches: 0,
                message: Some("The project working directory is unavailable.".to_string()),
            };
        }
    };

    let conversation = match conversation_id {
        Some(conversation_id) => state
            .chat_conversation_repo
            .get_by_id(conversation_id)
            .await
            .ok()
            .flatten(),
        None => state
            .chat_conversation_repo
            .get_active_for_context(context_type, context_id)
            .await
            .ok()
            .flatten(),
    };
    let launch_conversation_id = conversation
        .as_ref()
        .map(|conversation| conversation.id.clone())
        .or_else(|| conversation_id.cloned())
        .unwrap_or_else(|| {
            ChatConversationId::from_string(format!(
                "project-skill-distillation:{}",
                project_id.as_str()
            ))
        });
    let provider_harness = conversation
        .as_ref()
        .and_then(|conversation| conversation.provider_harness);
    let runtime = state
        .resolve_manual_role_background_agent_runtime(
            Some(project_id.as_str()),
            Some(&working_directory),
            crate::domain::agents::RoutingRole::MemoryCapture,
            crate::infrastructure::agents::claude::agent_names::SHORT_MEMORY_CAPTURE,
            "explicit project skill distillation",
            provider_harness,
        )
        .await
        .ok();
    let harness = runtime
        .as_ref()
        .and_then(|runtime| runtime.harness)
        .or(provider_harness)
        .unwrap_or(DEFAULT_AGENT_HARNESS);
    let bootstrap =
        crate::application::harness_runtime_registry::resolve_chat_service_bootstrap(harness);
    let cli_path = runtime
        .as_ref()
        .and_then(|runtime| runtime.cli_path_override.clone())
        .unwrap_or(bootstrap.cli_path);
    let plugin_dir = crate::application::harness_runtime_registry::resolve_harness_plugin_dir(
        harness,
        &working_directory,
    );

    let mut started_batches = 0;
    for prepared in &preparation.prepared {
        log_skill_distillation_event(
            &state.memory_event_repo,
            project_id,
            "skill_distillation_spawn_requested",
            serde_json::json!({
                "batch_id": prepared.batch.id.as_str(),
                "fingerprint": prepared.batch.fingerprint,
                "conversation_id": launch_conversation_id.as_str(),
                "context_type": context_type.to_string(),
                "context_id": context_id,
                "trigger": "explicit",
            }),
        )
        .await;
        match spawn_skill_distiller(
            prepared,
            &launch_conversation_id,
            context_type,
            context_id,
            project_id,
            &cli_path,
            &plugin_dir,
            &working_directory,
            runtime.clone(),
            Arc::clone(&state.project_skill_evidence_batch_repo),
            Arc::clone(&state.memory_event_repo),
        )
        .await
        {
            Ok(()) => started_batches += 1,
            Err(error) => {
                let released = state
                    .project_skill_evidence_batch_repo
                    .release_claim(
                        &prepared.batch.id,
                        &prepared.claim_token,
                        chrono::Utc::now(),
                    )
                    .await;
                log_skill_distillation_event(
                    &state.memory_event_repo,
                    project_id,
                    "skill_distillation_failed",
                    serde_json::json!({
                        "phase": "spawn_explicit",
                        "batch_id": prepared.batch.id.as_str(),
                        "error": error,
                        "claim_released": matches!(released, Ok(true)),
                    }),
                )
                .await;
            }
        }
    }

    ProjectSkillDistillationScheduleResult {
        status: if started_batches > 0 {
            ProjectSkillDistillationScheduleStatus::Started
        } else {
            ProjectSkillDistillationScheduleStatus::Failed
        },
        selected_outcomes: preparation.selected_outcomes,
        batch_count: preparation.batch_count,
        started_batches,
        message: (started_batches == 0)
            .then(|| "Evidence remains queued because the distiller could not start.".to_string()),
    }
}

async fn release_explicit_claims(
    state: &crate::application::AppState,
    prepared: &[PreparedProjectSkillDistillation],
) {
    for claim in prepared {
        if let Err(error) = state
            .project_skill_evidence_batch_repo
            .release_claim(&claim.batch.id, &claim.claim_token, chrono::Utc::now())
            .await
        {
            tracing::warn!(
                batch_id = claim.batch.id.as_str(),
                error = %error,
                "Failed to release explicit skill distillation claim"
            );
        }
    }
}

#[allow(clippy::too_many_arguments)]
async fn trigger_project_skill_distillation(
    project_id: &ProjectId,
    context_type: ChatContextType,
    context_id: &str,
    conversation_id: &ChatConversationId,
    cli_path: &Path,
    plugin_dir: &Path,
    working_directory: &Path,
    runtime: Option<ResolvedBackgroundAgentRuntime>,
    memory_event_repo: Option<Arc<dyn MemoryEventRepository>>,
    dependencies: ProjectSkillDistillationDependencies,
) {
    let Some(memory_event_repo) = memory_event_repo else {
        tracing::warn!(
            project_id = project_id.as_str(),
            "Skill distillation skipped because durable event storage is unavailable"
        );
        return;
    };
    let service = ProjectSkillDistillationService::new(
        dependencies.outcome_repo,
        Arc::clone(&dependencies.batch_repo),
        dependencies.settings_repo,
        dependencies.skill_repo,
        Arc::clone(&memory_event_repo),
    );
    let stale_after_secs =
        crate::infrastructure::agents::claude::limits_config().skill_distiller_claim_stale_secs;
    let prepared = match service
        .prepare_claim(
            project_id,
            ProjectSkillDistillationTrigger::Automatic,
            stale_after_secs,
        )
        .await
    {
        Ok(Some(prepared)) => prepared,
        Ok(None) => return,
        Err(error) => {
            log_skill_distillation_event(
                &memory_event_repo,
                project_id,
                "skill_distillation_failed",
                serde_json::json!({ "phase": "prepare", "error": error.to_string() }),
            )
            .await;
            return;
        }
    };

    log_skill_distillation_event(
        &memory_event_repo,
        project_id,
        "skill_distillation_spawn_requested",
        serde_json::json!({
            "batch_id": prepared.batch.id.as_str(),
            "fingerprint": prepared.batch.fingerprint,
            "conversation_id": conversation_id.as_str(),
            "context_type": context_type.to_string(),
            "context_id": context_id,
        }),
    )
    .await;

    if let Err(error) = spawn_skill_distiller(
        &prepared,
        conversation_id,
        context_type,
        context_id,
        project_id,
        cli_path,
        plugin_dir,
        working_directory,
        runtime,
        Arc::clone(&dependencies.batch_repo),
        Arc::clone(&memory_event_repo),
    )
    .await
    {
        let released = dependencies
            .batch_repo
            .release_claim(
                &prepared.batch.id,
                &prepared.claim_token,
                chrono::Utc::now(),
            )
            .await;
        log_skill_distillation_event(
            &memory_event_repo,
            project_id,
            "skill_distillation_failed",
            serde_json::json!({
                "phase": "spawn",
                "batch_id": prepared.batch.id.as_str(),
                "error": error,
                "claim_released": matches!(released, Ok(true)),
            }),
        )
        .await;
    }
}

async fn log_skill_distillation_event(
    repository: &Arc<dyn MemoryEventRepository>,
    project_id: &ProjectId,
    event_type: &str,
    details: serde_json::Value,
) {
    if let Err(error) = repository
        .create(MemoryEvent::new(
            project_id.clone(),
            event_type,
            MemoryActorType::System,
            details,
        ))
        .await
    {
        tracing::warn!(
            project_id = project_id.as_str(),
            event_type,
            error = %error,
            "Failed to persist skill distillation event"
        );
    }
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
                    MemoryPipelineSkipReason::SettingsLoadFailed.as_str(),
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

    let cmd = build_memory_agent_direct_command(
        MemoryAgentKind::Maintainer,
        cli_path,
        plugin_dir,
        &prompt,
        conversation_id,
        context_type,
        context_id,
        project_id,
        working_directory,
    )?;

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

    let cmd = build_memory_agent_direct_command(
        MemoryAgentKind::Capture,
        cli_path,
        plugin_dir,
        &prompt,
        conversation_id,
        context_type,
        context_id,
        project_id,
        working_directory,
    )?;

    // Spawn and ignore the child process (fire-and-forget)
    let _child = cmd
        .spawn()
        .await
        .map_err(|e| format!("Failed to spawn ralphx-memory-capture: {}", e))?;

    Ok(())
}

#[allow(clippy::too_many_arguments)]
async fn spawn_skill_distiller(
    prepared: &PreparedProjectSkillDistillation,
    conversation_id: &ChatConversationId,
    context_type: ChatContextType,
    context_id: &str,
    project_id: &ProjectId,
    cli_path: &Path,
    plugin_dir: &Path,
    working_directory: &Path,
    runtime: Option<ResolvedBackgroundAgentRuntime>,
    batch_repo: Arc<dyn ProjectSkillEvidenceBatchRepository>,
    memory_event_repo: Arc<dyn MemoryEventRepository>,
) -> Result<(), String> {
    if let Some(runtime) = runtime {
        let mut config = build_memory_agent_config(
            MemoryAgentKind::Distiller,
            &runtime,
            prepared.prompt.clone(),
            conversation_id,
            context_type,
            context_id,
            project_id,
            working_directory,
        )?;
        apply_distillation_claim_env(&mut config.env, prepared)?;
        let client = Arc::clone(&runtime.client);
        let handle = client
            .spawn_agent(config)
            .await
            .map_err(|error| format!("Failed to spawn skill distiller: {error}"))?;
        let batch_id = prepared.batch.id.clone();
        let claim_token = prepared.claim_token.clone();
        let project_id = project_id.clone();
        tokio::spawn(async move {
            if let Err(error) = client.wait_for_completion(&handle).await {
                tracing::warn!(
                    batch_id = batch_id.as_str(),
                    error = %error,
                    "Skill distiller failed after spawn; releasing claim"
                );
                let released = batch_repo
                    .release_claim(&batch_id, &claim_token, chrono::Utc::now())
                    .await;
                log_skill_distillation_event(
                    &memory_event_repo,
                    &project_id,
                    "skill_distillation_failed",
                    serde_json::json!({
                        "phase": "wait",
                        "batch_id": batch_id.as_str(),
                        "error": error.to_string(),
                        "claim_released": matches!(&released, Ok(true)),
                        "release_error": released.as_ref().err().map(ToString::to_string),
                    }),
                )
                .await;
            }
        });
        return Ok(());
    }

    let mut launch = prepare_memory_agent_launch(
        conversation_id,
        context_type,
        context_id,
        project_id,
        working_directory,
        Some("skill_distiller"),
    )?;
    apply_distillation_claim_env(&mut launch.env, prepared)?;
    launch.runtime_context = McpRuntimeContext::from_agent_env(&launch.env, working_directory)
        .ok_or_else(|| "Skill distiller launch requires project scope".to_string())?;
    let mut command = build_spawnable_command_with_mcp_runtime_context_and_profile(
        cli_path,
        plugin_dir,
        &prepared.prompt,
        Some(MEMORY_CAPTURE_AGENT),
        Some(SKILL_DISTILLER_PROFILE),
        None,
        None,
        working_directory,
        false,
        None,
        None,
        Some(&launch.runtime_context),
    )?;
    for (key, value) in &launch.env {
        command.env(key, value);
    }
    let child = command
        .spawn()
        .await
        .map_err(|error| format!("Failed to spawn skill distiller: {error}"))?;
    let batch_id = prepared.batch.id.clone();
    let claim_token = prepared.claim_token.clone();
    let project_id = project_id.clone();
    tokio::spawn(async move {
        let failure = match child.wait_with_output().await {
            Ok(output) if output.status.success() => None,
            Ok(output) => Some(format!("skill distiller exited with {}", output.status)),
            Err(error) => Some(format!("failed to wait for skill distiller: {error}")),
        };
        if let Some(error) = failure {
            let released = batch_repo
                .release_claim(&batch_id, &claim_token, chrono::Utc::now())
                .await;
            log_skill_distillation_event(
                &memory_event_repo,
                &project_id,
                "skill_distillation_failed",
                serde_json::json!({
                    "phase": "wait",
                    "batch_id": batch_id.as_str(),
                    "error": error,
                    "claim_released": matches!(&released, Ok(true)),
                    "release_error": released.as_ref().err().map(ToString::to_string),
                }),
            )
            .await;
        }
    });
    Ok(())
}

fn apply_distillation_claim_env(
    env: &mut HashMap<String, String>,
    prepared: &PreparedProjectSkillDistillation,
) -> Result<(), String> {
    env.insert(
        "RALPHX_AGENT_PROFILE".to_string(),
        SKILL_DISTILLER_PROFILE.to_string(),
    );
    env.insert(
        "RALPHX_SKILL_DISTILLATION_BATCH_ID".to_string(),
        prepared.batch.id.as_str().to_string(),
    );
    env.insert(
        "RALPHX_SKILL_DISTILLATION_CLAIM_TOKEN".to_string(),
        prepared.claim_token.clone(),
    );
    env.insert(
        "RALPHX_SKILL_DISTILLATION_FINGERPRINT".to_string(),
        prepared.batch.fingerprint.clone(),
    );
    env.insert(
        "RALPHX_SKILL_DISTILLATION_OUTCOME_IDS".to_string(),
        serde_json::to_string(
            &claim_outcome_ids(&prepared.batch)
                .iter()
                .map(TaskOutcomeId::as_str)
                .collect::<Vec<_>>(),
        )
        .map_err(|error| format!("Failed to serialize skill distillation outcomes: {error}"))?,
    );
    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum MemoryAgentKind {
    Maintainer,
    Capture,
    Distiller,
}

impl MemoryAgentKind {
    pub(crate) fn agent_name(self) -> &'static str {
        match self {
            Self::Maintainer => MEMORY_MAINTAINER_AGENT,
            Self::Capture | Self::Distiller => MEMORY_CAPTURE_AGENT,
        }
    }

    pub(crate) fn pipeline_role(self) -> &'static str {
        match self {
            Self::Maintainer => "memory_maintainer",
            Self::Capture => "memory_capture",
            Self::Distiller => "skill_distiller",
        }
    }

    fn short_name(self) -> &'static str {
        match self {
            Self::Maintainer => "ralphx-memory-maintainer",
            Self::Capture | Self::Distiller => "ralphx-memory-capture",
        }
    }
}

pub(crate) struct MemoryAgentLaunchContext {
    pub(crate) env: HashMap<String, String>,
    pub(crate) runtime_context: McpRuntimeContext,
}

pub(crate) fn prepare_memory_agent_launch(
    conversation_id: &ChatConversationId,
    context_type: ChatContextType,
    context_id: &str,
    project_id: &ProjectId,
    working_directory: &Path,
    pipeline_role: Option<&str>,
) -> Result<MemoryAgentLaunchContext, String> {
    let conversation_id = conversation_id.as_str().to_string();
    let mut env = HashMap::from([
        (
            "RALPHX_CONVERSATION_ID".to_string(),
            conversation_id.clone(),
        ),
        ("RALPHX_CONTEXT_TYPE".to_string(), context_type.to_string()),
        ("RALPHX_CONTEXT_ID".to_string(), context_id.to_string()),
        (
            "RALPHX_PROJECT_ID".to_string(),
            project_id.as_str().to_string(),
        ),
        ("RALPHX_PARENT_CONVERSATION_ID".to_string(), conversation_id),
    ]);
    if let Some(pipeline_role) = pipeline_role {
        let pipeline_role = pipeline_role.trim();
        if pipeline_role.is_empty() {
            return Err("Memory agent launch requires a non-blank pipeline role".to_string());
        }
        env.insert(
            "RALPHX_PIPELINE_ROLE".to_string(),
            pipeline_role.to_string(),
        );
    }
    let runtime_context = McpRuntimeContext::from_agent_env(&env, working_directory)
        .ok_or_else(|| "Memory agent launch requires a non-blank project ID".to_string())?;

    Ok(MemoryAgentLaunchContext {
        env,
        runtime_context,
    })
}

#[allow(clippy::too_many_arguments)]
fn build_memory_agent_direct_command(
    kind: MemoryAgentKind,
    cli_path: &Path,
    plugin_dir: &Path,
    prompt: &str,
    conversation_id: &ChatConversationId,
    context_type: ChatContextType,
    context_id: &str,
    project_id: &ProjectId,
    working_directory: &Path,
) -> Result<SpawnableCommand, String> {
    let launch = prepare_memory_agent_launch(
        conversation_id,
        context_type,
        context_id,
        project_id,
        working_directory,
        Some(kind.pipeline_role()),
    )?;
    let mut command = build_spawnable_command_with_mcp_runtime_context(
        cli_path,
        plugin_dir,
        prompt,
        Some(kind.agent_name()),
        None,
        working_directory,
        None,
        None,
        Some(&launch.runtime_context),
    )?;
    for (key, value) in &launch.env {
        command.env(key, value);
    }
    Ok(command)
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
    )?;
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

pub(crate) fn build_memory_agent_config(
    kind: MemoryAgentKind,
    runtime: &ResolvedBackgroundAgentRuntime,
    prompt: String,
    conversation_id: &ChatConversationId,
    context_type: ChatContextType,
    context_id: &str,
    project_id: &ProjectId,
    working_directory: &Path,
) -> Result<AgentConfig, String> {
    let harness = runtime.harness.unwrap_or(DEFAULT_AGENT_HARNESS);
    let bootstrap = resolve_harness_agent_bootstrap(
        harness,
        kind.agent_name(),
        working_directory.to_path_buf(),
    );
    let launch = prepare_memory_agent_launch(
        conversation_id,
        context_type,
        context_id,
        project_id,
        working_directory,
        Some(kind.pipeline_role()),
    )?;
    let mut env = runtime.env_with_overrides(bootstrap.env);
    env.extend(launch.env);

    Ok(AgentConfig {
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
        mcp_launch_policy: Default::default(),
    })
}
