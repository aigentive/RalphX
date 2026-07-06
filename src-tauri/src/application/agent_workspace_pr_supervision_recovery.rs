use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};

use dashmap::DashMap;
use futures::{stream, StreamExt as _};
use tauri::{AppHandle, Emitter};

use crate::application::agent_conversation_workspace::{
    ensure_linked_plan_branch_agent_worktree, resolve_valid_agent_conversation_workspace_path,
};
use crate::application::agent_workspace_publish_recovery::recover_stale_publish_repair_for_workspace_and_reload;
use crate::application::chat_service::ChatService;
use crate::application::git_service::GitService;
use crate::application::services::pr_merge_poller::cleanup_terminal_agent_workspace_after_pr;
use crate::application::services::PrPollerRegistry;
use crate::application::task_transition_service::TaskTransitionService;
use crate::domain::entities::plan_branch::{PrPushStatus, PrStatus as PlanPrStatus};
use crate::domain::entities::{
    AgentConversationWorkspace, AgentConversationWorkspaceMode,
    AgentConversationWorkspacePublicationEvent, AgentConversationWorkspaceStatus,
    ChatConversationId, PlanBranch, PlanBranchStatus, Project, ProjectId,
};
use crate::domain::repositories::{
    AgentConversationWorkspaceRepository, AgentRunRepository, PlanBranchRepository,
    ProjectRepository,
};
use crate::domain::services::{GithubServiceTrait, PrStatus as GithubPrStatus, PrSyncState};
use crate::error::AppResult;
use crate::infrastructure::agents::claude::git_runtime_config;

const STARTUP_PR_SUPERVISION_RECOVERY_LIMIT: usize = 25;
const STARTUP_PR_SUPERVISION_RECOVERY_CONCURRENCY: usize = 4;
const PR_SUPERVISION_RECOVERED_STEP: &str = "pr_supervision_recovered";
const PR_SUPERVISION_RECOVERED_SUMMARY: &str =
    "Recovered blocked PR supervision; RalphX is monitoring PR health again.";

static IN_FLIGHT_RECOVERIES: OnceLock<DashMap<String, ()>> = OnceLock::new();
static RECENT_RECOVERIES: OnceLock<DashMap<String, Instant>> = OnceLock::new();

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AgentWorkspacePrSupervisionRecoveryTrigger {
    WorkspaceLoad,
    AgentRunCompleted,
    Startup,
}

impl AgentWorkspacePrSupervisionRecoveryTrigger {
    fn as_str(self) -> &'static str {
        match self {
            Self::WorkspaceLoad => "workspace_load",
            Self::AgentRunCompleted => "agent_run_completed",
            Self::Startup => "startup",
        }
    }
}

#[derive(Clone)]
pub(crate) struct AgentWorkspacePrSupervisionRecoveryDeps {
    pub workspace_repo: Arc<dyn AgentConversationWorkspaceRepository>,
    pub project_repo: Arc<dyn ProjectRepository>,
    pub plan_branch_repo: Arc<dyn PlanBranchRepository>,
    pub github: Arc<dyn GithubServiceTrait>,
    pub pr_poller_registry: Option<Arc<PrPollerRegistry>>,
    pub transition_service: Option<Arc<TaskTransitionService<tauri::Wry>>>,
    pub chat_service: Option<Arc<dyn ChatService>>,
    pub agent_run_repo: Arc<dyn AgentRunRepository>,
    pub app_handle: Option<AppHandle>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum AgentWorkspacePrSupervisionRecoveryOutcome {
    Skipped(&'static str),
    Recovered { pr_number: i64, head_sha: String },
    Terminal { pr_number: i64, pr_status: String },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AgentWorkspacePrSupervisionRecoveryTargetKind {
    DirectWorkspace,
    IdeationPlan,
}

#[derive(Debug, Clone)]
struct AgentWorkspacePrSupervisionRecoveryTarget {
    kind: AgentWorkspacePrSupervisionRecoveryTargetKind,
    pr_number: i64,
    pr_url: Option<String>,
    worktree_path: PathBuf,
    branch_name: String,
    plan_branch: Option<PlanBranch>,
}

impl AgentWorkspacePrSupervisionRecoveryTarget {
    fn is_ideation_plan(&self) -> bool {
        self.kind == AgentWorkspacePrSupervisionRecoveryTargetKind::IdeationPlan
    }
}

pub(crate) fn schedule_agent_workspace_pr_supervision_recovery(
    deps: AgentWorkspacePrSupervisionRecoveryDeps,
    conversation_id: ChatConversationId,
    trigger: AgentWorkspacePrSupervisionRecoveryTrigger,
    force: bool,
) {
    if !claim_recovery(&conversation_id, force) {
        tracing::debug!(
            conversation_id = conversation_id.as_str(),
            trigger = trigger.as_str(),
            "Agent workspace PR supervision recovery skipped before scheduling"
        );
        return;
    }

    tokio::spawn(async move {
        let started = Instant::now();
        let result =
            recover_agent_workspace_pr_supervision(deps, conversation_id.clone(), trigger).await;
        RECENT_RECOVERIES
            .get_or_init(DashMap::new)
            .insert(conversation_id.as_str(), Instant::now());
        IN_FLIGHT_RECOVERIES
            .get_or_init(DashMap::new)
            .remove(&conversation_id.as_str());

        match result {
            Ok(outcome) => tracing::info!(
                conversation_id = conversation_id.as_str(),
                trigger = trigger.as_str(),
                elapsed_ms = started.elapsed().as_millis() as u64,
                outcome = ?outcome,
                "Agent workspace PR supervision recovery completed"
            ),
            Err(error) => tracing::warn!(
                conversation_id = conversation_id.as_str(),
                trigger = trigger.as_str(),
                elapsed_ms = started.elapsed().as_millis() as u64,
                error = %error,
                "Agent workspace PR supervision recovery failed"
            ),
        }
    });
}

pub(crate) async fn recover_agent_workspace_pr_supervision(
    deps: AgentWorkspacePrSupervisionRecoveryDeps,
    conversation_id: ChatConversationId,
    trigger: AgentWorkspacePrSupervisionRecoveryTrigger,
) -> AppResult<AgentWorkspacePrSupervisionRecoveryOutcome> {
    let Some(mut workspace) = deps
        .workspace_repo
        .get_by_conversation_id(&conversation_id)
        .await?
    else {
        return Ok(AgentWorkspacePrSupervisionRecoveryOutcome::Skipped(
            "workspace_missing",
        ));
    };

    if let Some(reason) = pr_supervision_recovery_schedule_skip_reason(&workspace) {
        return Ok(AgentWorkspacePrSupervisionRecoveryOutcome::Skipped(reason));
    }

    if deps
        .agent_run_repo
        .get_active_for_conversation(&conversation_id)
        .await?
        .is_some()
    {
        return Ok(AgentWorkspacePrSupervisionRecoveryOutcome::Skipped(
            "active_agent_run",
        ));
    }

    if workspace.mode == AgentConversationWorkspaceMode::Edit
        && workspace.publication_push_status.as_deref() == Some("needs_agent")
    {
        workspace = recover_stale_publish_repair_for_workspace_and_reload(
            Arc::clone(&deps.workspace_repo),
            Arc::clone(&deps.agent_run_repo),
            workspace,
        )
        .await?;
    }

    if let Some(reason) = blocked_pr_supervision_recovery_skip_reason(&workspace) {
        return Ok(AgentWorkspacePrSupervisionRecoveryOutcome::Skipped(reason));
    }

    if deps
        .agent_run_repo
        .get_active_for_conversation(&conversation_id)
        .await?
        .is_some()
    {
        return Ok(AgentWorkspacePrSupervisionRecoveryOutcome::Skipped(
            "active_agent_run",
        ));
    }

    let Some(project) = deps.project_repo.get_by_id(&workspace.project_id).await? else {
        return Ok(AgentWorkspacePrSupervisionRecoveryOutcome::Skipped(
            "project_missing",
        ));
    };
    if project.archived_at.is_some() {
        return Ok(AgentWorkspacePrSupervisionRecoveryOutcome::Skipped(
            "project_archived",
        ));
    }
    if !project.github_pr_enabled {
        return Ok(AgentWorkspacePrSupervisionRecoveryOutcome::Skipped(
            "github_pr_disabled",
        ));
    }

    let target =
        match resolve_pr_supervision_recovery_target(&deps, &project, &workspace, trigger).await? {
            Ok(target) => target,
            Err(reason) => return Ok(AgentWorkspacePrSupervisionRecoveryOutcome::Skipped(reason)),
        };

    if GitService::has_uncommitted_changes(&target.worktree_path).await? {
        return Ok(AgentWorkspacePrSupervisionRecoveryOutcome::Skipped(
            "worktree_dirty",
        ));
    }
    let local_head_sha = GitService::get_head_sha(&target.worktree_path).await?;
    let sync_state = deps
        .github
        .check_pr_sync_state(&target.worktree_path, target.pr_number)
        .await?;
    if is_terminal_pr_sync_status(&sync_state.status) {
        let pr_status = publication_status_for_sync_state(&sync_state);
        update_terminal_pr_recovery_state(&deps, &conversation_id, &workspace, &target, pr_status)
            .await?;
        deps.workspace_repo
            .append_publication_event(AgentConversationWorkspacePublicationEvent::new(
                conversation_id.clone(),
                format!("pr_{pr_status}"),
                "succeeded",
                terminal_pr_recovery_summary(pr_status),
                None,
            ))
            .await?;
        emit_workspace_changed(deps.app_handle.as_ref(), &conversation_id);
        if !target.is_ideation_plan() {
            cleanup_terminal_agent_workspace_after_pr(
                Arc::clone(&deps.workspace_repo),
                &conversation_id,
                &project,
                matches!(&sync_state.status, GithubPrStatus::Merged { .. })
                    .then(|| Arc::clone(&deps.github)),
                pr_status == "merged",
            )
            .await;
        }
        return Ok(AgentWorkspacePrSupervisionRecoveryOutcome::Terminal {
            pr_number: target.pr_number,
            pr_status: pr_status.to_string(),
        });
    }

    if let Some(reason) =
        pr_sync_state_recovery_skip_reason(&target.branch_name, &sync_state, &local_head_sha)
    {
        return Ok(AgentWorkspacePrSupervisionRecoveryOutcome::Skipped(reason));
    }

    let pr_status = publication_status_for_sync_state(&sync_state);
    update_recovered_pr_state(&deps, &conversation_id, &workspace, &target, pr_status).await?;
    deps.workspace_repo
        .update_pr_auto_merge_state(
            &conversation_id,
            workspace.pr_auto_merge_current,
            Some("monitoring"),
            Some(PR_SUPERVISION_RECOVERED_SUMMARY),
        )
        .await?;
    deps.workspace_repo
        .append_publication_event(AgentConversationWorkspacePublicationEvent::new(
            conversation_id.clone(),
            PR_SUPERVISION_RECOVERED_STEP,
            "succeeded",
            "Recovered blocked PR supervision; PR head still matches the local workspace branch.",
            Some(format!(
                "github_pr_supervision_recovered:{}:{local_head_sha}",
                target.pr_number
            )),
        ))
        .await?;
    emit_workspace_changed(deps.app_handle.as_ref(), &conversation_id);

    start_recovered_pr_polling(&deps, &conversation_id, &project, &target);

    Ok(AgentWorkspacePrSupervisionRecoveryOutcome::Recovered {
        pr_number: target.pr_number,
        head_sha: local_head_sha,
    })
}

async fn resolve_pr_supervision_recovery_target(
    deps: &AgentWorkspacePrSupervisionRecoveryDeps,
    project: &Project,
    workspace: &AgentConversationWorkspace,
    trigger: AgentWorkspacePrSupervisionRecoveryTrigger,
) -> AppResult<Result<AgentWorkspacePrSupervisionRecoveryTarget, &'static str>> {
    match workspace.mode {
        AgentConversationWorkspaceMode::Edit => {
            let Some(pr_number) = workspace.publication_pr_number else {
                return Ok(Err("missing_pr_number"));
            };
            let worktree_path = match resolve_valid_agent_conversation_workspace_path(
                project, workspace,
            )
            .await
            {
                Ok(path) => path,
                Err(error) => {
                    tracing::warn!(
                        conversation_id = workspace.conversation_id.as_str(),
                        pr_number,
                        trigger = trigger.as_str(),
                        error = %error,
                        "Agent workspace PR supervision recovery skipped unusable workspace path"
                    );
                    return Ok(Err("workspace_path_invalid"));
                }
            };
            Ok(Ok(AgentWorkspacePrSupervisionRecoveryTarget {
                kind: AgentWorkspacePrSupervisionRecoveryTargetKind::DirectWorkspace,
                pr_number,
                pr_url: workspace.publication_pr_url.clone(),
                worktree_path,
                branch_name: workspace.branch_name.clone(),
                plan_branch: None,
            }))
        }
        AgentConversationWorkspaceMode::Ideation => {
            let Some(plan_branch_id) = workspace.linked_plan_branch_id.as_ref() else {
                return Ok(Err("workspace_missing_plan_branch"));
            };
            let Some(plan_branch) = deps.plan_branch_repo.get_by_id(plan_branch_id).await? else {
                return Ok(Err("linked_plan_branch_missing"));
            };
            if plan_branch.status != PlanBranchStatus::Active
                || !plan_branch.pr_eligible
                || matches!(
                    plan_branch.pr_status,
                    Some(PlanPrStatus::Closed | PlanPrStatus::Merged)
                )
                || workspace.linked_ideation_session_id.as_ref() != Some(&plan_branch.session_id)
                || workspace.branch_name != plan_branch.branch_name
            {
                return Ok(Err("linked_plan_branch_not_current"));
            }
            let Some(pr_number) = plan_branch.pr_number else {
                return Ok(Err("missing_pr_number"));
            };
            let worktree_path = match ensure_linked_plan_branch_agent_worktree(
                project,
                &plan_branch,
            )
            .await
            {
                Ok(path) => path,
                Err(error) => {
                    tracing::warn!(
                        conversation_id = workspace.conversation_id.as_str(),
                        plan_branch_id = plan_branch.id.as_str(),
                        pr_number,
                        trigger = trigger.as_str(),
                        error = %error,
                        "Agent workspace PR supervision recovery skipped unusable linked plan worktree"
                    );
                    return Ok(Err("workspace_path_invalid"));
                }
            };
            Ok(Ok(AgentWorkspacePrSupervisionRecoveryTarget {
                kind: AgentWorkspacePrSupervisionRecoveryTargetKind::IdeationPlan,
                pr_number,
                pr_url: plan_branch.pr_url.clone(),
                worktree_path,
                branch_name: plan_branch.branch_name.clone(),
                plan_branch: Some(plan_branch),
            }))
        }
        _ => Ok(Err("workspace_not_edit_or_ideation_mode")),
    }
}

async fn update_terminal_pr_recovery_state(
    deps: &AgentWorkspacePrSupervisionRecoveryDeps,
    conversation_id: &ChatConversationId,
    workspace: &AgentConversationWorkspace,
    target: &AgentWorkspacePrSupervisionRecoveryTarget,
    pr_status: &str,
) -> AppResult<()> {
    if let Some(plan_branch) = target.plan_branch.as_ref() {
        deps.plan_branch_repo
            .update_pr_status(
                &plan_branch.id,
                plan_pr_status_from_publication_status(pr_status),
            )
            .await?;
        deps.plan_branch_repo
            .update_pr_push_status(&plan_branch.id, PrPushStatus::Pushed)
            .await?;
        deps.workspace_repo
            .update_pr_auto_merge_state(
                conversation_id,
                workspace.pr_auto_merge_current,
                None,
                Some(terminal_pr_recovery_summary(pr_status)),
            )
            .await?;
        return Ok(());
    }

    deps.workspace_repo
        .update_publication(
            conversation_id,
            Some(target.pr_number),
            target.pr_url.as_deref(),
            Some(pr_status),
            Some("pushed"),
        )
        .await
}

async fn update_recovered_pr_state(
    deps: &AgentWorkspacePrSupervisionRecoveryDeps,
    conversation_id: &ChatConversationId,
    _workspace: &AgentConversationWorkspace,
    target: &AgentWorkspacePrSupervisionRecoveryTarget,
    pr_status: &str,
) -> AppResult<()> {
    if let Some(plan_branch) = target.plan_branch.as_ref() {
        deps.plan_branch_repo
            .update_pr_status(
                &plan_branch.id,
                plan_pr_status_from_publication_status(pr_status),
            )
            .await?;
        deps.plan_branch_repo
            .update_pr_push_status(&plan_branch.id, PrPushStatus::Pushed)
            .await?;
        return Ok(());
    }

    deps.workspace_repo
        .update_publication(
            conversation_id,
            Some(target.pr_number),
            target.pr_url.as_deref(),
            Some(pr_status),
            Some("pushed"),
        )
        .await
}

fn start_recovered_pr_polling(
    deps: &AgentWorkspacePrSupervisionRecoveryDeps,
    conversation_id: &ChatConversationId,
    project: &Project,
    target: &AgentWorkspacePrSupervisionRecoveryTarget,
) {
    let Some(registry) = deps.pr_poller_registry.as_ref() else {
        return;
    };

    if let Some(plan_branch) = target.plan_branch.as_ref() {
        let (Some(task_id), Some(transition_service)) = (
            plan_branch.merge_task_id.as_ref(),
            deps.transition_service.as_ref(),
        ) else {
            return;
        };
        registry.start_polling(
            task_id.clone(),
            plan_branch.id.clone(),
            target.pr_number,
            PathBuf::from(&project.working_directory),
            plan_branch.source_branch.clone(),
            Arc::clone(transition_service),
        );
        return;
    }

    let Some(chat_service) = deps.chat_service.as_ref() else {
        return;
    };
    registry.start_agent_workspace_polling(
        conversation_id.clone(),
        target.pr_number,
        project.clone(),
        target.worktree_path.clone(),
        Arc::clone(&deps.workspace_repo),
        Arc::clone(&deps.agent_run_repo),
        Arc::clone(chat_service),
    );
}

pub(crate) async fn recover_recent_agent_workspace_pr_supervision_on_startup(
    deps: AgentWorkspacePrSupervisionRecoveryDeps,
    blocked_git_project_ids: Arc<HashSet<ProjectId>>,
) {
    let started = Instant::now();
    let workspaces = match list_pr_supervision_recovery_candidates(&deps).await {
        Ok(workspaces) => workspaces,
        Err(error) => {
            tracing::warn!(
                error = %error,
                "Agent workspace PR supervision startup recovery failed to list candidates"
            );
            return;
        }
    };

    let candidate_count = workspaces.len();
    if candidate_count == 0 {
        tracing::debug!("Agent workspace PR supervision startup recovery found no candidates");
        return;
    }

    let deps = Arc::new(deps);
    stream::iter(workspaces)
        .for_each_concurrent(STARTUP_PR_SUPERVISION_RECOVERY_CONCURRENCY, |workspace| {
            let deps = Arc::clone(&deps);
            let blocked_git_project_ids = Arc::clone(&blocked_git_project_ids);
            async move {
                if blocked_git_project_ids.contains(&workspace.project_id) {
                    tracing::info!(
                        conversation_id = workspace.conversation_id.as_str(),
                        project_id = %workspace.project_id,
                        "Agent workspace PR supervision startup recovery skipped blocked project"
                    );
                    return;
                }
                let conversation_id = workspace.conversation_id.clone();
                if !claim_recovery(&conversation_id, true) {
                    return;
                }
                let result = recover_agent_workspace_pr_supervision(
                    (*deps).clone(),
                    conversation_id.clone(),
                    AgentWorkspacePrSupervisionRecoveryTrigger::Startup,
                )
                .await;
                RECENT_RECOVERIES
                    .get_or_init(DashMap::new)
                    .insert(conversation_id.as_str(), Instant::now());
                IN_FLIGHT_RECOVERIES
                    .get_or_init(DashMap::new)
                    .remove(&conversation_id.as_str());
                if let Err(error) = result {
                    tracing::warn!(
                        conversation_id = conversation_id.as_str(),
                        error = %error,
                        "Agent workspace PR supervision startup recovery candidate failed"
                    );
                }
            }
        })
        .await;

    tracing::info!(
        candidate_count,
        elapsed_ms = started.elapsed().as_millis() as u64,
        "Agent workspace PR supervision startup recovery completed"
    );
}

async fn list_pr_supervision_recovery_candidates(
    deps: &AgentWorkspacePrSupervisionRecoveryDeps,
) -> AppResult<Vec<AgentConversationWorkspace>> {
    let mut workspaces = deps
        .workspace_repo
        .list_active_direct_pr_supervision_recovery_candidates(
            STARTUP_PR_SUPERVISION_RECOVERY_LIMIT,
        )
        .await?;
    let remaining = STARTUP_PR_SUPERVISION_RECOVERY_LIMIT.saturating_sub(workspaces.len());
    if remaining > 0 {
        workspaces.extend(
            deps.workspace_repo
                .list_active_linked_plan_pr_supervision_recovery_candidates(remaining)
                .await?,
        );
    }
    Ok(workspaces)
}

pub(crate) fn pr_supervision_recovery_schedule_skip_reason(
    workspace: &AgentConversationWorkspace,
) -> Option<&'static str> {
    if let Some(reason) = pr_supervision_recovery_base_skip_reason(workspace) {
        return Some(reason);
    }
    if workspace.mode == AgentConversationWorkspaceMode::Ideation {
        return if is_linked_plan_pr_supervision_recovery_candidate(workspace) {
            None
        } else {
            Some("workspace_supervision_not_recoverable")
        };
    }
    let blocked_failed = workspace.publication_push_status.as_deref() == Some("failed")
        && workspace.pr_supervision_status.as_deref() == Some("blocked");
    let stale_candidate = workspace.publication_push_status.as_deref() == Some("needs_agent");
    if blocked_failed || stale_candidate {
        None
    } else {
        Some("workspace_push_not_recoverable")
    }
}

fn blocked_pr_supervision_recovery_skip_reason(
    workspace: &AgentConversationWorkspace,
) -> Option<&'static str> {
    if let Some(reason) = pr_supervision_recovery_base_skip_reason(workspace) {
        return Some(reason);
    }
    if workspace.mode == AgentConversationWorkspaceMode::Ideation {
        return if is_linked_plan_pr_supervision_recovery_candidate(workspace) {
            None
        } else {
            Some("workspace_supervision_not_recoverable")
        };
    }
    if workspace.publication_push_status.as_deref() != Some("failed") {
        return Some("workspace_push_not_failed");
    }
    if workspace.pr_supervision_status.as_deref() != Some("blocked") {
        return Some("workspace_supervision_not_blocked");
    }
    None
}

fn pr_supervision_recovery_base_skip_reason(
    workspace: &AgentConversationWorkspace,
) -> Option<&'static str> {
    if workspace.status != AgentConversationWorkspaceStatus::Active {
        return Some("workspace_not_active");
    }
    if workspace.has_terminal_publication_pr_status() {
        return Some("workspace_terminal");
    }
    if !workspace.auto_publish_enabled {
        return Some("auto_publish_disabled");
    }
    if !workspace.pr_autofix_enabled && !workspace.pr_auto_merge_desired {
        return Some("pr_supervision_disabled");
    }
    match workspace.mode {
        AgentConversationWorkspaceMode::Edit => {
            if workspace.linked_plan_branch_id.is_some() {
                return Some("workspace_linked_to_plan_branch");
            }
            if workspace.publication_pr_number.is_none() {
                return Some("missing_pr_number");
            }
            None
        }
        AgentConversationWorkspaceMode::Ideation => {
            if workspace.linked_plan_branch_id.is_none() {
                return Some("workspace_missing_plan_branch");
            }
            None
        }
        _ => Some("workspace_not_edit_or_ideation_mode"),
    }
}

fn is_linked_plan_pr_supervision_recovery_candidate(
    workspace: &AgentConversationWorkspace,
) -> bool {
    workspace.linked_plan_branch_id.is_some()
        && matches!(
            workspace.pr_supervision_status.as_deref(),
            Some("blocked" | "fixing")
        )
}

fn pr_sync_state_recovery_skip_reason(
    expected_head_ref: &str,
    sync_state: &PrSyncState,
    local_head_sha: &str,
) -> Option<&'static str> {
    if sync_state.status != GithubPrStatus::Open {
        return Some("pr_not_open");
    }
    if sync_state.head_ref_name != expected_head_ref {
        return Some("pr_head_branch_mismatch");
    }
    let Some(remote_head_sha) = sync_state.head_ref_oid.as_deref() else {
        return Some("pr_head_sha_missing");
    };
    if !remote_head_sha.eq_ignore_ascii_case(local_head_sha) {
        return Some("pr_head_sha_mismatch");
    }
    None
}

fn publication_status_for_sync_state(sync_state: &PrSyncState) -> &'static str {
    match sync_state.status {
        GithubPrStatus::Merged { .. } => "merged",
        GithubPrStatus::Closed => "closed",
        GithubPrStatus::Open if sync_state.is_draft => "draft",
        GithubPrStatus::Open => "open",
    }
}

fn plan_pr_status_from_publication_status(pr_status: &str) -> PlanPrStatus {
    match pr_status {
        "draft" => PlanPrStatus::Draft,
        "merged" => PlanPrStatus::Merged,
        "closed" => PlanPrStatus::Closed,
        _ => PlanPrStatus::Open,
    }
}

fn is_terminal_pr_sync_status(status: &GithubPrStatus) -> bool {
    matches!(
        status,
        GithubPrStatus::Closed | GithubPrStatus::Merged { .. }
    )
}

fn terminal_pr_recovery_summary(pr_status: &str) -> &'static str {
    match pr_status {
        "merged" => "Pull request merged while PR supervision was blocked.",
        "closed" => "Pull request closed while PR supervision was blocked.",
        _ => "Pull request reached a terminal state while PR supervision was blocked.",
    }
}

fn emit_workspace_changed(app_handle: Option<&AppHandle>, conversation_id: &ChatConversationId) {
    if let Some(handle) = app_handle {
        let _ = handle.emit(
            "agent:workspace_changed",
            serde_json::json!({ "conversation_id": conversation_id.as_str() }),
        );
    }
}

fn claim_recovery(conversation_id: &ChatConversationId, force: bool) -> bool {
    let key = conversation_id.as_str();
    let in_flight = IN_FLIGHT_RECOVERIES.get_or_init(DashMap::new);
    if in_flight.contains_key(&key) {
        return false;
    }
    if !force {
        let ttl = recovery_cache_ttl();
        if !ttl.is_zero() {
            if let Some(last_checked) = RECENT_RECOVERIES.get_or_init(DashMap::new).get(&key) {
                if last_checked.elapsed() < ttl {
                    return false;
                }
            }
        }
    }
    in_flight.insert(key, ());
    true
}

fn recovery_cache_ttl() -> Duration {
    Duration::from_millis(git_runtime_config().agent_workspace_pr_reconciliation_cache_ttl_ms)
}
