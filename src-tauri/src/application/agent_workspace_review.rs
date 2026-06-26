use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;

use chrono::Utc;
use sha2::{Digest, Sha256};
use tracing::{error, info, warn};

use crate::application::git_service::git_cmd::{self, GitCommandLane};
use crate::application::harness_runtime_registry::resolve_harness_agent_bootstrap;
use crate::application::{AppState, GitService};
use crate::domain::agents::{AgentConfig, AgentHarnessKind, AgentRole, DEFAULT_AGENT_HARNESS};
use crate::domain::entities::{
    AgentConversationWorkspace, AgentWorkspaceReviewMonitor, AgentWorkspaceReviewMonitorStatus,
    AgentWorkspaceReviewTargetScope, Project,
};
use crate::error::{AppError, AppResult};
use crate::infrastructure::agents::claude::agent_names;

const WORKSPACE_REVIEWER_TIMEOUT_SECS: u64 = 900;
const WORKSPACE_REVIEW_LOG_TARGET: &str = "ralphx_lib::application::agent_workspace_review";

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
    let mut monitor = load_or_create_monitor(&state, workspace).await?;
    apply_current_target_to_monitor(&mut monitor, target.as_ref());
    if target.is_none() && monitor.status != AgentWorkspaceReviewMonitorStatus::Reviewing {
        monitor.status = AgentWorkspaceReviewMonitorStatus::Idle;
    }
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
        && monitor.is_current_for_target(
            target.scope,
            target.head_sha.as_deref(),
            &target.diff_fingerprint,
        )
        && monitor.review_artifact_id.is_some()
    {
        monitor.status = AgentWorkspaceReviewMonitorStatus::Ready;
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
    let runtime = state
        .resolve_workspace_reviewer_runtime(&conversation, latest_run.as_ref())
        .await?;
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
    let agent_client = Arc::clone(&runtime.client);
    let helper_harness = runtime.harness.unwrap_or(DEFAULT_AGENT_HARNESS);
    let bootstrap = resolve_harness_agent_bootstrap(
        helper_harness,
        agent_names::AGENT_WORKSPACE_REVIEWER,
        target.working_directory.clone(),
    );
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
        operation = "sidecar_runtime_resolved",
        conversation_id = %workspace.conversation_id,
        project_id = %workspace.project_id,
        branch = %workspace.branch_name,
        elapsed_ms = request_started.elapsed().as_millis(),
        target_scope = %target.scope,
        diff_fingerprint = %compact_log_fingerprint(Some(&target.diff_fingerprint)),
        latest_run_id = %latest_run_id,
        latest_run_harness = %latest_run_harness,
        helper_harness = %helper_harness,
        model = %runtime_model,
        logical_effort = %runtime_effort,
        approval_policy = %runtime_approval_policy,
        sandbox_mode = %runtime_sandbox_mode,
        has_cli_override = runtime.cli_path_override.is_some(),
        working_directory = %bootstrap.working_directory.display(),
        "Resolved workspace Review sidecar runtime"
    );
    let env = runtime.env_with_overrides(bootstrap.env);
    let spawn_started = Instant::now();
    let handle = agent_client
        .spawn_agent(AgentConfig {
            role: AgentRole::Custom(bootstrap.agent_role.clone()),
            prompt: message,
            working_directory: bootstrap.working_directory,
            plugin_dir: Some(bootstrap.plugin_dir),
            agent: Some(bootstrap.agent_name),
            model: runtime.model,
            harness: runtime.harness,
            cli_path_override: runtime.cli_path_override,
            logical_effort: runtime.logical_effort,
            approval_policy: runtime.approval_policy,
            sandbox_mode: runtime.sandbox_mode,
            max_tokens: None,
            timeout_secs: Some(WORKSPACE_REVIEWER_TIMEOUT_SECS),
            env,
        })
        .await
        .map_err(|error| {
            AppError::Infrastructure(format!("failed to spawn workspace reviewer agent: {error}"))
        })?;
    info!(
        target: WORKSPACE_REVIEW_LOG_TARGET,
        operation = "sidecar_spawned",
        conversation_id = %workspace.conversation_id,
        project_id = %workspace.project_id,
        branch = %workspace.branch_name,
        harness = %helper_harness,
        model = %runtime_model,
        logical_effort = %runtime_effort,
        helper_id = %handle.id,
        elapsed_ms = spawn_started.elapsed().as_millis(),
        total_elapsed_ms = request_started.elapsed().as_millis(),
        target_scope = %target.scope,
        diff_fingerprint = %compact_log_fingerprint(Some(&target.diff_fingerprint)),
        "Spawned agent workspace Review sidecar"
    );

    monitor.status = AgentWorkspaceReviewMonitorStatus::Reviewing;
    monitor.last_run_id = Some(handle.id.clone());
    monitor.last_error = None;
    let monitor = state
        .agent_conversation_workspace_repo
        .upsert_workspace_review_monitor(monitor)
        .await?;
    info!(
        target: WORKSPACE_REVIEW_LOG_TARGET,
        operation = "monitor_reviewing",
        conversation_id = %workspace.conversation_id,
        project_id = %workspace.project_id,
        branch = %workspace.branch_name,
        helper_id = %handle.id,
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
        agent_client,
        handle,
        helper_harness,
    );

    Ok(AgentWorkspaceReviewStart {
        context: build_context(workspace, monitor, Some(target)),
        started: true,
        skipped_reason: None,
        was_queued: false,
    })
}

fn spawn_workspace_review_waiter(
    state: Arc<AppState>,
    workspace: AgentConversationWorkspace,
    target: AgentWorkspaceReviewTarget,
    agent_client: Arc<dyn crate::domain::agents::AgenticClient>,
    handle: crate::domain::agents::AgentHandle,
    helper_harness: AgentHarnessKind,
) {
    tokio::spawn(async move {
        let wait_started = Instant::now();
        info!(
            target: WORKSPACE_REVIEW_LOG_TARGET,
            operation = "sidecar_wait_started",
            conversation_id = %workspace.conversation_id,
            project_id = %workspace.project_id,
            branch = %workspace.branch_name,
            harness = %helper_harness,
            helper_id = %handle.id,
            timeout_secs = WORKSPACE_REVIEWER_TIMEOUT_SECS,
            target_scope = %target.scope,
            diff_fingerprint = %compact_log_fingerprint(Some(&target.diff_fingerprint)),
            "Waiting for workspace Review sidecar completion"
        );
        let output = match agent_client.wait_for_completion(&handle).await {
            Ok(output) => output,
            Err(error) => {
                let error = format!("workspace reviewer agent failed: {error}");
                warn!(
                    target: WORKSPACE_REVIEW_LOG_TARGET,
                    operation = "sidecar_wait_failed",
                    conversation_id = %workspace.conversation_id,
                    project_id = %workspace.project_id,
                    branch = %workspace.branch_name,
                    harness = %helper_harness,
                    helper_id = %handle.id,
                    elapsed_ms = wait_started.elapsed().as_millis(),
                    target_scope = %target.scope,
                    diff_fingerprint = %compact_log_fingerprint(Some(&target.diff_fingerprint)),
                    "Workspace Review sidecar wait failed"
                );
                mark_workspace_review_blocked(&state, &workspace, &target, &handle.id, error).await;
                return;
            }
        };
        info!(
            target: WORKSPACE_REVIEW_LOG_TARGET,
            operation = "sidecar_completed",
            conversation_id = %workspace.conversation_id,
            project_id = %workspace.project_id,
            branch = %workspace.branch_name,
            harness = %helper_harness,
            helper_id = %handle.id,
            elapsed_ms = wait_started.elapsed().as_millis(),
            success = output.success,
            output_bytes = output.content.len(),
            target_scope = %target.scope,
            diff_fingerprint = %compact_log_fingerprint(Some(&target.diff_fingerprint)),
            "Agent workspace Review sidecar completed"
        );
        if !output.success {
            let error = format!(
                "workspace reviewer agent exited unsuccessfully: {}",
                output.content.trim()
            );
            mark_workspace_review_blocked(&state, &workspace, &target, &handle.id, error).await;
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
                ) && monitor.review_artifact_id.is_some() =>
            {
                info!(
                    target: WORKSPACE_REVIEW_LOG_TARGET,
                    operation = "sidecar_artifact_verified",
                    conversation_id = %workspace.conversation_id,
                    project_id = %workspace.project_id,
                    branch = %workspace.branch_name,
                    harness = %helper_harness,
                    helper_id = %handle.id,
                    elapsed_ms = wait_started.elapsed().as_millis(),
                    monitor_status = %monitor.status,
                    artifact_id = %monitor.review_artifact_id.as_ref().map(|id| id.as_str()).unwrap_or("none"),
                    artifact_version = monitor.review_artifact_version.unwrap_or_default(),
                    target_scope = %target.scope,
                    diff_fingerprint = %compact_log_fingerprint(Some(&target.diff_fingerprint)),
                    "Verified workspace Review artifact after sidecar completion"
                );
            }
            Ok(_) => {
                warn!(
                    target: WORKSPACE_REVIEW_LOG_TARGET,
                    operation = "sidecar_missing_artifact",
                    conversation_id = %workspace.conversation_id,
                    project_id = %workspace.project_id,
                    branch = %workspace.branch_name,
                    helper_id = %handle.id,
                    elapsed_ms = wait_started.elapsed().as_millis(),
                    target_scope = %target.scope,
                    diff_fingerprint = %compact_log_fingerprint(Some(&target.diff_fingerprint)),
                    "Workspace reviewer sidecar completed without writing a current Review artifact"
                );
                mark_workspace_review_blocked(
                    &state,
                    &workspace,
                    &target,
                    &handle.id,
                    "Workspace reviewer completed without writing a current Review artifact"
                        .to_string(),
                )
                .await;
            }
            Err(error) => {
                error!(
                    target: WORKSPACE_REVIEW_LOG_TARGET,
                    operation = "sidecar_verify_failed",
                    conversation_id = %workspace.conversation_id,
                    project_id = %workspace.project_id,
                    branch = %workspace.branch_name,
                    helper_id = %handle.id,
                    error = %error,
                    elapsed_ms = wait_started.elapsed().as_millis(),
                    target_scope = %target.scope,
                    diff_fingerprint = %compact_log_fingerprint(Some(&target.diff_fingerprint)),
                    "Failed to verify workspace reviewer sidecar completion"
                );
            }
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
    error!(
        target: WORKSPACE_REVIEW_LOG_TARGET,
        operation = "sidecar_blocked",
        conversation_id = %workspace.conversation_id,
        project_id = %workspace.project_id,
        branch = %workspace.branch_name,
        helper_id,
        target_scope = %target.scope,
        diff_fingerprint = %compact_log_fingerprint(Some(&target.diff_fingerprint)),
        error = %error,
        "Workspace Review sidecar failed"
    );
    match load_or_create_monitor(state, workspace).await {
        Ok(mut monitor) => {
            apply_current_target_to_monitor(&mut monitor, Some(target));
            monitor.status = AgentWorkspaceReviewMonitorStatus::Blocked;
            monitor.last_run_id = Some(helper_id.to_string());
            monitor.last_error = Some(error);
            if let Err(error) = state
                .agent_conversation_workspace_repo
                .upsert_workspace_review_monitor(monitor)
                .await
            {
                warn!(
                    target: WORKSPACE_REVIEW_LOG_TARGET,
                    operation = "sidecar_blocked_persist_failed",
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
                operation = "sidecar_blocked_monitor_load_failed",
                conversation_id = %workspace.conversation_id,
                helper_id,
                error = %load_error,
                "Failed to load workspace Review monitor for blocked sidecar"
            );
        }
    }
}

pub async fn complete_agent_workspace_review_run(
    state: &AppState,
    workspace: &AgentConversationWorkspace,
    outcome: Option<String>,
    blocker: Option<String>,
    created_by_run_id: Option<String>,
) -> AppResult<AgentWorkspaceReviewMonitor> {
    let started = Instant::now();
    let has_outcome = outcome
        .as_ref()
        .is_some_and(|value| !value.trim().is_empty());
    let has_blocker = blocker
        .as_ref()
        .is_some_and(|value| !value.trim().is_empty());
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
    monitor.last_error = blocker;
    monitor.status = if monitor.last_error.is_some() {
        AgentWorkspaceReviewMonitorStatus::Blocked
    } else if monitor.review_artifact_id.is_some() {
        AgentWorkspaceReviewMonitorStatus::Ready
    } else {
        AgentWorkspaceReviewMonitorStatus::Idle
    };
    if let Some(outcome) = outcome.filter(|value| !value.trim().is_empty()) {
        monitor.last_error = (monitor.status == AgentWorkspaceReviewMonitorStatus::Blocked)
            .then_some(outcome)
            .or(monitor.last_error);
    }
    state
        .agent_conversation_workspace_repo
        .upsert_workspace_review_monitor(monitor)
        .await
        .inspect(|monitor| {
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
                target_scope = %scope,
                diff_fingerprint = %fingerprint,
                has_artifact = monitor.review_artifact_id.is_some(),
                artifact_id = %monitor.review_artifact_id.as_ref().map(|id| id.as_str()).unwrap_or("none"),
                created_by_run_id = %created_by_run_id_label,
                has_outcome,
                has_blocker,
                "Completed workspace Review run"
            );
        })
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
    monitor.status = AgentWorkspaceReviewMonitorStatus::Ready;
    monitor.reviewed_target_scope = Some(target_scope);
    monitor.reviewed_head_sha = target_head_sha;
    monitor.reviewed_diff_fingerprint = Some(target_diff_fingerprint.clone());
    monitor.current_target_scope = Some(target_scope);
    monitor.current_diff_fingerprint = Some(target_diff_fingerprint);
    monitor.review_artifact_id = Some(artifact_id);
    monitor.review_artifact_version = Some(artifact_version);
    monitor.review_artifact_updated_at = Some(artifact_created_at);
    monitor.previous_version_id = previous_artifact_id;
    monitor.last_run_id = created_by_run_id.or(monitor.last_run_id.take());
    monitor.last_error = None;
}

fn build_context(
    workspace: &AgentConversationWorkspace,
    monitor: AgentWorkspaceReviewMonitor,
    target: Option<AgentWorkspaceReviewTarget>,
) -> AgentWorkspaceReviewContext {
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

    Ok(Some(AgentWorkspaceReviewTarget {
        scope: AgentWorkspaceReviewTargetScope::WorkspaceDelta,
        base_ref,
        base_sha,
        head_ref,
        head_sha,
        diff_fingerprint: fingerprint,
        working_directory: worktree_path,
        source_pull_request_number: None,
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

    Ok(Some(AgentWorkspaceReviewTarget {
        scope: AgentWorkspaceReviewTargetScope::SelectedSource,
        base_ref,
        base_sha,
        head_ref,
        head_sha,
        diff_fingerprint: fingerprint,
        working_directory: repo_path,
        source_pull_request_number: pr_number,
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
        "Create or refresh the Review artifact for this agent conversation.\n\n\
         Target:\n\
         - Scope: {scope}\n\
         - Base: {base_ref} ({base_sha})\n\
         - Head: {head_ref} ({head_sha})\n\
         - Diff fingerprint: {fingerprint}\n\
         {pr_line}\
         - Workspace conversation: {conversation_id}\n\n\
         This is a background sidecar run, so pass conversation_id `{conversation_id}` explicitly to every workspace Review tool call. \
         Inspect the target diff, write a concise reviewer-focused Markdown Review artifact with the `write_workspace_review_artifact` tool, then call `complete_workspace_review_run`. Do not modify files.",
        scope = target.scope,
        base_ref = target.base_ref,
        base_sha = target.base_sha.as_deref().unwrap_or("unknown"),
        head_ref = target.head_ref,
        head_sha = target.head_sha.as_deref().unwrap_or("unknown"),
        fingerprint = target.diff_fingerprint,
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
    use crate::domain::agents::AgenticClient;
    use crate::domain::entities::{
        AgentConversationWorkspaceMode, AgentWorkspaceSourcePullRequest, ArtifactId,
        ChatConversation, ChatConversationId, IdeationAnalysisBaseRefKind,
    };
    use crate::infrastructure::{MockAgenticClient, MockCallType};
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
        let target = context.target.expect("workspace delta should be reviewable");

        assert_eq!(target.scope, AgentWorkspaceReviewTargetScope::WorkspaceDelta);
        assert_eq!(target.base_ref, base_sha);
        assert_eq!(target.head_ref, "HEAD");
        assert!(target.base_sha.is_some());
        assert!(target.head_sha.is_some());
        assert!(!target.diff_fingerprint.is_empty());
        assert_eq!(target.working_directory, repo);
        assert!(!context.is_current);
        assert!(!context.is_outdated);
        assert!(context.should_show_tab);
        assert_eq!(
            context.monitor.current_target_scope,
            Some(AgentWorkspaceReviewTargetScope::WorkspaceDelta)
        );
        assert_eq!(context.monitor.workspace_head_ref.as_deref(), Some("HEAD"));
        assert_eq!(context.monitor.workspace_base_ref.as_deref(), Some(base_sha.as_str()));
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
        let target = context.target.expect("selected branch should be reviewable");

        assert_eq!(
            target.scope,
            AgentWorkspaceReviewTargetScope::SelectedSource
        );
        assert_eq!(target.base_ref, "main");
        assert_eq!(target.head_ref, "feature/source");
        assert_eq!(target.head_sha.as_deref(), Some(feature_head.as_str()));
        assert_eq!(target.source_pull_request_number, None);
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
        std::fs::write(repo.join("pr.rs"), "pub fn pr() {}\n")
            .expect("pr file should be written");
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
        assert_eq!(context.monitor.status, AgentWorkspaceReviewMonitorStatus::Idle);
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
        assert_eq!(current.monitor.status, AgentWorkspaceReviewMonitorStatus::Ready);

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
    async fn start_review_spawns_workspace_reviewer_sidecar_and_records_blocked_completion() {
        let (_temp, repo, base_sha) = init_repo();
        committed_workspace_delta(&repo);

        let client = Arc::new(MockAgenticClient::new());
        let agent_client: Arc<dyn AgenticClient> = client.clone();
        let state = Arc::new(AppState::new_test().with_agent_client(agent_client));
        let project = seed_project(&state, &repo).await;
        let workspace = workspace(
            &project,
            &repo,
            IdeationAnalysisBaseRefKind::ProjectDefault,
            "main",
            Some(base_sha),
        );
        seed_conversation(&state, &workspace).await;

        let start = start_agent_workspace_review(Arc::clone(&state), &workspace, true)
            .await
            .expect("review sidecar should start");

        assert!(start.started);
        assert_eq!(start.skipped_reason, None);
        assert_eq!(
            start.context.monitor.status,
            AgentWorkspaceReviewMonitorStatus::Reviewing
        );
        assert!(start.context.monitor.last_run_id.is_some());
        let spawn_calls = client.get_spawn_calls().await;
        assert_eq!(spawn_calls.len(), 1);
        let MockCallType::Spawn { role, prompt } = &spawn_calls[0].call_type else {
            panic!("expected spawn call");
        };
        assert_eq!(
            role,
            &AgentRole::Custom("ralphx-workspace-reviewer".to_string())
        );
        assert!(prompt.contains("Create or refresh the Review artifact"));
        assert!(prompt.contains("- Scope: workspace_delta"));
        assert!(prompt.contains(&workspace.conversation_id.as_str()));

        let mut blocked_monitor = None;
        for _ in 0..50 {
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
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
        let blocked_monitor = blocked_monitor.expect("waiter should mark missing artifact blocked");
        assert_eq!(
            blocked_monitor.last_run_id,
            start.context.monitor.last_run_id
        );
        assert_eq!(
            blocked_monitor.last_error.as_deref(),
            Some("Workspace reviewer completed without writing a current Review artifact")
        );
        assert!(client
            .get_calls_for_handle(
                start
                    .context
                    .monitor
                    .last_run_id
                    .as_deref()
                    .expect("run id should exist")
            )
            .await
            .iter()
            .any(|call| matches!(call.call_type, MockCallType::WaitForCompletion { .. })));
    }

    #[tokio::test]
    async fn complete_review_run_sets_ready_idle_and_blocked_statuses() {
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

        let idle = complete_agent_workspace_review_run(
            &state,
            &workspace,
            Some("no_changes".to_string()),
            None,
            Some("run-idle".to_string()),
        )
        .await
        .expect("idle completion should persist");
        assert_eq!(idle.status, AgentWorkspaceReviewMonitorStatus::Idle);
        assert_eq!(idle.last_run_id.as_deref(), Some("run-idle"));

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
        let ready =
            complete_agent_workspace_review_run(&state, &workspace, None, None, None)
                .await
                .expect("ready completion should persist");
        assert_eq!(ready.status, AgentWorkspaceReviewMonitorStatus::Ready);
        assert_eq!(ready.review_artifact_version, Some(3));

        let blocked = complete_agent_workspace_review_run(
            &state,
            &workspace,
            Some("blocked".to_string()),
            Some("tool failed".to_string()),
            Some("run-blocked".to_string()),
        )
        .await
        .expect("blocked completion should persist");
        assert_eq!(blocked.status, AgentWorkspaceReviewMonitorStatus::Blocked);
        assert_eq!(blocked.last_run_id.as_deref(), Some("run-blocked"));
        assert_eq!(blocked.last_error.as_deref(), Some("blocked"));
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
        };
        let message = build_review_request_message(&workspace, &selected);
        assert!(message.contains("Create or refresh the Review artifact"));
        assert!(message.contains("- Scope: selected_source"));
        assert!(message.contains("- Source pull request: #483"));
        assert!(message.contains(&workspace.conversation_id.as_str()));
        assert_eq!(
            review_started_summary(&selected),
            "Reviewing selected PR #483 against main."
        );

        let mut branch = selected.clone();
        branch.source_pull_request_number = None;
        assert_eq!(
            review_started_summary(&branch),
            "Reviewing selected source branch feature/review against main."
        );

        let mut workspace_delta = selected;
        workspace_delta.scope = AgentWorkspaceReviewTargetScope::WorkspaceDelta;
        assert_eq!(
            review_started_summary(&workspace_delta),
            "Reviewing current workspace changes."
        );
    }
}
