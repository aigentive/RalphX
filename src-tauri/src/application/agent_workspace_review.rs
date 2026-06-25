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
    Ok(build_context(workspace, monitor, target))
}

pub async fn start_agent_workspace_review(
    state: Arc<AppState>,
    workspace: &AgentConversationWorkspace,
    force: bool,
) -> AppResult<AgentWorkspaceReviewStart> {
    let project = state
        .project_repo
        .get_by_id(&workspace.project_id)
        .await?
        .ok_or_else(|| AppError::NotFound("Project not found".to_string()))?;
    let target = resolve_review_target(workspace, &project).await?;
    let mut monitor = load_or_create_monitor(&state, workspace).await?;
    apply_current_target_to_monitor(&mut monitor, target.as_ref());

    let Some(target) = target else {
        monitor.status = AgentWorkspaceReviewMonitorStatus::Idle;
        monitor.last_error = None;
        let monitor = state
            .agent_conversation_workspace_repo
            .upsert_workspace_review_monitor(monitor)
            .await?;
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
    let agent_client = Arc::clone(&runtime.client);
    let helper_harness = runtime.harness.unwrap_or(DEFAULT_AGENT_HARNESS);
    let bootstrap = resolve_harness_agent_bootstrap(
        helper_harness,
        agent_names::AGENT_WORKSPACE_REVIEWER,
        target.working_directory.clone(),
    );
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
            env: bootstrap.env,
        })
        .await
        .map_err(|error| {
            AppError::Infrastructure(format!("failed to spawn workspace reviewer agent: {error}"))
        })?;
    info!(
        target: "ralphx_lib::application::agent_workspace_review",
        conversation_id = %workspace.conversation_id,
        project_id = %workspace.project_id,
        branch = %workspace.branch_name,
        harness = %helper_harness,
        helper_id = %handle.id,
        elapsed_ms = spawn_started.elapsed().as_millis(),
        "Spawned agent workspace Review sidecar"
    );

    monitor.status = AgentWorkspaceReviewMonitorStatus::Reviewing;
    monitor.last_run_id = Some(handle.id.clone());
    monitor.last_error = None;
    let monitor = state
        .agent_conversation_workspace_repo
        .upsert_workspace_review_monitor(monitor)
        .await?;
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
        let output = match agent_client.wait_for_completion(&handle).await {
            Ok(output) => output,
            Err(error) => {
                let error = format!("workspace reviewer agent failed: {error}");
                mark_workspace_review_blocked(&state, &workspace, &target, &handle.id, error).await;
                return;
            }
        };
        info!(
            target: "ralphx_lib::application::agent_workspace_review",
            conversation_id = %workspace.conversation_id,
            project_id = %workspace.project_id,
            branch = %workspace.branch_name,
            harness = %helper_harness,
            helper_id = %handle.id,
            elapsed_ms = wait_started.elapsed().as_millis(),
            success = output.success,
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
                ) && monitor.review_artifact_id.is_some() => {}
            Ok(_) => {
                warn!(
                    target: "ralphx_lib::application::agent_workspace_review",
                    conversation_id = %workspace.conversation_id,
                    helper_id = %handle.id,
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
                    target: "ralphx_lib::application::agent_workspace_review",
                    conversation_id = %workspace.conversation_id,
                    helper_id = %handle.id,
                    error = %error,
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
        target: "ralphx_lib::application::agent_workspace_review",
        conversation_id = %workspace.conversation_id,
        project_id = %workspace.project_id,
        helper_id,
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
                    target: "ralphx_lib::application::agent_workspace_review",
                    conversation_id = %workspace.conversation_id,
                    helper_id,
                    error = %error,
                    "Failed to persist blocked workspace Review monitor"
                );
            }
        }
        Err(load_error) => {
            warn!(
                target: "ralphx_lib::application::agent_workspace_review",
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
