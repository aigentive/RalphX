use std::path::Path;
use std::sync::{Arc, OnceLock};

use chrono::{DateTime, Duration, Utc};
use dashmap::DashMap;

use crate::application::agent_conversation_workspace::resolve_effective_agent_conversation_workspace_path;
use crate::application::agent_workspace_publish_repair_state::validate_agent_workspace_repair_target_lease;
use crate::domain::entities::plan_branch::PrPushStatus;
use crate::domain::entities::{
    AgentConversationWorkspace, AgentWorkspaceRepairAttempt, AgentWorkspaceRepairContinuation,
    AgentWorkspaceRepairEffect, AgentWorkspaceRepairEffectKind, AgentWorkspaceRepairEffectStatus,
    AgentWorkspaceRepairOutcome, AgentWorkspaceRepairPhase, ChatConversation, ChatConversationId,
    GitMutationKind, GitTargetIdentity, GitTargetLeaseOwner, IdeationAnalysisBaseRefKind,
};
use crate::domain::repositories::{
    AgentWorkspaceRepairAttemptTransition, AgentWorkspaceRepairAttemptTransitionOutcome,
    AgentWorkspaceRepairRepository, BranchUpdateRepository, CompleteAgentWorkspaceRepairEffect,
    CompleteAgentWorkspaceRepairEffectOutcome, CompleteGitMutation,
    CreateAgentWorkspaceRepairEffect, CreateAgentWorkspaceRepairEffectOutcome,
    GitAuthorityCasOutcome, SettleAgentWorkspaceRepairAttempt,
    SettleAgentWorkspaceRepairAttemptOutcome,
};
use crate::domain::services::GithubServiceTrait;
use crate::domain::state_machine::transition_handler::{
    classify_commit_hook_failure_text, update_plan_from_main_isolated, update_source_from_target,
    CommitHookFailureKind, PlanUpdateResult, SourceUpdateResult,
};
use crate::error::{AppError, AppResult};
use crate::{application::AppState, application::GitService, domain::entities::Project};
use tauri::Manager;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PublishFailureClass {
    AgentFixable,
    Operational,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PublishBranchFreshnessOutcome {
    AlreadyFresh {
        base_commit: String,
        target_ref: String,
    },
    Updated {
        base_commit: String,
        target_ref: String,
    },
    NeedsAgent {
        message: String,
        conflict_files: Vec<String>,
        base_commit: String,
        target_ref: String,
    },
    OperationalError {
        message: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PublishBranchFreshnessStatus {
    pub target_ref: String,
    pub captured_base_commit: Option<String>,
    pub target_base_commit: String,
    pub is_base_ahead: bool,
}

/// The exact local/remote postconditions established by a repair-owned push. The normal
/// publisher may reuse the push, but it must prove these durable OIDs again before it can mutate
/// local state or hand the branch to PR creation, update, or monitoring.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct AgentWorkspaceRepairPrHandoff {
    pub target_base_ref: String,
    pub target_base_commit: String,
    pub expected_head_oid: String,
}

/// Command composition owns the normal publisher, while repair orchestration owns the durable
/// attempt, effect receipt, and lease. This callback keeps that composition dependency at the
/// edge: application recovery and HTTP completion can continue the same attempt without
/// depending outward on Tauri commands.
#[async_trait::async_trait]
pub(crate) trait AgentWorkspaceRepairPublishContinuation: Send + Sync {
    async fn publish_after_repair_push(
        &self,
        state: &AppState,
        conversation_id: ChatConversationId,
        repair_handoff: AgentWorkspaceRepairPrHandoff,
    ) -> Result<AgentWorkspaceRepairPrHandoffResult, PublishAfterRepairPushError>;
}

/// The normal publisher may already own the process-local publish guard. That is a retryable
/// collision for the repair coordinator, not a durable failure of its attempt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum PublishAfterRepairPushError {
    Busy,
    Failed(String),
}

fn agent_workspace_repair_publish_continuation_locks(
) -> &'static DashMap<String, Arc<tokio::sync::Mutex<()>>> {
    static LOCKS: OnceLock<DashMap<String, Arc<tokio::sync::Mutex<()>>>> = OnceLock::new();
    LOCKS.get_or_init(DashMap::new)
}

pub(crate) fn try_acquire_agent_workspace_repair_publish_continuation_guard(
    conversation_id: &ChatConversationId,
) -> Option<tokio::sync::OwnedMutexGuard<()>> {
    let lock = agent_workspace_repair_publish_continuation_locks()
        .entry(conversation_id.as_str())
        .or_insert_with(|| Arc::new(tokio::sync::Mutex::new(())))
        .clone();
    lock.try_lock_owned().ok()
}

/// The normal publisher's durable PR handoff receipt. Keep it application-local so the repair
/// coordinator records only the postcondition it needs, rather than depending on a command DTO.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct AgentWorkspaceRepairPrHandoffResult {
    pub pr_number: i64,
    pub pr_url: Option<String>,
}

pub fn classify_publish_failure(error: &str) -> PublishFailureClass {
    let normalized = error.to_lowercase();

    if is_operational_failure(&normalized) {
        return PublishFailureClass::Operational;
    }

    if is_commit_hook_failure_context(&normalized) {
        match classify_commit_hook_failure_text(error) {
            CommitHookFailureKind::PolicyFailure => return PublishFailureClass::AgentFixable,
            CommitHookFailureKind::EnvironmentFailure => return PublishFailureClass::Operational,
            CommitHookFailureKind::Unknown => {}
        }
    }

    if is_agent_fixable_failure(&normalized) {
        return PublishFailureClass::AgentFixable;
    }

    PublishFailureClass::Operational
}

pub fn publish_push_status_for_failure(error: &str) -> &'static str {
    match classify_publish_failure(error) {
        PublishFailureClass::AgentFixable => "needs_agent",
        PublishFailureClass::Operational => "failed",
    }
}

pub fn review_base_for_publish<'a>(
    captured_base_commit: Option<&'a str>,
    base_ref: &str,
) -> Result<&'a str, String> {
    captured_base_commit
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| {
            format!(
                "Agent conversation workspace is missing its captured base commit for base ref '{}'",
                base_ref
            )
        })
}

pub async fn count_publish_reviewable_commits(
    repo_path: &Path,
    source_branch: &str,
    review_base: &str,
) -> AppResult<u32> {
    GitService::count_commits_not_on_branch(repo_path, source_branch, review_base).await
}

pub async fn count_existing_publish_branch_reviewable_commits(
    repo_path: &Path,
    source_branch: &str,
    review_base: &str,
) -> AppResult<u32> {
    if !GitService::branch_exists(repo_path, source_branch)
        .await
        .unwrap_or(false)
    {
        return Ok(0);
    }

    count_publish_reviewable_commits(repo_path, source_branch, review_base).await
}

pub async fn count_unpublished_publish_commits(
    repo_path: &Path,
    source_branch: &str,
) -> AppResult<Option<u32>> {
    let remote_ref = remote_tracking_ref_for_publish(source_branch);
    if !GitService::ref_exists(repo_path, &remote_ref).await? {
        return Ok(None);
    }

    count_publish_reviewable_commits(repo_path, source_branch, &remote_ref)
        .await
        .map(Some)
}

pub async fn count_publishable_commits_with_base_fallback(
    repo_path: &Path,
    source_branch: &str,
    fallback_review_base: &str,
) -> AppResult<u32> {
    if let Some(unpublished_count) =
        count_unpublished_publish_commits(repo_path, source_branch).await?
    {
        return Ok(unpublished_count);
    }

    count_existing_publish_branch_reviewable_commits(repo_path, source_branch, fallback_review_base)
        .await
}

pub async fn push_publish_branch(
    github: &Arc<dyn GithubServiceTrait>,
    repo_path: &Path,
    branch: &str,
) -> AppResult<()> {
    github.push_branch(repo_path, branch).await
}

/// Continues only an already-persisted repair publication. The durable attempt snapshot remains
/// the single writer: the downstream push re-reads it before acquiring Git authority, creating an
/// effect receipt, or mutating the remote ref.
pub(crate) async fn continue_agent_workspace_repair_publish(
    state: &AppState,
    attempt: AgentWorkspaceRepairAttempt,
) -> AppResult<Option<AgentWorkspaceRepairPushOutcome>> {
    if !matches!(
        attempt.phase,
        AgentWorkspaceRepairPhase::ContinuationPending | AgentWorkspaceRepairPhase::Continuing
    ) || !matches!(
        attempt.continuation,
        AgentWorkspaceRepairContinuation::Publish
            | AgentWorkspaceRepairContinuation::ResumePrSupervision
    ) {
        return Ok(None);
    }
    let Some(_continuation_guard) =
        try_acquire_agent_workspace_repair_publish_continuation_guard(&attempt.conversation_id)
    else {
        return Ok(Some(AgentWorkspaceRepairPushOutcome::Busy));
    };
    let workspace = state
        .agent_conversation_workspace_repo
        .get_by_conversation_id(&attempt.conversation_id)
        .await?
        .ok_or_else(|| {
            AppError::NotFound(format!(
                "workspace {} for repair publication",
                attempt.conversation_id
            ))
        })?;
    if workspace.conversation_id != attempt.conversation_id {
        return Err(AppError::Conflict(
            "workspace repair publication does not match its persisted conversation".to_string(),
        ));
    }
    // A crash after the normal publisher has started supervision but before this continuation
    // records its own receipt must never push or create/update the PR again. The durable receipt
    // is the sole proof that the downstream owner is live; it is also safe to release the exact
    // repair lease before settling because a restart will take this branch again without Git.
    if has_observed_agent_workspace_repair_pr_handoff(
        state.agent_workspace_repair_repo.as_ref(),
        &attempt,
    )
    .await?
    {
        release_agent_workspace_repair_lease_after_pr_handoff(state, &attempt).await?;
        settle_agent_workspace_repair_after_pr_handoff(state, attempt).await?;
        return Ok(Some(AgentWorkspaceRepairPushOutcome::PrHandoffObserved));
    }

    let project = state
        .project_repo
        .get_by_id(&workspace.project_id)
        .await?
        .ok_or_else(|| {
            AppError::NotFound(format!(
                "project {} for workspace repair publication",
                workspace.project_id
            ))
        })?;
    let linked_plan_pr_number =
        if let Some(plan_branch_id) = workspace.linked_plan_branch_id.as_ref() {
            let plan_branch = state
                .plan_branch_repo
                .get_by_id(plan_branch_id)
                .await?
                .ok_or_else(|| {
                    AppError::NotFound(format!(
                        "linked plan branch {} for workspace repair publication",
                        plan_branch_id
                    ))
                })?;
            Some(plan_branch.pr_number.ok_or_else(|| {
                AppError::Conflict(
                    "linked plan repair publication cannot continue without its pull request"
                        .to_string(),
                )
            })?)
        } else {
            None
        };
    let Some(github) = state.github_service.as_ref() else {
        let error = "GitHub integration is unavailable for workspace repair publication";
        block_agent_workspace_repair_pr_handoff(state, attempt, error).await?;
        return Err(AppError::Conflict(error.to_string()));
    };
    let target = resolve_effective_agent_conversation_workspace_path(
        &project,
        &workspace,
        state.plan_branch_repo.as_ref(),
    )
    .await?;
    let expected_phase = attempt.phase;
    let push_outcome = push_agent_workspace_repair_branch(
        github,
        Arc::clone(&state.agent_workspace_repair_repo),
        Arc::clone(&state.branch_update_repo),
        AgentWorkspaceRepairPushRequest {
            target_worktree_path: &target.path,
            target_branch_name: &target.branch_name,
            attempt: attempt.clone(),
            expected_phase,
        },
    )
    .await?;
    if !matches!(
        push_outcome,
        AgentWorkspaceRepairPushOutcome::Observed { .. }
    ) {
        return Ok(Some(push_outcome));
    }

    let current = state
        .agent_workspace_repair_repo
        .get_repair_attempt(&attempt.id)
        .await?
        .ok_or_else(|| {
            AppError::NotFound(format!(
                "repair attempt {} disappeared after its push receipt",
                attempt.id
            ))
        })?;
    if current.generation != attempt.generation
        || current.phase != AgentWorkspaceRepairPhase::Continuing
        || current.settled_at.is_some()
    {
        return Ok(Some(AgentWorkspaceRepairPushOutcome::Stale));
    }

    let handoff = match repair_pr_handoff_from_observed_push(&current, &push_outcome) {
        Ok(handoff) => handoff,
        Err(error) => {
            block_agent_workspace_repair_pr_handoff(state, current, &error).await?;
            return Err(AppError::Conflict(error));
        }
    };
    match verify_agent_workspace_repair_pr_handoff(
        &target.path,
        &target.branch_name,
        &workspace.base_ref,
        &handoff,
    )
    .await
    {
        Ok(RepairPrHandoffVerification::Ok(_)) => {}
        Ok(RepairPrHandoffVerification::Retargetable { reason }) => {
            retarget_agent_workspace_repair_pr_handoff(state, &target.path, current, &reason)
                .await?;
            return Err(AppError::Conflict(reason));
        }
        Ok(RepairPrHandoffVerification::Fatal(error)) => {
            block_agent_workspace_repair_pr_handoff(state, current, &error).await?;
            return Err(AppError::Conflict(error));
        }
        Err(error) => {
            let error = error.to_string();
            block_agent_workspace_repair_pr_handoff(state, current, &error).await?;
            return Err(AppError::Conflict(error));
        }
    }

    let mut pr_effect = prepare_agent_workspace_repair_pr_handoff_effect(
        state.agent_workspace_repair_repo.as_ref(),
        &current,
        &workspace,
        linked_plan_pr_number,
    )
    .await?;
    if pr_effect.status != AgentWorkspaceRepairEffectStatus::Observed {
        if let Some((pr_number, pr_url)) =
            reconcile_agent_workspace_repair_pr_handoff(state, &workspace, &pr_effect).await?
        {
            pr_effect = observe_agent_workspace_repair_pr_handoff_effect(
                state.agent_workspace_repair_repo.as_ref(),
                &current,
                pr_effect,
                pr_number,
                pr_url.as_deref(),
            )
            .await?;
        }
    }
    if pr_effect.status != AgentWorkspaceRepairEffectStatus::Observed {
        let continuation = match state.agent_workspace_repair_publish_continuation() {
            Ok(continuation) => continuation,
            Err(error) => {
                let error = error.to_string();
                block_agent_workspace_repair_pr_handoff(state, current, &error).await?;
                return Err(AppError::Conflict(error));
            }
        };
        match continuation
            .publish_after_repair_push(state, attempt.conversation_id.clone(), handoff)
            .await
        {
            Ok(result) => {
                observe_agent_workspace_repair_pr_handoff_effect(
                    state.agent_workspace_repair_repo.as_ref(),
                    &current,
                    pr_effect,
                    result.pr_number,
                    result.pr_url.as_deref(),
                )
                .await?;
            }
            Err(PublishAfterRepairPushError::Busy) => {
                return Ok(Some(AgentWorkspaceRepairPushOutcome::Busy));
            }
            Err(PublishAfterRepairPushError::Failed(error)) => {
                block_agent_workspace_repair_pr_handoff(state, current, &error).await?;
                return Err(AppError::Conflict(error));
            }
        }
    }

    // Release first, while the durable receipt means a crash can re-enter the early handoff
    // branch above without reacquiring Git authority. This prevents a settled attempt from
    // orphaning the repair-owned exact lease.
    release_agent_workspace_repair_lease_after_pr_handoff(state, &current).await?;
    settle_agent_workspace_repair_after_pr_handoff(state, current).await?;
    Ok(Some(push_outcome))
}

pub(crate) async fn reconcile_agent_workspace_repair_pr_handoff(
    state: &AppState,
    workspace: &AgentConversationWorkspace,
    effect: &AgentWorkspaceRepairEffect,
) -> AppResult<Option<(i64, Option<String>)>> {
    if workspace.linked_plan_branch_id.is_some() {
        return reconcile_linked_plan_agent_workspace_repair_pr_handoff(state, workspace, effect)
            .await;
    }
    if workspace.mode != crate::domain::entities::AgentConversationWorkspaceMode::Edit {
        return Ok(None);
    }
    let current_workspace = state
        .agent_conversation_workspace_repo
        .get_by_conversation_id(&workspace.conversation_id)
        .await?
        .ok_or_else(|| {
            AppError::NotFound(format!(
                "workspace {} for repair handoff reconciliation",
                workspace.conversation_id
            ))
        })?;
    let Some(pr_number) = current_workspace.publication_pr_number else {
        return Ok(None);
    };
    if current_workspace.publication_push_status.as_deref() != Some("pushed")
        || effect
            .expected_pr_number
            .is_some_and(|expected| expected != pr_number)
    {
        return Ok(None);
    }
    Ok(Some((pr_number, current_workspace.publication_pr_url)))
}

pub(crate) fn repair_pr_handoff_from_observed_push(
    attempt: &AgentWorkspaceRepairAttempt,
    push_outcome: &AgentWorkspaceRepairPushOutcome,
) -> Result<AgentWorkspaceRepairPrHandoff, String> {
    let AgentWorkspaceRepairPushOutcome::Observed {
        effect, remote_oid, ..
    } = push_outcome
    else {
        return Err("workspace repair push did not produce an observed remote receipt".to_string());
    };
    let target_base_commit = attempt.target_base_commit.as_deref().ok_or_else(|| {
        "workspace repair push handoff is missing its exact target base commit".to_string()
    })?;
    if effect.intended_head_oid.as_deref() != Some(remote_oid.as_str())
        || attempt.repair_head_commit.as_deref() != Some(remote_oid.as_str())
    {
        return Err(
            "workspace repair push handoff does not match its exact durable head receipt"
                .to_string(),
        );
    }

    Ok(AgentWorkspaceRepairPrHandoff {
        target_base_ref: attempt.target_base_ref.clone(),
        target_base_commit: target_base_commit.to_string(),
        expected_head_oid: remote_oid.clone(),
    })
}

pub(crate) async fn has_observed_agent_workspace_repair_pr_handoff(
    repair_repo: &dyn AgentWorkspaceRepairRepository,
    attempt: &AgentWorkspaceRepairAttempt,
) -> AppResult<bool> {
    let mut in_flight = false;
    for kind in [
        AgentWorkspaceRepairEffectKind::CreatePr,
        AgentWorkspaceRepairEffectKind::UpdatePr,
    ] {
        let idempotency_key = format!(
            "agent_workspace_repair:{}:{}:{}",
            attempt.id, attempt.generation, kind
        );
        let Some(effect) = repair_repo
            .get_repair_effect_by_idempotency_key(&idempotency_key)
            .await?
        else {
            continue;
        };
        if effect.attempt_id != attempt.id || effect.kind != kind {
            return Err(AppError::Conflict(
                "repair PR handoff receipt does not match the current attempt".to_string(),
            ));
        }
        if effect.status == AgentWorkspaceRepairEffectStatus::Observed {
            return Ok(true);
        }
        in_flight = true;
    }
    if in_flight {
        return Ok(false);
    }
    Ok(false)
}

pub(crate) async fn prepare_agent_workspace_repair_pr_handoff_effect(
    repair_repo: &dyn AgentWorkspaceRepairRepository,
    attempt: &AgentWorkspaceRepairAttempt,
    workspace: &AgentConversationWorkspace,
    existing_pr_number: Option<i64>,
) -> AppResult<AgentWorkspaceRepairEffect> {
    for existing_kind in [
        AgentWorkspaceRepairEffectKind::CreatePr,
        AgentWorkspaceRepairEffectKind::UpdatePr,
    ] {
        let existing_key = format!(
            "agent_workspace_repair:{}:{}:{}",
            attempt.id, attempt.generation, existing_kind
        );
        if let Some(effect) = repair_repo
            .get_repair_effect_by_idempotency_key(&existing_key)
            .await?
        {
            if effect.attempt_id != attempt.id || effect.kind != existing_kind {
                return Err(AppError::Conflict(
                    "repair PR handoff receipt does not match the current attempt".to_string(),
                ));
            }
            return Ok(effect);
        }
    }

    let expected_pr_number = existing_pr_number.or(workspace.publication_pr_number);
    let kind = if expected_pr_number.is_some() {
        AgentWorkspaceRepairEffectKind::UpdatePr
    } else {
        AgentWorkspaceRepairEffectKind::CreatePr
    };
    let idempotency_key = format!(
        "agent_workspace_repair:{}:{}:{}",
        attempt.id, attempt.generation, kind
    );
    if let Some(effect) = repair_repo
        .get_repair_effect_by_idempotency_key(&idempotency_key)
        .await?
    {
        if effect.attempt_id != attempt.id || effect.kind != kind {
            return Err(AppError::Conflict(
                "repair PR handoff receipt does not match the current attempt".to_string(),
            ));
        }
        return Ok(effect);
    }

    let mut effect =
        AgentWorkspaceRepairEffect::new(attempt.id.clone(), kind, idempotency_key, Utc::now());
    effect.status = AgentWorkspaceRepairEffectStatus::InFlight;
    effect.intended_head_oid = attempt.repair_head_commit.clone();
    effect.expected_pr_number = expected_pr_number;
    match repair_repo
        .create_repair_effect(CreateAgentWorkspaceRepairEffect {
            attempt_id: attempt.id.clone(),
            generation: attempt.generation,
            expected_phase: AgentWorkspaceRepairPhase::Continuing,
            expected_attempt_updated_at: attempt.updated_at,
            effect,
            compatibility_projection: None,
            events: Vec::new(),
        })
        .await?
    {
        CreateAgentWorkspaceRepairEffectOutcome::Created(effect)
        | CreateAgentWorkspaceRepairEffectOutcome::OpenEffectExists(effect) => Ok(effect),
        CreateAgentWorkspaceRepairEffectOutcome::Stale(_)
        | CreateAgentWorkspaceRepairEffectOutcome::Missing => Err(AppError::Conflict(
            "repair attempt lost authority before the PR handoff checkpoint".to_string(),
        )),
    }
}

/// The linked-plan publisher owns the plan PR projection and monitor startup. On replay, that
/// projection is the target-aware postcondition for an already-in-flight durable handoff, so the
/// repair coordinator can record its receipt without repeating the publisher's Git or PR work.
pub(crate) async fn reconcile_linked_plan_agent_workspace_repair_pr_handoff(
    state: &AppState,
    workspace: &AgentConversationWorkspace,
    effect: &AgentWorkspaceRepairEffect,
) -> AppResult<Option<(i64, Option<String>)>> {
    let Some(plan_branch_id) = workspace.linked_plan_branch_id.as_ref() else {
        return Ok(None);
    };
    let expected_pr_number = effect.expected_pr_number.ok_or_else(|| {
        AppError::Conflict(
            "linked plan repair handoff is missing its expected pull-request number".to_string(),
        )
    })?;
    let plan_branch = state
        .plan_branch_repo
        .get_by_id(plan_branch_id)
        .await?
        .ok_or_else(|| {
            AppError::NotFound(format!(
                "linked plan branch {} for repair handoff reconciliation",
                plan_branch_id
            ))
        })?;
    if plan_branch.pr_number != Some(expected_pr_number) {
        return Err(AppError::Conflict(
            "linked plan repair handoff no longer matches its pull-request target".to_string(),
        ));
    }
    if plan_branch.pr_push_status != PrPushStatus::Pushed {
        return Ok(None);
    }
    let current_workspace = state
        .agent_conversation_workspace_repo
        .get_by_conversation_id(&workspace.conversation_id)
        .await?
        .ok_or_else(|| {
            AppError::NotFound(format!(
                "workspace {} for linked-plan repair handoff reconciliation",
                workspace.conversation_id
            ))
        })?;
    if current_workspace.publication_pr_number != Some(expected_pr_number)
        || current_workspace.publication_push_status.as_deref() != Some("pushed")
    {
        return Ok(None);
    }
    Ok(Some((expected_pr_number, plan_branch.pr_url)))
}

pub(crate) async fn observe_agent_workspace_repair_pr_handoff_effect(
    repair_repo: &dyn AgentWorkspaceRepairRepository,
    attempt: &AgentWorkspaceRepairAttempt,
    mut effect: AgentWorkspaceRepairEffect,
    pr_number: i64,
    pr_url: Option<&str>,
) -> AppResult<AgentWorkspaceRepairEffect> {
    if effect.status == AgentWorkspaceRepairEffectStatus::Observed {
        return Ok(effect);
    }
    let expected_effect_updated_at = effect.updated_at;
    let expected_effect_status = effect.status;
    let completed_at = Utc::now();
    effect.status = AgentWorkspaceRepairEffectStatus::Observed;
    effect.expected_pr_number = Some(pr_number);
    effect.receipt_json = Some(
        serde_json::json!({
            "pr_number": pr_number,
            "pr_url": pr_url,
            "monitoring_handoff": true,
        })
        .to_string(),
    );
    effect.completed_at = Some(completed_at);
    effect.updated_at = completed_at;
    match repair_repo
        .complete_repair_effect(CompleteAgentWorkspaceRepairEffect {
            attempt_id: attempt.id.clone(),
            generation: attempt.generation,
            expected_phase: AgentWorkspaceRepairPhase::Continuing,
            expected_attempt_updated_at: attempt.updated_at,
            expected_effect_updated_at,
            expected_effect_status,
            effect: effect.clone(),
            compatibility_projection: None,
            events: Vec::new(),
        })
        .await?
    {
        CompleteAgentWorkspaceRepairEffectOutcome::Applied(effect) => Ok(*effect),
        CompleteAgentWorkspaceRepairEffectOutcome::Stale(_)
        | CompleteAgentWorkspaceRepairEffectOutcome::Missing => Err(AppError::Conflict(
            "repair attempt lost authority before recording the PR handoff receipt".to_string(),
        )),
    }
}

async fn block_agent_workspace_repair_pr_handoff(
    state: &AppState,
    attempt: AgentWorkspaceRepairAttempt,
    error: &str,
) -> AppResult<()> {
    let auto_merge_current = state
        .agent_conversation_workspace_repo
        .get_by_conversation_id(&attempt.conversation_id)
        .await?
        .and_then(|workspace| workspace.pr_auto_merge_current);
    let blocker = format!(
        "Pull-request continuation could not complete: {error}. Retry the blocked operation."
    );
    let _ = crate::application::agent_workspace_publish_repair_state::block_agent_workspace_repair_completion(
        Arc::clone(&state.agent_workspace_repair_repo),
        Arc::clone(&state.branch_update_repo),
        attempt,
        "Workspace repair publish continuation is blocked.",
        &blocker,
        auto_merge_current,
    )
    .await?;
    Ok(())
}

/// Durably block a drifted-but-exact pre-PR repair receipt so the budgeted blocked-repair
/// successor machinery (automatic retry or explicit user retry) can retarget it from the
/// current workspace base. This deliberately starts no successor itself: successor creation,
/// its retry budget, its open-effect guard, and its dispatch rescue all stay owned by
/// `retry_safe_blocked_agent_workspace_repair` and the user-directed blocked retry path.
pub(crate) async fn retarget_agent_workspace_repair_pr_handoff(
    state: &AppState,
    repo_path: &Path,
    attempt: AgentWorkspaceRepairAttempt,
    reason: &str,
) -> AppResult<()> {
    // Refresh the persisted base commit first (origin was already fetched during receipt
    // verification) so the superseding generation targets the current base instead of
    // recapturing the drifted one. Best-effort: a read failure keeps the persisted commit.
    let mut base_ref_for_blocker = None;
    match state
        .agent_conversation_workspace_repo
        .get_by_conversation_id(&attempt.conversation_id)
        .await
    {
        Ok(Some(workspace)) => {
            base_ref_for_blocker = Some(workspace.base_ref.clone());
            let target_ref = resolve_publish_freshness_target(repo_path, &workspace.base_ref).await;
            if let Ok(current_base_commit) = GitService::get_branch_sha(repo_path, &target_ref).await
            {
                if workspace.base_commit.as_deref() != Some(current_base_commit.as_str()) {
                    let mut refreshed = workspace;
                    refreshed.base_commit = Some(current_base_commit);
                    refreshed.updated_at = Utc::now();
                    if let Err(error) = state
                        .agent_conversation_workspace_repo
                        .create_or_update(refreshed)
                        .await
                    {
                        tracing::warn!(
                            conversation_id = attempt.conversation_id.as_str(),
                            error = %error,
                            "Could not persist refreshed base commit before retargetable repair block"
                        );
                    }
                }
            }
        }
        Ok(None) => {}
        Err(error) => {
            tracing::warn!(
                conversation_id = attempt.conversation_id.as_str(),
                error = %error,
                "Could not read workspace before retargetable repair block"
            );
        }
    }
    let blocker = match base_ref_for_blocker {
        Some(base_ref) => format!(
            "Base changed to '{base_ref}' — retry to retarget the repair ({reason})"
        ),
        None => reason.to_string(),
    };
    block_agent_workspace_repair_pr_handoff(state, attempt, &blocker).await
}

async fn settle_agent_workspace_repair_after_pr_handoff(
    state: &AppState,
    attempt: AgentWorkspaceRepairAttempt,
) -> AppResult<()> {
    match state
        .agent_workspace_repair_repo
        .settle_repair_attempt(SettleAgentWorkspaceRepairAttempt {
            attempt_id: attempt.id,
            generation: attempt.generation,
            expected_phase: AgentWorkspaceRepairPhase::Continuing,
            expected_updated_at: attempt.updated_at,
            outcome: AgentWorkspaceRepairOutcome::Succeeded,
            settled_at: Utc::now(),
            compatibility_projection: None,
            events: Vec::new(),
        })
        .await?
    {
        SettleAgentWorkspaceRepairAttemptOutcome::Applied(_) => Ok(()),
        SettleAgentWorkspaceRepairAttemptOutcome::Stale(_)
        | SettleAgentWorkspaceRepairAttemptOutcome::Missing => Err(AppError::Conflict(
            "repair attempt lost authority before its PR monitoring handoff settled".to_string(),
        )),
    }
}

/// A post-PR receipt proves the repair branch no longer owns a Git mutation. Release only the
/// exact canonical lease persisted by that attempt; mismatched/newer owners are untouched.
async fn release_agent_workspace_repair_lease_after_pr_handoff(
    state: &AppState,
    attempt: &AgentWorkspaceRepairAttempt,
) -> AppResult<()> {
    if state
        .agent_workspace_repair_repo
        .get_open_repair_effect(&attempt.id)
        .await?
        .is_some()
    {
        return Err(AppError::Conflict(
            "cannot release a repair lease while a durable repair effect is open".to_string(),
        ));
    }
    let (Some(common_dir), Some(target_ref), Some(epoch)) = (
        attempt.git_common_dir.as_deref(),
        attempt.target_ref.as_deref(),
        attempt.target_lease_epoch,
    ) else {
        return Ok(());
    };
    let identity = GitTargetIdentity::new(std::path::PathBuf::from(common_dir), target_ref)
        .map_err(|error| {
            AppError::Validation(format!("invalid durable repair lease identity: {error}"))
        })?;
    let owner = GitTargetLeaseOwner::agent_workspace_repair(attempt.id.as_str());
    let _ = state
        .branch_update_repo
        .release_target_lease(&identity, &owner, epoch)
        .await?;
    Ok(())
}

/// The only safe outcomes of an attempt-scoped branch publication.
///
/// `Observed` means origin was freshly read and matched the intended local head. It may be the
/// direct result of this call, or a durable receipt recovered after an ambiguous prior call.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum AgentWorkspaceRepairPushOutcome {
    Observed {
        effect: Box<AgentWorkspaceRepairEffect>,
        remote_oid: String,
        reconciled_after_push_error: bool,
    },
    /// The repair-owned push was already observed and the durable PR monitoring handoff was
    /// reconciled after a crash/replay, so no Git or GitHub mutation was retried.
    PrHandoffObserved,
    /// Another invocation of this exact attempt currently owns the deterministic Git mutation
    /// claim. The caller must leave the attempt unchanged and let that owner settle its receipt.
    Busy,
    Stale,
}

/// Trusted inputs for a repair-owned branch publication. The caller resolves this target from
/// persisted workspace/project/plan metadata, never from model-provided branch or remote strings.
pub(crate) struct AgentWorkspaceRepairPushRequest<'a> {
    pub target_worktree_path: &'a Path,
    pub target_branch_name: &'a str,
    pub attempt: AgentWorkspaceRepairAttempt,
    pub expected_phase: AgentWorkspaceRepairPhase,
}

/// Publish a repaired workspace branch with an exact lease only when its remote history was
/// rewritten. First and fast-forward pushes keep the existing normal push path.
///
/// The durable effect is written before the Git mutation. Every re-entry reads that effect and a
/// freshly fetched origin ref before it can issue another push, which prevents an observed receipt
/// from becoming a second overwrite.
pub(crate) async fn push_agent_workspace_repair_branch(
    github: &Arc<dyn GithubServiceTrait>,
    repair_repo: Arc<dyn AgentWorkspaceRepairRepository>,
    branch_update_repo: Arc<dyn BranchUpdateRepository>,
    request: AgentWorkspaceRepairPushRequest<'_>,
) -> AppResult<AgentWorkspaceRepairPushOutcome> {
    let current = repair_repo.get_repair_attempt(&request.attempt.id).await?;
    let Some(current) = current else {
        return Ok(AgentWorkspaceRepairPushOutcome::Stale);
    };
    if current.id != request.attempt.id
        || current.generation != request.attempt.generation
        || current.phase != request.expected_phase
        || current.updated_at != request.attempt.updated_at
    {
        return Ok(AgentWorkspaceRepairPushOutcome::Stale);
    }

    let owner = GitTargetLeaseOwner::agent_workspace_repair(current.id.as_str());
    let persisted_identity =
        validate_agent_workspace_repair_target_lease(branch_update_repo.as_ref(), &current).await?;
    let expected_ref = format!("refs/heads/{}", request.target_branch_name);
    if persisted_identity.full_ref() != expected_ref.as_str() {
        return Err(AppError::Conflict(
            "workspace repair push target differs from its dispatch-acquired canonical lease"
                .to_string(),
        ));
    }
    let fencing_epoch = current
        .target_lease_epoch
        .expect("validated repair lease has an epoch");

    let prepared_attempt = prepare_agent_workspace_repair_push_attempt(
        repair_repo.as_ref(),
        current,
        request.expected_phase,
    )
    .await;
    let attempt = match prepared_attempt {
        Ok(Some(attempt)) => attempt,
        Ok(None) => return Ok(AgentWorkspaceRepairPushOutcome::Stale),
        Err(error) => return Err(error),
    };

    let local_ref = persisted_identity.full_ref().to_string();
    let branch_name = local_ref.strip_prefix("refs/heads/").ok_or_else(|| {
        AppError::Validation("workspace repair target is not a local branch ref".to_string())
    })?;
    let idempotency_key = format!(
        "agent_workspace_repair:{}:{}:push_branch",
        attempt.id, attempt.generation
    );
    let effect = match repair_repo
        .get_repair_effect_by_idempotency_key(&idempotency_key)
        .await?
    {
        Some(effect) => {
            if effect.status == AgentWorkspaceRepairEffectStatus::Observed {
                return observed_workspace_repair_push_outcome(effect);
            }
            if effect.status != AgentWorkspaceRepairEffectStatus::InFlight {
                return Err(AppError::Conflict(
                    "workspace repair push effect is not available for continuation".to_string(),
                ));
            }
            effect
        }
        None => {
            let mut effect = AgentWorkspaceRepairEffect::new(
                attempt.id.clone(),
                AgentWorkspaceRepairEffectKind::PushBranch,
                idempotency_key,
                Utc::now(),
            );
            effect.status = AgentWorkspaceRepairEffectStatus::InFlight;
            match repair_repo
                .create_repair_effect(CreateAgentWorkspaceRepairEffect {
                    attempt_id: attempt.id.clone(),
                    generation: attempt.generation,
                    expected_phase: AgentWorkspaceRepairPhase::Continuing,
                    expected_attempt_updated_at: attempt.updated_at,
                    effect: effect.clone(),
                    compatibility_projection: None,
                    events: Vec::new(),
                })
                .await?
            {
                CreateAgentWorkspaceRepairEffectOutcome::Created(effect) => effect,
                CreateAgentWorkspaceRepairEffectOutcome::OpenEffectExists(effect) => effect,
                CreateAgentWorkspaceRepairEffectOutcome::Stale(_)
                | CreateAgentWorkspaceRepairEffectOutcome::Missing => {
                    return Ok(AgentWorkspaceRepairPushOutcome::Stale);
                }
            }
        }
    };
    if effect.status == AgentWorkspaceRepairEffectStatus::Observed {
        return observed_workspace_repair_push_outcome(effect);
    }
    if effect.status != AgentWorkspaceRepairEffectStatus::InFlight {
        return Err(AppError::Conflict(
            "workspace repair push effect is not available for continuation".to_string(),
        ));
    }

    let claim_id = format!("{}:push", effect.id);
    match branch_update_repo
        .begin_git_mutation(crate::domain::repositories::BeginGitMutation {
            identity: persisted_identity.clone(),
            owner: owner.clone(),
            fencing_epoch,
            claim_id: claim_id.clone(),
            kind: GitMutationKind::Push,
        })
        .await?
    {
        GitAuthorityCasOutcome::Applied { .. } => {}
        GitAuthorityCasOutcome::MutationInFlight => {
            return Ok(AgentWorkspaceRepairPushOutcome::Busy);
        }
        outcome => {
            return Err(AppError::Conflict(format!(
                "workspace repair push lost Git target authority before mutation: {outcome:?}"
            )));
        }
    }

    // Once the deterministic effect and mutation claim exist, every Git observation is fenced by
    // the exact durable attempt. An interrupted preflight has an incomplete effect that startup
    // recovery can safely release for retry; it is never a reason to block a concurrent owner.
    let workspace_path = request.target_worktree_path;
    let mutation_result = async {
        let observed_identity =
            GitService::canonical_target_identity(workspace_path, request.target_branch_name)
                .await?;
        if observed_identity != persisted_identity {
            return Err(AppError::Conflict(
                "workspace repair push workspace/ref differs from its persisted canonical target"
                    .to_string(),
            ));
        }
        let checked_out_branch = GitService::get_current_branch(workspace_path).await?;
        if checked_out_branch != request.target_branch_name {
            return Err(AppError::Validation(format!(
                "workspace repair target is checked out at '{}' instead of '{}'",
                checked_out_branch, request.target_branch_name
            )));
        }
        let intended_head_oid = GitService::get_head_sha(workspace_path).await?;
        let observed_remote_oid = read_origin_branch_oid(workspace_path, branch_name).await?;
        let effect = initialize_agent_workspace_repair_push_effect(
            repair_repo.as_ref(),
            &attempt,
            effect,
            &intended_head_oid,
            observed_remote_oid.as_deref(),
        )
        .await?;
        if observed_remote_oid.as_deref() == effect.intended_head_oid.as_deref() {
            let remote_oid = observed_remote_oid.expect("matching remote OID is present");
            let effect = observe_agent_workspace_repair_push_effect(
                repair_repo.as_ref(),
                &attempt,
                effect,
                &local_ref,
                &remote_oid,
            )
            .await?;
            return Ok(AgentWorkspaceRepairPushOutcome::Observed {
                effect: Box::new(effect),
                remote_oid,
                reconciled_after_push_error: false,
            });
        }
        verify_workspace_repair_push_remote_precondition(&effect, observed_remote_oid.as_deref())?;
        let uses_exact_lease = effect.expected_remote_oid.is_some()
            && GitService::count_commits_not_on_branch(
                workspace_path,
                effect
                    .expected_remote_oid
                    .as_deref()
                    .expect("exact lease requires expected remote OID"),
                &local_ref,
            )
            .await?
                > 0;
        let push_result = if uses_exact_lease {
            github
                .push_branch_with_expected_remote_oid_lease(
                    workspace_path,
                    &local_ref,
                    effect
                        .expected_remote_oid
                        .as_deref()
                        .expect("exact lease requires expected remote OID"),
                )
                .await
        } else {
            github.push_branch(workspace_path, branch_name).await
        };
        let observed_remote_oid = read_origin_branch_oid(workspace_path, branch_name).await?;
        if observed_remote_oid.as_deref() == effect.intended_head_oid.as_deref() {
            let remote_oid = observed_remote_oid.expect("matching remote OID is present");
            let effect = observe_agent_workspace_repair_push_effect(
                repair_repo.as_ref(),
                &attempt,
                effect,
                &local_ref,
                &remote_oid,
            )
            .await?;
            return Ok(AgentWorkspaceRepairPushOutcome::Observed {
                effect: Box::new(effect),
                remote_oid,
                reconciled_after_push_error: push_result.is_err(),
            });
        }
        push_result?;
        Err(AppError::Conflict(
            "workspace repair push finished without its expected remote postcondition".to_string(),
        ))
    }
    .await;
    let completion = branch_update_repo
        .complete_git_mutation(CompleteGitMutation {
            identity: persisted_identity,
            owner,
            fencing_epoch,
            claim_id,
        })
        .await?;
    if !matches!(completion, GitAuthorityCasOutcome::Applied { .. }) {
        return Err(AppError::Conflict(format!(
            "workspace repair push lost Git target authority after mutation: {completion:?}"
        )));
    }
    mutation_result
}

pub(crate) async fn prepare_agent_workspace_repair_push_attempt(
    repair_repo: &dyn AgentWorkspaceRepairRepository,
    mut attempt: AgentWorkspaceRepairAttempt,
    expected_phase: AgentWorkspaceRepairPhase,
) -> AppResult<Option<AgentWorkspaceRepairAttempt>> {
    if matches!(attempt.phase, AgentWorkspaceRepairPhase::Continuing) {
        return Ok(Some(attempt));
    }
    if attempt.phase != AgentWorkspaceRepairPhase::ContinuationPending
        || expected_phase != AgentWorkspaceRepairPhase::ContinuationPending
    {
        return Ok(None);
    }
    attempt.phase = AgentWorkspaceRepairPhase::Continuing;
    let expected_updated_at = attempt.updated_at;
    attempt.updated_at = next_effect_checkpoint_at(expected_updated_at);
    match repair_repo
        .transition_repair_attempt(AgentWorkspaceRepairAttemptTransition {
            attempt,
            expected_phase: AgentWorkspaceRepairPhase::ContinuationPending,
            expected_updated_at,
            next_phase: AgentWorkspaceRepairPhase::Continuing,
            compatibility_projection: None,
            events: Vec::new(),
        })
        .await?
    {
        AgentWorkspaceRepairAttemptTransitionOutcome::Applied(attempt) => Ok(Some(attempt)),
        AgentWorkspaceRepairAttemptTransitionOutcome::Stale(_)
        | AgentWorkspaceRepairAttemptTransitionOutcome::Missing => Ok(None),
    }
}

pub(crate) fn observed_workspace_repair_push_outcome(
    effect: AgentWorkspaceRepairEffect,
) -> AppResult<AgentWorkspaceRepairPushOutcome> {
    let remote_oid = effect
        .receipt_json
        .as_deref()
        .and_then(|receipt| serde_json::from_str::<serde_json::Value>(receipt).ok())
        .and_then(|receipt| {
            receipt
                .get("remote_oid")
                .and_then(serde_json::Value::as_str)
                .map(ToOwned::to_owned)
        })
        .filter(|remote_oid| !remote_oid.is_empty())
        .ok_or_else(|| {
            AppError::Conflict(
                "observed workspace repair push effect is missing its remote receipt".to_string(),
            )
        })?;
    if effect.intended_head_oid.as_deref() != Some(remote_oid.as_str()) {
        return Err(AppError::Conflict(
            "observed workspace repair push receipt does not match its intended head".to_string(),
        ));
    }
    Ok(AgentWorkspaceRepairPushOutcome::Observed {
        effect: Box::new(effect),
        remote_oid,
        reconciled_after_push_error: false,
    })
}

pub(crate) async fn initialize_agent_workspace_repair_push_effect(
    repair_repo: &dyn AgentWorkspaceRepairRepository,
    attempt: &AgentWorkspaceRepairAttempt,
    mut effect: AgentWorkspaceRepairEffect,
    intended_head_oid: &str,
    observed_remote_oid: Option<&str>,
) -> AppResult<AgentWorkspaceRepairEffect> {
    if effect.attempt_id != attempt.id
        || effect.kind != AgentWorkspaceRepairEffectKind::PushBranch
        || effect.status != AgentWorkspaceRepairEffectStatus::InFlight
    {
        return Err(AppError::Conflict(
            "workspace repair push receipt does not match the current attempt target".to_string(),
        ));
    }
    let initialized = effect.intended_head_oid.is_some()
        && (effect.expected_remote_absent || effect.expected_remote_oid.is_some());
    if initialized {
        if effect.intended_head_oid.as_deref() != Some(intended_head_oid) {
            return Err(AppError::Conflict(
                "workspace repair push receipt does not match the current attempt head".to_string(),
            ));
        }
        return Ok(effect);
    }
    if effect.intended_head_oid.is_some()
        || effect.expected_remote_oid.is_some()
        || effect.expected_remote_absent
    {
        return Err(AppError::Conflict(
            "workspace repair push preflight receipt is partially initialized".to_string(),
        ));
    }

    let expected_effect_updated_at = effect.updated_at;
    effect.intended_head_oid = Some(intended_head_oid.to_string());
    effect.expected_remote_oid = observed_remote_oid.map(ToOwned::to_owned);
    effect.expected_remote_absent = observed_remote_oid.is_none();
    effect.updated_at = next_effect_checkpoint_at(effect.updated_at);
    match repair_repo
        .complete_repair_effect(CompleteAgentWorkspaceRepairEffect {
            attempt_id: attempt.id.clone(),
            generation: attempt.generation,
            expected_phase: AgentWorkspaceRepairPhase::Continuing,
            expected_attempt_updated_at: attempt.updated_at,
            expected_effect_updated_at,
            expected_effect_status: AgentWorkspaceRepairEffectStatus::InFlight,
            effect,
            compatibility_projection: None,
            events: Vec::new(),
        })
        .await?
    {
        CompleteAgentWorkspaceRepairEffectOutcome::Applied(effect) => Ok(*effect),
        CompleteAgentWorkspaceRepairEffectOutcome::Stale(_)
        | CompleteAgentWorkspaceRepairEffectOutcome::Missing => Err(AppError::Conflict(
            "workspace repair push preflight receipt lost current attempt authority".to_string(),
        )),
    }
}

pub(crate) fn verify_workspace_repair_push_remote_precondition(
    effect: &AgentWorkspaceRepairEffect,
    remote_oid: Option<&str>,
) -> AppResult<()> {
    if effect.expected_remote_absent {
        if remote_oid.is_none() {
            return Ok(());
        }
    } else if remote_oid == effect.expected_remote_oid.as_deref() {
        return Ok(());
    }
    Err(AppError::Conflict(
        "workspace repair push remote state drifted from its durable expected OID".to_string(),
    ))
}

async fn read_origin_branch_oid(repo_path: &Path, branch_name: &str) -> AppResult<Option<String>> {
    GitService::fetch_origin(repo_path).await?;
    let remote_ref = remote_tracking_ref_for_publish(branch_name);
    if !GitService::ref_exists(repo_path, &remote_ref).await? {
        return Ok(None);
    }
    GitService::get_branch_sha(repo_path, &remote_ref)
        .await
        .map(Some)
}

pub(crate) async fn observe_agent_workspace_repair_push_effect(
    repair_repo: &dyn AgentWorkspaceRepairRepository,
    attempt: &AgentWorkspaceRepairAttempt,
    mut effect: AgentWorkspaceRepairEffect,
    remote_ref: &str,
    remote_oid: &str,
) -> AppResult<AgentWorkspaceRepairEffect> {
    let expected_effect_updated_at = effect.updated_at;
    let expected_effect_status = effect.status;
    effect.status = AgentWorkspaceRepairEffectStatus::Observed;
    effect.receipt_json = Some(
        serde_json::json!({
            "remote_ref": remote_ref,
            "remote_oid": remote_oid,
        })
        .to_string(),
    );
    effect.last_error = None;
    effect.updated_at = next_effect_checkpoint_at(effect.updated_at);
    effect.completed_at = Some(effect.updated_at);
    match repair_repo
        .complete_repair_effect(CompleteAgentWorkspaceRepairEffect {
            attempt_id: attempt.id.clone(),
            generation: attempt.generation,
            expected_phase: AgentWorkspaceRepairPhase::Continuing,
            expected_attempt_updated_at: attempt.updated_at,
            expected_effect_updated_at,
            expected_effect_status,
            effect,
            compatibility_projection: None,
            events: Vec::new(),
        })
        .await?
    {
        CompleteAgentWorkspaceRepairEffectOutcome::Applied(effect) => Ok(*effect),
        CompleteAgentWorkspaceRepairEffectOutcome::Stale(_)
        | CompleteAgentWorkspaceRepairEffectOutcome::Missing => Err(AppError::Conflict(
            "workspace repair push receipt lost current attempt authority".to_string(),
        )),
    }
}

pub(crate) fn next_effect_checkpoint_at(previous: DateTime<Utc>) -> DateTime<Utc> {
    let now = Utc::now();
    if now > previous {
        now
    } else {
        previous + Duration::microseconds(1)
    }
}

/// Lazily publish an automation run's local-only base branch to origin before the
/// run's PR references it as `--base` (integration-branch model, B1/B2/B5).
///
/// Two load-bearing belts gate the push:
/// - **Scope**: the workspace must belong to an automation run
///   (`conversation.automation_id.is_some()`). A non-automation workspace on a
///   local-only branch is never pushed as a "base branch".
/// - **Safety**: `origin/<base_ref>` must be absent. When it already exists the
///   push is skipped (idempotent). This also correctly skips `pr_head_stacked` /
///   source-PR successors whose base is an already-pushed head branch.
///
/// `base_ref_kind == LocalBranch` is only a cheap pre-filter, never the authority.
///
/// On push failure the error is returned unchanged so the caller fails the
/// publish closed — it MUST NOT fall back to `base=main`, which would reintroduce
/// the wrong-base bug.
pub async fn ensure_publish_base_pushed(
    github: &Arc<dyn GithubServiceTrait>,
    repo_path: &Path,
    conversation: &ChatConversation,
    workspace: &AgentConversationWorkspace,
) -> AppResult<()> {
    // Scope belt (authority): only automation-owned runs lazily publish their base.
    if conversation.automation_id.is_none() {
        return Ok(());
    }
    // Cheap pre-filter: project-default / current-branch bases already live on origin.
    if workspace.base_ref_kind != IdeationAnalysisBaseRefKind::LocalBranch {
        return Ok(());
    }
    let base_ref = workspace.base_ref.trim();
    if base_ref.is_empty() {
        return Ok(());
    }
    // Safety belt: skip when the base is already on origin (idempotent; also covers
    // pr_head_stacked / source-PR successors whose base is a pushed head branch).
    let remote_ref = remote_tracking_ref_for_publish(base_ref);
    if GitService::ref_exists(repo_path, &remote_ref).await? {
        return Ok(());
    }
    github.push_branch(repo_path, base_ref).await
}

pub async fn ensure_publish_branch_fresh(
    repo_path: &Path,
    project: &Project,
    source_branch: &str,
    base_ref: &str,
    conversation_id: &str,
    app_handle: Option<&tauri::AppHandle>,
) -> PublishBranchFreshnessOutcome {
    if let Err(error) = GitService::fetch_origin(repo_path).await {
        return PublishBranchFreshnessOutcome::OperationalError {
            message: format!("Failed to refresh git remotes before publishing: {error}"),
        };
    }

    let target_ref = resolve_publish_freshness_target(repo_path, base_ref).await;
    let target_sha = match GitService::get_branch_sha(repo_path, &target_ref).await {
        Ok(sha) => sha,
        Err(error) => {
            return PublishBranchFreshnessOutcome::OperationalError {
                message: format!(
                    "Failed to resolve publish base ref '{}' before publishing: {}",
                    target_ref, error
                ),
            };
        }
    };

    let event_sink = app_handle
        .and_then(|handle| handle.try_state::<AppState>())
        .map(|state| Arc::clone(&state.events));

    let result = update_source_from_target(
        repo_path,
        source_branch,
        &target_ref,
        project,
        conversation_id,
        event_sink.as_deref(),
    )
    .await;

    publish_branch_freshness_outcome_from_source_update(result, &target_ref, &target_sha)
}

pub async fn ensure_plan_publish_branch_fresh(
    repo_path: &Path,
    project: &Project,
    plan_branch: &str,
    base_ref: &str,
    conversation_id: &str,
    app_handle: Option<&tauri::AppHandle>,
) -> PublishBranchFreshnessOutcome {
    if let Err(error) = GitService::fetch_origin(repo_path).await {
        return PublishBranchFreshnessOutcome::OperationalError {
            message: format!("Failed to refresh git remotes before publishing: {error}"),
        };
    }

    let target_ref = resolve_publish_freshness_target(repo_path, base_ref).await;
    let target_sha = match GitService::get_branch_sha(repo_path, &target_ref).await {
        Ok(sha) => sha,
        Err(error) => {
            return PublishBranchFreshnessOutcome::OperationalError {
                message: format!(
                    "Failed to resolve publish base ref '{}' before publishing: {}",
                    target_ref, error
                ),
            };
        }
    };

    let event_sink = app_handle
        .and_then(|handle| handle.try_state::<AppState>())
        .map(|state| Arc::clone(&state.events));

    let result = update_plan_from_main_isolated(
        repo_path,
        plan_branch,
        &target_ref,
        project,
        conversation_id,
        event_sink.as_deref(),
    )
    .await;

    publish_branch_freshness_outcome_from_plan_update(result, &target_ref, &target_sha)
}

pub async fn inspect_publish_branch_freshness(
    repo_path: &Path,
    base_ref: &str,
    captured_base_commit: Option<&str>,
) -> AppResult<PublishBranchFreshnessStatus> {
    GitService::fetch_origin(repo_path).await?;
    let target_ref = resolve_publish_freshness_target(repo_path, base_ref).await;
    let target_sha = GitService::get_branch_sha(repo_path, &target_ref).await?;

    Ok(publish_branch_freshness_status_from_commits(
        captured_base_commit,
        &target_ref,
        &target_sha,
    ))
}

pub async fn inspect_publish_branch_freshness_for_source(
    repo_path: &Path,
    base_ref: &str,
    source_branch: &str,
    captured_base_commit: Option<&str>,
) -> AppResult<PublishBranchFreshnessStatus> {
    inspect_publish_branch_freshness_for_source_with_fetch(
        repo_path,
        base_ref,
        source_branch,
        captured_base_commit,
        true,
    )
    .await
}

pub async fn inspect_publish_branch_freshness_for_source_after_fetch(
    repo_path: &Path,
    base_ref: &str,
    source_branch: &str,
    captured_base_commit: Option<&str>,
) -> AppResult<PublishBranchFreshnessStatus> {
    inspect_publish_branch_freshness_for_source_with_fetch(
        repo_path,
        base_ref,
        source_branch,
        captured_base_commit,
        false,
    )
    .await
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum RepairPrHandoffVerification {
    Ok(PublishBranchFreshnessStatus),
    /// The repair branch still exactly matches its durable push receipt, but its base moved before
    /// a PR handoff was observed. A successor may safely retarget from the current workspace.
    Retargetable {
        reason: String,
    },
    Fatal(String),
}

/// Re-prove the immutable repair-owned push receipt before the normal publisher may reuse it.
/// This intentionally fetches and only reads refs: a repaired branch that no longer matches its
/// exact local, remote, or base OID must re-enter durable repair rather than being locally
/// refreshed while the normal branch push is suppressed.
pub(crate) async fn verify_agent_workspace_repair_pr_handoff(
    repo_path: &Path,
    source_branch: &str,
    base_ref: &str,
    handoff: &AgentWorkspaceRepairPrHandoff,
) -> AppResult<RepairPrHandoffVerification> {
    GitService::fetch_origin(repo_path).await?;
    let local_head_oid = GitService::get_head_sha(repo_path).await?;
    let local_branch_oid = GitService::get_branch_sha(repo_path, source_branch).await?;
    let remote_ref = remote_tracking_ref_for_publish(source_branch);
    if !GitService::ref_exists(repo_path, &remote_ref).await? {
        return Ok(RepairPrHandoffVerification::Fatal(format!(
            "workspace repair push handoff remote ref '{}' is missing",
            remote_ref
        )));
    }
    let remote_head_oid = GitService::get_branch_sha(repo_path, &remote_ref).await?;
    if local_head_oid != handoff.expected_head_oid
        || local_branch_oid != handoff.expected_head_oid
        || remote_head_oid != handoff.expected_head_oid
    {
        return Ok(RepairPrHandoffVerification::Fatal(format!(
            "workspace repair push handoff head no longer matches its exact remote receipt '{}'",
            handoff.expected_head_oid
        )));
    }

    if base_ref != handoff.target_base_ref {
        return Ok(RepairPrHandoffVerification::Retargetable {
            reason: format!(
                "workspace repair push handoff base ref changed from '{}' to '{}'",
                handoff.target_base_ref, base_ref
            ),
        });
    }

    let target_ref = resolve_publish_freshness_target(repo_path, base_ref).await;
    let target_base_commit = GitService::get_branch_sha(repo_path, &target_ref).await?;
    if target_base_commit != handoff.target_base_commit {
        return Ok(RepairPrHandoffVerification::Retargetable {
            reason: format!(
                "workspace repair push handoff base advanced from '{}' to '{}'",
                handoff.target_base_commit, target_base_commit
            ),
        });
    }

    Ok(RepairPrHandoffVerification::Ok(
        PublishBranchFreshnessStatus {
            target_ref,
            captured_base_commit: Some(handoff.target_base_commit.clone()),
            target_base_commit,
            is_base_ahead: false,
        },
    ))
}

async fn inspect_publish_branch_freshness_for_source_with_fetch(
    repo_path: &Path,
    base_ref: &str,
    source_branch: &str,
    captured_base_commit: Option<&str>,
    should_fetch: bool,
) -> AppResult<PublishBranchFreshnessStatus> {
    if should_fetch {
        GitService::fetch_origin(repo_path).await?;
    }
    let target_ref = resolve_publish_freshness_target(repo_path, base_ref).await;
    let target_sha = GitService::get_branch_sha(repo_path, &target_ref).await?;
    let source_contains_target =
        GitService::is_ancestor(repo_path, &target_sha, source_branch).await?;

    Ok(publish_branch_freshness_status_from_commits_and_branch(
        captured_base_commit,
        &target_ref,
        &target_sha,
        source_contains_target,
    ))
}

pub fn publish_branch_freshness_status_from_commits(
    captured_base_commit: Option<&str>,
    target_ref: &str,
    target_base_commit: &str,
) -> PublishBranchFreshnessStatus {
    let captured_base_commit = captured_base_commit
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string);
    let is_base_ahead = captured_base_commit
        .as_deref()
        .map(|captured| captured != target_base_commit)
        .unwrap_or(false);

    PublishBranchFreshnessStatus {
        target_ref: target_ref.to_string(),
        captured_base_commit,
        target_base_commit: target_base_commit.to_string(),
        is_base_ahead,
    }
}

pub fn publish_branch_freshness_status_from_commits_and_branch(
    captured_base_commit: Option<&str>,
    target_ref: &str,
    target_base_commit: &str,
    source_contains_target_base: bool,
) -> PublishBranchFreshnessStatus {
    if source_contains_target_base {
        return PublishBranchFreshnessStatus {
            target_ref: target_ref.to_string(),
            captured_base_commit: Some(target_base_commit.to_string()),
            target_base_commit: target_base_commit.to_string(),
            is_base_ahead: false,
        };
    }

    publish_branch_freshness_status_from_commits(
        captured_base_commit,
        target_ref,
        target_base_commit,
    )
}

pub struct AgentWorkspaceRepairCompletionCheck<'a> {
    pub freshness_status: &'a PublishBranchFreshnessStatus,
    pub workspace_base_ref: &'a str,
    pub resolved_base_ref: &'a str,
    pub resolved_base_commit: &'a str,
    pub repair_commit_sha: &'a str,
    pub workspace_head_sha: &'a str,
    pub has_uncommitted_changes: bool,
    pub is_merge_in_progress: bool,
    pub is_rebase_in_progress: bool,
    pub has_conflict_files: bool,
    pub has_conflict_markers: bool,
}

pub struct AgentWorkspaceSettledHeadCheck<'a> {
    pub reported_head_sha: &'a str,
    pub workspace_head_sha: &'a str,
    pub has_uncommitted_changes: bool,
    pub is_merge_in_progress: bool,
    pub is_rebase_in_progress: bool,
    pub has_conflict_files: bool,
    pub has_conflict_markers: bool,
}

pub fn verify_agent_workspace_settled_current_head(
    check: AgentWorkspaceSettledHeadCheck<'_>,
) -> Result<(), String> {
    if check.workspace_head_sha != check.reported_head_sha {
        return Err(format!(
            "reported fix commit '{}' is not the current workspace HEAD '{}'",
            check.reported_head_sha, check.workspace_head_sha
        ));
    }
    if check.has_uncommitted_changes {
        return Err("workspace has uncommitted changes".to_string());
    }
    if check.is_merge_in_progress {
        return Err("workspace merge is still in progress".to_string());
    }
    if check.is_rebase_in_progress {
        return Err("workspace rebase is still in progress".to_string());
    }
    if check.has_conflict_files {
        return Err("workspace still contains unresolved conflict files".to_string());
    }
    if check.has_conflict_markers {
        return Err("workspace still contains conflict markers".to_string());
    }
    Ok(())
}

pub fn verify_agent_workspace_repair_completion(
    check: AgentWorkspaceRepairCompletionCheck<'_>,
) -> Result<(), String> {
    let target_ref = check.freshness_status.target_ref.as_str();
    if check.resolved_base_ref != check.workspace_base_ref && check.resolved_base_ref != target_ref
    {
        return Err(format!(
            "resolved_base_ref '{}' does not match workspace base '{}' or target '{}'",
            check.resolved_base_ref, check.workspace_base_ref, target_ref
        ));
    }

    if check.resolved_base_commit != check.freshness_status.target_base_commit {
        return Err(format!(
            "resolved_base_commit '{}' does not match current target base '{}'",
            check.resolved_base_commit, check.freshness_status.target_base_commit
        ));
    }

    if check.freshness_status.is_base_ahead {
        return Err(format!(
            "workspace branch is still behind {} at {}",
            check.freshness_status.target_ref, check.freshness_status.target_base_commit
        ));
    }

    verify_agent_workspace_settled_current_head(AgentWorkspaceSettledHeadCheck {
        reported_head_sha: check.repair_commit_sha,
        workspace_head_sha: check.workspace_head_sha,
        has_uncommitted_changes: check.has_uncommitted_changes,
        is_merge_in_progress: check.is_merge_in_progress,
        is_rebase_in_progress: check.is_rebase_in_progress,
        has_conflict_files: check.has_conflict_files,
        has_conflict_markers: check.has_conflict_markers,
    })
}

pub(crate) fn publish_branch_freshness_outcome_from_source_update(
    result: SourceUpdateResult,
    target_ref: &str,
    target_sha: &str,
) -> PublishBranchFreshnessOutcome {
    match result {
        SourceUpdateResult::AlreadyUpToDate => PublishBranchFreshnessOutcome::AlreadyFresh {
            base_commit: target_sha.to_string(),
            target_ref: target_ref.to_string(),
        },
        SourceUpdateResult::Updated => PublishBranchFreshnessOutcome::Updated {
            base_commit: target_sha.to_string(),
            target_ref: target_ref.to_string(),
        },
        SourceUpdateResult::Conflicts { conflict_files } => {
            let conflict_files = conflict_files
                .into_iter()
                .map(|path| path.to_string_lossy().to_string())
                .collect::<Vec<_>>();
            let files_label = if conflict_files.is_empty() {
                "unknown files".to_string()
            } else {
                conflict_files.join(", ")
            };
            PublishBranchFreshnessOutcome::NeedsAgent {
                message: format!(
                    "Merge conflict updating agent workspace branch from {target_ref}: {files_label}"
                ),
                conflict_files,
                base_commit: target_sha.to_string(),
                target_ref: target_ref.to_string(),
            }
        }
        SourceUpdateResult::BranchMissing { branch } => {
            PublishBranchFreshnessOutcome::OperationalError {
                message: format!("branch missing before freshness update: {}", branch),
            }
        }
        SourceUpdateResult::Error(message) => {
            PublishBranchFreshnessOutcome::OperationalError { message }
        }
    }
}

pub(crate) fn publish_branch_freshness_outcome_from_plan_update(
    result: PlanUpdateResult,
    target_ref: &str,
    target_sha: &str,
) -> PublishBranchFreshnessOutcome {
    match result {
        PlanUpdateResult::AlreadyUpToDate | PlanUpdateResult::NotPlanBranch => {
            PublishBranchFreshnessOutcome::AlreadyFresh {
                base_commit: target_sha.to_string(),
                target_ref: target_ref.to_string(),
            }
        }
        PlanUpdateResult::Updated => PublishBranchFreshnessOutcome::Updated {
            base_commit: target_sha.to_string(),
            target_ref: target_ref.to_string(),
        },
        PlanUpdateResult::Conflicts { conflict_files } => {
            let conflict_files = conflict_files
                .into_iter()
                .map(|path| path.to_string_lossy().to_string())
                .collect::<Vec<_>>();
            let files_label = if conflict_files.is_empty() {
                "unknown files".to_string()
            } else {
                conflict_files.join(", ")
            };
            PublishBranchFreshnessOutcome::NeedsAgent {
                message: format!(
                    "Merge conflict updating plan branch from {target_ref}: {files_label}"
                ),
                conflict_files,
                base_commit: target_sha.to_string(),
                target_ref: target_ref.to_string(),
            }
        }
        PlanUpdateResult::Error(message) => {
            PublishBranchFreshnessOutcome::OperationalError { message }
        }
    }
}

pub fn remote_tracking_ref_for_publish(base_ref: &str) -> String {
    if base_ref.starts_with("origin/") {
        base_ref.to_string()
    } else {
        format!("origin/{base_ref}")
    }
}

pub(crate) async fn resolve_publish_freshness_target(repo_path: &Path, base_ref: &str) -> String {
    let remote_ref = remote_tracking_ref_for_publish(base_ref);
    if remote_ref != base_ref
        && GitService::ref_exists(repo_path, &remote_ref)
            .await
            .unwrap_or(false)
    {
        remote_ref
    } else {
        base_ref.to_string()
    }
}

fn is_agent_fixable_failure(normalized: &str) -> bool {
    const PATTERNS: &[&str] = &[
        "conflict",
        "unmerged paths",
        "<<<<<<<",
        "pre-commit",
        "precommit",
        "typecheck",
        "tsc",
        "clippy",
        "lint",
        "test failed",
        "tests failed",
        "non-fast-forward",
        "failed to push some refs",
        "updates were rejected",
        "fetch first",
        "would be overwritten by merge",
        "please commit your changes or stash them before you merge",
    ];

    PATTERNS.iter().any(|pattern| normalized.contains(pattern))
}

fn is_commit_hook_failure_context(normalized: &str) -> bool {
    const PATTERNS: &[&str] = &[
        "pre-commit",
        "precommit",
        "[pre-commit]",
        "commit-msg",
        "prepare-commit-msg",
        "husky",
        "hook declined",
    ];

    PATTERNS.iter().any(|pattern| normalized.contains(pattern))
}

fn is_operational_failure(normalized: &str) -> bool {
    const PATTERNS: &[&str] = &[
        "github integration is not available",
        "git authentication",
        "workspace not found",
        "conversation not found",
        "project not found",
        "authentication",
        "authorization",
        "permission denied",
        "cannot find package",
        "could not resolve",
    ];

    PATTERNS.iter().any(|pattern| normalized.contains(pattern))
}
