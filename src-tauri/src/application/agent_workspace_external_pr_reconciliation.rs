use std::collections::HashSet;
use std::path::Path;
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};

use dashmap::DashMap;
use futures::{stream, StreamExt as _};
use tauri::{AppHandle, Emitter, Manager};

use crate::application::agent_conversation_workspace::resolve_valid_agent_conversation_workspace_path;
use crate::application::agent_workspace_publication_reconciliation::{
    correct_foreign_agent_workspace_publication, PublicationCorrectionOutcome,
};
use crate::application::agent_workspace_terminal_cleanup::{
    terminalize_agent_workspace_after_pr, TerminalAgentWorkspaceCause,
};
use crate::application::chat_service::ChatService;
use crate::application::clickup_git_association::{
    reconcile_clickup_pr_to_conversation, ClickUpGitEvidence, ClickUpPrAssociationInput,
    ClickUpPrAssociationOutcome,
};
use crate::application::clickup_integration_service::ClickUpIntegrationService;
use crate::application::external_issue_link_service::ExternalIssueLinkService;
use crate::application::git_service::GitService;
use crate::application::ticketing_cache_invalidator::{
    TicketingCacheInvalidatedEvent, TICKETING_CACHE_INVALIDATED_EVENT,
};
use crate::application::{AppState, PrPollerRegistry};
use crate::domain::entities::{
    AgentConversationWorkspace, AgentConversationWorkspaceMode,
    AgentConversationWorkspacePublicationEvent, AgentConversationWorkspaceStatus,
    ChatConversationId, Project, ProjectId,
};
use crate::domain::repositories::{
    AgentConversationWorkspaceRepository, AgentRunRepository, AgentWorkspaceRepairRepository,
    ChatConversationRepository, PlanBranchRepository, ProjectRepository,
};
use crate::domain::services::{GithubServiceTrait, PrStatus};
use crate::error::{AppError, AppResult};
use crate::infrastructure::agents::claude::git_runtime_config;

const STARTUP_EXTERNAL_PR_RECONCILIATION_LIMIT: usize = 25;
const STARTUP_EXTERNAL_PR_RECONCILIATION_CONCURRENCY: usize = 4;

static IN_FLIGHT_RECONCILIATIONS: OnceLock<DashMap<String, ()>> = OnceLock::new();
static RECENT_RECONCILIATIONS: OnceLock<DashMap<String, Instant>> = OnceLock::new();

/// Releases the in-flight claim and stamps the recent map even if the
/// reconciliation task panics (for example inside a lazy deps factory);
/// otherwise a single panic would permanently suppress reconciliation for
/// this workspace until restart.
struct InFlightReconciliationClaim {
    conversation_id: ChatConversationId,
}

impl Drop for InFlightReconciliationClaim {
    fn drop(&mut self) {
        RECENT_RECONCILIATIONS
            .get_or_init(DashMap::new)
            .insert(self.conversation_id.as_str(), Instant::now());
        IN_FLIGHT_RECONCILIATIONS
            .get_or_init(DashMap::new)
            .remove(&self.conversation_id.as_str());
    }
}

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
    pub chat_conversation_repo: Arc<dyn ChatConversationRepository>,
    pub project_repo: Arc<dyn ProjectRepository>,
    pub github: Arc<dyn GithubServiceTrait>,
    pub clickup_integration_service: Option<Arc<ClickUpIntegrationService>>,
    pub external_issue_link_service: Option<Arc<ExternalIssueLinkService>>,
    pub pr_poller_registry: Option<Arc<PrPollerRegistry>>,
    pub chat_service: Option<Arc<dyn ChatService>>,
    pub agent_run_repo: Arc<dyn AgentRunRepository>,
    /// Every live reconciliation, including startup, supplies this repository so the poller
    /// cannot fall back to legacy repair authority.
    pub agent_workspace_repair_repo: Option<Arc<dyn AgentWorkspaceRepairRepository>>,
    pub plan_branch_repo: Arc<dyn PlanBranchRepository>,
    pub app_handle: Option<AppHandle>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum AgentWorkspaceExternalPrReconciliationOutcome {
    Skipped(&'static str),
    NotFound,
    Linked { pr_number: i64, pr_status: String },
}

pub(crate) fn schedule_agent_workspace_external_pr_reconciliation_with_lazy_deps(
    deps_factory: impl FnOnce() -> AgentWorkspaceExternalPrReconciliationDeps + Send + 'static,
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
        let _claim = InFlightReconciliationClaim {
            conversation_id: conversation_id.clone(),
        };
        let started = Instant::now();
        let result =
            reconcile_agent_workspace_external_pr(deps_factory(), conversation_id.clone(), trigger)
                .await;

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

    // Archived linked workspaces are normally skipped by the gate below, but a foreign
    // publication association must remain correctable so terminal cleanup that consumed a
    // PR the workspace never owned can be reversed. Owned associations stay skipped so
    // reconciliation cannot replay terminal events for archived workspaces.
    if workspace.mode == AgentConversationWorkspaceMode::Edit
        && workspace.linked_plan_branch_id.is_none()
        && workspace.publication_pr_number.is_some()
        && workspace.status == AgentConversationWorkspaceStatus::Archived
    {
        let project = match deps.project_repo.get_by_id(&workspace.project_id).await {
            Ok(project) => project,
            Err(error) => {
                tracing::warn!(
                    conversation_id = workspace.conversation_id.as_str(),
                    error = %error,
                    "Foreign publication correction skipped because project could not be read"
                );
                return Ok(AgentWorkspaceExternalPrReconciliationOutcome::Skipped(
                    "foreign_publication_unverified",
                ));
            }
        };
        let Some(project) = project else {
            return Ok(AgentWorkspaceExternalPrReconciliationOutcome::Skipped(
                "project_missing",
            ));
        };
        if project.archived_at.is_some() {
            return Ok(AgentWorkspaceExternalPrReconciliationOutcome::Skipped(
                "project_archived",
            ));
        }
        return match correct_foreign_agent_workspace_publication(
            Arc::clone(&deps.workspace_repo),
            Arc::clone(&deps.chat_conversation_repo),
            Arc::clone(&deps.github),
            &project,
            &workspace,
        )
        .await?
        {
            PublicationCorrectionOutcome::Corrected => {
                emit_workspace_changed(deps.app_handle.as_ref(), &workspace.conversation_id);
                Ok(AgentWorkspaceExternalPrReconciliationOutcome::Skipped(
                    "foreign_publication_corrected",
                ))
            }
            PublicationCorrectionOutcome::Unverified => {
                Ok(AgentWorkspaceExternalPrReconciliationOutcome::Skipped(
                    "foreign_publication_unverified",
                ))
            }
            PublicationCorrectionOutcome::Skipped | PublicationCorrectionOutcome::NotApplicable => {
                Ok(AgentWorkspaceExternalPrReconciliationOutcome::Skipped(
                    "workspace_not_active",
                ))
            }
        };
    }

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

    if workspace.publication_pr_number.is_some() {
        return reconcile_linked_agent_workspace_pr(deps, &workspace, &project).await;
    }

    if !project.github_pr_enabled {
        return Ok(AgentWorkspaceExternalPrReconciliationOutcome::Skipped(
            "github_pr_disabled",
        ));
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

    if pr.head_ref_name != workspace.branch_name {
        tracing::warn!(
            conversation_id = conversation_id.as_str(),
            workspace_branch = workspace.branch_name.as_str(),
            pr_number = pr.number,
            pr_head_branch = pr.head_ref_name.as_str(),
            "Agent workspace external PR lookup returned a mismatched head branch"
        );
        return Ok(AgentWorkspaceExternalPrReconciliationOutcome::NotFound);
    }

    require_durable_live_pr_poller(&deps, matches!(&pr.status, PrStatus::Open))?;

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
    reconcile_clickup_ticket_for_workspace_pr(
        &deps,
        &workspace,
        &project,
        pr.number,
        Some(pr.url.clone()),
        pr_status,
    )
    .await;

    if matches!(&pr.status, PrStatus::Open) {
        start_agent_workspace_pr_poller(
            &deps,
            conversation_id.clone(),
            pr.number,
            project.clone(),
            Path::new(&project.working_directory).to_path_buf(),
        );
    } else {
        let terminalized = terminalize_agent_workspace_after_pr(
            Arc::clone(&deps.workspace_repo),
            Arc::clone(&deps.agent_run_repo),
            Some(Arc::clone(&deps.plan_branch_repo)),
            deps.chat_service.as_ref().map(Arc::clone),
            &conversation_id,
            &project,
            TerminalAgentWorkspaceCause::from_pr_status(pr_status),
        )
        .await;
        terminalized
            .require_runtime_shutdown()
            .map_err(AppError::Infrastructure)?;
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

    match correct_foreign_agent_workspace_publication(
        Arc::clone(&deps.workspace_repo),
        Arc::clone(&deps.chat_conversation_repo),
        Arc::clone(&deps.github),
        project,
        workspace,
    )
    .await?
    {
        PublicationCorrectionOutcome::Corrected => {
            emit_workspace_changed(deps.app_handle.as_ref(), &workspace.conversation_id);
            return Ok(AgentWorkspaceExternalPrReconciliationOutcome::Skipped(
                "foreign_publication_corrected",
            ));
        }
        PublicationCorrectionOutcome::Unverified => {
            return Ok(AgentWorkspaceExternalPrReconciliationOutcome::Skipped(
                "foreign_publication_unverified",
            ));
        }
        PublicationCorrectionOutcome::Skipped | PublicationCorrectionOutcome::NotApplicable => {}
    }

    let status = deps
        .github
        .check_pr_status(Path::new(&project.working_directory), pr_number)
        .await?;
    let pr_status = publication_status_for_pr_status(&status);
    require_durable_live_pr_poller(&deps, matches!(&status, PrStatus::Open))?;
    reconcile_clickup_ticket_for_workspace_pr(
        &deps,
        workspace,
        project,
        pr_number,
        workspace.publication_pr_url.clone(),
        pr_status,
    )
    .await;
    if matches!(status, PrStatus::Open) {
        start_agent_workspace_pr_poller(
            &deps,
            workspace.conversation_id.clone(),
            pr_number,
            project.clone(),
            Path::new(&workspace.worktree_path).to_path_buf(),
        );
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
    let terminalized = terminalize_agent_workspace_after_pr(
        Arc::clone(&deps.workspace_repo),
        Arc::clone(&deps.agent_run_repo),
        Some(Arc::clone(&deps.plan_branch_repo)),
        deps.chat_service.as_ref().map(Arc::clone),
        &workspace.conversation_id,
        project,
        TerminalAgentWorkspaceCause::from_pr_status(pr_status),
    )
    .await;
    terminalized
        .require_runtime_shutdown()
        .map_err(AppError::Infrastructure)?;

    Ok(AgentWorkspaceExternalPrReconciliationOutcome::Linked {
        pr_number,
        pr_status: pr_status.to_string(),
    })
}

fn require_durable_live_pr_poller(
    deps: &AgentWorkspaceExternalPrReconciliationDeps,
    pr_is_open: bool,
) -> AppResult<()> {
    if pr_is_open
        && deps.pr_poller_registry.is_some()
        && deps.chat_service.is_some()
        && deps.agent_workspace_repair_repo.is_none()
    {
        return Err(AppError::Infrastructure(
            "Live external PR reconciliation requires the durable workspace repair repository"
                .to_string(),
        ));
    }
    Ok(())
}

fn start_agent_workspace_pr_poller(
    deps: &AgentWorkspaceExternalPrReconciliationDeps,
    conversation_id: ChatConversationId,
    pr_number: i64,
    project: Project,
    working_dir: std::path::PathBuf,
) {
    let (Some(registry), Some(chat_service)) =
        (deps.pr_poller_registry.as_ref(), deps.chat_service.as_ref())
    else {
        return;
    };

    if let Some(repair_repo) = deps.agent_workspace_repair_repo.as_ref() {
        let recovery_state = deps
            .app_handle
            .as_ref()
            .and_then(|app_handle| app_handle.try_state::<AppState>())
            .map(|state| Arc::new(state.inner().clone()));
        registry.start_agent_workspace_polling_with_repair_repo_and_recovery_state(
            conversation_id,
            pr_number,
            project,
            working_dir,
            Arc::clone(&deps.workspace_repo),
            Arc::clone(&deps.agent_run_repo),
            Arc::clone(repair_repo),
            Arc::clone(chat_service),
            recovery_state,
        );
    }
}

async fn reconcile_clickup_ticket_for_workspace_pr(
    deps: &AgentWorkspaceExternalPrReconciliationDeps,
    workspace: &AgentConversationWorkspace,
    project: &Project,
    pr_number: i64,
    pr_url: Option<String>,
    pr_status: &str,
) {
    let (Some(clickup), Some(links)) = (
        deps.clickup_integration_service.as_deref(),
        deps.external_issue_link_service.as_deref(),
    ) else {
        return;
    };

    let detail = deps
        .github
        .fetch_pr_detail(Path::new(&project.working_directory), pr_number)
        .await;
    let (branch, title, body, detail_url) = match detail {
        Ok(detail) => (detail.head_ref_name, detail.title, detail.body, detail.url),
        Err(error) => {
            tracing::debug!(
                conversation_id = workspace.conversation_id.as_str(),
                pr_number,
                error = %error,
                "ClickUp association PR detail unavailable; using branch evidence"
            );
            (workspace.branch_name.clone(), String::new(), None, None)
        }
    };
    let worktree_path =
        match resolve_valid_agent_conversation_workspace_path(project, workspace).await {
            Ok(path) => Some(path),
            Err(error) => {
                tracing::debug!(
                    conversation_id = workspace.conversation_id.as_str(),
                    pr_number,
                    error = %error,
                    "ClickUp association skipped local Git evidence for an invalid workspace path"
                );
                None
            }
        };
    // Only branch-authored commits may serve as ticket evidence: a plain
    // `base..HEAD` walk imports the base's own commit subjects (e.g. squash
    // merges from unrelated tickets) after a base-update merge when the local
    // base ref is stale.
    let commit_subjects = match worktree_path.as_deref() {
        Some(worktree_path) => {
            match GitService::get_branch_authored_commits(worktree_path, &workspace.base_ref).await
            {
                Ok(commits) => commits
                    .into_iter()
                    .take(40)
                    .map(|commit| commit.message)
                    .collect(),
                Err(error) => {
                    tracing::debug!(
                        conversation_id = workspace.conversation_id.as_str(),
                        pr_number,
                        error = %error,
                        "ClickUp association commit evidence unavailable"
                    );
                    Vec::new()
                }
            }
        }
        None => Vec::new(),
    };
    let head_sha = match worktree_path.as_deref() {
        Some(worktree_path) => GitService::get_head_sha(worktree_path).await.ok(),
        None => None,
    };
    let outcome = reconcile_clickup_pr_to_conversation(
        clickup,
        links,
        ClickUpPrAssociationInput {
            conversation_id: workspace.conversation_id.as_str(),
            project_id: project.id.to_string(),
            evidence: ClickUpGitEvidence {
                branch,
                title,
                body,
                commit_subjects,
            },
            pr_number,
            pr_url: pr_url.or(detail_url),
            pr_status: pr_status.to_string(),
            head_sha,
        },
    )
    .await;

    match outcome {
        Ok(ClickUpPrAssociationOutcome::Linked { task_id, link_id }) => {
            tracing::info!(
                conversation_id = workspace.conversation_id.as_str(),
                pr_number,
                task_id,
                link_id,
                "Linked ClickUp task to RalphX conversation from Git evidence"
            );
            emit_clickup_link_changed(deps.app_handle.as_ref(), project, workspace, &task_id);
        }
        Ok(ClickUpPrAssociationOutcome::Ambiguous { task_ids }) => tracing::warn!(
            conversation_id = workspace.conversation_id.as_str(),
            pr_number,
            task_ids = ?task_ids,
            "ClickUp Git association was ambiguous and was not persisted"
        ),
        Ok(ClickUpPrAssociationOutcome::PendingValidation { errors }) => tracing::warn!(
            conversation_id = workspace.conversation_id.as_str(),
            pr_number,
            errors = ?errors,
            "ClickUp Git association validation is pending"
        ),
        Ok(other) => tracing::debug!(
            conversation_id = workspace.conversation_id.as_str(),
            pr_number,
            outcome = ?other,
            "ClickUp Git association produced no link"
        ),
        Err(error) => tracing::warn!(
            conversation_id = workspace.conversation_id.as_str(),
            pr_number,
            error = %error,
            "ClickUp Git association failed without blocking PR reconciliation"
        ),
    }
}

fn emit_clickup_link_changed(
    app_handle: Option<&AppHandle>,
    project: &Project,
    workspace: &AgentConversationWorkspace,
    task_id: &str,
) {
    let Some(handle) = app_handle else {
        return;
    };
    let event = TicketingCacheInvalidatedEvent {
        provider: "clickup".to_string(),
        ticket_id: Some(task_id.to_string()),
        ticket_key: None,
        container_id: None,
        project_id: Some(project.id.to_string()),
        reason: "git_conversation_linked".to_string(),
        invalidated_at: chrono::Utc::now().to_rfc3339(),
    };
    let _ = handle.emit(TICKETING_CACHE_INVALIDATED_EVENT, event);
    emit_workspace_changed(Some(handle), &workspace.conversation_id);
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
