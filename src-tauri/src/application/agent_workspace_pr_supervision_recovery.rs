use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};

use async_trait::async_trait;
use dashmap::mapref::entry::Entry;
use dashmap::DashMap;
use futures::{stream, StreamExt as _};
use tauri::{AppHandle, Emitter};

use crate::application::agent_conversation_workspace::{
    ensure_linked_plan_branch_agent_worktree, resolve_valid_agent_conversation_workspace_path,
};
use crate::application::agent_workspace_pr_autofix_attempt::load_pr_autofix_attempt_decision;
use crate::application::agent_workspace_publish_recovery::recover_stale_publish_repair_for_workspace_in_state;
#[cfg(any(test, feature = "test-utils"))]
use crate::application::agent_workspace_publish_recovery::{
    recover_stale_publish_repair_for_workspace_with_project_repo_outcome,
    StalePublishRepairRecoveryOutcome,
};
use crate::application::agent_workspace_review::resolve_review_target;
use crate::application::agent_workspace_review_publish_handoff::{
    resume_pr_fix_publish_after_passed_workspace_review, PrFixReviewPublishResumeOutcome,
};
use crate::application::agent_workspace_terminal_cleanup::{
    terminalize_agent_workspace_after_pr, TerminalAgentWorkspaceCause,
};
use crate::application::chat_service::ChatService;
use crate::application::git_service::GitService;
use crate::application::services::pr_merge_poller::classify_agent_workspace_pr_autofix_issue;
use crate::application::services::PrPollerRegistry;
use crate::application::task_transition_service::TaskTransitionService;
use crate::application::AppState;
use crate::domain::entities::plan_branch::{PrPushStatus, PrStatus as PlanPrStatus};
use crate::domain::entities::{
    AgentConversationWorkspace, AgentConversationWorkspaceMode,
    AgentConversationWorkspacePublicationEvent, AgentConversationWorkspaceStatus,
    AgentWorkspaceRepairPhase, ChatConversationId, PlanBranch, PlanBranchStatus, Project,
    ProjectId,
};
use crate::domain::repositories::{
    AgentConversationWorkspaceRepository, AgentRunRepository, PlanBranchRepository,
    ProjectRepository,
};
use crate::domain::services::{GithubServiceTrait, PrStatus as GithubPrStatus, PrSyncState};
use crate::error::{AppError, AppResult};
use crate::infrastructure::agents::claude::git_runtime_config;

const STARTUP_PR_SUPERVISION_RECOVERY_LIMIT: usize = 25;
const STARTUP_PR_SUPERVISION_RECOVERY_CONCURRENCY: usize = 4;
const PR_SUPERVISION_RECOVERED_STEP: &str = "pr_supervision_recovered";
const PR_SUPERVISION_RECOVERED_SUMMARY: &str =
    "Recovered blocked PR supervision; RalphX is monitoring PR health again.";

static IN_FLIGHT_RECOVERIES: OnceLock<DashMap<String, ()>> = OnceLock::new();
static RECENT_RECOVERIES: OnceLock<DashMap<String, Instant>> = OnceLock::new();

/// Releases the in-flight claim and stamps the recent map even if the
/// recovery task panics (for example inside a lazy deps factory); otherwise a
/// single panic would permanently suppress PR supervision recovery for this
/// workspace until restart.
struct InFlightRecoveryClaim {
    conversation_id: ChatConversationId,
}

impl Drop for InFlightRecoveryClaim {
    fn drop(&mut self) {
        RECENT_RECOVERIES
            .get_or_init(DashMap::new)
            .insert(self.conversation_id.as_str(), Instant::now());
        IN_FLIGHT_RECOVERIES
            .get_or_init(DashMap::new)
            .remove(&self.conversation_id.as_str());
    }
}

fn is_in_flight_durable_repair_phase(phase: AgentWorkspaceRepairPhase) -> bool {
    matches!(
        phase,
        AgentWorkspaceRepairPhase::Requested
            | AgentWorkspaceRepairPhase::Dispatching
            | AgentWorkspaceRepairPhase::Repairing
            | AgentWorkspaceRepairPhase::Validating
            | AgentWorkspaceRepairPhase::ContinuationPending
            | AgentWorkspaceRepairPhase::Continuing
            | AgentWorkspaceRepairPhase::AwaitingReview
    )
}

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
    pub transition_service: Option<Arc<TaskTransitionService>>,
    pub chat_service: Option<Arc<dyn ChatService>>,
    pub agent_run_repo: Arc<dyn AgentRunRepository>,
    pub app_handle: Option<AppHandle>,
    pub pr_fix_review_publish_resumer: Option<Arc<dyn AgentWorkspacePrFixReviewPublishResumer>>,
    /// Production recovery reuses AppState so one canonical durable repair reconciler owns
    /// legacy import, attempt fencing, and continuation settlement before PR supervision.
    /// `None` exists only for focused legacy compatibility tests.
    pub durable_recovery_state: Option<Arc<AppState>>,
}

pub(crate) fn build_agent_workspace_pr_supervision_recovery_deps(
    state: &AppState,
    app_handle: Option<AppHandle>,
    transition_service: Option<Arc<TaskTransitionService>>,
    chat_service: Option<Arc<dyn ChatService>>,
    pr_fix_review_publish_resumer: Option<Arc<dyn AgentWorkspacePrFixReviewPublishResumer>>,
) -> Option<AgentWorkspacePrSupervisionRecoveryDeps> {
    let github = state.github_service.as_ref().map(Arc::clone)?;
    Some(AgentWorkspacePrSupervisionRecoveryDeps {
        workspace_repo: Arc::clone(&state.agent_conversation_workspace_repo),
        project_repo: Arc::clone(&state.project_repo),
        plan_branch_repo: Arc::clone(&state.plan_branch_repo),
        github,
        pr_poller_registry: Some(Arc::clone(&state.pr_poller_registry)),
        transition_service,
        chat_service,
        agent_run_repo: Arc::clone(&state.agent_run_repo),
        app_handle,
        pr_fix_review_publish_resumer,
        durable_recovery_state: Some(Arc::new(state.clone())),
    })
}

#[async_trait]
pub(crate) trait AgentWorkspacePrFixReviewPublishResumer: Send + Sync {
    async fn publish_pr_fix_after_workspace_review(
        &self,
        conversation_id: ChatConversationId,
    ) -> Result<Option<bool>, String>;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum AgentWorkspacePrSupervisionRecoveryOutcome {
    Skipped(&'static str),
    Recovered { pr_number: i64, head_sha: String },
    ReviewPublished { pr_number: i64 },
    Terminal { pr_number: i64, pr_status: String },
}

#[derive(Debug, Clone)]
struct AgentWorkspacePrSupervisionRecoveryTarget {
    pr_number: i64,
    pr_url: Option<String>,
    worktree_path: PathBuf,
    branch_name: String,
    plan_branch: Option<PlanBranch>,
}

pub(crate) fn schedule_agent_workspace_pr_supervision_recovery(
    deps: AgentWorkspacePrSupervisionRecoveryDeps,
    conversation_id: ChatConversationId,
    trigger: AgentWorkspacePrSupervisionRecoveryTrigger,
    force: bool,
) {
    schedule_agent_workspace_pr_supervision_recovery_with_lazy_deps(
        move || deps,
        conversation_id,
        trigger,
        force,
    );
}

pub(crate) fn schedule_agent_workspace_pr_supervision_recovery_with_lazy_deps(
    deps_factory: impl FnOnce() -> AgentWorkspacePrSupervisionRecoveryDeps + Send + 'static,
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
        let _claim = InFlightRecoveryClaim {
            conversation_id: conversation_id.clone(),
        };
        let started = Instant::now();
        let result = recover_agent_workspace_pr_supervision(
            deps_factory(),
            conversation_id.clone(),
            trigger,
        )
        .await;

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

    #[cfg(not(any(test, feature = "test-utils")))]
    if deps.durable_recovery_state.is_none() {
        tracing::error!(
            conversation_id = conversation_id.as_str(),
            "Agent workspace PR supervision recovery: refusing legacy repair authority without durable state"
        );
        return Ok(AgentWorkspacePrSupervisionRecoveryOutcome::Skipped(
            "durable_repair_authority_unavailable",
        ));
    }

    if let Some(state) = deps.durable_recovery_state.as_deref() {
        workspace = recover_stale_publish_repair_for_workspace_in_state(state, workspace).await?;
        if state
            .agent_workspace_repair_repo
            .get_current_repair_attempt(&conversation_id)
            .await?
            .is_some_and(|attempt| is_in_flight_durable_repair_phase(attempt.phase))
        {
            return Ok(AgentWorkspacePrSupervisionRecoveryOutcome::Skipped(
                "durable_repair_active",
            ));
        }
    } else {
        #[cfg(any(test, feature = "test-utils"))]
        if workspace.mode == AgentConversationWorkspaceMode::Edit
            && matches!(
                (
                    workspace.publication_push_status.as_deref(),
                    workspace.pr_supervision_status.as_deref(),
                ),
                (Some("needs_agent"), _) | (Some("refreshed"), Some("fixing" | "reviewing"))
            )
        {
            let (recovered_workspace, repair_outcome) =
                recover_stale_publish_repair_for_workspace_with_project_repo_outcome(
                    Arc::clone(&deps.workspace_repo),
                    Arc::clone(
                        &deps
                            .durable_recovery_state
                            .as_ref()
                            .expect("durable recovery state checked above")
                            .agent_workspace_repair_repo,
                    ),
                    Arc::clone(&deps.agent_run_repo),
                    Arc::clone(&deps.project_repo),
                    workspace,
                )
                .await?;
            workspace = recovered_workspace;
            match repair_outcome {
                StalePublishRepairRecoveryOutcome::RetryEligible => {}
                StalePublishRepairRecoveryOutcome::ActiveReplacement => {
                    return Ok(AgentWorkspacePrSupervisionRecoveryOutcome::Skipped(
                        "active_pr_autofix_replacement",
                    ));
                }
                StalePublishRepairRecoveryOutcome::ActiveRepairReconciled => {
                    return Ok(AgentWorkspacePrSupervisionRecoveryOutcome::Skipped(
                        "active_agent_run",
                    ));
                }
                StalePublishRepairRecoveryOutcome::HandoffPreserved => {}
                StalePublishRepairRecoveryOutcome::Manual => {
                    return Ok(AgentWorkspacePrSupervisionRecoveryOutcome::Skipped(
                        "stale_repair_manual",
                    ));
                }
                StalePublishRepairRecoveryOutcome::TerminalRecovered => {
                    return Ok(AgentWorkspacePrSupervisionRecoveryOutcome::Skipped(
                        "stale_repair_recovered",
                    ));
                }
                StalePublishRepairRecoveryOutcome::Noop => {}
            }
        }
        #[cfg(not(any(test, feature = "test-utils")))]
        return Ok(AgentWorkspacePrSupervisionRecoveryOutcome::Skipped(
            "durable_repair_authority_unavailable",
        ));
    }

    // The durable repair coordinator is the first recovery authority. Its work may be a base
    // update with no PR and may intentionally run while legacy Auto Publish/PR-autofix gates are
    // disabled, so those compatibility gates apply only after it has no current generation.
    if let Some(reason) = pr_supervision_recovery_schedule_skip_reason(&workspace) {
        return Ok(AgentWorkspacePrSupervisionRecoveryOutcome::Skipped(reason));
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
    if !project.github_pr_enabled && workspace.publication_pr_number.is_none() {
        return Ok(AgentWorkspacePrSupervisionRecoveryOutcome::Skipped(
            "github_pr_disabled",
        ));
    }

    if workspace.publication_push_status.as_deref() == Some("refreshed")
        && workspace.pr_supervision_status.as_deref() == Some("reviewing")
    {
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
        if let Some(outcome) = resume_passed_pr_fix_review_handoff_if_ready(
            &deps,
            &conversation_id,
            &workspace,
            &project,
        )
        .await?
        {
            return Ok(outcome);
        }
        return Ok(AgentWorkspacePrSupervisionRecoveryOutcome::Skipped(
            "workspace_review_pending",
        ));
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
        if let Ok(target) =
            resolve_pr_supervision_recovery_target(&deps, &project, &workspace, trigger).await?
        {
            let sync_state = match deps
                .github
                .check_pr_sync_state(&target.worktree_path, target.pr_number)
                .await
            {
                Ok(sync_state) => sync_state,
                Err(error) => {
                    tracing::warn!(
                        conversation_id = conversation_id.as_str(),
                        pr_number = target.pr_number,
                        error = %error,
                        "Agent workspace PR supervision recovery could not inspect active-run PR state"
                    );
                    return Ok(AgentWorkspacePrSupervisionRecoveryOutcome::Skipped(
                        "active_agent_run",
                    ));
                }
            };
            if is_terminal_pr_sync_status(&sync_state.status) {
                let pr_status = publication_status_for_sync_state(&sync_state);
                update_terminal_pr_recovery_state(
                    &deps,
                    &conversation_id,
                    &workspace,
                    &target,
                    pr_status,
                )
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
                let terminalized = terminalize_agent_workspace_after_pr(
                    Arc::clone(&deps.workspace_repo),
                    deps.durable_recovery_state
                        .as_ref()
                        .map(|state| Arc::clone(&state.agent_workspace_repair_repo))
                        .ok_or_else(|| {
                            AppError::Infrastructure(
                                "terminal workspace cleanup requires durable repair authority"
                                    .to_string(),
                            )
                        })?,
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
                return Ok(AgentWorkspacePrSupervisionRecoveryOutcome::Terminal {
                    pr_number: target.pr_number,
                    pr_status: pr_status.to_string(),
                });
            }
        }

        return Ok(AgentWorkspacePrSupervisionRecoveryOutcome::Skipped(
            "active_agent_run",
        ));
    }

    if let Some(outcome) =
        resume_passed_pr_fix_review_handoff_if_ready(&deps, &conversation_id, &workspace, &project)
            .await?
    {
        return Ok(outcome);
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
        let terminalized = terminalize_agent_workspace_after_pr(
            Arc::clone(&deps.workspace_repo),
            deps.durable_recovery_state
                .as_ref()
                .map(|state| Arc::clone(&state.agent_workspace_repair_repo))
                .ok_or_else(|| {
                    AppError::Infrastructure(
                        "terminal workspace cleanup requires durable repair authority".to_string(),
                    )
                })?,
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

    let health = deps
        .github
        .fetch_pr_health(&target.worktree_path, target.pr_number)
        .await?;
    if let Some(issue) = classify_agent_workspace_pr_autofix_issue(target.pr_number, &health) {
        let events = deps
            .workspace_repo
            .list_publication_events(&conversation_id)
            .await?;
        let legacy_event_exists = events
            .iter()
            .any(|event| event.classification.as_deref() == Some(issue.classification.as_str()));
        let decision = load_pr_autofix_attempt_decision(
            deps.agent_run_repo.as_ref(),
            &conversation_id,
            target.pr_number,
            &issue.classification,
            legacy_event_exists,
        )
        .await?;
        let summary = decision.manual_summary().unwrap_or(
            "The same PR issue remains unresolved; RalphX is keeping supervision blocked while polling for an authorized autofix attempt.",
        );
        deps.workspace_repo
            .update_pr_auto_merge_state(
                &conversation_id,
                workspace.pr_auto_merge_current,
                Some("blocked"),
                Some(summary),
            )
            .await?;
        start_recovered_pr_polling(&deps, &conversation_id, &project, &target);
        return Ok(AgentWorkspacePrSupervisionRecoveryOutcome::Skipped(
            "pr_issue_unresolved",
        ));
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

async fn resume_passed_pr_fix_review_handoff_if_ready(
    deps: &AgentWorkspacePrSupervisionRecoveryDeps,
    conversation_id: &ChatConversationId,
    workspace: &AgentConversationWorkspace,
    project: &Project,
) -> AppResult<Option<AgentWorkspacePrSupervisionRecoveryOutcome>> {
    let Some(resumer) = deps.pr_fix_review_publish_resumer.as_ref().map(Arc::clone) else {
        return Ok(None);
    };
    let Some(monitor) = deps
        .workspace_repo
        .get_workspace_review_monitor(conversation_id)
        .await?
    else {
        return Ok(None);
    };
    let current_target = resolve_review_target(workspace, project).await?;
    let outcome = resume_pr_fix_publish_after_passed_workspace_review(
        Arc::clone(&deps.workspace_repo),
        conversation_id,
        workspace,
        &monitor,
        current_target.as_ref(),
        move |conversation_id| {
            let resumer = Arc::clone(&resumer);
            async move {
                resumer
                    .publish_pr_fix_after_workspace_review(conversation_id)
                    .await
            }
        },
    )
    .await?;
    match outcome {
        PrFixReviewPublishResumeOutcome::Skipped => Ok(None),
        PrFixReviewPublishResumeOutcome::Published => {
            let Some(pr_number) = workspace.publication_pr_number else {
                return Ok(None);
            };
            emit_workspace_changed(deps.app_handle.as_ref(), conversation_id);
            Ok(Some(
                AgentWorkspacePrSupervisionRecoveryOutcome::ReviewPublished { pr_number },
            ))
        }
        PrFixReviewPublishResumeOutcome::Failed { error } => {
            tracing::warn!(
                conversation_id = conversation_id.as_str(),
                error = %error,
                "Agent workspace PR supervision recovery failed to resume passed Workspace Review publish handoff"
            );
            emit_workspace_changed(deps.app_handle.as_ref(), conversation_id);
            Ok(Some(AgentWorkspacePrSupervisionRecoveryOutcome::Skipped(
                "pr_fix_review_publish_failed",
            )))
        }
    }
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
        clear_terminal_plan_pr_auto_merge_marker(deps, plan_branch, pr_status).await;
        deps.workspace_repo
            .update_pr_auto_merge_state(
                conversation_id,
                workspace.pr_auto_merge_current,
                None,
                Some(terminal_pr_recovery_summary(pr_status)),
            )
            .await?;
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

async fn clear_terminal_plan_pr_auto_merge_marker(
    deps: &AgentWorkspacePrSupervisionRecoveryDeps,
    plan_branch: &PlanBranch,
    pr_status: &str,
) {
    let (Some(transition_service), Some(task_id)) = (
        deps.transition_service.as_ref(),
        plan_branch.merge_task_id.as_ref(),
    ) else {
        return;
    };

    if let Err(error) = transition_service
        .clear_github_auto_merge_correction_marker_for_terminal_pr(task_id, pr_status)
        .await
    {
        tracing::warn!(
            task_id = task_id.as_str(),
            pr_status,
            error = %error,
            "Agent workspace PR supervision recovery failed to clear terminal auto-merge correction marker"
        );
    }
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
    if let Some(state) = deps.durable_recovery_state.as_ref() {
        registry.start_agent_workspace_polling_with_repair_repo(
            conversation_id.clone(),
            target.pr_number,
            project.clone(),
            target.worktree_path.clone(),
            Arc::clone(&deps.workspace_repo),
            Arc::clone(&deps.agent_run_repo),
            Arc::clone(&state.agent_workspace_repair_repo),
            Arc::clone(chat_service),
        );
    } else {
        #[cfg(test)]
        registry.start_agent_workspace_polling(
            conversation_id.clone(),
            target.pr_number,
            project.clone(),
            target.worktree_path.clone(),
            Arc::clone(&deps.workspace_repo),
            Arc::clone(&deps.agent_run_repo),
            Arc::clone(chat_service),
        );
        #[cfg(not(test))]
        tracing::error!(
            conversation_id = conversation_id.as_str(),
            pr_number = target.pr_number,
            "Agent workspace PR supervision recovery: refusing legacy poller construction without durable repair authority"
        );
    }
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
    let pending_review_handoff = workspace.publication_push_status.as_deref() == Some("refreshed")
        && workspace.pr_supervision_status.as_deref() == Some("reviewing");
    let stranded_pr_fix = workspace.publication_push_status.as_deref() == Some("refreshed")
        && workspace.pr_supervision_status.as_deref() == Some("fixing");
    let stale_candidate = workspace.publication_push_status.as_deref() == Some("needs_agent");
    if blocked_failed || pending_review_handoff || stranded_pr_fix || stale_candidate {
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

    match in_flight.entry(key) {
        Entry::Occupied(_) => false,
        Entry::Vacant(entry) => {
            entry.insert(());
            true
        }
    }
}

fn recovery_cache_ttl() -> Duration {
    Duration::from_millis(git_runtime_config().agent_workspace_pr_reconciliation_cache_ttl_ms)
}
