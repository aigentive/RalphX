use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::sync::Arc;
use std::time::{Duration, Instant};

use chrono::Utc;
use sha2::{Digest, Sha256};
use tokio::time::sleep;
use tracing::{error, info, warn};

use crate::application::chat_service::{ChatService, SendCallerContext, SendMessageOptions};
use crate::application::git_service::git_cmd::{self, GitCommandLane};
use crate::application::{AppState, GitService};
use crate::domain::entities::{
    AgentConversationWorkspace, AgentRunId, AgentRunStatus, AgentWorkspaceReviewGateStatus,
    AgentWorkspaceReviewMonitor, AgentWorkspaceReviewMonitorStatus, AgentWorkspaceReviewOutcome,
    AgentWorkspaceReviewTargetScope, ChatContextType, ChatConversation, ChatConversationId,
    MessageRole, Project,
};
use crate::domain::services::{
    ComposerArtifactReference, ComposerIntegrationReference, ComposerProjectReference,
};
use crate::error::{AppError, AppResult};
use crate::infrastructure::agents::claude::agent_names;

const WORKSPACE_REVIEWER_TIMEOUT_SECS: u64 = 900;
const WORKSPACE_REVIEW_RUN_POLL_INTERVAL_MS: u64 = 250;
const WORKSPACE_REVIEW_LOG_TARGET: &str = "ralphx_lib::application::agent_workspace_review";
const WORKSPACE_REVIEW_PATCH_EXCERPT_CHARS: usize = 42_000;
const WORKSPACE_REVIEW_MAX_CHANGED_FILES: usize = 120;
const WORKSPACE_REVIEW_MAX_INHERITED_PROJECT_REFERENCES: usize = 8;
const WORKSPACE_REVIEW_MAX_INHERITED_INTEGRATION_REFERENCES: usize = 8;
const WORKSPACE_REVIEW_MAX_INHERITED_ARTIFACT_REFERENCES: usize = 8;

fn compact_log_fingerprint(value: Option<&str>) -> String {
    value
        .map(|value| value.chars().take(12).collect())
        .unwrap_or_else(|| "none".to_string())
}

fn target_scope_label(target: Option<&AgentWorkspaceReviewTarget>) -> String {
    target
        .map(|target| target.scope.to_string())
        .unwrap_or_else(|| "none".to_string())
}

fn target_fingerprint_label(target: Option<&AgentWorkspaceReviewTarget>) -> String {
    compact_log_fingerprint(target.map(|target| target.diff_fingerprint.as_str()))
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentWorkspaceReviewTarget {
    pub scope: AgentWorkspaceReviewTargetScope,
    pub base_ref: String,
    pub base_sha: Option<String>,
    pub head_ref: String,
    pub head_sha: Option<String>,
    pub diff_fingerprint: String,
    pub working_directory: PathBuf,
    pub source_pull_request_number: Option<i64>,
    pub review_packet: AgentWorkspaceReviewPacket,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct AgentWorkspaceReviewPacket {
    pub summary: AgentWorkspaceReviewDiffSummary,
    pub changed_files: Vec<AgentWorkspaceReviewChangedFile>,
    pub patch_excerpt: String,
    pub patch_excerpt_truncated: bool,
    pub notes: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct AgentWorkspaceReviewDiffSummary {
    pub files_changed: u32,
    pub insertions: u32,
    pub deletions: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct AgentWorkspaceReviewChangedFile {
    pub path: String,
    pub status: String,
    pub sources: Vec<String>,
}

#[derive(Debug, Default)]
struct WorkspaceReviewInheritedReferences {
    project_references: Vec<ComposerProjectReference>,
    integration_references: Vec<ComposerIntegrationReference>,
    artifact_references: Vec<ComposerArtifactReference>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentWorkspaceReviewContext {
    pub monitor: AgentWorkspaceReviewMonitor,
    pub target: Option<AgentWorkspaceReviewTarget>,
    pub is_current: bool,
    pub is_outdated: bool,
    pub should_show_tab: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentWorkspaceReviewStart {
    pub context: AgentWorkspaceReviewContext,
    pub started: bool,
    pub skipped_reason: Option<String>,
    pub was_queued: bool,
}

pub async fn load_agent_workspace_review_context(
    state: &AppState,
    workspace: &AgentConversationWorkspace,
) -> AppResult<AgentWorkspaceReviewContext> {
    let started = Instant::now();
    let project = state
        .project_repo
        .get_by_id(&workspace.project_id)
        .await?
        .ok_or_else(|| AppError::NotFound("Project not found".to_string()))?;
    let target = resolve_review_target(workspace, &project).await?;
    let mut monitor = load_or_create_monitor(state, workspace).await?;
    apply_current_target_to_monitor(&mut monitor, target.as_ref());
    if target.is_none() && monitor.status != AgentWorkspaceReviewMonitorStatus::Reviewing {
        monitor.status = AgentWorkspaceReviewMonitorStatus::Idle;
    }
    apply_review_gate_to_monitor(&mut monitor, target.as_ref());
    let monitor = state
        .agent_conversation_workspace_repo
        .upsert_workspace_review_monitor(monitor)
        .await?;
    let context = build_context(workspace, monitor, target);
    let scope = target_scope_label(context.target.as_ref());
    let fingerprint = target_fingerprint_label(context.target.as_ref());
    info!(
        target: WORKSPACE_REVIEW_LOG_TARGET,
        operation = "context",
        conversation_id = %workspace.conversation_id,
        project_id = %workspace.project_id,
        branch = %workspace.branch_name,
        elapsed_ms = started.elapsed().as_millis(),
        monitor_status = %context.monitor.status,
        target_scope = %scope,
        diff_fingerprint = %fingerprint,
        is_current = context.is_current,
        is_outdated = context.is_outdated,
        should_show_tab = context.should_show_tab,
        has_artifact = context.monitor.review_artifact_id.is_some(),
        "Loaded workspace Review context"
    );
    Ok(context)
}

pub async fn start_agent_workspace_review(
    state: Arc<AppState>,
    workspace: &AgentConversationWorkspace,
    force: bool,
) -> AppResult<AgentWorkspaceReviewStart> {
    let chat_service = state.build_chat_service();
    start_agent_workspace_review_with_chat_service(state, workspace, force, &chat_service).await
}

async fn start_agent_workspace_review_with_chat_service<S: ChatService + ?Sized>(
    state: Arc<AppState>,
    workspace: &AgentConversationWorkspace,
    force: bool,
    chat_service: &S,
) -> AppResult<AgentWorkspaceReviewStart> {
    let request_started = Instant::now();
    info!(
        target: WORKSPACE_REVIEW_LOG_TARGET,
        operation = "start_request",
        conversation_id = %workspace.conversation_id,
        project_id = %workspace.project_id,
        branch = %workspace.branch_name,
        force,
        "Received workspace Review start request"
    );
    let project = state
        .project_repo
        .get_by_id(&workspace.project_id)
        .await?
        .ok_or_else(|| AppError::NotFound("Project not found".to_string()))?;
    let target = resolve_review_target(workspace, &project).await?;
    let mut monitor = load_or_create_monitor(&state, workspace).await?;
    apply_current_target_to_monitor(&mut monitor, target.as_ref());
    let target_scope = target_scope_label(target.as_ref());
    let target_fingerprint = target_fingerprint_label(target.as_ref());
    info!(
        target: WORKSPACE_REVIEW_LOG_TARGET,
        operation = "start_target_resolved",
        conversation_id = %workspace.conversation_id,
        project_id = %workspace.project_id,
        branch = %workspace.branch_name,
        elapsed_ms = request_started.elapsed().as_millis(),
        monitor_status = %monitor.status,
        target_scope = %target_scope,
        diff_fingerprint = %target_fingerprint,
        has_artifact = monitor.review_artifact_id.is_some(),
        "Resolved workspace Review start target"
    );

    let Some(target) = target else {
        monitor.status = AgentWorkspaceReviewMonitorStatus::Idle;
        monitor.review_outcome = AgentWorkspaceReviewOutcome::NoChanges;
        monitor.review_gate_status = AgentWorkspaceReviewGateStatus::NotRequired;
        clear_review_blocking_state(&mut monitor);
        monitor.last_error = None;
        let monitor = state
            .agent_conversation_workspace_repo
            .upsert_workspace_review_monitor(monitor)
            .await?;
        info!(
            target: WORKSPACE_REVIEW_LOG_TARGET,
            operation = "start_skipped",
            conversation_id = %workspace.conversation_id,
            project_id = %workspace.project_id,
            branch = %workspace.branch_name,
            skip_reason = "no_reviewable_changes",
            elapsed_ms = request_started.elapsed().as_millis(),
            monitor_status = %monitor.status,
            "Skipped workspace Review start"
        );
        return Ok(AgentWorkspaceReviewStart {
            context: build_context(workspace, monitor, None),
            started: false,
            skipped_reason: Some("no_reviewable_changes".to_string()),
            was_queued: false,
        });
    };

    if !force
        && monitor.has_current_passing_review_for_target(
            target.scope,
            target.head_sha.as_deref(),
            &target.diff_fingerprint,
        )
    {
        monitor.status = AgentWorkspaceReviewMonitorStatus::Ready;
        apply_review_gate_to_monitor(&mut monitor, Some(&target));
        let monitor = state
            .agent_conversation_workspace_repo
            .upsert_workspace_review_monitor(monitor)
            .await?;
        info!(
            target: WORKSPACE_REVIEW_LOG_TARGET,
            operation = "start_skipped",
            conversation_id = %workspace.conversation_id,
            project_id = %workspace.project_id,
            branch = %workspace.branch_name,
            skip_reason = "current",
            elapsed_ms = request_started.elapsed().as_millis(),
            monitor_status = %monitor.status,
            target_scope = %target.scope,
            diff_fingerprint = %compact_log_fingerprint(Some(&target.diff_fingerprint)),
            artifact_id = %monitor.review_artifact_id.as_ref().map(|id| id.as_str()).unwrap_or("none"),
            "Skipped workspace Review start"
        );
        return Ok(AgentWorkspaceReviewStart {
            context: build_context(workspace, monitor, Some(target)),
            started: false,
            skipped_reason: Some("current".to_string()),
            was_queued: false,
        });
    }

    if !force
        && monitor.status == AgentWorkspaceReviewMonitorStatus::Reviewing
        && monitor.current_target_scope == Some(target.scope)
        && monitor.current_diff_fingerprint.as_deref() == Some(target.diff_fingerprint.as_str())
    {
        apply_review_gate_to_monitor(&mut monitor, Some(&target));
        let monitor = state
            .agent_conversation_workspace_repo
            .upsert_workspace_review_monitor(monitor)
            .await?;
        info!(
            target: WORKSPACE_REVIEW_LOG_TARGET,
            operation = "start_skipped",
            conversation_id = %workspace.conversation_id,
            project_id = %workspace.project_id,
            branch = %workspace.branch_name,
            skip_reason = "already_reviewing",
            elapsed_ms = request_started.elapsed().as_millis(),
            monitor_status = %monitor.status,
            target_scope = %target.scope,
            diff_fingerprint = %compact_log_fingerprint(Some(&target.diff_fingerprint)),
            helper_id = %monitor.last_run_id.as_deref().unwrap_or("none"),
            "Skipped workspace Review start"
        );
        return Ok(AgentWorkspaceReviewStart {
            context: build_context(workspace, monitor, Some(target)),
            started: false,
            skipped_reason: Some("already_reviewing".to_string()),
            was_queued: false,
        });
    }

    let conversation = state
        .chat_conversation_repo
        .get_by_id(&workspace.conversation_id)
        .await?
        .ok_or_else(|| AppError::NotFound("Conversation not found".to_string()))?;
    let latest_run = state
        .agent_run_repo
        .get_latest_for_conversation(&workspace.conversation_id)
        .await?;
    let message = build_review_request_message(workspace, &target);
    let inherited_references =
        collect_workspace_review_inherited_references(&state, workspace).await?;
    let runtime = state
        .resolve_workspace_reviewer_runtime(&conversation, latest_run.as_ref())
        .await?;
    let review_conversation_id =
        create_workspace_review_conversation(&state, workspace, &target).await?;
    let runtime_model = runtime
        .model
        .clone()
        .unwrap_or_else(|| "default".to_string());
    let runtime_effort = runtime
        .logical_effort
        .map(|effort| effort.to_string())
        .unwrap_or_else(|| "default".to_string());
    let runtime_approval_policy = runtime
        .approval_policy
        .clone()
        .unwrap_or_else(|| "default".to_string());
    let runtime_sandbox_mode = runtime
        .sandbox_mode
        .clone()
        .unwrap_or_else(|| "default".to_string());
    let review_harness = runtime.harness;
    let latest_run_id = latest_run
        .as_ref()
        .map(|run| run.id.to_string())
        .unwrap_or_else(|| "none".to_string());
    let latest_run_harness = latest_run
        .as_ref()
        .and_then(|run| run.harness)
        .map(|harness| harness.to_string())
        .unwrap_or_else(|| "none".to_string());
    info!(
        target: WORKSPACE_REVIEW_LOG_TARGET,
        operation = "child_chat_runtime_resolved",
        conversation_id = %workspace.conversation_id,
        review_conversation_id = %review_conversation_id,
        project_id = %workspace.project_id,
        branch = %workspace.branch_name,
        elapsed_ms = request_started.elapsed().as_millis(),
        target_scope = %target.scope,
        diff_fingerprint = %compact_log_fingerprint(Some(&target.diff_fingerprint)),
        latest_run_id = %latest_run_id,
        latest_run_harness = %latest_run_harness,
        review_harness = %review_harness
            .map(|harness| harness.to_string())
            .unwrap_or_else(|| "default".to_string()),
        model = %runtime_model,
        logical_effort = %runtime_effort,
        approval_policy = %runtime_approval_policy,
        sandbox_mode = %runtime_sandbox_mode,
        has_cli_override = runtime.cli_path_override.is_some(),
        working_directory = %target.working_directory.display(),
        inherited_project_references = inherited_references.project_references.len(),
        inherited_integration_references = inherited_references.integration_references.len(),
        inherited_artifact_references = inherited_references.artifact_references.len(),
        "Resolved workspace Review child chat runtime"
    );
    let send_started = Instant::now();
    let send_result = match chat_service
        .send_message(
            ChatContextType::Project,
            workspace.project_id.as_str(),
            &message,
            SendMessageOptions {
                conversation_id_override: Some(review_conversation_id.clone()),
                harness_override: runtime.harness,
                agent_name_override: Some(agent_names::AGENT_WORKSPACE_REVIEWER.to_string()),
                model_override: runtime.model,
                working_directory_override: Some(target.working_directory.clone()),
                logical_effort_override: runtime.logical_effort,
                approval_policy_override: runtime.approval_policy,
                sandbox_mode_override: runtime.sandbox_mode,
                service_tier_override: runtime.service_tier,
                composer_project_references: inherited_references.project_references,
                composer_integration_references: inherited_references.integration_references,
                composer_artifact_references: inherited_references.artifact_references,
                force_new_provider_session: true,
                metadata: Some(workspace_review_request_metadata()),
                caller_context: SendCallerContext::UserInitiated,
                ..Default::default()
            },
        )
        .await
    {
        Ok(send_result) => send_result,
        Err(error) => {
            let error = format!("failed to start workspace reviewer chat: {error}");
            monitor.status = AgentWorkspaceReviewMonitorStatus::Blocked;
            monitor.review_outcome = AgentWorkspaceReviewOutcome::RunFailed;
            monitor.review_gate_status = AgentWorkspaceReviewGateStatus::Failed;
            clear_review_blocking_state(&mut monitor);
            monitor.review_conversation_id = Some(review_conversation_id.clone());
            monitor.last_error = Some(error.clone());
            state
                .agent_conversation_workspace_repo
                .upsert_workspace_review_monitor(monitor)
                .await?;
            return Err(AppError::Infrastructure(error));
        }
    };
    info!(
        target: WORKSPACE_REVIEW_LOG_TARGET,
        operation = "child_chat_started",
        conversation_id = %workspace.conversation_id,
        review_conversation_id = %send_result.conversation_id,
        project_id = %workspace.project_id,
        branch = %workspace.branch_name,
        harness = %review_harness
            .map(|harness| harness.to_string())
            .unwrap_or_else(|| "default".to_string()),
        model = %runtime_model,
        logical_effort = %runtime_effort,
        run_id = %send_result.agent_run_id,
        was_queued = send_result.was_queued,
        queued_as_pending = send_result.queued_as_pending,
        elapsed_ms = send_started.elapsed().as_millis(),
        total_elapsed_ms = request_started.elapsed().as_millis(),
        target_scope = %target.scope,
        diff_fingerprint = %compact_log_fingerprint(Some(&target.diff_fingerprint)),
        "Started agent workspace Review child chat"
    );

    monitor.status = AgentWorkspaceReviewMonitorStatus::Reviewing;
    monitor.review_outcome = AgentWorkspaceReviewOutcome::None;
    monitor.review_gate_status = AgentWorkspaceReviewGateStatus::Reviewing;
    clear_review_blocking_state(&mut monitor);
    monitor.review_conversation_id = Some(review_conversation_id.clone());
    monitor.last_run_id = Some(send_result.agent_run_id.clone());
    monitor.last_error = None;
    let monitor = state
        .agent_conversation_workspace_repo
        .upsert_workspace_review_monitor(monitor)
        .await?;
    info!(
        target: WORKSPACE_REVIEW_LOG_TARGET,
        operation = "monitor_reviewing",
        conversation_id = %workspace.conversation_id,
        review_conversation_id = %review_conversation_id,
        project_id = %workspace.project_id,
        branch = %workspace.branch_name,
        run_id = %send_result.agent_run_id,
        monitor_status = %monitor.status,
        elapsed_ms = request_started.elapsed().as_millis(),
        target_scope = %target.scope,
        diff_fingerprint = %compact_log_fingerprint(Some(&target.diff_fingerprint)),
        "Marked workspace Review monitor as reviewing"
    );
    state
        .agent_conversation_workspace_repo
        .append_publication_event(
            crate::domain::entities::AgentConversationWorkspacePublicationEvent::new(
                workspace.conversation_id.clone(),
                "workspace_review",
                "reviewing",
                review_started_summary(&target),
                Some(format!(
                    "workspace_review:{}:{}",
                    target.scope, target.diff_fingerprint
                )),
            ),
        )
        .await?;
    spawn_workspace_review_waiter(
        Arc::clone(&state),
        workspace.clone(),
        target.clone(),
        send_result.agent_run_id.clone(),
    );

    Ok(AgentWorkspaceReviewStart {
        context: build_context(workspace, monitor, Some(target)),
        started: true,
        skipped_reason: None,
        was_queued: send_result.was_queued || send_result.queued_as_pending,
    })
}

async fn create_workspace_review_conversation(
    state: &AppState,
    workspace: &AgentConversationWorkspace,
    target: &AgentWorkspaceReviewTarget,
) -> AppResult<ChatConversationId> {
    let mut conversation = ChatConversation::new_project(workspace.project_id.clone());
    conversation.parent_conversation_id = Some(workspace.conversation_id.as_str());
    conversation.title = Some(workspace_review_conversation_title(target));
    let conversation = state.chat_conversation_repo.create(conversation).await?;
    Ok(conversation.id)
}

fn workspace_review_conversation_title(target: &AgentWorkspaceReviewTarget) -> String {
    match target.scope {
        AgentWorkspaceReviewTargetScope::SelectedSource => {
            if let Some(number) = target.source_pull_request_number {
                format!("Review PR #{number}")
            } else {
                format!("Review {}", target.head_ref)
            }
        }
        AgentWorkspaceReviewTargetScope::WorkspaceDelta => "Review workspace changes".to_string(),
    }
}

fn workspace_review_request_metadata() -> String {
    serde_json::json!({
        "hidden_from_ui": true,
        "source": "workspace_review_request",
    })
    .to_string()
}

async fn collect_workspace_review_inherited_references(
    state: &AppState,
    workspace: &AgentConversationWorkspace,
) -> AppResult<WorkspaceReviewInheritedReferences> {
    let mut inherited = WorkspaceReviewInheritedReferences::default();
    let mut project_seen = BTreeSet::new();
    let mut integration_seen = BTreeSet::new();
    let mut artifact_seen = BTreeSet::new();

    let messages = state
        .chat_message_repo
        .get_by_conversation(&workspace.conversation_id)
        .await?;
    for message in messages {
        if message.role != MessageRole::User {
            continue;
        }
        merge_workspace_review_references_from_metadata(
            message.metadata.as_deref(),
            &mut inherited,
            &mut project_seen,
            &mut integration_seen,
            &mut artifact_seen,
        );
    }

    if let Some(link) = state
        .agent_conversation_jira_issue_repo
        .get_by_conversation_id(&workspace.conversation_id)
        .await?
    {
        push_inherited_integration_reference(
            &mut inherited.integration_references,
            &mut integration_seen,
            crate::application::agent_conversation_jira_issue::assigned_issue_to_composer_reference(
                &link,
            ),
        );
    }
    if let Some(link) = state
        .agent_conversation_linear_issue_repo
        .get_by_conversation_id(&workspace.conversation_id)
        .await?
    {
        push_inherited_integration_reference(
            &mut inherited.integration_references,
            &mut integration_seen,
            crate::application::agent_conversation_linear_issue::assigned_issue_to_composer_reference(
                &link,
            ),
        );
    }
    if let Some(link) = state
        .agent_conversation_granola_note_repo
        .get_by_conversation_id(&workspace.conversation_id)
        .await?
    {
        push_inherited_integration_reference(
            &mut inherited.integration_references,
            &mut integration_seen,
            crate::application::agent_conversation_granola_note::assigned_note_to_composer_reference(
                &link,
            ),
        );
    }

    if let Some(plan_reference) = linked_workspace_plan_artifact_reference(state, workspace).await?
    {
        push_inherited_artifact_reference(
            &mut inherited.artifact_references,
            &mut artifact_seen,
            plan_reference,
        );
    }

    Ok(inherited)
}

fn merge_workspace_review_references_from_metadata(
    metadata: Option<&str>,
    inherited: &mut WorkspaceReviewInheritedReferences,
    project_seen: &mut BTreeSet<String>,
    integration_seen: &mut BTreeSet<String>,
    artifact_seen: &mut BTreeSet<String>,
) {
    let Some(metadata) = metadata else {
        return;
    };
    let Ok(value) = serde_json::from_str::<serde_json::Value>(metadata) else {
        return;
    };
    let Some(object) = value.as_object() else {
        return;
    };

    if let Some(references) = parse_workspace_review_metadata_references::<ComposerProjectReference>(
        object.get("composer_project_references"),
    ) {
        for reference in references {
            push_inherited_project_reference(
                &mut inherited.project_references,
                project_seen,
                reference,
            );
        }
    }
    if let Some(references) = parse_workspace_review_metadata_references::<
        ComposerIntegrationReference,
    >(object.get("composer_integration_references"))
    {
        for reference in references {
            push_inherited_integration_reference(
                &mut inherited.integration_references,
                integration_seen,
                reference,
            );
        }
    }
    if let Some(references) = parse_workspace_review_metadata_references::<ComposerArtifactReference>(
        object.get("composer_artifact_references"),
    ) {
        for reference in references {
            push_inherited_artifact_reference(
                &mut inherited.artifact_references,
                artifact_seen,
                reference,
            );
        }
    }
}

fn parse_workspace_review_metadata_references<T>(
    value: Option<&serde_json::Value>,
) -> Option<Vec<T>>
where
    T: serde::de::DeserializeOwned,
{
    value
        .cloned()
        .and_then(|value| serde_json::from_value::<Vec<T>>(value).ok())
}

async fn linked_workspace_plan_artifact_reference(
    state: &AppState,
    workspace: &AgentConversationWorkspace,
) -> AppResult<Option<ComposerArtifactReference>> {
    let Some(session_id) = workspace.linked_ideation_session_id.as_ref() else {
        return Ok(None);
    };
    let Some(session) = state.ideation_session_repo.get_by_id(session_id).await? else {
        return Ok(None);
    };
    let Some(artifact_id) = session
        .plan_artifact_id
        .clone()
        .or_else(|| session.inherited_plan_artifact_id.clone())
    else {
        return Ok(None);
    };
    let artifact = state.artifact_repo.get_by_id(&artifact_id).await?;
    Ok(Some(ComposerArtifactReference {
        artifact_id: artifact_id.as_str().to_string(),
        kind: "plan".to_string(),
        title: artifact.as_ref().map(|artifact| artifact.name.clone()),
        session_id: Some(session.id.as_str().to_string()),
        version: artifact.as_ref().map(|artifact| artifact.metadata.version),
        status: None,
    }))
}

fn push_inherited_project_reference(
    references: &mut Vec<ComposerProjectReference>,
    seen: &mut BTreeSet<String>,
    reference: ComposerProjectReference,
) {
    if references.len() >= WORKSPACE_REVIEW_MAX_INHERITED_PROJECT_REFERENCES {
        return;
    }
    let key = reference.path.trim();
    if key.is_empty() || !seen.insert(key.to_string()) {
        return;
    }
    references.push(reference);
}

fn push_inherited_integration_reference(
    references: &mut Vec<ComposerIntegrationReference>,
    seen: &mut BTreeSet<String>,
    reference: ComposerIntegrationReference,
) {
    if references.len() >= WORKSPACE_REVIEW_MAX_INHERITED_INTEGRATION_REFERENCES {
        return;
    }
    let key = format!(
        "{}\n{}\n{}\n{}",
        reference.provider.trim(),
        reference.kind.trim(),
        reference.id.trim(),
        reference.key.as_deref().unwrap_or("").trim()
    );
    if key.trim().is_empty() || !seen.insert(key) {
        return;
    }
    references.push(reference);
}

fn push_inherited_artifact_reference(
    references: &mut Vec<ComposerArtifactReference>,
    seen: &mut BTreeSet<String>,
    reference: ComposerArtifactReference,
) {
    if references.len() >= WORKSPACE_REVIEW_MAX_INHERITED_ARTIFACT_REFERENCES {
        return;
    }
    let key = reference.artifact_id.trim();
    if key.is_empty() || !seen.insert(key.to_string()) {
        return;
    }
    references.push(reference);
}

fn spawn_workspace_review_waiter(
    state: Arc<AppState>,
    workspace: AgentConversationWorkspace,
    target: AgentWorkspaceReviewTarget,
    run_id: String,
) {
    tokio::spawn(async move {
        let wait_started = Instant::now();
        let run_entity_id = AgentRunId::from_string(run_id.clone());
        info!(
            target: WORKSPACE_REVIEW_LOG_TARGET,
            operation = "child_chat_wait_started",
            conversation_id = %workspace.conversation_id,
            project_id = %workspace.project_id,
            branch = %workspace.branch_name,
            run_id = %run_id,
            timeout_secs = WORKSPACE_REVIEWER_TIMEOUT_SECS,
            target_scope = %target.scope,
            diff_fingerprint = %compact_log_fingerprint(Some(&target.diff_fingerprint)),
            "Waiting for workspace Review child chat completion"
        );

        loop {
            if wait_started.elapsed() >= Duration::from_secs(WORKSPACE_REVIEWER_TIMEOUT_SECS) {
                mark_workspace_review_blocked(
                    &state,
                    &workspace,
                    &target,
                    &run_id,
                    "Workspace reviewer timed out before producing a current Review".to_string(),
                )
                .await;
                return;
            }

            let run = match state.agent_run_repo.get_by_id(&run_entity_id).await {
                Ok(Some(run)) => run,
                Ok(None) => {
                    mark_workspace_review_blocked(
                        &state,
                        &workspace,
                        &target,
                        &run_id,
                        "Workspace reviewer run disappeared before completion".to_string(),
                    )
                    .await;
                    return;
                }
                Err(error) => {
                    warn!(
                        target: WORKSPACE_REVIEW_LOG_TARGET,
                        operation = "child_chat_run_poll_failed",
                        conversation_id = %workspace.conversation_id,
                        run_id = %run_id,
                        error = %error,
                        elapsed_ms = wait_started.elapsed().as_millis(),
                        "Failed to poll workspace Review child chat run"
                    );
                    sleep(Duration::from_millis(WORKSPACE_REVIEW_RUN_POLL_INTERVAL_MS)).await;
                    continue;
                }
            };

            if run.status == AgentRunStatus::Running {
                sleep(Duration::from_millis(WORKSPACE_REVIEW_RUN_POLL_INTERVAL_MS)).await;
                continue;
            }

            info!(
                target: WORKSPACE_REVIEW_LOG_TARGET,
                operation = "child_chat_completed",
                conversation_id = %workspace.conversation_id,
                project_id = %workspace.project_id,
                branch = %workspace.branch_name,
                run_id = %run_id,
                elapsed_ms = wait_started.elapsed().as_millis(),
                run_status = %run.status,
                target_scope = %target.scope,
                diff_fingerprint = %compact_log_fingerprint(Some(&target.diff_fingerprint)),
                "Workspace Review child chat reached a terminal state"
            );

            if run.status != AgentRunStatus::Completed {
                let error = run.error_message.unwrap_or_else(|| {
                    format!("Workspace reviewer ended with status {}", run.status)
                });
                mark_workspace_review_blocked(&state, &workspace, &target, &run_id, error).await;
                return;
            }

            match state
                .agent_conversation_workspace_repo
                .get_workspace_review_monitor(&workspace.conversation_id)
                .await
            {
                Ok(Some(monitor))
                    if monitor.is_current_for_target(
                        target.scope,
                        target.head_sha.as_deref(),
                        &target.diff_fingerprint,
                    ) && monitor.review_artifact_id.is_some()
                        && matches!(
                            monitor.review_outcome,
                            AgentWorkspaceReviewOutcome::Passed
                                | AgentWorkspaceReviewOutcome::Blocking
                        ) =>
                {
                    info!(
                        target: WORKSPACE_REVIEW_LOG_TARGET,
                        operation = "child_chat_artifact_verified",
                        conversation_id = %workspace.conversation_id,
                        project_id = %workspace.project_id,
                        branch = %workspace.branch_name,
                        run_id = %run_id,
                        elapsed_ms = wait_started.elapsed().as_millis(),
                        monitor_status = %monitor.status,
                        review_outcome = %monitor.review_outcome,
                        review_gate_status = %monitor.review_gate_status,
                        artifact_id = %monitor.review_artifact_id.as_ref().map(|id| id.as_str()).unwrap_or("none"),
                        artifact_version = monitor.review_artifact_version.unwrap_or_default(),
                        target_scope = %target.scope,
                        diff_fingerprint = %compact_log_fingerprint(Some(&target.diff_fingerprint)),
                        "Verified workspace Review after child chat completion"
                    );
                }
                Ok(_) => {
                    warn!(
                        target: WORKSPACE_REVIEW_LOG_TARGET,
                        operation = "child_chat_missing_review",
                        conversation_id = %workspace.conversation_id,
                        project_id = %workspace.project_id,
                        branch = %workspace.branch_name,
                        run_id = %run_id,
                        elapsed_ms = wait_started.elapsed().as_millis(),
                        target_scope = %target.scope,
                        diff_fingerprint = %compact_log_fingerprint(Some(&target.diff_fingerprint)),
                        "Workspace reviewer child chat completed without writing a current Review"
                    );
                    mark_workspace_review_blocked(
                        &state,
                        &workspace,
                        &target,
                        &run_id,
                        "Workspace reviewer completed without writing a current Review".to_string(),
                    )
                    .await;
                }
                Err(error) => {
                    error!(
                        target: WORKSPACE_REVIEW_LOG_TARGET,
                        operation = "child_chat_verify_failed",
                        conversation_id = %workspace.conversation_id,
                        project_id = %workspace.project_id,
                        branch = %workspace.branch_name,
                        run_id = %run_id,
                        error = %error,
                        elapsed_ms = wait_started.elapsed().as_millis(),
                        target_scope = %target.scope,
                        diff_fingerprint = %compact_log_fingerprint(Some(&target.diff_fingerprint)),
                        "Failed to verify workspace Review child chat completion"
                    );
                }
            }
            return;
        }
    });
}

async fn mark_workspace_review_blocked(
    state: &AppState,
    workspace: &AgentConversationWorkspace,
    target: &AgentWorkspaceReviewTarget,
    helper_id: &str,
    error: String,
) {
    match load_or_create_monitor(state, workspace).await {
        Ok(mut monitor) => {
            if !workspace_review_block_matches_active_monitor(&monitor, target, helper_id) {
                warn!(
                    target: WORKSPACE_REVIEW_LOG_TARGET,
                    operation = "child_chat_blocked_stale_ignored",
                    conversation_id = %workspace.conversation_id,
                    project_id = %workspace.project_id,
                    branch = %workspace.branch_name,
                    helper_id,
                    monitor_run_id = %monitor.last_run_id.as_deref().unwrap_or("none"),
                    monitor_target_scope = %monitor
                        .current_target_scope
                        .map(|scope| scope.to_string())
                        .unwrap_or_else(|| "none".to_string()),
                    monitor_diff_fingerprint = %compact_log_fingerprint(
                        monitor.current_diff_fingerprint.as_deref(),
                    ),
                    target_scope = %target.scope,
                    diff_fingerprint = %compact_log_fingerprint(Some(&target.diff_fingerprint)),
                    "Ignored stale workspace Review child chat failure"
                );
                return;
            }
            error!(
                target: WORKSPACE_REVIEW_LOG_TARGET,
                operation = "child_chat_blocked",
                conversation_id = %workspace.conversation_id,
                project_id = %workspace.project_id,
                branch = %workspace.branch_name,
                helper_id,
                target_scope = %target.scope,
                diff_fingerprint = %compact_log_fingerprint(Some(&target.diff_fingerprint)),
                error = %error,
                "Workspace Review child chat failed"
            );
            apply_current_target_to_monitor(&mut monitor, Some(target));
            monitor.status = AgentWorkspaceReviewMonitorStatus::Blocked;
            monitor.review_outcome = AgentWorkspaceReviewOutcome::RunFailed;
            monitor.review_gate_status = AgentWorkspaceReviewGateStatus::Failed;
            clear_review_blocking_state(&mut monitor);
            monitor.last_run_id = Some(helper_id.to_string());
            monitor.last_error = Some(error);
            if let Err(error) = state
                .agent_conversation_workspace_repo
                .upsert_workspace_review_monitor(monitor)
                .await
            {
                warn!(
                    target: WORKSPACE_REVIEW_LOG_TARGET,
                    operation = "child_chat_blocked_persist_failed",
                    conversation_id = %workspace.conversation_id,
                    helper_id,
                    error = %error,
                    "Failed to persist blocked workspace Review monitor"
                );
            }
        }
        Err(load_error) => {
            warn!(
                target: WORKSPACE_REVIEW_LOG_TARGET,
                operation = "child_chat_blocked_monitor_load_failed",
                conversation_id = %workspace.conversation_id,
                helper_id,
                error = %load_error,
                "Failed to load workspace Review monitor for blocked child chat"
            );
        }
    }
}

fn workspace_review_block_matches_active_monitor(
    monitor: &AgentWorkspaceReviewMonitor,
    target: &AgentWorkspaceReviewTarget,
    helper_id: &str,
) -> bool {
    let run_matches = match monitor.last_run_id.as_deref() {
        Some(last_run_id) => last_run_id == helper_id,
        None => true,
    };
    let target_matches = match (
        monitor.current_target_scope,
        monitor.current_diff_fingerprint.as_deref(),
    ) {
        (Some(scope), Some(fingerprint)) => {
            scope == target.scope && fingerprint == target.diff_fingerprint
        }
        _ => true,
    };
    run_matches && target_matches
}

pub async fn complete_agent_workspace_review_run(
    state: &AppState,
    workspace: &AgentConversationWorkspace,
    outcome: Option<String>,
    summary: Option<String>,
    blocker: Option<String>,
    created_by_run_id: Option<String>,
) -> AppResult<AgentWorkspaceReviewMonitor> {
    let started = Instant::now();
    let normalized_outcome = outcome
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string);
    let summary = summary
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty());
    let blocker = blocker
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty());
    let has_outcome = outcome
        .as_ref()
        .is_some_and(|value| !value.trim().is_empty());
    let has_blocker = blocker.is_some();
    let created_by_run_id_label = created_by_run_id.as_deref().unwrap_or("none").to_string();
    let target = resolve_review_target(
        workspace,
        &state
            .project_repo
            .get_by_id(&workspace.project_id)
            .await?
            .ok_or_else(|| AppError::NotFound("Project not found".to_string()))?,
    )
    .await?;
    let mut monitor = load_or_create_monitor(state, workspace).await?;
    apply_current_target_to_monitor(&mut monitor, target.as_ref());
    monitor.last_run_id = created_by_run_id.or(monitor.last_run_id);
    let parsed_outcome = normalized_outcome
        .as_deref()
        .and_then(|value| AgentWorkspaceReviewOutcome::from_str(value).ok())
        .unwrap_or_else(|| {
            if target.is_none() {
                AgentWorkspaceReviewOutcome::NoChanges
            } else {
                AgentWorkspaceReviewOutcome::RunFailed
            }
        });
    let artifact_current = target.as_ref().is_some_and(|target| {
        monitor.is_current_for_target(
            target.scope,
            target.head_sha.as_deref(),
            &target.diff_fingerprint,
        ) && monitor.review_artifact_id.is_some()
    });
    let previous_blocking_fingerprint = monitor.review_blocking_fingerprint.clone();
    let previous_fixer_status = monitor.review_fixer_status.clone();

    match parsed_outcome {
        AgentWorkspaceReviewOutcome::Passed if artifact_current => {
            monitor.status = AgentWorkspaceReviewMonitorStatus::Ready;
            monitor.review_outcome = AgentWorkspaceReviewOutcome::Passed;
            monitor.last_error = None;
            clear_review_blocking_state(&mut monitor);
        }
        AgentWorkspaceReviewOutcome::Blocking if artifact_current => {
            let blocking_summary = blocker.or(summary).ok_or_else(|| {
                AppError::Validation(
                    "blocking workspace Review completion requires a summary or blocker"
                        .to_string(),
                )
            })?;
            let blocking_fingerprint = target
                .as_ref()
                .map(|target| workspace_review_blocking_fingerprint(target, &blocking_summary));
            let is_new_blocking_fingerprint =
                previous_blocking_fingerprint.as_deref() != blocking_fingerprint.as_deref();
            let should_route_fixer = blocking_fingerprint.is_some()
                && (is_new_blocking_fingerprint || previous_fixer_status.is_none());
            monitor.status = AgentWorkspaceReviewMonitorStatus::Ready;
            monitor.review_outcome = AgentWorkspaceReviewOutcome::Blocking;
            monitor.review_blocking_fingerprint = blocking_fingerprint;
            monitor.review_blocking_summary = Some(blocking_summary);
            monitor.last_error = None;
            if should_route_fixer {
                monitor.review_fixer_status = Some("routing".to_string());
                monitor.review_fixer_run_id = None;
                monitor.review_fixer_conversation_id = None;
            }
        }
        AgentWorkspaceReviewOutcome::NoChanges if target.is_none() => {
            monitor.status = AgentWorkspaceReviewMonitorStatus::Idle;
            monitor.review_outcome = AgentWorkspaceReviewOutcome::NoChanges;
            monitor.last_error = None;
            clear_review_blocking_state(&mut monitor);
        }
        AgentWorkspaceReviewOutcome::RunFailed | AgentWorkspaceReviewOutcome::None => {
            monitor.status = AgentWorkspaceReviewMonitorStatus::Blocked;
            monitor.review_outcome = AgentWorkspaceReviewOutcome::RunFailed;
            monitor.last_error = blocker.or(summary).or(normalized_outcome).or_else(|| {
                Some("Workspace reviewer did not produce a passing Review".to_string())
            });
            clear_review_blocking_state(&mut monitor);
        }
        AgentWorkspaceReviewOutcome::Passed
        | AgentWorkspaceReviewOutcome::Blocking
        | AgentWorkspaceReviewOutcome::NoChanges => {
            monitor.status = AgentWorkspaceReviewMonitorStatus::Blocked;
            monitor.review_outcome = AgentWorkspaceReviewOutcome::RunFailed;
            monitor.last_error = Some(
                "Workspace reviewer completion did not match the current Review target".to_string(),
            );
            clear_review_blocking_state(&mut monitor);
        }
    }
    apply_review_gate_to_monitor(&mut monitor, target.as_ref());
    let mut monitor = state
        .agent_conversation_workspace_repo
        .upsert_workspace_review_monitor(monitor)
        .await?;
    if monitor.review_outcome == AgentWorkspaceReviewOutcome::Blocking
        && monitor.review_fixer_status.as_deref() == Some("routing")
    {
        monitor =
            route_workspace_review_blocking_fixer(state, workspace, &monitor, target.as_ref())
                .await?;
    }
    let scope = target_scope_label(target.as_ref());
    let fingerprint = target_fingerprint_label(target.as_ref());
    info!(
        target: WORKSPACE_REVIEW_LOG_TARGET,
        operation = "complete_tool",
        conversation_id = %workspace.conversation_id,
        project_id = %workspace.project_id,
        branch = %workspace.branch_name,
        elapsed_ms = started.elapsed().as_millis(),
        monitor_status = %monitor.status,
        review_outcome = %monitor.review_outcome,
        review_gate_status = %monitor.review_gate_status,
        review_fixer_status = %monitor.review_fixer_status.as_deref().unwrap_or("none"),
        review_fixer_run_id = %monitor.review_fixer_run_id.as_deref().unwrap_or("none"),
        target_scope = %scope,
        diff_fingerprint = %fingerprint,
        has_artifact = monitor.review_artifact_id.is_some(),
        artifact_id = %monitor.review_artifact_id.as_ref().map(|id| id.as_str()).unwrap_or("none"),
        created_by_run_id = %created_by_run_id_label,
        has_outcome,
        has_blocker,
        "Completed workspace Review run"
    );
    Ok(monitor)
}

fn workspace_review_blocking_fingerprint(
    target: &AgentWorkspaceReviewTarget,
    blocking_summary: &str,
) -> String {
    let mut hasher = Sha256::new();
    hasher.update(target.scope.to_string().as_bytes());
    hasher.update(b":");
    hasher.update(target.diff_fingerprint.as_bytes());
    hasher.update(b":");
    hasher.update(blocking_summary.trim().as_bytes());
    format!("{:x}", hasher.finalize())
}

async fn route_workspace_review_blocking_fixer(
    state: &AppState,
    workspace: &AgentConversationWorkspace,
    monitor: &AgentWorkspaceReviewMonitor,
    target: Option<&AgentWorkspaceReviewTarget>,
) -> AppResult<AgentWorkspaceReviewMonitor> {
    let Some(target) = target else {
        return Ok(monitor.clone());
    };
    let Some(blocking_summary) = monitor.review_blocking_summary.as_deref() else {
        return Ok(monitor.clone());
    };
    let message = build_workspace_review_blocking_repair_message(workspace, monitor, target);
    let conversation = state
        .chat_conversation_repo
        .get_by_id(&workspace.conversation_id)
        .await?
        .ok_or_else(|| AppError::NotFound("Conversation not found".to_string()))?;
    let latest_run = state
        .agent_run_repo
        .get_latest_for_conversation(&workspace.conversation_id)
        .await?;
    let harness_override = conversation
        .provider_session_ref()
        .map(|session_ref| session_ref.harness)
        .or_else(|| latest_run.as_ref().and_then(|run| run.harness));
    let model_override = latest_run.as_ref().and_then(|run| {
        run.logical_model
            .clone()
            .or_else(|| run.effective_model_id.clone())
    });
    let logical_effort_override = latest_run.as_ref().and_then(|run| run.logical_effort);
    let chat_service = state.build_chat_service();
    let mut next = monitor.clone();
    match chat_service
        .send_message(
            ChatContextType::Project,
            workspace.project_id.as_str(),
            &message,
            SendMessageOptions {
                conversation_id_override: Some(workspace.conversation_id.clone()),
                agent_name_override: Some(agent_names::AGENT_WORKSPACE_REPAIR.to_string()),
                harness_override,
                model_override,
                logical_effort_override,
                working_directory_override: Some(target.working_directory.clone()),
                force_new_provider_session: true,
                preserve_conversation_provider_session_ref: true,
                metadata: Some(workspace_review_fixer_request_metadata(
                    monitor.review_blocking_fingerprint.as_deref(),
                )),
                caller_context: SendCallerContext::UserInitiated,
                ..Default::default()
            },
        )
        .await
    {
        Ok(result) => {
            next.review_fixer_status = Some(if result.was_queued || result.queued_as_pending {
                "queued".to_string()
            } else {
                "running".to_string()
            });
            next.review_fixer_run_id = if result.agent_run_id.trim().is_empty() {
                None
            } else {
                Some(result.agent_run_id)
            };
            next.review_fixer_conversation_id =
                Some(ChatConversationId::from_string(result.conversation_id));
            info!(
                target: WORKSPACE_REVIEW_LOG_TARGET,
                operation = "blocking_fixer_sent",
                conversation_id = %workspace.conversation_id,
                project_id = %workspace.project_id,
                branch = %workspace.branch_name,
                target_scope = %target.scope,
                diff_fingerprint = %compact_log_fingerprint(Some(&target.diff_fingerprint)),
                review_artifact_id = %monitor.review_artifact_id.as_ref().map(|id| id.as_str()).unwrap_or("none"),
                blocking_fingerprint = %monitor.review_blocking_fingerprint.as_deref().unwrap_or("none"),
                fixer_run_id = %next.review_fixer_run_id.as_deref().unwrap_or("none"),
                fixer_status = %next.review_fixer_status.as_deref().unwrap_or("none"),
                "Routed blocking workspace Review findings to parent workspace fixer"
            );
        }
        Err(error) => {
            next.review_fixer_status = Some("failed".to_string());
            next.last_error = Some(format!("Failed to route Review fixer: {error}"));
            warn!(
                target: WORKSPACE_REVIEW_LOG_TARGET,
                operation = "blocking_fixer_send_failed",
                conversation_id = %workspace.conversation_id,
                project_id = %workspace.project_id,
                branch = %workspace.branch_name,
                target_scope = %target.scope,
                diff_fingerprint = %compact_log_fingerprint(Some(&target.diff_fingerprint)),
                blocking_fingerprint = %monitor.review_blocking_fingerprint.as_deref().unwrap_or("none"),
                blocking_summary,
                error = %error,
                "Failed to route blocking workspace Review findings to parent workspace fixer"
            );
        }
    }
    state
        .agent_conversation_workspace_repo
        .upsert_workspace_review_monitor(next)
        .await
}

fn workspace_review_fixer_request_metadata(blocking_fingerprint: Option<&str>) -> String {
    let fingerprint_json = blocking_fingerprint
        .map(|value| format!("\"{}\"", value.replace('"', "\\\"")))
        .unwrap_or_else(|| "null".to_string());
    format!(
        "{{\"source\":\"workspace_review_blocking_fixer\",\"blocking_fingerprint\":{fingerprint_json}}}"
    )
}

fn build_workspace_review_blocking_repair_message(
    workspace: &AgentConversationWorkspace,
    monitor: &AgentWorkspaceReviewMonitor,
    target: &AgentWorkspaceReviewTarget,
) -> String {
    let artifact = match (
        monitor.review_artifact_id.as_ref(),
        monitor.review_artifact_version,
    ) {
        (Some(id), Some(version)) => format!("{} v{}", id.as_str(), version),
        (Some(id), None) => id.as_str().to_string(),
        _ => "not recorded".to_string(),
    };
    [
        "Workspace Review found blocking issues for this agent workspace.".to_string(),
        String::new(),
        "Please fix the workspace changes described by the Review. After the repair is complete, continue normally; RalphX will run a fresh local workspace Review before publishing can proceed.".to_string(),
        String::new(),
        format!("Conversation ID: {}", workspace.conversation_id),
        format!("Workspace branch: {}", workspace.branch_name),
        format!("Review artifact: {artifact}"),
        format!("Review target scope: {}", target.scope),
        format!("Review diff fingerprint: {}", target.diff_fingerprint),
        format!(
            "Review child conversation: {}",
            monitor
                .review_conversation_id
                .as_ref()
                .map(ChatConversationId::as_str)
                .unwrap_or_else(|| "not recorded".to_string())
        ),
        format!(
            "Review run ID: {}",
            monitor.last_run_id.as_deref().unwrap_or("not recorded")
        ),
        String::new(),
        "Blocking Review summary:".to_string(),
        monitor
            .review_blocking_summary
            .as_deref()
            .unwrap_or("The reviewer reported blocking issues without a summary.")
            .to_string(),
    ]
    .join("\n")
}

pub async fn load_or_create_monitor(
    state: &AppState,
    workspace: &AgentConversationWorkspace,
) -> AppResult<AgentWorkspaceReviewMonitor> {
    if let Some(monitor) = state
        .agent_conversation_workspace_repo
        .get_workspace_review_monitor(&workspace.conversation_id)
        .await?
    {
        return Ok(monitor);
    }
    Ok(AgentWorkspaceReviewMonitor::new(
        workspace.conversation_id.clone(),
        workspace.project_id.clone(),
    ))
}

pub fn apply_review_artifact_to_monitor(
    monitor: &mut AgentWorkspaceReviewMonitor,
    target_scope: AgentWorkspaceReviewTargetScope,
    target_head_sha: Option<String>,
    target_diff_fingerprint: String,
    created_by_run_id: Option<String>,
    artifact_id: crate::domain::entities::ArtifactId,
    artifact_version: u32,
    artifact_created_at: chrono::DateTime<Utc>,
    previous_artifact_id: Option<crate::domain::entities::ArtifactId>,
) {
    if monitor.status != AgentWorkspaceReviewMonitorStatus::Reviewing {
        monitor.status = AgentWorkspaceReviewMonitorStatus::Ready;
    }
    monitor.review_outcome = AgentWorkspaceReviewOutcome::None;
    if monitor.status == AgentWorkspaceReviewMonitorStatus::Reviewing {
        monitor.review_gate_status = AgentWorkspaceReviewGateStatus::Reviewing;
    } else {
        monitor.review_gate_status = AgentWorkspaceReviewGateStatus::Required;
    }
    monitor.reviewed_target_scope = Some(target_scope);
    monitor.reviewed_head_sha = target_head_sha;
    monitor.reviewed_diff_fingerprint = Some(target_diff_fingerprint.clone());
    monitor.current_target_scope = Some(target_scope);
    monitor.current_diff_fingerprint = Some(target_diff_fingerprint);
    monitor.review_artifact_id = Some(artifact_id);
    monitor.review_artifact_version = Some(artifact_version);
    monitor.review_artifact_updated_at = Some(artifact_created_at);
    monitor.previous_version_id = previous_artifact_id;
    clear_review_blocking_state(monitor);
    monitor.last_run_id = created_by_run_id.or(monitor.last_run_id.take());
    monitor.last_error = None;
}

fn build_context(
    workspace: &AgentConversationWorkspace,
    mut monitor: AgentWorkspaceReviewMonitor,
    target: Option<AgentWorkspaceReviewTarget>,
) -> AgentWorkspaceReviewContext {
    apply_review_gate_to_monitor(&mut monitor, target.as_ref());
    let is_current = target.as_ref().is_some_and(|target| {
        monitor.is_current_for_target(
            target.scope,
            target.head_sha.as_deref(),
            &target.diff_fingerprint,
        ) && monitor.review_artifact_id.is_some()
    });
    let is_outdated = monitor.review_artifact_id.is_some() && target.is_some() && !is_current;
    let should_show_tab = target.is_some() || monitor.review_artifact_id.is_some();
    let should_show_tab = should_show_tab
        && matches!(
            workspace.mode,
            crate::domain::entities::AgentConversationWorkspaceMode::Edit
                | crate::domain::entities::AgentConversationWorkspaceMode::Ideation
                | crate::domain::entities::AgentConversationWorkspaceMode::Plan
                | crate::domain::entities::AgentConversationWorkspaceMode::ReviewPr
        );
    AgentWorkspaceReviewContext {
        monitor,
        target,
        is_current,
        is_outdated,
        should_show_tab,
    }
}

pub fn review_gate_allows_publish(status: AgentWorkspaceReviewGateStatus) -> bool {
    matches!(
        status,
        AgentWorkspaceReviewGateStatus::NotRequired | AgentWorkspaceReviewGateStatus::Passed
    )
}

pub fn review_gate_publish_blocker(context: &AgentWorkspaceReviewContext) -> Option<String> {
    match context.monitor.review_gate_status {
        AgentWorkspaceReviewGateStatus::NotRequired | AgentWorkspaceReviewGateStatus::Passed => {
            None
        }
        AgentWorkspaceReviewGateStatus::Required => {
            Some("Workspace Review is required before publishing".to_string())
        }
        AgentWorkspaceReviewGateStatus::Reviewing => {
            Some("Workspace Review is still running".to_string())
        }
        AgentWorkspaceReviewGateStatus::Blocking => Some(
            context
                .monitor
                .review_blocking_summary
                .clone()
                .unwrap_or_else(|| "Workspace Review found blocking changes".to_string()),
        ),
        AgentWorkspaceReviewGateStatus::Failed => {
            Some(
                context.monitor.last_error.clone().unwrap_or_else(|| {
                    "Workspace Review failed; retry before publishing".to_string()
                }),
            )
        }
    }
}

pub async fn load_workspace_review_publish_blocker(
    state: &AppState,
    workspace: &AgentConversationWorkspace,
) -> AppResult<Option<String>> {
    let context = load_agent_workspace_review_context(state, workspace).await?;
    Ok(review_gate_publish_blocker(&context))
}

fn apply_review_gate_to_monitor(
    monitor: &mut AgentWorkspaceReviewMonitor,
    target: Option<&AgentWorkspaceReviewTarget>,
) {
    let Some(target) = target else {
        monitor.review_gate_status = AgentWorkspaceReviewGateStatus::NotRequired;
        if monitor.status != AgentWorkspaceReviewMonitorStatus::Reviewing {
            monitor.review_outcome = AgentWorkspaceReviewOutcome::NoChanges;
            monitor.review_blocking_summary = None;
            monitor.review_blocking_fingerprint = None;
            monitor.review_fixer_run_id = None;
            monitor.review_fixer_conversation_id = None;
            monitor.review_fixer_status = None;
        }
        return;
    };

    let current_target_matches = monitor.current_target_scope == Some(target.scope)
        && monitor.current_diff_fingerprint.as_deref() == Some(target.diff_fingerprint.as_str());
    let artifact_current = monitor.is_current_for_target(
        target.scope,
        target.head_sha.as_deref(),
        &target.diff_fingerprint,
    ) && monitor.review_artifact_id.is_some();

    monitor.review_gate_status = if monitor.status == AgentWorkspaceReviewMonitorStatus::Reviewing
        && current_target_matches
    {
        AgentWorkspaceReviewGateStatus::Reviewing
    } else if monitor.status == AgentWorkspaceReviewMonitorStatus::Blocked && current_target_matches
    {
        AgentWorkspaceReviewGateStatus::Failed
    } else if artifact_current && monitor.review_outcome == AgentWorkspaceReviewOutcome::Passed {
        AgentWorkspaceReviewGateStatus::Passed
    } else if artifact_current && monitor.review_outcome == AgentWorkspaceReviewOutcome::Blocking {
        AgentWorkspaceReviewGateStatus::Blocking
    } else if current_target_matches
        && monitor.review_outcome == AgentWorkspaceReviewOutcome::RunFailed
    {
        AgentWorkspaceReviewGateStatus::Failed
    } else {
        AgentWorkspaceReviewGateStatus::Required
    };
}

fn clear_review_blocking_state(monitor: &mut AgentWorkspaceReviewMonitor) {
    monitor.review_blocking_summary = None;
    monitor.review_blocking_fingerprint = None;
    monitor.review_fixer_run_id = None;
    monitor.review_fixer_conversation_id = None;
    monitor.review_fixer_status = None;
}

fn apply_current_target_to_monitor(
    monitor: &mut AgentWorkspaceReviewMonitor,
    target: Option<&AgentWorkspaceReviewTarget>,
) {
    let now = Utc::now();
    monitor.updated_at = now;
    let Some(target) = target else {
        monitor.current_target_scope = None;
        monitor.current_diff_fingerprint = None;
        return;
    };
    monitor.current_target_scope = Some(target.scope);
    monitor.current_diff_fingerprint = Some(target.diff_fingerprint.clone());
    match target.scope {
        AgentWorkspaceReviewTargetScope::SelectedSource => {
            monitor.selected_source_base_ref = Some(target.base_ref.clone());
            monitor.selected_source_base_sha = target.base_sha.clone();
            monitor.selected_source_head_ref = Some(target.head_ref.clone());
            monitor.selected_source_head_sha = target.head_sha.clone();
            monitor.selected_source_pull_request_number = target.source_pull_request_number;
        }
        AgentWorkspaceReviewTargetScope::WorkspaceDelta => {
            monitor.workspace_base_ref = Some(target.base_ref.clone());
            monitor.workspace_base_sha = target.base_sha.clone();
            monitor.workspace_head_ref = Some(target.head_ref.clone());
            monitor.workspace_head_sha = target.head_sha.clone();
        }
    }
}

#[derive(Debug, Default)]
struct ChangedFileAccumulator {
    status: String,
    sources: BTreeSet<String>,
}

fn build_selected_source_review_packet(diff: &str) -> AgentWorkspaceReviewPacket {
    build_review_packet(
        &[("selected_source diff", diff)],
        None,
        &[("selected_source", diff)],
    )
}

fn build_workspace_delta_review_packet(
    committed_diff: &str,
    staged_diff: &str,
    unstaged_diff: &str,
    status: &str,
) -> AgentWorkspaceReviewPacket {
    build_review_packet(
        &[
            ("committed diff", committed_diff),
            ("staged diff", staged_diff),
            ("unstaged diff", unstaged_diff),
        ],
        Some(status),
        &[
            ("committed", committed_diff),
            ("staged", staged_diff),
            ("unstaged", unstaged_diff),
        ],
    )
}

fn build_review_packet(
    patch_sections: &[(&str, &str)],
    status: Option<&str>,
    diff_sources: &[(&str, &str)],
) -> AgentWorkspaceReviewPacket {
    let mut files = BTreeMap::<String, ChangedFileAccumulator>::new();
    let mut insertions = 0u32;
    let mut deletions = 0u32;

    for (source, diff) in diff_sources {
        let (added, removed) = diff_line_counts(diff);
        insertions = insertions.saturating_add(added);
        deletions = deletions.saturating_add(removed);
        collect_diff_changed_files(diff, source, &mut files);
    }
    if let Some(status) = status {
        collect_status_changed_files(status, &mut files);
    }

    let files_count = files.len();
    let mut notes = Vec::new();
    if files.values().any(|entry| entry.status == "untracked") {
        notes.push(
            "Untracked files are listed from git status; read them with fs_read_file when they are relevant because they are not present in git diff output."
                .to_string(),
        );
    }
    if files_count > WORKSPACE_REVIEW_MAX_CHANGED_FILES {
        notes.push(format!(
            "Changed file list is limited to the first {WORKSPACE_REVIEW_MAX_CHANGED_FILES} paths."
        ));
    }

    let changed_files = files
        .into_iter()
        .take(WORKSPACE_REVIEW_MAX_CHANGED_FILES)
        .map(|(path, entry)| AgentWorkspaceReviewChangedFile {
            path,
            status: entry.status,
            sources: entry.sources.into_iter().collect(),
        })
        .collect::<Vec<_>>();
    let (patch_excerpt, patch_excerpt_truncated) = build_patch_excerpt(patch_sections, status);
    if patch_excerpt_truncated {
        notes.push(format!(
            "Patch excerpt is limited to {WORKSPACE_REVIEW_PATCH_EXCERPT_CHARS} characters; inspect listed files with read-only filesystem tools only when needed."
        ));
    }

    AgentWorkspaceReviewPacket {
        summary: AgentWorkspaceReviewDiffSummary {
            files_changed: files_count as u32,
            insertions,
            deletions,
        },
        changed_files,
        patch_excerpt,
        patch_excerpt_truncated,
        notes,
    }
}

fn collect_diff_changed_files(
    diff: &str,
    source: &str,
    files: &mut BTreeMap<String, ChangedFileAccumulator>,
) {
    let mut current_path: Option<String> = None;
    for line in diff.lines() {
        if let Some(path) = parse_diff_git_new_path(line) {
            add_changed_file(files, &path, "modified", source);
            current_path = Some(path);
            continue;
        }
        let Some(path) = current_path.as_deref() else {
            continue;
        };
        if line.starts_with("new file mode ") {
            add_changed_file(files, path, "added", source);
        } else if line.starts_with("deleted file mode ") {
            add_changed_file(files, path, "deleted", source);
        } else if let Some(renamed_to) = line.strip_prefix("rename to ") {
            let renamed_to = clean_git_path(renamed_to);
            add_changed_file(files, &renamed_to, "renamed", source);
            current_path = Some(renamed_to);
        }
    }
}

fn collect_status_changed_files(
    status: &str,
    files: &mut BTreeMap<String, ChangedFileAccumulator>,
) {
    for line in status.lines() {
        let Some((code, path)) = parse_status_line(line) else {
            continue;
        };
        let status = if code == "??" {
            "untracked"
        } else if code.contains('D') {
            "deleted"
        } else if code.contains('A') {
            "added"
        } else if code.contains('R') {
            "renamed"
        } else {
            "modified"
        };
        add_changed_file(files, &path, status, "status");
    }
}

fn add_changed_file(
    files: &mut BTreeMap<String, ChangedFileAccumulator>,
    path: &str,
    status: &str,
    source: &str,
) {
    if path.trim().is_empty() || path == "/dev/null" {
        return;
    }
    let entry = files
        .entry(path.to_string())
        .or_insert_with(|| ChangedFileAccumulator {
            status: status.to_string(),
            sources: BTreeSet::new(),
        });
    if status_rank(status) > status_rank(&entry.status) {
        entry.status = status.to_string();
    }
    entry.sources.insert(source.to_string());
}

fn status_rank(status: &str) -> u8 {
    match status {
        "untracked" => 5,
        "deleted" => 4,
        "added" => 3,
        "renamed" => 2,
        "modified" => 1,
        _ => 0,
    }
}

fn parse_diff_git_new_path(line: &str) -> Option<String> {
    let rest = line.strip_prefix("diff --git ")?;
    let marker = " b/";
    let marker_index = rest.rfind(marker)?;
    Some(clean_git_path(&rest[marker_index + marker.len()..]))
}

fn parse_status_line(line: &str) -> Option<(&str, String)> {
    if line.len() < 4 {
        return None;
    }
    let code = line.get(0..2)?;
    let raw_path = line.get(3..)?.trim();
    let path = raw_path
        .rsplit_once(" -> ")
        .map(|(_, new_path)| new_path)
        .unwrap_or(raw_path);
    Some((code, clean_git_path(path)))
}

fn clean_git_path(path: &str) -> String {
    path.trim().trim_matches('"').to_string()
}

fn diff_line_counts(diff: &str) -> (u32, u32) {
    let mut insertions = 0u32;
    let mut deletions = 0u32;
    for line in diff.lines() {
        if line.starts_with("+++") || line.starts_with("---") {
            continue;
        }
        if line.starts_with('+') {
            insertions = insertions.saturating_add(1);
        } else if line.starts_with('-') {
            deletions = deletions.saturating_add(1);
        }
    }
    (insertions, deletions)
}

fn build_patch_excerpt(patch_sections: &[(&str, &str)], status: Option<&str>) -> (String, bool) {
    let mut packet = String::new();
    for (label, diff) in patch_sections {
        if diff.trim().is_empty() {
            continue;
        }
        packet.push_str("### ");
        packet.push_str(label);
        packet.push('\n');
        packet.push_str(diff.trim_end());
        packet.push_str("\n\n");
    }
    if let Some(status) = status.filter(|value| !value.trim().is_empty()) {
        packet.push_str("### git status --porcelain=v1 -uall\n");
        packet.push_str(status.trim_end());
        packet.push('\n');
    }
    let truncated = packet.chars().count() > WORKSPACE_REVIEW_PATCH_EXCERPT_CHARS;
    if truncated {
        (
            packet
                .chars()
                .take(WORKSPACE_REVIEW_PATCH_EXCERPT_CHARS)
                .collect(),
            true,
        )
    } else {
        (packet, false)
    }
}

async fn resolve_review_target(
    workspace: &AgentConversationWorkspace,
    project: &Project,
) -> AppResult<Option<AgentWorkspaceReviewTarget>> {
    if let Some(workspace_target) = resolve_workspace_delta_target(workspace).await? {
        return Ok(Some(workspace_target));
    }
    resolve_selected_source_target(workspace, project).await
}

async fn resolve_workspace_delta_target(
    workspace: &AgentConversationWorkspace,
) -> AppResult<Option<AgentWorkspaceReviewTarget>> {
    let worktree_path = PathBuf::from(&workspace.worktree_path);
    if !worktree_path.exists()
        || !git_success(&["rev-parse", "--is-inside-work-tree"], &worktree_path).await
    {
        return Ok(None);
    }

    let base_ref = workspace
        .base_commit
        .clone()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| workspace.base_ref.clone());
    let head_ref = "HEAD".to_string();
    let committed_diff = git_stdout_lossy(
        &["diff", "--binary", "--no-ext-diff", &base_ref, &head_ref],
        &worktree_path,
    )
    .await?;
    let staged_diff = git_stdout_lossy(
        &["diff", "--cached", "--binary", "--no-ext-diff"],
        &worktree_path,
    )
    .await?;
    let unstaged_diff =
        git_stdout_lossy(&["diff", "--binary", "--no-ext-diff"], &worktree_path).await?;
    let status = git_stdout_lossy(&["status", "--porcelain=v1", "-uall"], &worktree_path).await?;
    if committed_diff.trim().is_empty()
        && staged_diff.trim().is_empty()
        && unstaged_diff.trim().is_empty()
        && status.trim().is_empty()
    {
        return Ok(None);
    }

    let base_sha = rev_parse(&worktree_path, &base_ref).await.ok();
    let head_sha = rev_parse(&worktree_path, &head_ref).await.ok();
    let fingerprint = fingerprint_parts([
        "workspace_delta",
        &base_ref,
        base_sha.as_deref().unwrap_or(""),
        &head_ref,
        head_sha.as_deref().unwrap_or(""),
        &committed_diff,
        &staged_diff,
        &unstaged_diff,
        &status,
    ]);
    let review_packet =
        build_workspace_delta_review_packet(&committed_diff, &staged_diff, &unstaged_diff, &status);

    Ok(Some(AgentWorkspaceReviewTarget {
        scope: AgentWorkspaceReviewTargetScope::WorkspaceDelta,
        base_ref,
        base_sha,
        head_ref,
        head_sha,
        diff_fingerprint: fingerprint,
        working_directory: worktree_path,
        source_pull_request_number: None,
        review_packet,
    }))
}

async fn resolve_selected_source_target(
    workspace: &AgentConversationWorkspace,
    project: &Project,
) -> AppResult<Option<AgentWorkspaceReviewTarget>> {
    let repo_path = PathBuf::from(&project.working_directory);
    if !repo_path.exists()
        || !git_success(&["rev-parse", "--is-inside-work-tree"], &repo_path).await
    {
        return Ok(None);
    }

    let selected_pr = workspace.source_pull_request.as_ref();
    let published_pr_number = workspace.publication_pr_number.filter(|number| *number > 0);
    let is_selected_non_default = workspace.base_ref_kind
        != crate::domain::entities::IdeationAnalysisBaseRefKind::ProjectDefault;
    if !is_selected_non_default && selected_pr.is_none() && published_pr_number.is_none() {
        return Ok(None);
    }

    let default_base =
        GitService::resolve_project_default_branch(&repo_path, project.base_branch.as_deref())
            .await;
    let (base_ref, head_ref, pr_number, explicit_head_sha) = if let Some(pr) = selected_pr {
        let base = pr
            .base_ref_name
            .clone()
            .filter(|value| !value.trim().is_empty())
            .unwrap_or_else(|| default_base.clone());
        let head = if let Some(fetched) =
            GitService::fetch_pull_request_head_for_review(&repo_path, pr.number).await?
        {
            fetched
        } else if !pr.head_ref_name.trim().is_empty() {
            pr.head_ref_name.clone()
        } else {
            workspace.base_ref.clone()
        };
        (base, head, Some(pr.number), pr.head_ref_oid.clone())
    } else if let Some(pr_number) = published_pr_number {
        let Some(head) =
            resolve_published_pull_request_head_ref(&repo_path, workspace, pr_number).await?
        else {
            return Ok(None);
        };
        let base_source = if workspace.base_ref.trim().is_empty() {
            default_base.clone()
        } else {
            workspace.base_ref.clone()
        };
        let base = if workspace.has_terminal_publication_pr_status() {
            resolve_selected_source_merge_base(&repo_path, &base_source, &head)
                .await
                .unwrap_or(base_source)
        } else {
            base_source
        };
        (base, head, Some(pr_number), None)
    } else {
        (default_base, workspace.base_ref.clone(), None, None)
    };

    if base_ref.trim().is_empty() || head_ref.trim().is_empty() || base_ref == head_ref {
        return Ok(None);
    }

    let diff = match git_stdout_lossy(
        &["diff", "--binary", "--no-ext-diff", &base_ref, &head_ref],
        &repo_path,
    )
    .await
    {
        Ok(diff) => diff,
        Err(error) => {
            tracing::warn!(
                conversation_id = %workspace.conversation_id,
                base_ref,
                head_ref,
                error = %error,
                "Failed to derive selected-source review diff"
            );
            return Ok(None);
        }
    };
    if diff.trim().is_empty() {
        return Ok(None);
    }
    let base_sha = rev_parse(&repo_path, &base_ref).await.ok();
    let head_sha = if let Some(sha) = explicit_head_sha.filter(|sha| !sha.trim().is_empty()) {
        Some(sha)
    } else {
        rev_parse(&repo_path, &head_ref).await.ok()
    };
    let fingerprint = fingerprint_parts([
        "selected_source",
        &base_ref,
        base_sha.as_deref().unwrap_or(""),
        &head_ref,
        head_sha.as_deref().unwrap_or(""),
        &diff,
    ]);
    let review_packet = build_selected_source_review_packet(&diff);

    Ok(Some(AgentWorkspaceReviewTarget {
        scope: AgentWorkspaceReviewTargetScope::SelectedSource,
        base_ref,
        base_sha,
        head_ref,
        head_sha,
        diff_fingerprint: fingerprint,
        working_directory: repo_path,
        source_pull_request_number: pr_number,
        review_packet,
    }))
}

async fn resolve_published_pull_request_head_ref(
    repo_path: &Path,
    workspace: &AgentConversationWorkspace,
    pr_number: i64,
) -> AppResult<Option<String>> {
    if let Some(preserved_ref) = GitService::pull_request_head_review_ref(pr_number) {
        if GitService::ref_exists(repo_path, &preserved_ref).await? {
            return Ok(Some(preserved_ref));
        }
    }

    if let Some(fetched_ref) =
        GitService::fetch_pull_request_head_for_review(repo_path, pr_number).await?
    {
        return Ok(Some(fetched_ref));
    }

    if !workspace.branch_name.trim().is_empty()
        && GitService::ref_exists(repo_path, &workspace.branch_name).await?
    {
        return Ok(Some(workspace.branch_name.clone()));
    }

    Ok(None)
}

async fn resolve_selected_source_merge_base(
    repo_path: &Path,
    base_ref: &str,
    head_ref: &str,
) -> Option<String> {
    match git_stdout_lossy(&["merge-base", base_ref, head_ref], repo_path).await {
        Ok(output) => {
            let merge_base = output.trim();
            (!merge_base.is_empty()).then(|| merge_base.to_string())
        }
        Err(error) => {
            warn!(
                base_ref,
                head_ref,
                error = %error,
                "Failed to resolve selected-source merge base for workspace Review"
            );
            None
        }
    }
}

async fn rev_parse(repo: &Path, rev: &str) -> AppResult<String> {
    let output = git_stdout_lossy(&["rev-parse", rev], repo).await?;
    let sha = output.trim().to_string();
    if sha.is_empty() {
        return Err(AppError::GitOperation(format!(
            "git rev-parse {rev} returned an empty value"
        )));
    }
    Ok(sha)
}

async fn git_success(args: &[&str], cwd: &Path) -> bool {
    git_cmd::run_status(args, cwd).await.unwrap_or(false)
}

async fn git_stdout_lossy(args: &[&str], cwd: &Path) -> AppResult<String> {
    let output = git_cmd::with_git_command_lane(GitCommandLane::Background, async {
        git_cmd::run(args, cwd).await
    })
    .await?;
    if !output.status.success() {
        return Err(AppError::GitOperation(
            String::from_utf8_lossy(&output.stderr).trim().to_string(),
        ));
    }
    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

fn fingerprint_parts<'a>(parts: impl IntoIterator<Item = &'a str>) -> String {
    let mut hasher = Sha256::new();
    for part in parts {
        hasher.update(part.as_bytes());
        hasher.update(b"\0");
    }
    format!("{:x}", hasher.finalize())
}

fn build_review_request_message(
    workspace: &AgentConversationWorkspace,
    target: &AgentWorkspaceReviewTarget,
) -> String {
    let pr_line = target
        .source_pull_request_number
        .map(|number| format!("- Source pull request: #{number}\n"))
        .unwrap_or_default();
    format!(
        "Create or refresh the Review for this agent conversation.\n\n\
         Target:\n\
         - Scope: {scope}\n\
         - Base: {base_ref} ({base_sha})\n\
         - Head: {head_ref} ({head_sha})\n\
         - Diff fingerprint: {fingerprint}\n\
         - Review packet: {files_changed} files changed, {insertions} insertions, {deletions} deletions\n\
         {pr_line}\
         - Workspace conversation: {conversation_id}\n\n\
         RalphX scopes workspace Review tools to this parent conversation from runtime context. \
         Use the `target.review_packet` returned by `get_workspace_review_context` as the primary diff input, then inspect only targeted files with read-only filesystem tools if needed. \
         Do not run shell commands, tests, linters, or validation suites. \
         Write a concise reviewer-focused Markdown Review with the `write_workspace_review_artifact` tool, then call `complete_workspace_review_run` with outcome `passed`, `blocking`, `no_changes`, or `run_failed`. Do not modify files.",
        scope = target.scope,
        base_ref = target.base_ref,
        base_sha = target.base_sha.as_deref().unwrap_or("unknown"),
        head_ref = target.head_ref,
        head_sha = target.head_sha.as_deref().unwrap_or("unknown"),
        fingerprint = target.diff_fingerprint,
        files_changed = target.review_packet.summary.files_changed,
        insertions = target.review_packet.summary.insertions,
        deletions = target.review_packet.summary.deletions,
        conversation_id = workspace.conversation_id.as_str(),
    )
}

fn review_started_summary(target: &AgentWorkspaceReviewTarget) -> String {
    match target.scope {
        AgentWorkspaceReviewTargetScope::SelectedSource => {
            if let Some(number) = target.source_pull_request_number {
                format!(
                    "Reviewing selected PR #{number} against {}.",
                    target.base_ref
                )
            } else {
                format!(
                    "Reviewing selected source branch {} against {}.",
                    target.head_ref, target.base_ref
                )
            }
        }
        AgentWorkspaceReviewTargetScope::WorkspaceDelta => {
            "Reviewing current workspace changes.".to_string()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::application::chat_service::MockChatService;
    use crate::domain::agents::AgenticClient;
    use crate::domain::entities::{
        AgentConversationWorkspaceMode, AgentRun, AgentWorkspaceReviewGateStatus,
        AgentWorkspaceReviewOutcome, AgentWorkspaceSourcePullRequest, Artifact, ArtifactId,
        ArtifactType, ChatConversation, ChatConversationId, ChatMessage,
        IdeationAnalysisBaseRefKind, IdeationSession, IdeationSessionFlow, IdeationSessionId,
    };
    use crate::infrastructure::MockAgenticClient;
    use std::process::Command;

    fn git(repo: &Path, args: &[&str]) -> String {
        let output = Command::new("git")
            .args(args)
            .current_dir(repo)
            .output()
            .expect("git command should spawn");
        assert!(
            output.status.success(),
            "git {:?} failed\nstdout:\n{}\nstderr:\n{}",
            args,
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8_lossy(&output.stdout).trim().to_string()
    }

    fn init_repo() -> (tempfile::TempDir, PathBuf, String) {
        let temp = tempfile::tempdir().expect("tempdir should be created");
        let repo = temp.path().join("repo");
        std::fs::create_dir_all(&repo).expect("repo dir should be created");
        git(&repo, &["init", "-b", "main"]);
        git(&repo, &["config", "user.email", "test@example.com"]);
        git(&repo, &["config", "user.name", "Test User"]);
        std::fs::write(repo.join("README.md"), "base\n").expect("base file should be written");
        git(&repo, &["add", "README.md"]);
        git(&repo, &["commit", "-m", "base"]);
        let base_sha = git(&repo, &["rev-parse", "HEAD"]);
        (temp, repo, base_sha)
    }

    async fn seed_project(state: &AppState, repo: &Path) -> Project {
        let mut project = Project::new(
            "Workspace Review".to_string(),
            repo.to_string_lossy().to_string(),
        );
        project.base_branch = Some("main".to_string());
        state
            .project_repo
            .create(project.clone())
            .await
            .expect("project should persist");
        project
    }

    fn workspace(
        project: &Project,
        worktree_path: &Path,
        base_kind: IdeationAnalysisBaseRefKind,
        base_ref: &str,
        base_commit: Option<String>,
    ) -> AgentConversationWorkspace {
        AgentConversationWorkspace::new(
            ChatConversationId::new(),
            project.id.clone(),
            AgentConversationWorkspaceMode::Edit,
            base_kind,
            base_ref.to_string(),
            Some(base_ref.to_string()),
            base_commit,
            "ralphx/test/workspace-review".to_string(),
            worktree_path.to_string_lossy().to_string(),
        )
    }

    fn committed_workspace_delta(repo: &Path) {
        std::fs::write(repo.join("committed.rs"), "pub fn committed() {}\n")
            .expect("committed file should be written");
        git(repo, &["add", "committed.rs"]);
        git(repo, &["commit", "-m", "committed change"]);
    }

    async fn seed_conversation(state: &AppState, workspace: &AgentConversationWorkspace) {
        let mut conversation = ChatConversation::new_project(workspace.project_id.clone());
        conversation.id = workspace.conversation_id.clone();
        conversation.agent_mode = Some(workspace.mode);
        state
            .chat_conversation_repo
            .create(conversation)
            .await
            .expect("conversation should persist");
    }

    async fn wait_for_monitor_status(
        state: &AppState,
        workspace: &AgentConversationWorkspace,
        status: AgentWorkspaceReviewMonitorStatus,
    ) -> AgentWorkspaceReviewMonitor {
        for _ in 0..100 {
            if let Some(monitor) = state
                .agent_conversation_workspace_repo
                .get_workspace_review_monitor(&workspace.conversation_id)
                .await
                .expect("monitor read should succeed")
            {
                if monitor.status == status {
                    return monitor;
                }
            }
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
        panic!("monitor did not reach status {status}");
    }

    #[test]
    fn inherited_reference_metadata_deduplicates_limits_and_ignores_invalid_payloads() {
        let mut inherited = WorkspaceReviewInheritedReferences::default();
        let mut project_seen = BTreeSet::new();
        let mut integration_seen = BTreeSet::new();
        let mut artifact_seen = BTreeSet::new();

        merge_workspace_review_references_from_metadata(
            None,
            &mut inherited,
            &mut project_seen,
            &mut integration_seen,
            &mut artifact_seen,
        );
        merge_workspace_review_references_from_metadata(
            Some("not-json"),
            &mut inherited,
            &mut project_seen,
            &mut integration_seen,
            &mut artifact_seen,
        );
        merge_workspace_review_references_from_metadata(
            Some("[]"),
            &mut inherited,
            &mut project_seen,
            &mut integration_seen,
            &mut artifact_seen,
        );

        let metadata = serde_json::json!({
            "composer_project_references": [
                { "path": "README.md", "kind": "file" },
                { "path": "README.md", "kind": "file" },
                { "path": "src", "kind": "directory" },
                { "path": "docs", "kind": "directory" },
                { "path": "frontend", "kind": "directory" },
                { "path": "src-tauri", "kind": "directory" },
                { "path": "package.json", "kind": "file" },
                { "path": "Cargo.toml", "kind": "file" },
                { "path": "CLAUDE.md", "kind": "file" },
                { "path": "ignored-after-cap.md", "kind": "file" }
            ],
            "composer_integration_references": [
                {
                    "provider": "atlassian",
                    "kind": "jira",
                    "id": "RX-42",
                    "key": "RX-42",
                    "title": "Fix Review gate"
                },
                {
                    "provider": "atlassian",
                    "kind": "jira",
                    "id": "RX-42",
                    "key": "RX-42",
                    "title": "Duplicate"
                },
                { "provider": "linear", "kind": "issue", "id": "LIN-1" },
                { "provider": "clickup", "kind": "task", "id": "CU-1" },
                { "provider": "granola", "kind": "note", "id": "GN-1" },
                { "provider": "github", "kind": "issue", "id": "GH-1" },
                { "provider": "sentry", "kind": "issue", "id": "SEN-1" },
                { "provider": "notion", "kind": "page", "id": "NOT-1" },
                { "provider": "slack", "kind": "thread", "id": "SL-1" },
                { "provider": "ignored", "kind": "thread", "id": "IGN-1" }
            ],
            "composer_artifact_references": [
                { "artifactId": "artifact-1", "kind": "plan", "title": "Plan" },
                { "artifactId": "artifact-1", "kind": "plan", "title": "Duplicate" },
                { "artifactId": "artifact-2", "kind": "design" },
                { "artifactId": "artifact-3", "kind": "spec" },
                { "artifactId": "artifact-4", "kind": "notes" },
                { "artifactId": "artifact-5", "kind": "review" },
                { "artifactId": "artifact-6", "kind": "diff" },
                { "artifactId": "artifact-7", "kind": "trace" },
                { "artifactId": "artifact-8", "kind": "context" },
                { "artifactId": "artifact-9", "kind": "ignored" }
            ]
        })
        .to_string();

        merge_workspace_review_references_from_metadata(
            Some(&metadata),
            &mut inherited,
            &mut project_seen,
            &mut integration_seen,
            &mut artifact_seen,
        );

        assert_eq!(inherited.project_references.len(), 8);
        assert_eq!(inherited.project_references[0].path, "README.md");
        assert_eq!(inherited.project_references[1].path, "src");
        assert!(!inherited
            .project_references
            .iter()
            .any(|reference| reference.path == "ignored-after-cap.md"));
        assert_eq!(inherited.integration_references.len(), 8);
        assert_eq!(
            inherited.integration_references[0].key.as_deref(),
            Some("RX-42")
        );
        assert!(!inherited
            .integration_references
            .iter()
            .any(|reference| reference.id == "IGN-1"));
        assert_eq!(inherited.artifact_references.len(), 8);
        assert_eq!(inherited.artifact_references[0].artifact_id, "artifact-1");
        assert!(!inherited
            .artifact_references
            .iter()
            .any(|reference| reference.artifact_id == "artifact-9"));

        merge_workspace_review_references_from_metadata(
            Some(&metadata),
            &mut inherited,
            &mut project_seen,
            &mut integration_seen,
            &mut artifact_seen,
        );
        assert_eq!(inherited.project_references.len(), 8);
        assert_eq!(inherited.integration_references.len(), 8);
        assert_eq!(inherited.artifact_references.len(), 8);
    }

    #[tokio::test]
    async fn linked_workspace_plan_reference_handles_missing_links_and_missing_artifact() {
        let (_temp, repo, base_sha) = init_repo();
        let state = AppState::new_test();
        let project = seed_project(&state, &repo).await;
        let mut workspace = workspace(
            &project,
            &repo,
            IdeationAnalysisBaseRefKind::ProjectDefault,
            "main",
            Some(base_sha),
        );

        assert!(linked_workspace_plan_artifact_reference(&state, &workspace)
            .await
            .expect("missing link should load")
            .is_none());

        workspace.linked_ideation_session_id =
            Some(IdeationSessionId::from_string("missing-session"));
        assert!(linked_workspace_plan_artifact_reference(&state, &workspace)
            .await
            .expect("missing session should load")
            .is_none());

        let empty_session = IdeationSession::builder()
            .project_id(project.id.clone())
            .session_flow(IdeationSessionFlow::Planning)
            .build();
        let empty_session = state
            .ideation_session_repo
            .create(empty_session)
            .await
            .expect("empty planning session should persist");
        workspace.linked_ideation_session_id = Some(empty_session.id.clone());
        assert!(linked_workspace_plan_artifact_reference(&state, &workspace)
            .await
            .expect("empty session should load")
            .is_none());

        let missing_artifact_id = ArtifactId::from_string("missing-plan-artifact");
        let missing_artifact_session = IdeationSession::builder()
            .project_id(project.id.clone())
            .session_flow(IdeationSessionFlow::Planning)
            .inherited_plan_artifact_id(missing_artifact_id.clone())
            .build();
        let missing_artifact_session = state
            .ideation_session_repo
            .create(missing_artifact_session)
            .await
            .expect("missing-artifact planning session should persist");
        workspace.linked_ideation_session_id = Some(missing_artifact_session.id.clone());

        let reference = linked_workspace_plan_artifact_reference(&state, &workspace)
            .await
            .expect("missing artifact reference should load")
            .expect("missing artifact id should still produce a reference");
        assert_eq!(reference.artifact_id, missing_artifact_id.as_str());
        assert_eq!(
            reference.session_id.as_deref(),
            Some(missing_artifact_session.id.as_str())
        );
        assert_eq!(reference.kind, "plan");
        assert_eq!(reference.title, None);
        assert_eq!(reference.version, None);
    }

    #[test]
    fn review_packet_handles_status_edges_limits_and_truncation() {
        let diff = "\
metadata before first file
diff --git a/modified.rs b/modified.rs
--- a/modified.rs
+++ b/modified.rs
@@
-old
+new
diff --git a/added.rs b/added.rs
new file mode 100644
--- /dev/null
+++ b/added.rs
@@
+added
diff --git a/deleted.rs b/deleted.rs
deleted file mode 100644
--- a/deleted.rs
+++ /dev/null
@@
-deleted
diff --git a/old_name.rs b/old_name.rs
similarity index 100%
rename from old_name.rs
rename to \"renamed file.rs\"
diff --git a/status_added.rs b/status_added.rs
--- a/status_added.rs
+++ b/status_added.rs
@@
+status added
";
        let large_diff = format!(
            "diff --git a/large.rs b/large.rs\n--- a/large.rs\n+++ b/large.rs\n@@\n+{}\n",
            "x".repeat(WORKSPACE_REVIEW_PATCH_EXCERPT_CHARS + 64)
        );
        let mut status = String::from(
            "\
A  status_added.rs
 D status_deleted.rs
R  old_status.rs -> status_renamed.rs
 M status_modified.rs
?? untracked.rs
?? /dev/null
x
",
        );
        status.push_str("??    \n");
        for index in 0..=WORKSPACE_REVIEW_MAX_CHANGED_FILES {
            status.push_str(&format!("?? zz-overflow-{index:03}.rs\n"));
        }

        let packet = build_review_packet(
            &[
                ("edge diff", diff),
                ("empty diff", "   "),
                ("large diff", &large_diff),
            ],
            Some(&status),
            &[("edge", diff), ("large", &large_diff)],
        );

        assert_eq!(
            packet.changed_files.len(),
            WORKSPACE_REVIEW_MAX_CHANGED_FILES
        );
        assert!(packet.summary.files_changed > WORKSPACE_REVIEW_MAX_CHANGED_FILES as u32);
        assert_eq!(packet.summary.deletions, 2);
        assert!(packet.summary.insertions >= 4);
        assert!(packet.patch_excerpt_truncated);
        assert_eq!(
            packet.patch_excerpt.chars().count(),
            WORKSPACE_REVIEW_PATCH_EXCERPT_CHARS
        );
        assert!(packet
            .notes
            .iter()
            .any(|note| note.contains("Untracked files are listed")));
        assert!(packet
            .notes
            .iter()
            .any(|note| note.contains("Changed file list is limited")));
        assert!(packet
            .notes
            .iter()
            .any(|note| note.contains("Patch excerpt is limited")));
        assert!(!packet.patch_excerpt.contains("### empty diff"));

        let file = |path: &str| {
            packet
                .changed_files
                .iter()
                .find(|file| file.path == path)
                .expect("changed file should be listed")
        };
        assert_eq!(file("added.rs").status, "added");
        assert_eq!(file("deleted.rs").status, "deleted");
        assert_eq!(file("renamed file.rs").status, "renamed");
        assert_eq!(file("status_added.rs").status, "added");
        assert!(file("status_added.rs")
            .sources
            .contains(&"status".to_string()));
        assert!(!packet
            .changed_files
            .iter()
            .any(|file| file.path == "/dev/null" || file.path.is_empty()));

        let mut ranked_files = BTreeMap::<String, ChangedFileAccumulator>::new();
        add_changed_file(&mut ranked_files, "ranked.rs", "modified", "low");
        add_changed_file(&mut ranked_files, "ranked.rs", "unknown", "ignored");
        add_changed_file(&mut ranked_files, "ranked.rs", "untracked", "high");
        let ranked = ranked_files
            .get("ranked.rs")
            .expect("ranked file should be tracked");
        assert_eq!(ranked.status, "untracked");
        assert!(ranked.sources.contains("ignored"));
    }

    #[tokio::test]
    async fn load_context_resolves_workspace_delta_and_monitor_fields() {
        let (_temp, repo, base_sha) = init_repo();
        committed_workspace_delta(&repo);
        std::fs::write(repo.join("staged.rs"), "pub fn staged() {}\n")
            .expect("staged file should be written");
        git(&repo, &["add", "staged.rs"]);
        std::fs::write(repo.join("unstaged.rs"), "pub fn unstaged() {}\n")
            .expect("unstaged file should be written");

        let state = AppState::new_test();
        let project = seed_project(&state, &repo).await;
        let workspace = workspace(
            &project,
            &repo,
            IdeationAnalysisBaseRefKind::ProjectDefault,
            "main",
            Some(base_sha.clone()),
        );

        let context = load_agent_workspace_review_context(&state, &workspace)
            .await
            .expect("workspace delta context should load");
        let target = context
            .target
            .expect("workspace delta should be reviewable");

        assert_eq!(
            target.scope,
            AgentWorkspaceReviewTargetScope::WorkspaceDelta
        );
        assert_eq!(target.base_ref, base_sha);
        assert_eq!(target.head_ref, "HEAD");
        assert!(target.base_sha.is_some());
        assert!(target.head_sha.is_some());
        assert!(!target.diff_fingerprint.is_empty());
        assert_eq!(target.working_directory, repo);
        assert_eq!(target.review_packet.summary.files_changed, 3);
        assert_eq!(target.review_packet.summary.insertions, 2);
        assert_eq!(target.review_packet.summary.deletions, 0);
        assert!(target.review_packet.changed_files.iter().any(|file| {
            file.path == "committed.rs" && file.sources.contains(&"committed".to_string())
        }));
        assert!(target.review_packet.changed_files.iter().any(|file| {
            file.path == "staged.rs" && file.sources.contains(&"staged".to_string())
        }));
        assert!(target
            .review_packet
            .changed_files
            .iter()
            .any(|file| file.path == "unstaged.rs" && file.status == "untracked"));
        assert!(target
            .review_packet
            .patch_excerpt
            .contains("### committed diff"));
        assert!(target
            .review_packet
            .patch_excerpt
            .contains("### staged diff"));
        assert!(target
            .review_packet
            .patch_excerpt
            .contains("### git status --porcelain=v1 -uall"));
        assert!(!context.is_current);
        assert!(!context.is_outdated);
        assert!(context.should_show_tab);
        assert_eq!(
            context.monitor.current_target_scope,
            Some(AgentWorkspaceReviewTargetScope::WorkspaceDelta)
        );
        assert_eq!(context.monitor.workspace_head_ref.as_deref(), Some("HEAD"));
        assert_eq!(
            context.monitor.workspace_base_ref.as_deref(),
            Some(base_sha.as_str())
        );
    }

    #[tokio::test]
    async fn load_context_resolves_selected_branch_when_workspace_has_no_delta() {
        let (temp, repo, _base_sha) = init_repo();
        git(&repo, &["checkout", "-b", "feature/source"]);
        std::fs::write(repo.join("feature.rs"), "pub fn feature() {}\n")
            .expect("feature file should be written");
        git(&repo, &["add", "feature.rs"]);
        git(&repo, &["commit", "-m", "feature change"]);
        let feature_head = git(&repo, &["rev-parse", "HEAD"]);
        git(&repo, &["checkout", "main"]);

        let state = AppState::new_test();
        let project = seed_project(&state, &repo).await;
        let missing_worktree = temp.path().join("missing-worktree");
        let workspace = workspace(
            &project,
            &missing_worktree,
            IdeationAnalysisBaseRefKind::LocalBranch,
            "feature/source",
            None,
        );

        let context = load_agent_workspace_review_context(&state, &workspace)
            .await
            .expect("selected branch context should load");
        let target = context
            .target
            .expect("selected branch should be reviewable");

        assert_eq!(
            target.scope,
            AgentWorkspaceReviewTargetScope::SelectedSource
        );
        assert_eq!(target.base_ref, "main");
        assert_eq!(target.head_ref, "feature/source");
        assert_eq!(target.head_sha.as_deref(), Some(feature_head.as_str()));
        assert_eq!(target.source_pull_request_number, None);
        assert_eq!(target.review_packet.summary.files_changed, 1);
        assert_eq!(target.review_packet.summary.insertions, 1);
        assert!(target.review_packet.changed_files.iter().any(|file| {
            file.path == "feature.rs" && file.sources.contains(&"selected_source".to_string())
        }));
        assert!(target
            .review_packet
            .patch_excerpt
            .contains("### selected_source diff"));
        assert_eq!(
            context.monitor.selected_source_head_ref.as_deref(),
            Some("feature/source")
        );
        assert!(context.should_show_tab);
    }

    #[tokio::test]
    async fn load_context_resolves_selected_pull_request_metadata() {
        let (temp, repo, _base_sha) = init_repo();
        git(&repo, &["checkout", "-b", "feature/pr-42"]);
        std::fs::write(repo.join("pr.rs"), "pub fn pr() {}\n").expect("pr file should be written");
        git(&repo, &["add", "pr.rs"]);
        git(&repo, &["commit", "-m", "pr change"]);
        let pr_head = git(&repo, &["rev-parse", "HEAD"]);
        git(&repo, &["checkout", "main"]);

        let state = AppState::new_test();
        let project = seed_project(&state, &repo).await;
        let mut workspace = workspace(
            &project,
            &temp.path().join("missing-worktree"),
            IdeationAnalysisBaseRefKind::PullRequest,
            "feature/pr-42",
            None,
        );
        workspace.source_pull_request = Some(AgentWorkspaceSourcePullRequest {
            number: 42,
            url: Some("https://github.example/pr/42".to_string()),
            title: Some("Review source".to_string()),
            head_ref_name: "feature/pr-42".to_string(),
            base_ref_name: Some("main".to_string()),
            head_ref_oid: Some(pr_head.clone()),
        });

        let context = load_agent_workspace_review_context(&state, &workspace)
            .await
            .expect("selected PR context should load");
        let target = context.target.expect("selected PR should be reviewable");

        assert_eq!(target.base_ref, "main");
        assert_eq!(target.head_ref, "feature/pr-42");
        assert_eq!(target.head_sha.as_deref(), Some(pr_head.as_str()));
        assert_eq!(target.source_pull_request_number, Some(42));
        assert_eq!(
            context.monitor.selected_source_pull_request_number,
            Some(42)
        );
    }

    #[tokio::test]
    async fn load_context_resolves_published_pr_preserved_ref_and_terminal_merge_base() {
        let (temp, repo, base_sha) = init_repo();
        git(&repo, &["checkout", "-b", "feature/published-pr"]);
        std::fs::write(repo.join("published.rs"), "pub fn published() {}\n")
            .expect("published file should be written");
        git(&repo, &["add", "published.rs"]);
        git(&repo, &["commit", "-m", "published pr change"]);
        let pr_head = git(&repo, &["rev-parse", "HEAD"]);
        git(&repo, &["update-ref", "refs/ralphx/pr-heads/483", &pr_head]);
        git(&repo, &["checkout", "main"]);

        let state = AppState::new_test();
        let project = seed_project(&state, &repo).await;
        let mut workspace = workspace(
            &project,
            &temp.path().join("missing-worktree"),
            IdeationAnalysisBaseRefKind::ProjectDefault,
            "main",
            None,
        );
        workspace.publication_pr_number = Some(483);
        workspace.publication_pr_status = Some("merged".to_string());

        let context = load_agent_workspace_review_context(&state, &workspace)
            .await
            .expect("published PR context should load");
        let target = context.target.expect("published PR should be reviewable");

        assert_eq!(
            target.scope,
            AgentWorkspaceReviewTargetScope::SelectedSource
        );
        assert_eq!(target.base_ref, base_sha);
        assert_eq!(target.head_ref, "refs/ralphx/pr-heads/483");
        assert_eq!(target.head_sha.as_deref(), Some(pr_head.as_str()));
        assert_eq!(target.source_pull_request_number, Some(483));
        assert_eq!(
            context.monitor.selected_source_base_ref.as_deref(),
            Some(target.base_ref.as_str())
        );
    }

    #[tokio::test]
    async fn load_context_handles_missing_sources_without_review_tab() {
        let temp = tempfile::tempdir().expect("tempdir should be created");
        let missing_repo = temp.path().join("missing-repo");
        let state = AppState::new_test();
        let project = seed_project(&state, &missing_repo).await;
        let workspace = workspace(
            &project,
            &temp.path().join("missing-worktree"),
            IdeationAnalysisBaseRefKind::ProjectDefault,
            "main",
            None,
        );

        let context = load_agent_workspace_review_context(&state, &workspace)
            .await
            .expect("empty context should load");

        assert!(context.target.is_none());
        assert_eq!(
            context.monitor.status,
            AgentWorkspaceReviewMonitorStatus::Idle
        );
        assert!(!context.is_current);
        assert!(!context.is_outdated);
        assert!(!context.should_show_tab);
    }

    #[tokio::test]
    async fn existing_review_artifact_marks_context_current_then_outdated() {
        let (_temp, repo, base_sha) = init_repo();
        committed_workspace_delta(&repo);

        let state = AppState::new_test();
        let project = seed_project(&state, &repo).await;
        let workspace = workspace(
            &project,
            &repo,
            IdeationAnalysisBaseRefKind::ProjectDefault,
            "main",
            Some(base_sha),
        );
        seed_conversation(&state, &workspace).await;
        let initial = load_agent_workspace_review_context(&state, &workspace)
            .await
            .expect("initial context should load");
        let target = initial.target.expect("initial target should exist");
        let mut monitor = initial.monitor;
        apply_review_artifact_to_monitor(
            &mut monitor,
            target.scope,
            target.head_sha.clone(),
            target.diff_fingerprint.clone(),
            Some("run-1".to_string()),
            ArtifactId::from_string("artifact-1"),
            1,
            Utc::now(),
            None,
        );
        state
            .agent_conversation_workspace_repo
            .upsert_workspace_review_monitor(monitor)
            .await
            .expect("monitor should persist");

        let current = load_agent_workspace_review_context(&state, &workspace)
            .await
            .expect("current context should load");
        assert!(current.is_current);
        assert!(!current.is_outdated);
        assert_eq!(
            current.monitor.status,
            AgentWorkspaceReviewMonitorStatus::Ready
        );

        std::fs::write(repo.join("later.rs"), "pub fn later() {}\n")
            .expect("later file should be written");
        let outdated = load_agent_workspace_review_context(&state, &workspace)
            .await
            .expect("outdated context should load");
        assert!(!outdated.is_current);
        assert!(outdated.is_outdated);
        assert!(outdated.should_show_tab);
    }

    #[tokio::test]
    async fn start_review_skips_current_and_already_reviewing_targets() {
        let (_temp, repo, base_sha) = init_repo();
        committed_workspace_delta(&repo);

        let state = Arc::new(AppState::new_test());
        let project = seed_project(&state, &repo).await;
        let workspace = workspace(
            &project,
            &repo,
            IdeationAnalysisBaseRefKind::ProjectDefault,
            "main",
            Some(base_sha),
        );
        let initial = load_agent_workspace_review_context(&state, &workspace)
            .await
            .expect("initial context should load");
        let target = initial.target.expect("target should exist");

        let mut current_monitor = initial.monitor.clone();
        apply_review_artifact_to_monitor(
            &mut current_monitor,
            target.scope,
            target.head_sha.clone(),
            target.diff_fingerprint.clone(),
            Some("run-current".to_string()),
            ArtifactId::from_string("artifact-current"),
            2,
            Utc::now(),
            None,
        );
        current_monitor.review_outcome = AgentWorkspaceReviewOutcome::Passed;
        current_monitor.review_gate_status = AgentWorkspaceReviewGateStatus::Passed;
        state
            .agent_conversation_workspace_repo
            .upsert_workspace_review_monitor(current_monitor)
            .await
            .expect("current monitor should persist");
        let current_start = start_agent_workspace_review(Arc::clone(&state), &workspace, false)
            .await
            .expect("current start should not spawn");
        assert!(!current_start.started);
        assert_eq!(current_start.skipped_reason.as_deref(), Some("current"));
        assert_eq!(
            current_start.context.monitor.status,
            AgentWorkspaceReviewMonitorStatus::Ready
        );

        let mut reviewing_monitor =
            AgentWorkspaceReviewMonitor::new(workspace.conversation_id.clone(), project.id.clone());
        apply_current_target_to_monitor(&mut reviewing_monitor, Some(&target));
        reviewing_monitor.status = AgentWorkspaceReviewMonitorStatus::Reviewing;
        state
            .agent_conversation_workspace_repo
            .upsert_workspace_review_monitor(reviewing_monitor)
            .await
            .expect("reviewing monitor should persist");
        let reviewing_start = start_agent_workspace_review(state, &workspace, false)
            .await
            .expect("reviewing start should not spawn");
        assert!(!reviewing_start.started);
        assert_eq!(
            reviewing_start.skipped_reason.as_deref(),
            Some("already_reviewing")
        );
    }

    #[tokio::test]
    async fn start_review_runs_workspace_reviewer_child_chat_and_records_blocked_completion() {
        let (_temp, repo, base_sha) = init_repo();
        committed_workspace_delta(&repo);

        let agent_client: Arc<dyn AgenticClient> = Arc::new(MockAgenticClient::new());
        let state = Arc::new(AppState::new_test().with_agent_client(agent_client));
        let chat_service = MockChatService::new();
        let project = seed_project(&state, &repo).await;
        let mut workspace = workspace(
            &project,
            &repo,
            IdeationAnalysisBaseRefKind::ProjectDefault,
            "main",
            Some(base_sha),
        );
        let mut plan_artifact = Artifact::new_inline(
            "Approved implementation plan",
            ArtifactType::Specification,
            "# Plan\n\nUse the backend-owned Review gate.",
            "ralphx-ideation",
        );
        plan_artifact.metadata.version = 4;
        let plan_artifact = state
            .artifact_repo
            .create(plan_artifact)
            .await
            .expect("plan artifact should persist");
        let planning_session = IdeationSession::builder()
            .project_id(project.id.clone())
            .session_flow(IdeationSessionFlow::Planning)
            .plan_artifact_id(plan_artifact.id.clone())
            .build();
        let planning_session = state
            .ideation_session_repo
            .create(planning_session)
            .await
            .expect("planning session should persist");
        workspace.linked_ideation_session_id = Some(planning_session.id.clone());
        seed_conversation(&state, &workspace).await;
        let mut parent_message = ChatMessage::user_in_project(project.id.clone(), "Build it");
        parent_message.conversation_id = Some(workspace.conversation_id.clone());
        parent_message.metadata = Some(
            serde_json::json!({
                "composer_project_references": [
                    { "path": "README.md", "kind": "file" }
                ],
                "composer_integration_references": [
                    {
                        "provider": "atlassian",
                        "kind": "jira",
                        "id": "RX-42",
                        "key": "RX-42",
                        "title": "Fix Review gate",
                        "url": "https://jira.test/browse/RX-42"
                    },
                    {
                        "provider": "clickup",
                        "kind": "clickup",
                        "id": "task-1",
                        "key": "CU-1",
                        "title": "ClickUp review task",
                        "url": "https://clickup.test/t/task-1"
                    }
                ],
                "composer_artifact_references": [
                    {
                        "artifactId": "design-artifact-1",
                        "kind": "design",
                        "title": "Design context"
                    }
                ]
            })
            .to_string(),
        );
        state
            .chat_message_repo
            .create(parent_message)
            .await
            .expect("parent message should persist");

        let start = start_agent_workspace_review_with_chat_service(
            Arc::clone(&state),
            &workspace,
            true,
            &chat_service,
        )
        .await
        .expect("review child chat should start");

        assert!(start.started);
        assert_eq!(start.skipped_reason, None);
        assert_eq!(
            start.context.monitor.status,
            AgentWorkspaceReviewMonitorStatus::Reviewing
        );
        assert!(start.context.monitor.last_run_id.is_some());
        let review_conversation_id = start
            .context
            .monitor
            .review_conversation_id
            .clone()
            .expect("review conversation id should be recorded");
        let review_conversation = state
            .chat_conversation_repo
            .get_by_id(&review_conversation_id)
            .await
            .expect("review conversation lookup should succeed")
            .expect("review conversation should exist");
        let parent_conversation_id = workspace.conversation_id.as_str();
        assert_eq!(
            review_conversation.parent_conversation_id.as_deref(),
            Some(parent_conversation_id.as_str())
        );
        assert_eq!(review_conversation.context_type, ChatContextType::Project);
        assert_eq!(review_conversation.context_id, project.id.as_str());
        assert_eq!(
            review_conversation.title.as_deref(),
            Some("Review workspace changes")
        );

        let sent_messages = chat_service.get_sent_messages().await;
        assert_eq!(sent_messages.len(), 1);
        let review_prompt = &sent_messages[0];
        assert!(review_prompt.contains("Create or refresh the Review"));
        assert!(review_prompt.contains("- Scope: workspace_delta"));
        assert!(review_prompt.contains(&workspace.conversation_id.as_str()));
        assert!(!review_prompt.contains("pass conversation_id"));

        let sent_options = chat_service.get_sent_options().await;
        assert_eq!(sent_options.len(), 1);
        let options = &sent_options[0];
        assert_eq!(
            options.conversation_id_override,
            Some(review_conversation_id.clone())
        );
        assert_eq!(
            options.agent_name_override.as_deref(),
            Some(agent_names::AGENT_WORKSPACE_REVIEWER)
        );
        assert_eq!(
            options.working_directory_override.as_deref(),
            Some(repo.as_path())
        );
        assert_eq!(options.composer_project_references.len(), 1);
        assert_eq!(options.composer_project_references[0].path, "README.md");
        assert_eq!(options.composer_integration_references.len(), 2);
        assert!(options
            .composer_integration_references
            .iter()
            .any(|reference| reference.provider == "atlassian"
                && reference.kind == "jira"
                && reference.key.as_deref() == Some("RX-42")));
        assert!(options
            .composer_integration_references
            .iter()
            .any(|reference| reference.provider == "clickup"
                && reference.kind == "clickup"
                && reference.id == "task-1"));
        assert_eq!(options.composer_artifact_references.len(), 2);
        assert!(options
            .composer_artifact_references
            .iter()
            .any(|reference| reference.artifact_id == "design-artifact-1"
                && reference.kind == "design"));
        assert!(options
            .composer_artifact_references
            .iter()
            .any(
                |reference| reference.artifact_id == plan_artifact.id.as_str()
                    && reference.kind == "plan"
                    && reference.session_id.as_deref() == Some(planning_session.id.as_str())
                    && reference.title.as_deref() == Some("Approved implementation plan")
                    && reference.version == Some(4)
            ));
        assert!(options.force_new_provider_session);
        let metadata: serde_json::Value = serde_json::from_str(
            options
                .metadata
                .as_deref()
                .expect("review kickoff should carry hidden message metadata"),
        )
        .expect("review kickoff metadata should be valid json");
        assert_eq!(metadata["hidden_from_ui"], true);
        assert_eq!(metadata["source"], "workspace_review_request");

        let mut blocked_monitor = None;
        for _ in 0..100 {
            if let Some(monitor) = state
                .agent_conversation_workspace_repo
                .get_workspace_review_monitor(&workspace.conversation_id)
                .await
                .expect("monitor read should succeed")
            {
                if monitor.status == AgentWorkspaceReviewMonitorStatus::Blocked {
                    blocked_monitor = Some(monitor);
                    break;
                }
            }
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
        let blocked_monitor = blocked_monitor.expect("watcher should mark missing Review blocked");
        assert_eq!(
            blocked_monitor.last_run_id,
            start.context.monitor.last_run_id
        );
        assert_eq!(
            blocked_monitor.review_conversation_id,
            Some(review_conversation_id)
        );
        assert_eq!(
            blocked_monitor.last_error.as_deref(),
            Some("Workspace reviewer run disappeared before completion")
        );
    }

    #[tokio::test]
    async fn start_review_blocks_monitor_when_child_chat_send_fails() {
        let (_temp, repo, base_sha) = init_repo();
        committed_workspace_delta(&repo);

        let agent_client: Arc<dyn AgenticClient> = Arc::new(MockAgenticClient::new());
        let state = Arc::new(AppState::new_test().with_agent_client(agent_client));
        let chat_service = MockChatService::new();
        chat_service.set_available(false).await;
        let project = seed_project(&state, &repo).await;
        let workspace = workspace(
            &project,
            &repo,
            IdeationAnalysisBaseRefKind::ProjectDefault,
            "main",
            Some(base_sha),
        );
        seed_conversation(&state, &workspace).await;

        let error = start_agent_workspace_review_with_chat_service(
            Arc::clone(&state),
            &workspace,
            true,
            &chat_service,
        )
        .await
        .expect_err("review child chat send should fail");

        assert!(error
            .to_string()
            .contains("failed to start workspace reviewer chat"));
        let sent_options = chat_service.get_sent_options().await;
        assert_eq!(sent_options.len(), 1);
        let review_conversation_id = sent_options[0]
            .conversation_id_override
            .clone()
            .expect("review conversation override should be created before send");
        let monitor = state
            .agent_conversation_workspace_repo
            .get_workspace_review_monitor(&workspace.conversation_id)
            .await
            .expect("monitor read should succeed")
            .expect("monitor should persist");
        assert_eq!(monitor.status, AgentWorkspaceReviewMonitorStatus::Blocked);
        assert_eq!(monitor.review_conversation_id, Some(review_conversation_id));
        assert!(monitor.last_run_id.is_none());
        assert_eq!(
            monitor.last_error.as_deref(),
            Some(
                "failed to start workspace reviewer chat: Agent not available: Mock agent not available"
            )
        );
    }

    #[tokio::test]
    async fn workspace_review_waiter_handles_failed_and_completed_child_runs() {
        let (_temp, repo, base_sha) = init_repo();
        committed_workspace_delta(&repo);

        let state = Arc::new(AppState::new_test());
        let project = seed_project(&state, &repo).await;
        let workspace = workspace(
            &project,
            &repo,
            IdeationAnalysisBaseRefKind::ProjectDefault,
            "main",
            Some(base_sha),
        );
        let context = load_agent_workspace_review_context(&state, &workspace)
            .await
            .expect("context should load");
        let target = context.target.expect("target should exist");

        let mut failed_run = AgentRun::new(ChatConversationId::new());
        let failed_run_id = failed_run.id.as_str().to_string();
        failed_run.fail("review process crashed");
        state
            .agent_run_repo
            .create(failed_run)
            .await
            .expect("failed run should persist");
        let mut reviewing_monitor = context.monitor.clone();
        apply_current_target_to_monitor(&mut reviewing_monitor, Some(&target));
        reviewing_monitor.status = AgentWorkspaceReviewMonitorStatus::Reviewing;
        reviewing_monitor.last_run_id = Some(failed_run_id.clone());
        state
            .agent_conversation_workspace_repo
            .upsert_workspace_review_monitor(reviewing_monitor)
            .await
            .expect("reviewing monitor should persist");

        spawn_workspace_review_waiter(
            Arc::clone(&state),
            workspace.clone(),
            target.clone(),
            failed_run_id.clone(),
        );

        let blocked = wait_for_monitor_status(
            &state,
            &workspace,
            AgentWorkspaceReviewMonitorStatus::Blocked,
        )
        .await;
        assert_eq!(blocked.last_run_id.as_deref(), Some(failed_run_id.as_str()));
        assert_eq!(
            blocked.last_error.as_deref(),
            Some("review process crashed")
        );

        let mut completed_run = AgentRun::new(ChatConversationId::new());
        let completed_run_id = completed_run.id.as_str().to_string();
        completed_run.complete();
        state
            .agent_run_repo
            .create(completed_run)
            .await
            .expect("completed run should persist");
        let mut ready_monitor = blocked;
        apply_review_artifact_to_monitor(
            &mut ready_monitor,
            target.scope,
            target.head_sha.clone(),
            target.diff_fingerprint.clone(),
            Some(completed_run_id.clone()),
            ArtifactId::from_string("artifact-ready"),
            4,
            Utc::now(),
            None,
        );
        ready_monitor.review_outcome = AgentWorkspaceReviewOutcome::Passed;
        ready_monitor.review_gate_status = AgentWorkspaceReviewGateStatus::Passed;
        state
            .agent_conversation_workspace_repo
            .upsert_workspace_review_monitor(ready_monitor)
            .await
            .expect("ready monitor should persist");

        spawn_workspace_review_waiter(
            Arc::clone(&state),
            workspace.clone(),
            target,
            completed_run_id.clone(),
        );
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;

        let monitor = state
            .agent_conversation_workspace_repo
            .get_workspace_review_monitor(&workspace.conversation_id)
            .await
            .expect("monitor read should succeed")
            .expect("monitor should exist");
        assert_eq!(monitor.status, AgentWorkspaceReviewMonitorStatus::Ready);
        assert_eq!(
            monitor.last_run_id.as_deref(),
            Some(completed_run_id.as_str())
        );
        assert_eq!(monitor.review_artifact_version, Some(4));
        assert_eq!(monitor.last_error, None);
    }

    #[tokio::test]
    async fn complete_review_run_sets_typed_outcome_and_gate_statuses() {
        let (_temp, repo, base_sha) = init_repo();
        committed_workspace_delta(&repo);

        let state = AppState::new_test();
        let project = seed_project(&state, &repo).await;
        let workspace = workspace(
            &project,
            &repo,
            IdeationAnalysisBaseRefKind::ProjectDefault,
            "main",
            Some(base_sha),
        );
        seed_conversation(&state, &workspace).await;

        let failed = complete_agent_workspace_review_run(
            &state,
            &workspace,
            Some("run_failed".to_string()),
            Some("review failed".to_string()),
            None,
            Some("run-failed".to_string()),
        )
        .await
        .expect("failed completion should persist");
        assert_eq!(failed.status, AgentWorkspaceReviewMonitorStatus::Blocked);
        assert_eq!(
            failed.review_outcome,
            AgentWorkspaceReviewOutcome::RunFailed
        );
        assert_eq!(
            failed.review_gate_status,
            AgentWorkspaceReviewGateStatus::Failed
        );
        assert_eq!(failed.last_run_id.as_deref(), Some("run-failed"));

        let context = load_agent_workspace_review_context(&state, &workspace)
            .await
            .expect("context should load");
        let target = context.target.expect("target should exist");
        let mut ready_monitor = context.monitor;
        apply_review_artifact_to_monitor(
            &mut ready_monitor,
            target.scope,
            target.head_sha.clone(),
            target.diff_fingerprint.clone(),
            Some("run-ready".to_string()),
            ArtifactId::from_string("artifact-ready"),
            3,
            Utc::now(),
            None,
        );
        state
            .agent_conversation_workspace_repo
            .upsert_workspace_review_monitor(ready_monitor)
            .await
            .expect("ready monitor should persist");
        let ready = complete_agent_workspace_review_run(
            &state,
            &workspace,
            Some("passed".to_string()),
            Some("No blocking findings".to_string()),
            None,
            None,
        )
        .await
        .expect("ready completion should persist");
        assert_eq!(ready.status, AgentWorkspaceReviewMonitorStatus::Ready);
        assert_eq!(ready.review_outcome, AgentWorkspaceReviewOutcome::Passed);
        assert_eq!(
            ready.review_gate_status,
            AgentWorkspaceReviewGateStatus::Passed
        );
        assert_eq!(ready.review_artifact_version, Some(3));

        let blocked = complete_agent_workspace_review_run(
            &state,
            &workspace,
            Some("blocking".to_string()),
            Some("Blocking issue summary".to_string()),
            None,
            Some("run-blocked".to_string()),
        )
        .await
        .expect("blocked completion should persist");
        assert_eq!(blocked.status, AgentWorkspaceReviewMonitorStatus::Ready);
        assert_eq!(
            blocked.review_outcome,
            AgentWorkspaceReviewOutcome::Blocking
        );
        assert_eq!(
            blocked.review_gate_status,
            AgentWorkspaceReviewGateStatus::Blocking
        );
        assert_eq!(blocked.last_run_id.as_deref(), Some("run-blocked"));
        assert_eq!(
            blocked.review_blocking_summary.as_deref(),
            Some("Blocking issue summary")
        );
        assert!(blocked.review_blocking_fingerprint.is_some());
        assert_eq!(blocked.review_fixer_status.as_deref(), Some("failed"));
        assert!(blocked.review_fixer_run_id.is_none());
        assert!(blocked.review_fixer_conversation_id.is_none());
        assert!(blocked
            .last_error
            .as_deref()
            .is_some_and(|error| error.contains("Failed to route Review fixer")));
    }

    #[tokio::test]
    async fn mark_workspace_review_blocked_persists_monitor_error() {
        let (_temp, repo, base_sha) = init_repo();
        committed_workspace_delta(&repo);

        let state = AppState::new_test();
        let project = seed_project(&state, &repo).await;
        let workspace = workspace(
            &project,
            &repo,
            IdeationAnalysisBaseRefKind::ProjectDefault,
            "main",
            Some(base_sha),
        );
        let context = load_agent_workspace_review_context(&state, &workspace)
            .await
            .expect("context should load");
        let target = context.target.expect("target should exist");

        mark_workspace_review_blocked(
            &state,
            &workspace,
            &target,
            "helper-1",
            "review failed".to_string(),
        )
        .await;

        let monitor = state
            .agent_conversation_workspace_repo
            .get_workspace_review_monitor(&workspace.conversation_id)
            .await
            .expect("monitor read should succeed")
            .expect("monitor should exist");
        assert_eq!(monitor.status, AgentWorkspaceReviewMonitorStatus::Blocked);
        assert_eq!(monitor.last_run_id.as_deref(), Some("helper-1"));
        assert_eq!(monitor.last_error.as_deref(), Some("review failed"));
        assert_eq!(
            monitor.current_target_scope,
            Some(AgentWorkspaceReviewTargetScope::WorkspaceDelta)
        );
    }

    #[tokio::test]
    async fn stale_workspace_review_block_does_not_clobber_newer_review() {
        let (_temp, repo, base_sha) = init_repo();
        committed_workspace_delta(&repo);

        let state = AppState::new_test();
        let project = seed_project(&state, &repo).await;
        let workspace = workspace(
            &project,
            &repo,
            IdeationAnalysisBaseRefKind::ProjectDefault,
            "main",
            Some(base_sha),
        );
        let context = load_agent_workspace_review_context(&state, &workspace)
            .await
            .expect("context should load");
        let target = context.target.expect("target should exist");
        let mut reviewing_monitor = context.monitor;
        reviewing_monitor.status = AgentWorkspaceReviewMonitorStatus::Reviewing;
        reviewing_monitor.last_run_id = Some("new-run".to_string());
        state
            .agent_conversation_workspace_repo
            .upsert_workspace_review_monitor(reviewing_monitor)
            .await
            .expect("reviewing monitor should persist");

        mark_workspace_review_blocked(
            &state,
            &workspace,
            &target,
            "old-run",
            "old run failed".to_string(),
        )
        .await;

        let monitor = state
            .agent_conversation_workspace_repo
            .get_workspace_review_monitor(&workspace.conversation_id)
            .await
            .expect("monitor read should succeed")
            .expect("monitor should exist");
        assert_eq!(monitor.status, AgentWorkspaceReviewMonitorStatus::Reviewing);
        assert_eq!(monitor.last_run_id.as_deref(), Some("new-run"));
        assert_eq!(monitor.last_error, None);
        assert_eq!(
            monitor.current_diff_fingerprint.as_deref(),
            Some(target.diff_fingerprint.as_str())
        );
    }

    #[test]
    fn review_request_message_and_started_summary_describe_targets() {
        let project_id = crate::domain::entities::ProjectId::new();
        let workspace = AgentConversationWorkspace::new(
            ChatConversationId::from_string("conversation-review-message"),
            project_id,
            AgentConversationWorkspaceMode::Edit,
            IdeationAnalysisBaseRefKind::ProjectDefault,
            "main".to_string(),
            Some("main".to_string()),
            Some("base-sha".to_string()),
            "feature/review".to_string(),
            "/tmp/worktree".to_string(),
        );
        let selected = AgentWorkspaceReviewTarget {
            scope: AgentWorkspaceReviewTargetScope::SelectedSource,
            base_ref: "main".to_string(),
            base_sha: Some("base-sha".to_string()),
            head_ref: "feature/review".to_string(),
            head_sha: Some("head-sha".to_string()),
            diff_fingerprint: "fingerprint".to_string(),
            working_directory: PathBuf::from("/tmp/worktree"),
            source_pull_request_number: Some(483),
            review_packet: AgentWorkspaceReviewPacket::default(),
        };
        let message = build_review_request_message(&workspace, &selected);
        assert!(message.contains("Create or refresh the Review"));
        assert!(message.contains("- Scope: selected_source"));
        assert!(message.contains("- Source pull request: #483"));
        assert!(message.contains("- Review packet: 0 files changed"));
        assert!(message.contains("target.review_packet"));
        assert!(
            message.contains("Do not run shell commands, tests, linters, or validation suites.")
        );
        assert!(message.contains(&workspace.conversation_id.as_str()));
        assert_eq!(
            review_started_summary(&selected),
            "Reviewing selected PR #483 against main."
        );
        assert_eq!(
            workspace_review_conversation_title(&selected),
            "Review PR #483"
        );

        let mut branch = selected.clone();
        branch.source_pull_request_number = None;
        assert_eq!(
            workspace_review_conversation_title(&branch),
            "Review feature/review"
        );
        assert_eq!(
            review_started_summary(&branch),
            "Reviewing selected source branch feature/review against main."
        );

        let mut workspace_delta = selected;
        workspace_delta.scope = AgentWorkspaceReviewTargetScope::WorkspaceDelta;
        assert_eq!(
            workspace_review_conversation_title(&workspace_delta),
            "Review workspace changes"
        );
        assert_eq!(
            review_started_summary(&workspace_delta),
            "Reviewing current workspace changes."
        );
    }
}
