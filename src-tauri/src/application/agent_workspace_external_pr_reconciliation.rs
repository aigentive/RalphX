use std::collections::HashSet;
use std::path::Path;
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};

use dashmap::DashMap;
use futures::{stream, StreamExt as _};
use tauri::{AppHandle, Emitter};

use crate::application::chat_service::ChatService;
use crate::application::services::pr_merge_poller::terminalize_agent_workspace_after_pr;
use crate::application::PrPollerRegistry;
use crate::domain::entities::{
    AgentConversationWorkspace, AgentConversationWorkspaceMode,
    AgentConversationWorkspacePublicationEvent, AgentConversationWorkspaceStatus,
    ChatConversationId, Project, ProjectId,
};
use crate::domain::repositories::{
    AgentConversationWorkspaceRepository, AgentRunRepository, ProjectRepository,
};
use crate::domain::services::{GithubServiceTrait, PrStatus};
use crate::error::AppResult;
use crate::infrastructure::agents::claude::git_runtime_config;

const STARTUP_EXTERNAL_PR_RECONCILIATION_LIMIT: usize = 25;
const STARTUP_EXTERNAL_PR_RECONCILIATION_CONCURRENCY: usize = 4;

static IN_FLIGHT_RECONCILIATIONS: OnceLock<DashMap<String, ()>> = OnceLock::new();
static RECENT_RECONCILIATIONS: OnceLock<DashMap<String, Instant>> = OnceLock::new();

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AgentWorkspaceExternalPrReconciliationTrigger {
    WorkspaceLoad,
    AgentRunCompleted,
    Startup,
}

impl AgentWorkspaceExternalPrReconciliationTrigger {
    fn as_str(self) -> &'static str {
        match self {
            Self::WorkspaceLoad => "workspace_load",
            Self::AgentRunCompleted => "agent_run_completed",
            Self::Startup => "startup",
        }
    }
}

#[derive(Clone)]
pub(crate) struct AgentWorkspaceExternalPrReconciliationDeps {
    pub workspace_repo: Arc<dyn AgentConversationWorkspaceRepository>,
    pub project_repo: Arc<dyn ProjectRepository>,
    pub github: Arc<dyn GithubServiceTrait>,
    pub pr_poller_registry: Option<Arc<PrPollerRegistry>>,
    pub chat_service: Option<Arc<dyn ChatService>>,
    pub agent_run_repo: Arc<dyn AgentRunRepository>,
    pub app_handle: Option<AppHandle>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum AgentWorkspaceExternalPrReconciliationOutcome {
    Skipped(&'static str),
    NotFound,
    Linked { pr_number: i64, pr_status: String },
}

pub(crate) fn schedule_agent_workspace_external_pr_reconciliation(
    deps: AgentWorkspaceExternalPrReconciliationDeps,
    conversation_id: ChatConversationId,
    trigger: AgentWorkspaceExternalPrReconciliationTrigger,
    force: bool,
) {
    if !claim_reconciliation(&conversation_id, force) {
        tracing::debug!(
            conversation_id = conversation_id.as_str(),
            trigger = trigger.as_str(),
            "Agent workspace external PR reconciliation skipped before scheduling"
        );
        return;
    }

    tokio::spawn(async move {
        let started = Instant::now();
        let result =
            reconcile_agent_workspace_external_pr(deps, conversation_id.clone(), trigger).await;
        RECENT_RECONCILIATIONS
            .get_or_init(DashMap::new)
            .insert(conversation_id.as_str(), Instant::now());
        IN_FLIGHT_RECONCILIATIONS
            .get_or_init(DashMap::new)
            .remove(&conversation_id.as_str());

        match result {
            Ok(outcome) => tracing::info!(
                conversation_id = conversation_id.as_str(),
                trigger = trigger.as_str(),
                elapsed_ms = started.elapsed().as_millis() as u64,
                outcome = ?outcome,
                "Agent workspace external PR reconciliation completed"
            ),
            Err(error) => tracing::warn!(
                conversation_id = conversation_id.as_str(),
                trigger = trigger.as_str(),
                elapsed_ms = started.elapsed().as_millis() as u64,
                error = %error,
                "Agent workspace external PR reconciliation failed"
            ),
        }
    });
}

pub(crate) async fn reconcile_agent_workspace_external_pr(
    deps: AgentWorkspaceExternalPrReconciliationDeps,
    conversation_id: ChatConversationId,
    trigger: AgentWorkspaceExternalPrReconciliationTrigger,
) -> AppResult<AgentWorkspaceExternalPrReconciliationOutcome> {
    let Some(workspace) = deps
        .workspace_repo
        .get_by_conversation_id(&conversation_id)
        .await?
    else {
        return Ok(AgentWorkspaceExternalPrReconciliationOutcome::Skipped(
            "workspace_missing",
        ));
    };

    if let Some(reason) = external_pr_reconciliation_skip_reason(&workspace) {
        return Ok(AgentWorkspaceExternalPrReconciliationOutcome::Skipped(
            reason,
        ));
    }

    let Some(project) = deps.project_repo.get_by_id(&workspace.project_id).await? else {
        return Ok(AgentWorkspaceExternalPrReconciliationOutcome::Skipped(
            "project_missing",
        ));
    };

    if project.archived_at.is_some() {
        return Ok(AgentWorkspaceExternalPrReconciliationOutcome::Skipped(
            "project_archived",
        ));
    }
    if !project.github_pr_enabled {
        return Ok(AgentWorkspaceExternalPrReconciliationOutcome::Skipped(
            "github_pr_disabled",
        ));
    }

    if workspace.publication_pr_number.is_some() {
        return reconcile_linked_agent_workspace_pr(deps, &workspace, &project).await;
    }

    let lookup_started = Instant::now();
    let branch_name = workspace.branch_name.clone();
    let pr = deps
        .github
        .find_latest_pr_by_head_branch(Path::new(&project.working_directory), &branch_name)
        .await?;
    tracing::info!(
        conversation_id = conversation_id.as_str(),
        trigger = trigger.as_str(),
        branch_name,
        elapsed_ms = lookup_started.elapsed().as_millis() as u64,
        found = pr.is_some(),
        "Agent workspace external PR lookup completed"
    );

    let Some(pr) = pr else {
        return Ok(AgentWorkspaceExternalPrReconciliationOutcome::NotFound);
    };

    let pr_status = pr.publication_status();
    deps.workspace_repo
        .update_publication(
            &conversation_id,
            Some(pr.number),
            Some(&pr.url),
            Some(pr_status),
            Some("pushed"),
        )
        .await?;
    append_external_pr_reconciliation_event(&deps.workspace_repo, &conversation_id, pr_status)
        .await?;
    emit_workspace_changed(deps.app_handle.as_ref(), &conversation_id);

    if matches!(pr.status, PrStatus::Open) {
        if let (Some(registry), Some(chat_service)) =
            (deps.pr_poller_registry.as_ref(), deps.chat_service.as_ref())
        {
            registry.start_agent_workspace_polling(
                conversation_id.clone(),
                pr.number,
                project.clone(),
                Path::new(&project.working_directory).to_path_buf(),
                Arc::clone(&deps.workspace_repo),
                Arc::clone(&deps.agent_run_repo),
                Arc::clone(chat_service),
            );
        }
    } else {
        terminalize_agent_workspace_after_pr(
            Arc::clone(&deps.workspace_repo),
            Arc::clone(&deps.agent_run_repo),
            deps.chat_service.as_ref().map(Arc::clone),
            &conversation_id,
            &project,
            Some(Arc::clone(&deps.github)),
            matches!(pr.status, PrStatus::Merged { .. }),
            true,
            pr_status,
        )
        .await;
    }

    Ok(AgentWorkspaceExternalPrReconciliationOutcome::Linked {
        pr_number: pr.number,
        pr_status: pr_status.to_string(),
    })
}

async fn reconcile_linked_agent_workspace_pr(
    deps: AgentWorkspaceExternalPrReconciliationDeps,
    workspace: &AgentConversationWorkspace,
    project: &Project,
) -> AppResult<AgentWorkspaceExternalPrReconciliationOutcome> {
    let Some(pr_number) = workspace.publication_pr_number else {
        return Ok(AgentWorkspaceExternalPrReconciliationOutcome::Skipped(
            "missing_pr_number",
        ));
    };

    let status = deps
        .github
        .check_pr_status(Path::new(&project.working_directory), pr_number)
        .await?;
    let pr_status = publication_status_for_pr_status(&status);
    if !matches!(status, PrStatus::Closed | PrStatus::Merged { .. }) {
        return Ok(AgentWorkspaceExternalPrReconciliationOutcome::Skipped(
            "linked_pr_not_terminal",
        ));
    }

    deps.workspace_repo
        .update_publication(
            &workspace.conversation_id,
            Some(pr_number),
            workspace.publication_pr_url.as_deref(),
            Some(pr_status),
            Some("pushed"),
        )
        .await?;
    deps.workspace_repo
        .append_publication_event(AgentConversationWorkspacePublicationEvent::new(
            workspace.conversation_id.clone(),
            format!("pr_{pr_status}"),
            "succeeded",
            terminal_linked_pr_summary(pr_status),
            None,
        ))
        .await?;
    emit_workspace_changed(deps.app_handle.as_ref(), &workspace.conversation_id);
    terminalize_agent_workspace_after_pr(
        Arc::clone(&deps.workspace_repo),
        Arc::clone(&deps.agent_run_repo),
        deps.chat_service.as_ref().map(Arc::clone),
        &workspace.conversation_id,
        project,
        matches!(status, PrStatus::Merged { .. }).then(|| Arc::clone(&deps.github)),
        pr_status == "merged",
        true,
        pr_status,
    )
    .await;

    Ok(AgentWorkspaceExternalPrReconciliationOutcome::Linked {
        pr_number,
        pr_status: pr_status.to_string(),
    })
}

fn publication_status_for_pr_status(status: &PrStatus) -> &'static str {
    match status {
        PrStatus::Open => "open",
        PrStatus::Closed => "closed",
        PrStatus::Merged { .. } => "merged",
    }
}

fn terminal_linked_pr_summary(status: &str) -> &'static str {
    match status {
        "merged" => "Pull request merged",
        "closed" => "Pull request closed without merging",
        _ => "Pull request reached a terminal state",
    }
}

pub(crate) async fn reconcile_recent_agent_workspace_external_prs_on_startup(
    deps: AgentWorkspaceExternalPrReconciliationDeps,
    blocked_git_project_ids: Arc<HashSet<ProjectId>>,
) {
    let started = Instant::now();
    let workspaces = match deps
        .workspace_repo
        .list_active_direct_external_pr_reconciliation_candidates(
            STARTUP_EXTERNAL_PR_RECONCILIATION_LIMIT,
        )
        .await
    {
        Ok(workspaces) => workspaces,
        Err(error) => {
            tracing::warn!(
                error = %error,
                "Agent workspace external PR startup reconciliation failed to list candidates"
            );
            return;
        }
    };

    let candidate_count = workspaces.len();
    if candidate_count == 0 {
        tracing::debug!("Agent workspace external PR startup reconciliation found no candidates");
        return;
    }

    let deps = Arc::new(deps);
    stream::iter(workspaces)
        .for_each_concurrent(STARTUP_EXTERNAL_PR_RECONCILIATION_CONCURRENCY, |workspace| {
            let deps = Arc::clone(&deps);
            let blocked_git_project_ids = Arc::clone(&blocked_git_project_ids);
            async move {
                if blocked_git_project_ids.contains(&workspace.project_id) {
                    tracing::info!(
                        conversation_id = workspace.conversation_id.as_str(),
                        project_id = %workspace.project_id,
                        "Agent workspace external PR startup reconciliation skipped blocked project"
                    );
                    return;
                }
                let conversation_id = workspace.conversation_id.clone();
                if !claim_reconciliation(&conversation_id, true) {
                    return;
                }
                let result = reconcile_agent_workspace_external_pr(
                    (*deps).clone(),
                    conversation_id.clone(),
                    AgentWorkspaceExternalPrReconciliationTrigger::Startup,
                )
                .await;
                RECENT_RECONCILIATIONS
                    .get_or_init(DashMap::new)
                    .insert(conversation_id.as_str(), Instant::now());
                IN_FLIGHT_RECONCILIATIONS
                    .get_or_init(DashMap::new)
                    .remove(&conversation_id.as_str());
                if let Err(error) = result {
                    tracing::warn!(
                        conversation_id = conversation_id.as_str(),
                        error = %error,
                        "Agent workspace external PR startup reconciliation candidate failed"
                    );
                }
            }
        })
        .await;

    tracing::info!(
        candidate_count,
        elapsed_ms = started.elapsed().as_millis() as u64,
        "Agent workspace external PR startup reconciliation completed"
    );
}

pub(crate) fn external_pr_reconciliation_skip_reason(
    workspace: &AgentConversationWorkspace,
) -> Option<&'static str> {
    if workspace.mode != AgentConversationWorkspaceMode::Edit {
        return Some("workspace_not_edit_mode");
    }
    if workspace.linked_plan_branch_id.is_some() {
        return Some("workspace_linked_to_plan_branch");
    }
    if matches!(
        workspace.publication_pr_status.as_deref(),
        Some("closed") | Some("merged")
    ) && workspace.publication_pr_number.is_none()
    {
        return Some("workspace_terminal");
    }
    if workspace.publication_pr_number.is_some() {
        return match workspace.status {
            AgentConversationWorkspaceStatus::Active
            | AgentConversationWorkspaceStatus::Missing => None,
            AgentConversationWorkspaceStatus::Archived => Some("workspace_not_active"),
        };
    }
    if workspace.status != AgentConversationWorkspaceStatus::Active {
        return Some("workspace_not_active");
    }
    if matches!(
        workspace.publication_push_status.as_deref(),
        Some("needs_agent" | "pending" | "failed" | "description_failed")
    ) {
        return Some("workspace_push_not_reconcilable");
    }
    None
}

async fn append_external_pr_reconciliation_event(
    workspace_repo: &Arc<dyn AgentConversationWorkspaceRepository>,
    conversation_id: &ChatConversationId,
    pr_status: &str,
) -> AppResult<()> {
    let (step, summary) = match pr_status {
        "merged" => ("external_pr_merged", "External pull request was merged"),
        "closed" => (
            "external_pr_closed",
            "External pull request was closed without merging",
        ),
        "draft" => ("external_pr_linked", "External draft pull request linked"),
        _ => ("external_pr_linked", "External pull request linked"),
    };
    workspace_repo
        .append_publication_event(AgentConversationWorkspacePublicationEvent::new(
            conversation_id.clone(),
            step,
            "succeeded",
            summary,
            None,
        ))
        .await
}

fn emit_workspace_changed(app_handle: Option<&AppHandle>, conversation_id: &ChatConversationId) {
    if let Some(handle) = app_handle {
        let _ = handle.emit(
            "agent:workspace_changed",
            serde_json::json!({ "conversation_id": conversation_id.as_str() }),
        );
    }
}

fn claim_reconciliation(conversation_id: &ChatConversationId, force: bool) -> bool {
    let key = conversation_id.as_str();
    let in_flight = IN_FLIGHT_RECONCILIATIONS.get_or_init(DashMap::new);
    if in_flight.contains_key(&key) {
        return false;
    }
    if !force {
        let ttl = reconciliation_cache_ttl();
        if !ttl.is_zero() {
            if let Some(last_checked) = RECENT_RECONCILIATIONS.get_or_init(DashMap::new).get(&key) {
                if last_checked.elapsed() < ttl {
                    return false;
                }
            }
        }
    }
    in_flight.insert(key, ());
    true
}

fn reconciliation_cache_ttl() -> Duration {
    Duration::from_millis(git_runtime_config().agent_workspace_pr_reconciliation_cache_ttl_ms)
}
