use std::collections::HashSet;
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};

use dashmap::DashMap;
use futures::{stream, StreamExt as _};
use tauri::{AppHandle, Emitter};

use crate::application::agent_conversation_workspace::resolve_valid_agent_conversation_workspace_path;
use crate::application::agent_workspace_publish_recovery::recover_stale_publish_repair_for_workspace_and_reload;
use crate::application::chat_service::ChatService;
use crate::application::git_service::GitService;
use crate::application::services::PrPollerRegistry;
use crate::domain::entities::{
    AgentConversationWorkspace, AgentConversationWorkspaceMode,
    AgentConversationWorkspacePublicationEvent, AgentConversationWorkspaceStatus,
    ChatConversationId, ProjectId,
};
use crate::domain::repositories::{
    AgentConversationWorkspaceRepository, AgentRunRepository, ProjectRepository,
};
use crate::domain::services::{GithubServiceTrait, PrStatus, PrSyncState};
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
    pub github: Arc<dyn GithubServiceTrait>,
    pub pr_poller_registry: Option<Arc<PrPollerRegistry>>,
    pub chat_service: Option<Arc<dyn ChatService>>,
    pub agent_run_repo: Arc<dyn AgentRunRepository>,
    pub app_handle: Option<AppHandle>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum AgentWorkspacePrSupervisionRecoveryOutcome {
    Skipped(&'static str),
    Recovered { pr_number: i64, head_sha: String },
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

    if workspace.publication_push_status.as_deref() == Some("needs_agent") {
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

    let Some(pr_number) = workspace.publication_pr_number else {
        return Ok(AgentWorkspacePrSupervisionRecoveryOutcome::Skipped(
            "missing_pr_number",
        ));
    };

    let worktree_path =
        match resolve_valid_agent_conversation_workspace_path(&project, &workspace).await {
            Ok(path) => path,
            Err(error) => {
                tracing::warn!(
                    conversation_id = conversation_id.as_str(),
                    pr_number,
                    trigger = trigger.as_str(),
                    error = %error,
                    "Agent workspace PR supervision recovery skipped unusable workspace path"
                );
                return Ok(AgentWorkspacePrSupervisionRecoveryOutcome::Skipped(
                    "workspace_path_invalid",
                ));
            }
        };

    if GitService::has_uncommitted_changes(&worktree_path).await? {
        return Ok(AgentWorkspacePrSupervisionRecoveryOutcome::Skipped(
            "worktree_dirty",
        ));
    }
    let local_head_sha = GitService::get_head_sha(&worktree_path).await?;
    let sync_state = deps
        .github
        .check_pr_sync_state(&worktree_path, pr_number)
        .await?;

    if let Some(reason) =
        pr_sync_state_recovery_skip_reason(&workspace, &sync_state, &local_head_sha)
    {
        return Ok(AgentWorkspacePrSupervisionRecoveryOutcome::Skipped(reason));
    }

    let pr_status = publication_status_for_sync_state(&sync_state);
    deps.workspace_repo
        .update_publication(
            &conversation_id,
            Some(pr_number),
            workspace.publication_pr_url.as_deref(),
            Some(pr_status),
            Some("pushed"),
        )
        .await?;
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
                "github_pr_supervision_recovered:{pr_number}:{local_head_sha}"
            )),
        ))
        .await?;
    emit_workspace_changed(deps.app_handle.as_ref(), &conversation_id);

    if let (Some(registry), Some(chat_service)) =
        (deps.pr_poller_registry.as_ref(), deps.chat_service.as_ref())
    {
        registry.start_agent_workspace_polling(
            conversation_id.clone(),
            pr_number,
            project,
            worktree_path,
            Arc::clone(&deps.workspace_repo),
            Arc::clone(&deps.agent_run_repo),
            Arc::clone(chat_service),
        );
    }

    Ok(AgentWorkspacePrSupervisionRecoveryOutcome::Recovered {
        pr_number,
        head_sha: local_head_sha,
    })
}

pub(crate) async fn recover_recent_agent_workspace_pr_supervision_on_startup(
    deps: AgentWorkspacePrSupervisionRecoveryDeps,
    blocked_git_project_ids: Arc<HashSet<ProjectId>>,
) {
    let started = Instant::now();
    let workspaces = match deps
        .workspace_repo
        .list_active_direct_pr_supervision_recovery_candidates(
            STARTUP_PR_SUPERVISION_RECOVERY_LIMIT,
        )
        .await
    {
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

pub(crate) fn pr_supervision_recovery_schedule_skip_reason(
    workspace: &AgentConversationWorkspace,
) -> Option<&'static str> {
    if let Some(reason) = pr_supervision_recovery_base_skip_reason(workspace) {
        return Some(reason);
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
    if workspace.mode != AgentConversationWorkspaceMode::Edit {
        return Some("workspace_not_edit_mode");
    }
    if workspace.linked_plan_branch_id.is_some() {
        return Some("workspace_linked_to_plan_branch");
    }
    if workspace.publication_pr_number.is_none() {
        return Some("missing_pr_number");
    }
    if workspace.has_terminal_publication_pr_status() {
        return Some("workspace_terminal");
    }
    if !workspace.pr_autofix_enabled && !workspace.pr_auto_merge_desired {
        return Some("pr_supervision_disabled");
    }
    None
}

fn pr_sync_state_recovery_skip_reason(
    workspace: &AgentConversationWorkspace,
    sync_state: &PrSyncState,
    local_head_sha: &str,
) -> Option<&'static str> {
    if sync_state.status != PrStatus::Open {
        return Some("pr_not_open");
    }
    if sync_state.head_ref_name != workspace.branch_name {
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
    if sync_state.is_draft {
        "draft"
    } else {
        "open"
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
