use std::{
    path::{Component, Path, PathBuf},
    time::Instant,
};

use chrono::Utc;
use sha2::{Digest, Sha256};

use crate::application::agent_conversation_workspace_base::{
    apply_workspace_base_resolution, is_commit_contained_in,
    resolve_workspace_base_from_local_snapshot, BaseStatus,
};
use crate::application::git_service::GitService;
use crate::domain::entities::{
    is_terminal_publication_pr_status, AgentConversationWorkspace,
    AgentConversationWorkspaceBranchMode, AgentConversationWorkspaceMode,
    AgentWorkspaceSourcePullRequest, ChatConversationId, IdeationAnalysisBaseRefKind, PlanBranch,
    Project, Task,
};
use crate::domain::execution::ExecutionSettings;
use crate::domain::repositories::PlanBranchRepository;
use crate::domain::state_machine::transition_handler::run_pre_execution_setup;
use crate::error::{AppError, AppResult};
use crate::infrastructure::agents::claude::agent_names::{
    AGENT_AUTOMATION_SETUP, AGENT_CHAT_PROJECT, AGENT_GENERAL_EXPLORER, AGENT_GENERAL_WORKER,
    AGENT_ORCHESTRATOR_IDEATION, AGENT_PERSONA_EXTRACTOR, AGENT_PR_REVIEWER, AGENT_TASK_MANAGER,
};

pub const AGENT_CONVERSATION_WORKSPACE_CONTINUATION_MESSAGE: &str =
    "A new workspace branch has been created automatically.";
pub(crate) const REVIEW_PR_SOURCE_PULL_REQUEST_REQUIRED_ERROR: &str =
    "Review PR mode requires a selected pull request";
pub(crate) const PERSONA_BUILDER_GENERIC_ENTRY_POINT_ERROR: &str =
    "PersonaBuilder mode can only be created through the persona builder command";

#[derive(Debug, Clone, Default)]
pub struct AgentConversationWorkspaceBaseSelection {
    pub kind: Option<IdeationAnalysisBaseRefKind>,
    pub branch_mode: Option<AgentConversationWorkspaceBranchMode>,
    pub base_ref: Option<String>,
    pub display_name: Option<String>,
    pub source_pull_request: Option<AgentWorkspaceSourcePullRequest>,
}

impl AgentConversationWorkspaceBaseSelection {
    pub(crate) fn for_workspace_reuse(workspace: &AgentConversationWorkspace) -> Self {
        let source_pull_request = workspace.source_pull_request.clone();
        let (kind, base_ref) = if let Some(source_pull_request) = source_pull_request.as_ref() {
            (
                Some(IdeationAnalysisBaseRefKind::LocalBranch),
                Some(source_pull_request.head_ref_name.clone()),
            )
        } else {
            (
                Some(workspace.base_ref_kind),
                Some(workspace.base_ref.clone()),
            )
        };

        Self {
            kind,
            branch_mode: Some(workspace.branch_mode),
            base_ref,
            display_name: workspace.base_display_name.clone(),
            source_pull_request,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentConversationWorkspaceBranchNameHint {
    pub provider: String,
    pub ticket_token: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgentConversationWorkspaceSetupMode {
    Blocking,
    Deferred,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct AgentConversationWorkspacePrAutomationDefaults {
    pub autofix_enabled: bool,
    pub auto_merge_desired: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedAgentConversationWorkspacePath {
    pub path: PathBuf,
    pub branch_name: String,
}

impl From<&ExecutionSettings> for AgentConversationWorkspacePrAutomationDefaults {
    fn from(settings: &ExecutionSettings) -> Self {
        Self {
            autofix_enabled: settings.agent_workspace_pr_autofix_default,
            auto_merge_desired: settings.agent_workspace_pr_auto_merge_default,
        }
    }
}

pub async fn prepare_agent_conversation_workspace(
    project: &Project,
    conversation_id: &ChatConversationId,
    mode: AgentConversationWorkspaceMode,
    selection: AgentConversationWorkspaceBaseSelection,
) -> AppResult<AgentConversationWorkspace> {
    prepare_agent_conversation_workspace_with_setup_mode(
        project,
        conversation_id,
        mode,
        selection,
        AgentConversationWorkspaceSetupMode::Blocking,
    )
    .await
}

pub async fn prepare_agent_conversation_workspace_with_setup_mode(
    project: &Project,
    conversation_id: &ChatConversationId,
    mode: AgentConversationWorkspaceMode,
    selection: AgentConversationWorkspaceBaseSelection,
    setup_mode: AgentConversationWorkspaceSetupMode,
) -> AppResult<AgentConversationWorkspace> {
    prepare_agent_conversation_workspace_with_setup_mode_and_defaults(
        project,
        conversation_id,
        mode,
        selection,
        setup_mode,
        AgentConversationWorkspacePrAutomationDefaults::default(),
        false,
    )
    .await
}

/// Prepare an agent conversation workspace.
///
/// `prefer_advanced_origin_base` scopes the automation integration-branch model
/// (see `base-integration-branch-spec.md`): when `true` and the resolved base is
/// an isolated `LocalBranch`, the successor worktree is cut from the (advanced)
/// remote-tracking ref `origin/<base>` instead of the stale, permanently
/// checked-out local automation branch. Non-automation callers pass `false` and
/// keep the existing local start-point behavior.
#[allow(clippy::too_many_arguments)]
pub async fn prepare_agent_conversation_workspace_with_setup_mode_and_defaults(
    project: &Project,
    conversation_id: &ChatConversationId,
    mode: AgentConversationWorkspaceMode,
    selection: AgentConversationWorkspaceBaseSelection,
    setup_mode: AgentConversationWorkspaceSetupMode,
    pr_automation_defaults: AgentConversationWorkspacePrAutomationDefaults,
    prefer_advanced_origin_base: bool,
) -> AppResult<AgentConversationWorkspace> {
    prepare_agent_conversation_workspace_with_setup_mode_defaults_and_branch_name_hint(
        project,
        conversation_id,
        mode,
        selection,
        setup_mode,
        pr_automation_defaults,
        prefer_advanced_origin_base,
        None,
    )
    .await
}

/// Prepare an agent conversation workspace.
///
/// `prefer_advanced_origin_base` scopes the automation integration-branch model
/// (see `base-integration-branch-spec.md`): when `true` and the resolved base is
/// an isolated `LocalBranch`, the successor worktree is cut from the (advanced)
/// remote-tracking ref `origin/<base>` instead of the stale, permanently
/// checked-out local automation branch. Non-automation callers pass `false` and
/// keep the existing local start-point behavior.
#[allow(clippy::too_many_arguments)]
pub async fn prepare_agent_conversation_workspace_with_setup_mode_defaults_and_branch_name_hint(
    project: &Project,
    conversation_id: &ChatConversationId,
    mode: AgentConversationWorkspaceMode,
    selection: AgentConversationWorkspaceBaseSelection,
    setup_mode: AgentConversationWorkspaceSetupMode,
    pr_automation_defaults: AgentConversationWorkspacePrAutomationDefaults,
    prefer_advanced_origin_base: bool,
    branch_name_hint: Option<AgentConversationWorkspaceBranchNameHint>,
) -> AppResult<AgentConversationWorkspace> {
    let total_started = Instant::now();
    let repo_path = PathBuf::from(&project.working_directory);
    tracing::info!(
        conversation_id = %conversation_id,
        project_id = %project.id,
        mode = %mode,
        setup_mode = ?setup_mode,
        selected_base_kind = ?selection.kind,
        selected_branch_mode = ?selection.branch_mode,
        selected_base_ref = ?selection.base_ref,
        "Agent conversation workspace prepare started"
    );

    let selected_kind = selection.kind;
    let selected_branch_mode = selection.branch_mode;
    let source_pull_request = selection.source_pull_request;
    validate_review_pr_workspace_source_pull_request(mode, source_pull_request.as_ref())?;
    let selected_base_ref = selection
        .base_ref
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string);
    let selected_display_name = selection
        .display_name
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string);

    let branch_probe_started = Instant::now();
    let needs_current_branch = selected_kind.is_none()
        || (selected_kind == Some(IdeationAnalysisBaseRefKind::CurrentBranch)
            && selected_base_ref.is_none());
    let needs_project_default = selected_kind.is_none()
        || (selected_kind == Some(IdeationAnalysisBaseRefKind::ProjectDefault)
            && selected_base_ref.is_none())
        || selected_branch_mode == Some(AgentConversationWorkspaceBranchMode::Linked);
    let current_branch_probe_started = Instant::now();
    let project_default_probe_started = Instant::now();
    let (current_branch, project_default) = tokio::join!(
        async {
            if !needs_current_branch {
                tracing::info!(
                    conversation_id = %conversation_id,
                    phase = "resolve_current_branch",
                    elapsed_ms = current_branch_probe_started.elapsed().as_millis() as u64,
                    result = ?Option::<&str>::None,
                    error = ?Option::<String>::None,
                    skipped = true,
                    "Agent conversation workspace probe completed"
                );
                return Ok(None);
            }

            let result = GitService::get_current_branch(&repo_path).await;
            log_agent_workspace_probe_result(
                conversation_id,
                "resolve_current_branch",
                current_branch_probe_started,
                result.as_ref().ok().map(String::as_str),
                result.as_ref().err().map(ToString::to_string),
            );
            result.map(Some)
        },
        async {
            if !needs_project_default {
                tracing::info!(
                    conversation_id = %conversation_id,
                    phase = "resolve_project_default_branch",
                    elapsed_ms = project_default_probe_started.elapsed().as_millis() as u64,
                    configured_base_branch = ?project.base_branch,
                    branch = ?Option::<&str>::None,
                    skipped = true,
                    "Agent conversation workspace probe completed"
                );
                return None;
            }

            let branch = GitService::resolve_project_default_branch(
                &repo_path,
                project.base_branch.as_deref(),
            )
            .await;
            tracing::info!(
                conversation_id = %conversation_id,
                phase = "resolve_project_default_branch",
                elapsed_ms = project_default_probe_started.elapsed().as_millis() as u64,
                configured_base_branch = ?project.base_branch,
                branch = %branch,
                skipped = false,
                "Agent conversation workspace probe completed"
            );
            Some(branch)
        }
    );
    let current_branch = current_branch
        .ok()
        .flatten()
        .filter(|branch| branch != "HEAD");
    log_agent_workspace_phase(
        conversation_id,
        "resolve_base_context",
        branch_probe_started,
    );

    let kind = selected_kind.unwrap_or_else(|| {
        let project_default = project_default
            .as_deref()
            .unwrap_or_else(|| project.base_branch.as_deref().unwrap_or("main"));
        if current_branch
            .as_deref()
            .is_some_and(|branch| branch != project_default)
        {
            IdeationAnalysisBaseRefKind::CurrentBranch
        } else {
            IdeationAnalysisBaseRefKind::ProjectDefault
        }
    });

    if kind == IdeationAnalysisBaseRefKind::PullRequest {
        return Err(AppError::Validation(
            "PR-backed agent conversation base refs require PR workspace provisioning and are not enabled in this slice"
                .to_string(),
        ));
    }

    let branch_mode =
        resolve_agent_conversation_workspace_branch_mode(mode, kind, selected_branch_mode);

    let selected_work_ref = match kind {
        IdeationAnalysisBaseRefKind::ProjectDefault => selected_base_ref
            .clone()
            .or_else(|| project_default.clone())
            .unwrap_or_else(|| "main".to_string()),
        IdeationAnalysisBaseRefKind::CurrentBranch => selected_base_ref
            .clone()
            .or(current_branch)
            .ok_or_else(|| AppError::Validation("Unable to resolve current branch".to_string()))?,
        IdeationAnalysisBaseRefKind::LocalBranch => selected_base_ref.clone().ok_or_else(|| {
            AppError::Validation(
                "Local branch agent conversation base requires base_ref".to_string(),
            )
        })?,
        IdeationAnalysisBaseRefKind::PullRequest => unreachable!("handled above"),
    };
    let selected_work_ref = if kind == IdeationAnalysisBaseRefKind::LocalBranch {
        GitService::ensure_local_branch_from_origin_if_missing(&repo_path, &selected_work_ref)
            .await?
    } else {
        selected_work_ref
    };

    let target_base_ref = if branch_mode == AgentConversationWorkspaceBranchMode::Linked {
        let source_pr_base = source_pull_request
            .as_ref()
            .and_then(|pull_request| pull_request.base_ref_name.as_deref())
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string);
        let fallback_default = project_default
            .clone()
            .or_else(|| project.base_branch.clone())
            .unwrap_or_else(|| "main".to_string());
        source_pr_base.unwrap_or(fallback_default)
    } else {
        selected_work_ref.clone()
    };
    let target_base_ref =
        GitService::ensure_local_branch_from_origin_if_missing(&repo_path, &target_base_ref)
            .await?;

    // Automation integration-branch model (B3/B7): a successor run's worktree must
    // build on merged work. The automation base branch is permanently checked out
    // in the setup worktree, so the local ref can never be fast-forwarded. Instead
    // advance the remote-tracking ref `origin/<base>` (best-effort maintenance
    // fetch, never forced) and cut the worktree from it when present. The stored
    // `base_ref` stays the plain branch name so the run still publishes its PR with
    // `--base <automation-branch>`. Non-automation workspaces keep the local
    // start-point unchanged.
    let checkout_ref = if prefer_advanced_origin_base
        && kind == IdeationAnalysisBaseRefKind::LocalBranch
        && branch_mode == AgentConversationWorkspaceBranchMode::Isolated
    {
        match GitService::try_fetch_origin_ref_for_maintenance(&repo_path, &target_base_ref).await {
            Ok(outcome) => tracing::info!(
                conversation_id = %conversation_id,
                base_ref = %target_base_ref,
                fetch_outcome = ?outcome,
                "Advanced origin base ref for automation successor worktree"
            ),
            Err(error) => tracing::warn!(
                conversation_id = %conversation_id,
                base_ref = %target_base_ref,
                error = %error,
                "Failed to advance origin base ref for automation successor; using current tip"
            ),
        }
        let remote_ref = format!("origin/{target_base_ref}");
        if GitService::ref_exists(&repo_path, &remote_ref)
            .await
            .unwrap_or(false)
        {
            remote_ref
        } else {
            target_base_ref.clone()
        }
    } else {
        target_base_ref.clone()
    };

    let project_default_ref = project_default
        .as_deref()
        .or(project.base_branch.as_deref())
        .unwrap_or("main");
    let stored_base_kind = if branch_mode == AgentConversationWorkspaceBranchMode::Linked {
        if target_base_ref == project_default_ref {
            IdeationAnalysisBaseRefKind::ProjectDefault
        } else {
            IdeationAnalysisBaseRefKind::LocalBranch
        }
    } else {
        kind
    };

    let ref_check_started = Instant::now();
    let base_commit = GitService::get_branch_sha(&repo_path, &checkout_ref)
        .await
        .map_err(|error| {
            AppError::Validation(format!(
                "Agent conversation base ref '{}' does not exist in the project repository: {}",
                checkout_ref, error
            ))
        })?;
    log_agent_workspace_phase(conversation_id, "validate_base_ref", ref_check_started);

    let display_name = selected_display_name.unwrap_or_else(|| match kind {
        IdeationAnalysisBaseRefKind::ProjectDefault => {
            format!("Project default ({selected_work_ref})")
        }
        IdeationAnalysisBaseRefKind::CurrentBranch => {
            format!("Current branch ({selected_work_ref})")
        }
        IdeationAnalysisBaseRefKind::LocalBranch => selected_work_ref.clone(),
        IdeationAnalysisBaseRefKind::PullRequest => selected_work_ref.clone(),
    });
    let branch_name = if branch_mode == AgentConversationWorkspaceBranchMode::Linked {
        selected_work_ref.clone()
    } else {
        agent_conversation_branch_name_for_mode_with_hint(
            project,
            conversation_id,
            mode,
            branch_name_hint.as_ref(),
        )
    };
    let workspace_path_started = Instant::now();
    let worktree_path = resolve_agent_conversation_workspace_path(project, conversation_id)?;
    tracing::info!(
        conversation_id = %conversation_id,
        phase = "resolve_workspace_path",
        elapsed_ms = workspace_path_started.elapsed().as_millis() as u64,
        branch_name = %branch_name,
        worktree_path = %worktree_path.display(),
        "Agent conversation workspace path resolved"
    );

    let worktree_started = Instant::now();
    if branch_mode == AgentConversationWorkspaceBranchMode::Linked {
        ensure_linked_agent_conversation_branch_worktree(&repo_path, &worktree_path, &branch_name)
            .await?;
    } else {
        ensure_agent_conversation_worktree(&repo_path, &worktree_path, &branch_name, &checkout_ref)
            .await?;
    }
    log_agent_workspace_phase(conversation_id, "ensure_worktree", worktree_started);

    let setup_started = Instant::now();
    run_or_defer_agent_conversation_workspace_setup(
        project,
        conversation_id,
        &worktree_path,
        &branch_name,
        setup_mode,
    )
    .await;
    log_agent_workspace_phase(conversation_id, "workspace_setup", setup_started);
    tracing::info!(
        conversation_id = %conversation_id,
        phase = "capture_base_commit",
        elapsed_ms = 0u64,
        source_ref = %target_base_ref,
        "Agent conversation workspace phase completed"
    );
    log_agent_workspace_phase(conversation_id, "prepare_total", total_started);
    let (publication_pr_number, publication_pr_url, publication_pr_status) =
        if branch_mode == AgentConversationWorkspaceBranchMode::Linked {
            source_pull_request
                .as_ref()
                .map(|pull_request| {
                    (
                        Some(pull_request.number),
                        pull_request.url.clone(),
                        Some("open".to_string()),
                    )
                })
                .unwrap_or((None, None, None))
        } else {
            (None, None, None)
        };
    let pr_automation_defaults = if mode == AgentConversationWorkspaceMode::ReviewPr {
        AgentConversationWorkspacePrAutomationDefaults::default()
    } else {
        pr_automation_defaults
    };

    Ok(AgentConversationWorkspace {
        conversation_id: conversation_id.clone(),
        project_id: project.id.clone(),
        mode,
        branch_mode,
        base_ref_kind: stored_base_kind,
        base_ref: target_base_ref,
        base_display_name: Some(display_name),
        base_commit: Some(base_commit),
        branch_name,
        worktree_path: worktree_path.to_string_lossy().to_string(),
        linked_ideation_session_id: None,
        task_pipeline_session_id: None,
        linked_plan_branch_id: None,
        source_pull_request,
        publication_pr_number,
        publication_pr_url,
        publication_pr_status,
        publication_push_status: None,
        publication_metadata_phase: None,
        publication_metadata_state: None,
        publication_metadata_attempt_id: None,
        auto_publish_enabled: true,
        auto_publish_initial_pr_enabled: false,
        auto_publish_paused_pr_autofix_enabled: None,
        auto_publish_paused_pr_auto_merge_desired: None,
        pr_autofix_enabled: pr_automation_defaults.autofix_enabled,
        pr_auto_merge_desired: pr_automation_defaults.auto_merge_desired,
        pr_auto_merge_method: crate::domain::entities::DEFAULT_AGENT_WORKSPACE_PR_AUTO_MERGE_METHOD
            .to_string(),
        pr_auto_merge_current: None,
        pr_supervision_status: None,
        pr_supervision_summary: None,
        pr_supervision_updated_at: None,
        status: crate::domain::entities::AgentConversationWorkspaceStatus::Active,
        created_at: Utc::now(),
        updated_at: Utc::now(),
    })
}

pub async fn rollover_agent_conversation_workspace(
    project: &Project,
    workspace: &AgentConversationWorkspace,
) -> AppResult<AgentConversationWorkspace> {
    rollover_agent_conversation_workspace_with_setup_mode(
        project,
        workspace,
        AgentConversationWorkspaceSetupMode::Blocking,
    )
    .await
}

pub async fn rollover_agent_conversation_workspace_with_setup_mode(
    project: &Project,
    workspace: &AgentConversationWorkspace,
    setup_mode: AgentConversationWorkspaceSetupMode,
) -> AppResult<AgentConversationWorkspace> {
    let total_started = Instant::now();
    if !is_terminal_agent_conversation_publication_status(
        workspace.publication_pr_status.as_deref(),
    ) {
        return Ok(workspace.clone());
    }

    if workspace.project_id != project.id {
        return Err(AppError::Validation(format!(
            "Agent conversation workspace {} belongs to project {} instead of {}",
            workspace.conversation_id, workspace.project_id, project.id
        )));
    }

    let repo_path = PathBuf::from(&project.working_directory);
    let expected_path =
        resolve_agent_conversation_workspace_path(project, &workspace.conversation_id)?;
    let stored_path = PathBuf::from(&workspace.worktree_path);
    if stored_path != expected_path {
        return Err(AppError::Validation(format!(
            "Agent conversation workspace path mismatch for conversation {}",
            workspace.conversation_id
        )));
    }

    let project_root = PathBuf::from(&project.working_directory);
    if expected_path == project_root {
        return Err(AppError::Validation(format!(
            "Agent conversation workspace {} points to the project root",
            workspace.conversation_id
        )));
    }

    if expected_path.exists() {
        if !expected_path.is_dir() {
            return Err(AppError::Validation(format!(
                "Agent conversation workspace path exists but is not a directory: {}",
                expected_path.display()
            )));
        }

        let dirty_check_started = Instant::now();
        if GitService::has_uncommitted_changes(&expected_path).await? {
            return Err(AppError::Validation(format!(
                "Cannot continue agent conversation {} on a new branch because the old workspace has uncommitted changes",
                workspace.conversation_id
            )));
        }
        log_agent_workspace_phase(
            &workspace.conversation_id,
            "rollover_dirty_check",
            dirty_check_started,
        );
    }

    let base_resolution_started = Instant::now();
    let base_resolution = resolve_workspace_base_from_local_snapshot(project, workspace).await?;
    log_agent_workspace_phase(
        &workspace.conversation_id,
        "rollover_resolve_base",
        base_resolution_started,
    );
    if base_resolution.status == BaseStatus::Blocked {
        return Err(AppError::Validation(
            base_resolution
                .block_reason
                .clone()
                .unwrap_or_else(|| "Agent workspace base is blocked".to_string()),
        ));
    }
    if base_resolution.status == BaseStatus::Retargeted {
        let old_branch_head = GitService::get_branch_sha(&repo_path, &workspace.branch_name)
            .await
            .map_err(|error| {
                AppError::Validation(format!(
                    "Cannot verify old workspace branch containment before rollover: {error}"
                ))
            })?;
        let target_ref = base_resolution.effective_checkout_ref()?;
        if !is_commit_contained_in(&repo_path, &old_branch_head, target_ref).await? {
            return Err(AppError::Validation(format!(
                "Cannot continue agent conversation {} on a new branch because old workspace branch HEAD is not contained in the default branch",
                workspace.conversation_id
            )));
        }
    }

    if expected_path.exists() {
        let delete_started = Instant::now();
        GitService::delete_worktree(&repo_path, &expected_path).await?;
        log_agent_workspace_phase(
            &workspace.conversation_id,
            "rollover_delete_old_worktree",
            delete_started,
        );
    }

    let base_checkout_ref = base_resolution.effective_checkout_ref()?.to_string();
    let branch_name = agent_conversation_continuation_branch_name(&workspace.branch_name);

    let worktree_started = Instant::now();
    ensure_agent_conversation_worktree(
        &repo_path,
        &expected_path,
        &branch_name,
        &base_checkout_ref,
    )
    .await?;
    log_agent_workspace_phase(
        &workspace.conversation_id,
        "rollover_ensure_worktree",
        worktree_started,
    );
    let setup_started = Instant::now();
    run_or_defer_agent_conversation_workspace_setup(
        project,
        &workspace.conversation_id,
        &expected_path,
        &branch_name,
        setup_mode,
    )
    .await;
    log_agent_workspace_phase(
        &workspace.conversation_id,
        "rollover_workspace_setup",
        setup_started,
    );
    let base_commit_started = Instant::now();
    let base_commit = GitService::get_branch_sha(&repo_path, &branch_name).await?;
    log_agent_workspace_phase(
        &workspace.conversation_id,
        "rollover_capture_base_commit",
        base_commit_started,
    );
    let mut updated = workspace.clone();
    apply_workspace_base_resolution(&mut updated, &base_resolution)?;
    updated.base_commit = Some(base_commit);
    updated.branch_name = branch_name;
    updated.worktree_path = expected_path.to_string_lossy().to_string();
    updated.publication_pr_number = None;
    updated.publication_pr_url = None;
    updated.publication_pr_status = None;
    updated.publication_push_status = None;
    updated.status = crate::domain::entities::AgentConversationWorkspaceStatus::Active;
    updated.updated_at = Utc::now();
    log_agent_workspace_phase(&workspace.conversation_id, "rollover_total", total_started);
    Ok(updated)
}

pub fn is_terminal_agent_conversation_publication_status(status: Option<&str>) -> bool {
    is_terminal_publication_pr_status(status)
}

pub(crate) fn validate_review_pr_workspace_source_pull_request(
    mode: AgentConversationWorkspaceMode,
    source_pull_request: Option<&AgentWorkspaceSourcePullRequest>,
) -> AppResult<()> {
    if mode == AgentConversationWorkspaceMode::ReviewPr && source_pull_request.is_none() {
        return Err(AppError::Validation(
            REVIEW_PR_SOURCE_PULL_REQUEST_REQUIRED_ERROR.to_string(),
        ));
    }
    Ok(())
}

fn resolve_agent_conversation_workspace_branch_mode(
    mode: AgentConversationWorkspaceMode,
    kind: IdeationAnalysisBaseRefKind,
    selected_branch_mode: Option<AgentConversationWorkspaceBranchMode>,
) -> AgentConversationWorkspaceBranchMode {
    if mode == AgentConversationWorkspaceMode::ReviewPr
        || kind == IdeationAnalysisBaseRefKind::ProjectDefault
    {
        return AgentConversationWorkspaceBranchMode::Isolated;
    }
    selected_branch_mode.unwrap_or_default()
}

fn log_agent_workspace_phase(
    conversation_id: &ChatConversationId,
    phase: &'static str,
    started: Instant,
) {
    tracing::info!(
        conversation_id = %conversation_id,
        phase,
        elapsed_ms = started.elapsed().as_millis() as u64,
        "Agent conversation workspace phase completed"
    );
}

fn log_agent_workspace_probe_result(
    conversation_id: &ChatConversationId,
    phase: &'static str,
    started: Instant,
    result: Option<&str>,
    error: Option<String>,
) {
    tracing::info!(
        conversation_id = %conversation_id,
        phase,
        elapsed_ms = started.elapsed().as_millis() as u64,
        result = ?result,
        error = ?error,
        "Agent conversation workspace probe completed"
    );
}

async fn run_or_defer_agent_conversation_workspace_setup(
    project: &Project,
    conversation_id: &ChatConversationId,
    worktree_path: &Path,
    branch_name: &str,
    setup_mode: AgentConversationWorkspaceSetupMode,
) {
    match setup_mode {
        AgentConversationWorkspaceSetupMode::Blocking => {
            run_agent_conversation_workspace_setup(
                project,
                conversation_id,
                worktree_path,
                branch_name,
            )
            .await;
        }
        AgentConversationWorkspaceSetupMode::Deferred => {
            let project = project.clone();
            let conversation_id = conversation_id.clone();
            let worktree_path = worktree_path.to_path_buf();
            let branch_name = branch_name.to_string();
            tokio::spawn(async move {
                run_agent_conversation_workspace_setup(
                    &project,
                    &conversation_id,
                    &worktree_path,
                    &branch_name,
                )
                .await;
            });
        }
    }
}

async fn run_agent_conversation_workspace_setup(
    project: &Project,
    conversation_id: &ChatConversationId,
    worktree_path: &Path,
    branch_name: &str,
) {
    let conversation_id_str = conversation_id.as_str();
    let mut setup_task = Task::new(
        project.id.clone(),
        format!("Agent conversation {conversation_id_str}"),
    );
    setup_task.task_branch = Some(branch_name.to_string());
    setup_task.worktree_path = Some(worktree_path.to_string_lossy().to_string());

    let Some(result) = run_pre_execution_setup(
        project,
        &setup_task,
        worktree_path,
        &conversation_id_str,
        None,
        "agent_conversation_setup",
        &tokio_util::sync::CancellationToken::new(),
    )
    .await
    else {
        return;
    };

    if result.success {
        tracing::info!(
            conversation_id = %conversation_id,
            worktree_path = %worktree_path.display(),
            command_count = result.log.len(),
            "Agent conversation worktree setup completed"
        );
    } else {
        tracing::warn!(
            conversation_id = %conversation_id,
            worktree_path = %worktree_path.display(),
            command_count = result.log.len(),
            "Agent conversation worktree setup had failures; continuing with workspace launch"
        );
    }
}

pub fn agent_conversation_branch_name(
    project: &Project,
    conversation_id: &ChatConversationId,
) -> String {
    agent_conversation_branch_name_with_segment(project, conversation_id, "agent")
}

fn agent_conversation_branch_name_for_mode(
    project: &Project,
    conversation_id: &ChatConversationId,
    mode: AgentConversationWorkspaceMode,
) -> String {
    let segment = match mode {
        AgentConversationWorkspaceMode::Automation => "automation",
        _ => "agent",
    };
    agent_conversation_branch_name_with_segment(project, conversation_id, segment)
}

fn agent_conversation_branch_name_for_mode_with_hint(
    project: &Project,
    conversation_id: &ChatConversationId,
    mode: AgentConversationWorkspaceMode,
    branch_name_hint: Option<&AgentConversationWorkspaceBranchNameHint>,
) -> String {
    if mode != AgentConversationWorkspaceMode::Automation {
        if let Some(segment) = agent_conversation_ticket_branch_segment(branch_name_hint) {
            return agent_conversation_branch_name_with_segment(project, conversation_id, &segment);
        }
    }
    agent_conversation_branch_name_for_mode(project, conversation_id, mode)
}

fn agent_conversation_ticket_branch_segment(
    branch_name_hint: Option<&AgentConversationWorkspaceBranchNameHint>,
) -> Option<String> {
    let hint = branch_name_hint?;
    let provider = slug_branch_component(&hint.provider);
    if !matches!(provider.as_str(), "jira" | "linear" | "clickup") {
        return None;
    }
    let ticket_token = ticket_branch_token_component(&hint.ticket_token)?;
    Some(format!("agent-{provider}-{ticket_token}"))
}

fn agent_conversation_branch_name_with_segment(
    project: &Project,
    conversation_id: &ChatConversationId,
    segment: &str,
) -> String {
    let project_slug = slug_branch_component(&project.name);
    let short_id = agent_conversation_short_id(conversation_id);
    format!("ralphx/{project_slug}/{segment}-{short_id}")
}

pub fn is_expected_agent_conversation_branch_name(
    project: &Project,
    conversation_id: &ChatConversationId,
    branch_name: &str,
) -> bool {
    let canonical_branch = agent_conversation_branch_name(project, conversation_id);
    if is_branch_or_numeric_continuation(&canonical_branch, branch_name) {
        return true;
    }

    let project_slug = slug_branch_component(&project.name);
    let path_prefix = format!("ralphx/{project_slug}/");
    let Some(branch_leaf) = branch_name.strip_prefix(&path_prefix) else {
        return false;
    };
    if branch_leaf.contains('/') {
        return false;
    }

    let short_id = agent_conversation_short_id(conversation_id);
    if is_provider_ticket_agent_branch_leaf(branch_leaf, &short_id) {
        return true;
    }

    let branch_leaf_without_continuation = strip_numeric_continuation_suffix(branch_leaf);
    branch_leaf_without_continuation != branch_leaf
        && is_provider_ticket_agent_branch_leaf(branch_leaf_without_continuation, &short_id)
}

fn is_provider_ticket_agent_branch_leaf(branch_leaf: &str, short_id: &str) -> bool {
    let Some(ticket_segment) = branch_leaf.strip_suffix(&format!("-{short_id}")) else {
        return false;
    };
    let Some(ticket_segment) = ticket_segment.strip_prefix("agent-") else {
        return false;
    };
    let Some((provider, ticket_token)) = ticket_segment.split_once('-') else {
        return false;
    };

    matches!(provider, "jira" | "linear" | "clickup") && !ticket_token.is_empty()
}

fn is_branch_or_numeric_continuation(base_branch_name: &str, branch_name: &str) -> bool {
    if branch_name == base_branch_name {
        return true;
    }
    let continuation_prefix = format!("{base_branch_name}-");
    branch_name
        .strip_prefix(&continuation_prefix)
        .is_some_and(|suffix| !suffix.is_empty() && suffix.chars().all(|ch| ch.is_ascii_digit()))
}

fn strip_numeric_continuation_suffix(branch_leaf: &str) -> &str {
    branch_leaf
        .rsplit_once('-')
        .and_then(|(base, suffix)| {
            (!suffix.is_empty() && suffix.chars().all(|ch| ch.is_ascii_digit())).then_some(base)
        })
        .unwrap_or(branch_leaf)
}

fn agent_conversation_short_id(conversation_id: &ChatConversationId) -> String {
    let short_id = conversation_id
        .as_str()
        .chars()
        .filter(|ch| ch.is_ascii_alphanumeric())
        .take(8)
        .collect::<String>();
    if short_id.is_empty() {
        "conversation".to_string()
    } else {
        short_id
    }
}

fn ticket_branch_token_component(value: &str) -> Option<String> {
    let mut token = String::new();
    let mut last_dash = false;

    for ch in value.trim().chars() {
        if ch.is_ascii_alphanumeric() {
            token.push(ch);
            last_dash = false;
        } else if !last_dash && !token.is_empty() {
            token.push('-');
            last_dash = true;
        }
    }

    while token.ends_with('-') {
        token.pop();
    }

    (!token.is_empty()).then_some(token)
}

fn agent_conversation_continuation_branch_name(current_branch_name: &str) -> String {
    format!("{}-{}", current_branch_name, Utc::now().timestamp_millis())
}

pub fn agent_name_for_workspace_mode(mode: AgentConversationWorkspaceMode) -> &'static str {
    match mode {
        AgentConversationWorkspaceMode::Chat => AGENT_GENERAL_EXPLORER,
        AgentConversationWorkspaceMode::Edit => AGENT_GENERAL_WORKER,
        AgentConversationWorkspaceMode::Plan => AGENT_ORCHESTRATOR_IDEATION,
        AgentConversationWorkspaceMode::Tasks => AGENT_TASK_MANAGER,
        AgentConversationWorkspaceMode::Autopilot | AgentConversationWorkspaceMode::Ideation => {
            AGENT_CHAT_PROJECT
        }
        AgentConversationWorkspaceMode::ReviewPr => AGENT_PR_REVIEWER,
        AgentConversationWorkspaceMode::Automation => AGENT_AUTOMATION_SETUP,
        AgentConversationWorkspaceMode::PersonaBuilder => AGENT_PERSONA_EXTRACTOR,
    }
}

/// Reject the reserved builder mode at generic workspace entry points.
pub(crate) fn reject_persona_builder_workspace_mode(mode: &str) -> Result<(), String> {
    if mode.trim() == AgentConversationWorkspaceMode::PersonaBuilder.to_string() {
        Err(PERSONA_BUILDER_GENERIC_ENTRY_POINT_ERROR.to_string())
    } else {
        Ok(())
    }
}

async fn ensure_agent_conversation_worktree(
    repo_path: &Path,
    workspace_path: &Path,
    branch_name: &str,
    base_ref: &str,
) -> AppResult<()> {
    let branch_ref_check_started = Instant::now();
    let branch_name_is_valid = GitService::check_ref_format(repo_path, branch_name).await?;
    tracing::info!(
        branch_name,
        elapsed_ms = branch_ref_check_started.elapsed().as_millis() as u64,
        branch_name_is_valid,
        "Agent conversation branch ref format check completed"
    );
    if !branch_name_is_valid {
        return Err(AppError::Validation(format!(
            "Agent conversation workspace branch name '{}' is not a valid git ref",
            branch_name
        )));
    }

    let exists_started = Instant::now();
    let workspace_exists = workspace_path.exists();
    tracing::info!(
        branch_name,
        base_ref,
        worktree_path = %workspace_path.display(),
        elapsed_ms = exists_started.elapsed().as_millis() as u64,
        workspace_exists,
        "Agent conversation worktree path probe completed"
    );

    if workspace_exists {
        if !workspace_path.is_dir() {
            return Err(AppError::Validation(format!(
                "Agent conversation workspace path exists but is not a directory: {}",
                workspace_path.display()
            )));
        }
        let branch_check_started = Instant::now();
        let checked_out = GitService::get_current_branch(workspace_path).await?;
        tracing::info!(
            branch_name,
            checked_out = %checked_out,
            worktree_path = %workspace_path.display(),
            elapsed_ms = branch_check_started.elapsed().as_millis() as u64,
            "Agent conversation existing worktree branch check completed"
        );
        if checked_out != branch_name {
            return Err(AppError::Validation(format!(
                "Existing agent conversation workspace {} is checked out at '{}' instead of '{}'",
                workspace_path.display(),
                checked_out,
                branch_name
            )));
        }
        return Ok(());
    }

    let create_started = Instant::now();
    let result =
        GitService::create_worktree(repo_path, workspace_path, branch_name, base_ref).await;
    tracing::info!(
        branch_name,
        base_ref,
        worktree_path = %workspace_path.display(),
        elapsed_ms = create_started.elapsed().as_millis() as u64,
        success = result.is_ok(),
        error = ?result.as_ref().err().map(ToString::to_string),
        "Agent conversation worktree creation completed"
    );
    result
}

async fn ensure_linked_agent_conversation_branch_worktree(
    repo_path: &Path,
    workspace_path: &Path,
    branch_name: &str,
) -> AppResult<()> {
    let project_root =
        crate::utils::path_safety::validate_absolute_non_root_path(repo_path, "project checkout")?;

    if workspace_path == project_root {
        return Err(AppError::Validation(format!(
            "Linked agent conversation branch '{}' points to the project root",
            branch_name
        )));
    }

    if workspace_path.exists() {
        if !workspace_path.is_dir() {
            return Err(AppError::Validation(format!(
                "Linked agent conversation workspace path exists but is not a directory: {}",
                workspace_path.display()
            )));
        }
        let checked_out = GitService::get_current_branch(workspace_path).await?;
        if checked_out != branch_name {
            return Err(AppError::Validation(format!(
                "Existing linked agent conversation workspace {} is checked out at '{}' instead of '{}'",
                workspace_path.display(),
                checked_out,
                branch_name
            )));
        }
        return Ok(());
    }

    let project_root_canonical = project_root
        .canonicalize()
        .unwrap_or_else(|_| project_root.clone());
    let workspace_path_canonical = workspace_path
        .canonicalize()
        .unwrap_or_else(|_| workspace_path.to_path_buf());
    for worktree in GitService::list_worktrees(&project_root).await? {
        if worktree.branch.as_deref() != Some(branch_name) {
            continue;
        }
        let existing_path = PathBuf::from(&worktree.path);
        let existing_path_canonical = existing_path
            .canonicalize()
            .unwrap_or_else(|_| existing_path.clone());
        if existing_path_canonical == workspace_path_canonical {
            continue;
        }
        if existing_path_canonical == project_root_canonical {
            return Err(AppError::Validation(format!(
                "Selected branch '{}' is checked out in the project root; choose isolated branch mode or switch the primary checkout first",
                branch_name
            )));
        }
        return Err(AppError::Validation(format!(
            "Selected branch '{}' is already checked out at {}; choose isolated branch mode or continue in that conversation",
            branch_name,
            existing_path.display()
        )));
    }

    GitService::checkout_existing_branch_worktree_strict(&project_root, workspace_path, branch_name)
        .await
}

pub fn resolve_agent_conversation_workspace_path(
    project: &Project,
    conversation_id: &ChatConversationId,
) -> AppResult<PathBuf> {
    Ok(
        resolve_agent_conversation_project_workspace_dir(project)?.join(hashed_path_component(
            "agent-conversation",
            &conversation_id.as_str(),
        )),
    )
}

pub(crate) fn resolve_agent_conversation_project_workspace_dir(
    project: &Project,
) -> AppResult<PathBuf> {
    let parent = expand_worktree_parent(project.worktree_parent_or_default())?;
    Ok(parent.join(hashed_path_component("project", project.id.as_str())))
}

pub async fn ensure_linked_plan_branch_agent_worktree(
    project: &Project,
    plan_branch: &PlanBranch,
) -> AppResult<PathBuf> {
    let workspace_path = resolve_linked_plan_branch_agent_worktree_path(project, plan_branch)?;
    let repo_path = PathBuf::from(&project.working_directory);
    let project_root =
        crate::utils::path_safety::validate_absolute_non_root_path(&repo_path, "project checkout")?;

    if workspace_path == project_root {
        return Err(AppError::Validation(format!(
            "Linked plan branch worktree {} points to the project root",
            plan_branch.id
        )));
    }

    if workspace_path.exists() {
        if !workspace_path.is_dir() {
            return Err(AppError::Validation(format!(
                "Linked plan branch worktree path exists but is not a directory: {}",
                workspace_path.display()
            )));
        }
        let checked_out = GitService::get_current_branch(&workspace_path).await?;
        if checked_out != plan_branch.branch_name {
            return Err(AppError::Validation(format!(
                "Existing linked plan branch worktree {} is checked out at '{}' instead of '{}'",
                workspace_path.display(),
                checked_out,
                plan_branch.branch_name
            )));
        }
        return Ok(workspace_path);
    }

    let project_root_canonical = project_root
        .canonicalize()
        .unwrap_or_else(|_| project_root.clone());
    let workspace_path_canonical = workspace_path
        .canonicalize()
        .unwrap_or_else(|_| workspace_path.clone());
    for worktree in GitService::list_worktrees(&project_root).await? {
        if worktree.branch.as_deref() != Some(plan_branch.branch_name.as_str()) {
            continue;
        }
        let existing_path = PathBuf::from(&worktree.path);
        let existing_path_canonical = existing_path
            .canonicalize()
            .unwrap_or_else(|_| existing_path.clone());
        if existing_path_canonical == workspace_path_canonical {
            continue;
        }
        if existing_path_canonical == project_root_canonical {
            return Err(AppError::Validation(format!(
                "Linked plan branch '{}' is checked out in the project root; refusing to publish from the primary checkout",
                plan_branch.branch_name
            )));
        }
        return Err(AppError::Validation(format!(
            "Linked plan branch '{}' is already checked out at {}; refusing to move or delete another worktree",
            plan_branch.branch_name,
            existing_path.display()
        )));
    }

    GitService::checkout_existing_branch_worktree(
        &project_root,
        &workspace_path,
        &plan_branch.branch_name,
    )
    .await?;
    Ok(workspace_path)
}

pub fn resolve_linked_plan_branch_agent_worktree_path(
    project: &Project,
    plan_branch: &PlanBranch,
) -> AppResult<PathBuf> {
    if plan_branch.project_id != project.id {
        return Err(AppError::Validation(format!(
            "Plan branch {} belongs to project {} instead of {}",
            plan_branch.id, plan_branch.project_id, project.id
        )));
    }

    Ok(
        resolve_agent_conversation_project_workspace_dir(project)?.join(hashed_path_component(
            "linked-plan-branch",
            plan_branch.id.as_str(),
        )),
    )
}

pub async fn resolve_valid_agent_conversation_workspace_path(
    project: &Project,
    workspace: &AgentConversationWorkspace,
) -> AppResult<PathBuf> {
    let expected_path = resolve_agent_conversation_workspace_path_from_record(project, workspace)?;

    let checked_out = GitService::get_current_branch(&expected_path).await?;
    if checked_out != workspace.branch_name {
        return Err(AppError::Validation(format!(
            "Agent conversation workspace {} is checked out at '{}' instead of '{}'",
            workspace.conversation_id, checked_out, workspace.branch_name
        )));
    }

    Ok(expected_path)
}

pub async fn resolve_effective_agent_conversation_workspace_path(
    project: &Project,
    workspace: &AgentConversationWorkspace,
    plan_branch_repo: &dyn PlanBranchRepository,
) -> AppResult<ResolvedAgentConversationWorkspacePath> {
    validate_agent_workspace_project(project, workspace)?;

    if let Some(plan_branch_id) = workspace.linked_plan_branch_id.as_ref() {
        let plan_branch = plan_branch_repo
            .get_by_id(plan_branch_id)
            .await?
            .ok_or_else(|| {
                AppError::Validation(format!(
                    "Linked plan branch not found for agent conversation workspace {}: {}",
                    workspace.conversation_id, plan_branch_id
                ))
            })?;
        validate_workspace_linked_plan_branch(project, workspace, &plan_branch)?;
        let path = ensure_linked_plan_branch_agent_worktree(project, &plan_branch).await?;
        return Ok(ResolvedAgentConversationWorkspacePath {
            path,
            branch_name: plan_branch.branch_name,
        });
    }

    Ok(ResolvedAgentConversationWorkspacePath {
        path: resolve_agent_conversation_workspace_path_from_record(project, workspace)?,
        branch_name: workspace.branch_name.clone(),
    })
}

pub fn resolve_agent_conversation_workspace_path_for_send(
    project: &Project,
    workspace: &AgentConversationWorkspace,
) -> AppResult<PathBuf> {
    resolve_agent_conversation_workspace_path_from_record(project, workspace)
}

fn resolve_agent_conversation_workspace_path_from_record(
    project: &Project,
    workspace: &AgentConversationWorkspace,
) -> AppResult<PathBuf> {
    let expected_path =
        resolve_agent_conversation_workspace_path_from_record_identity(project, workspace)?;

    if !expected_path.is_dir() {
        return Err(AppError::Validation(format!(
            "Agent conversation workspace is missing: {}",
            expected_path.display()
        )));
    }

    if !expected_path.join(".git").exists() {
        return Err(AppError::Validation(format!(
            "Agent conversation workspace {} is not a git worktree: {}",
            workspace.conversation_id,
            expected_path.display()
        )));
    }

    Ok(expected_path)
}

pub(super) fn resolve_agent_conversation_workspace_path_from_record_identity(
    project: &Project,
    workspace: &AgentConversationWorkspace,
) -> AppResult<PathBuf> {
    validate_agent_workspace_project(project, workspace)?;

    let expected_path =
        resolve_agent_conversation_workspace_path(project, &workspace.conversation_id)?;
    let stored_path = PathBuf::from(&workspace.worktree_path);
    if stored_path != expected_path {
        return Err(AppError::Validation(format!(
            "Agent conversation workspace path mismatch for conversation {}",
            workspace.conversation_id
        )));
    }

    let project_root = PathBuf::from(&project.working_directory);
    if expected_path == project_root {
        return Err(AppError::Validation(format!(
            "Agent conversation workspace {} points to the project root",
            workspace.conversation_id
        )));
    }

    Ok(expected_path)
}

fn validate_agent_workspace_project(
    project: &Project,
    workspace: &AgentConversationWorkspace,
) -> AppResult<()> {
    if workspace.project_id != project.id {
        return Err(AppError::Validation(format!(
            "Agent conversation workspace {} belongs to project {} instead of {}",
            workspace.conversation_id, workspace.project_id, project.id
        )));
    }
    Ok(())
}

pub(super) fn validate_workspace_linked_plan_branch(
    project: &Project,
    workspace: &AgentConversationWorkspace,
    plan_branch: &PlanBranch,
) -> AppResult<()> {
    if plan_branch.project_id != project.id {
        return Err(AppError::Validation(format!(
            "Linked plan branch {} belongs to project {} instead of {}",
            plan_branch.id, plan_branch.project_id, project.id
        )));
    }
    if let Some(session_id) = workspace.linked_ideation_session_id.as_ref() {
        if &plan_branch.session_id != session_id {
            return Err(AppError::Validation(format!(
                "Linked plan branch {} belongs to ideation session {} instead of {}",
                plan_branch.id, plan_branch.session_id, session_id
            )));
        }
    }
    if workspace.branch_name != plan_branch.branch_name {
        return Err(AppError::Validation(format!(
            "Agent conversation workspace {} is linked to plan branch '{}' but records branch '{}'",
            workspace.conversation_id, plan_branch.branch_name, workspace.branch_name
        )));
    }
    Ok(())
}

pub(crate) fn expand_worktree_parent_public(parent: &str) -> AppResult<PathBuf> {
    expand_worktree_parent(parent)
}

fn expand_worktree_parent(parent: &str) -> AppResult<PathBuf> {
    let expanded = if let Some(rest) = parent.strip_prefix("~/") {
        let home = dirs::home_dir().ok_or_else(|| {
            AppError::Validation(
                "Cannot expand worktree parent because home directory is unavailable".to_string(),
            )
        })?;
        home.join(rest)
    } else {
        PathBuf::from(parent)
    };

    if !expanded.is_absolute()
        || expanded
            .components()
            .any(|component| matches!(component, Component::ParentDir))
    {
        return Err(AppError::Validation(format!(
            "Invalid agent conversation worktree parent path: {}",
            expanded.display()
        )));
    }

    Ok(expanded)
}

fn hashed_path_component(prefix: &str, value: &str) -> String {
    let digest = Sha256::digest(value.as_bytes());
    let hash = digest[..12]
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    format!("{prefix}-{hash}")
}

fn slug_branch_component(value: &str) -> String {
    let mut slug = String::new();
    let mut last_dash = false;

    for ch in value.chars().flat_map(char::to_lowercase) {
        if ch.is_ascii_alphanumeric() {
            slug.push(ch);
            last_dash = false;
        } else if !last_dash && !slug.is_empty() {
            slug.push('-');
            last_dash = true;
        }
    }

    while slug.ends_with('-') {
        slug.pop();
    }

    if slug.is_empty() {
        "project".to_string()
    } else {
        slug
    }
}
