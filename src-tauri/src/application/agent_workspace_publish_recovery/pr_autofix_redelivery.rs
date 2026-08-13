//! PR-autofix-specific halves of durable repair redelivery.
//!
//! A PR autofix generation is owned by the PR fixer, not the generic workspace repairer, and its
//! successors are only worth spending when GitHub reports something new. Both concerns are
//! evidence-driven rather than phase-driven, so they live beside the recovery reconciler instead
//! of inside it.

use crate::application::agent_conversation_workspace::resolve_effective_agent_conversation_workspace_path;
use crate::application::agent_workspace_publish_repair_state::{
    held_repair_has_unpublished_head, PrAutofixCarryover,
};
use crate::application::services::pr_merge_poller::classify_agent_workspace_pr_autofix_issue;
use crate::application::AppState;
use crate::domain::entities::{AgentConversationWorkspace, AgentWorkspaceRepairAttempt};

use super::durable_attempt_recovery::{
    human_repair_dispatch_context, DEFAULT_REPAIR_DISPATCH_CONTEXT,
};

/// How much agent time has already been spent on one failure identity, and whether that spend has
/// passed the configured budget.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct PrAutofixFingerprintSpend {
    pub(crate) generations: u32,
    pub(crate) minutes: u64,
    pub(crate) budget_minutes: u64,
}

impl PrAutofixFingerprintSpend {
    pub(crate) fn is_exhausted(self) -> bool {
        self.budget_minutes > 0 && self.minutes >= self.budget_minutes
    }
}

/// Sums the wall-clock time of every agent run that already worked on this exact failure identity.
///
/// Retry caps count attempts, not cost: three cheap generations and three hour-long Opus
/// generations look identical to a streak counter. Only finished runs are counted, so an
/// in-flight run can never push a conversation over budget mid-repair.
pub(crate) async fn pr_autofix_fingerprint_spend(
    state: &AppState,
    conversation_id: &crate::domain::entities::ChatConversationId,
    fingerprint: &str,
) -> crate::error::AppResult<PrAutofixFingerprintSpend> {
    let budget_minutes =
        crate::infrastructure::agents::limits_config().repair_fingerprint_budget_minutes;
    let attempts = state
        .agent_workspace_repair_repo
        .list_repair_attempts_for_conversation(conversation_id)
        .await?;

    let mut generations = 0;
    let mut seconds: i64 = 0;
    for attempt in attempts {
        if attempt.pr_autofix_health_fingerprint.as_deref() != Some(fingerprint) {
            continue;
        }
        generations += 1;
        let Some(run_id) = attempt.reserved_agent_run_id.as_ref() else {
            continue;
        };
        let Some(run) = state.agent_run_repo.get_by_id(run_id).await? else {
            continue;
        };
        if run.conversation_id != *conversation_id {
            continue;
        }
        if let Some(completed_at) = run.completed_at {
            seconds += (completed_at - run.started_at).num_seconds().max(0);
        }
    }

    Ok(PrAutofixFingerprintSpend {
        generations,
        minutes: (seconds / 60) as u64,
        budget_minutes,
    })
}

/// Whether a blocked PR autofix generation has earned another agent generation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum PrAutofixSuccessorDecision {
    /// A successor is authorized. Carries freshly observed PR evidence when GitHub could be read,
    /// so downstream fingerprint suppression has something to compare on the next round.
    Proceed(Option<PrAutofixCarryover>),
    /// The prior generation produced a validated local repair head that has not reached the PR.
    /// Re-drive the durable publish continuation instead of holding or buying another fixer run:
    /// GitHub health cannot change until that head is published.
    RedrivePublish,
    /// GitHub reports the exact same failure identity. Another generation would re-read identical
    /// evidence and can only repeat the previous outcome, so the attempt parks instead.
    HoldUnchanged,
    /// Nothing observed here authorizes spending an agent. Covers unresolvable PR identity,
    /// health-fetch failure, and a PR that no longer reports any failing issue to fix.
    Withhold(&'static str),
}

/// Resolves the exact PR this workspace's autofix work belongs to. Edit-mode workspaces own their
/// publication PR directly; Ideation-mode workspaces borrow it from the linked plan branch, which
/// must still be the one this workspace is bound to.
async fn resolve_pr_autofix_pr_number(
    state: &AppState,
    workspace: &AgentConversationWorkspace,
) -> Option<i64> {
    if let Some(pr_number) = workspace.publication_pr_number {
        return Some(pr_number);
    }
    let plan_branch_id = workspace.linked_plan_branch_id.as_ref()?;
    let plan_branch = state
        .plan_branch_repo
        .get_by_id(plan_branch_id)
        .await
        .ok()
        .flatten()?;
    if workspace.linked_ideation_session_id.as_ref() != Some(&plan_branch.session_id) {
        return None;
    }
    plan_branch.pr_number
}

/// True when the workspace's base has moved since this generation was targeted. That is new input
/// for the repair independent of anything GitHub reports about the PR, so it authorizes a
/// successor on its own.
pub(super) fn repair_base_advanced(
    current: &AgentWorkspaceRepairAttempt,
    workspace: &AgentConversationWorkspace,
) -> bool {
    let (Some(target), Some(workspace_base)) = (
        current.target_base_commit.as_deref(),
        workspace.base_commit.as_deref(),
    ) else {
        return false;
    };
    !target.trim().is_empty() && !workspace_base.trim().is_empty() && target != workspace_base
}

/// Decides whether a blocked PR autofix attempt may start a successor generation.
///
/// The gate is deliberately narrow. It applies only when this generation was dispatched against an
/// exact observed PR failure and nothing else has moved — the shape that burned four Opus
/// generations on one unchanged failing check. Once it does apply, every failure mode returns
/// `Withhold`: a repository error, an unreachable GitHub, or an unresolvable PR must never look
/// like "health changed", because that is the one answer that authorizes spending an agent.
pub(crate) async fn evaluate_pr_autofix_successor(
    state: &AppState,
    current: &AgentWorkspaceRepairAttempt,
    workspace: &AgentConversationWorkspace,
) -> PrAutofixSuccessorDecision {
    // No prior failure identity means this generation was never dispatched against an observed PR
    // failure (legacy rows, base-update blockers routed through a PR autofix attempt). There is
    // nothing to compare, so the existing retry behavior stands.
    if current.pr_autofix_health_fingerprint.is_none() {
        return PrAutofixSuccessorDecision::Proceed(None);
    }
    // A moved base is new input for the repair even when the PR still reports the same failure.
    if repair_base_advanced(current, workspace) {
        return PrAutofixSuccessorDecision::Proceed(None);
    }
    let Some(github) = state.github_service.as_ref() else {
        return PrAutofixSuccessorDecision::Withhold("github_service_unavailable");
    };
    let Some(pr_number) = resolve_pr_autofix_pr_number(state, workspace).await else {
        return PrAutofixSuccessorDecision::Withhold("pr_number_unresolved");
    };
    let project = match state.project_repo.get_by_id(&workspace.project_id).await {
        Ok(Some(project)) => project,
        Ok(None) => return PrAutofixSuccessorDecision::Withhold("project_missing"),
        Err(error) => {
            tracing::warn!(
                conversation_id = workspace.conversation_id.as_str(),
                %error,
                "Blocked PR autofix successor evaluation could not load its project"
            );
            return PrAutofixSuccessorDecision::Withhold("project_unreadable");
        }
    };
    let resolved = match resolve_effective_agent_conversation_workspace_path(
        &project,
        workspace,
        state.plan_branch_repo.as_ref(),
    )
    .await
    {
        Ok(resolved) => resolved,
        Err(error) => {
            tracing::warn!(
                conversation_id = workspace.conversation_id.as_str(),
                pr_number,
                %error,
                "Blocked PR autofix successor evaluation could not resolve its workspace path"
            );
            return PrAutofixSuccessorDecision::Withhold("workspace_path_unresolved");
        }
    };
    let health = match github.fetch_pr_health(&resolved.path, pr_number).await {
        Ok(health) => health,
        Err(error) => {
            tracing::warn!(
                conversation_id = workspace.conversation_id.as_str(),
                pr_number,
                %error,
                "Blocked PR autofix successor evaluation could not read current PR health"
            );
            return PrAutofixSuccessorDecision::Withhold("pr_health_unreadable");
        }
    };
    let Some(issue) = classify_agent_workspace_pr_autofix_issue(pr_number, &health) else {
        // Nothing is failing on the PR any more. A fixer generation dispatched now would have
        // nothing to fix, which is exactly the wasted generation this path exists to prevent.
        return PrAutofixSuccessorDecision::Withhold("pr_issue_resolved");
    };
    if current.pr_autofix_health_fingerprint.as_deref() == Some(issue.classification.as_str()) {
        if held_repair_has_unpublished_head(current, health.sync_state.head_ref_oid.as_deref()) {
            return PrAutofixSuccessorDecision::RedrivePublish;
        }
        return PrAutofixSuccessorDecision::HoldUnchanged;
    }
    PrAutofixSuccessorDecision::Proceed(Some(PrAutofixCarryover {
        dispatch_head_commit: health.sync_state.head_ref_oid.clone(),
        health_fingerprint: Some(issue.classification),
    }))
}

/// Persists the failure identity a PR autofix generation stopped against, so a later streak can
/// recognise it. Best effort by design: failing to remember must never block the settlement that
/// is already correct, it only costs the cross-streak suppression on the next poll.
pub(super) async fn remember_blocked_pr_autofix_fingerprint(
    state: &AppState,
    attempt: &AgentWorkspaceRepairAttempt,
) {
    if attempt.source != crate::domain::entities::AgentWorkspaceRepairSource::PrAutofix {
        return;
    }
    let Some(fingerprint) = attempt.pr_autofix_health_fingerprint.as_deref() else {
        return;
    };
    if let Err(error) = state
        .agent_conversation_workspace_repo
        .set_last_blocked_pr_health_fingerprint(&attempt.conversation_id, Some(fingerprint))
        .await
    {
        tracing::warn!(
            conversation_id = attempt.conversation_id.as_str(),
            attempt_id = attempt.id.as_str(),
            %error,
            "Could not remember the blocked PR autofix failure identity; a later streak may repeat it"
        );
    }
}

/// A redelivered PR autofix is addressed to the PR fixer, so the assignment must carry the same PR
/// identity and completion contract the poller's first dispatch uses. A redelivery is also not a
/// fresh dispatch: an earlier generation may already have committed part of the work, so the
/// recipient must re-observe GitHub before changing anything.
pub(crate) fn due_pr_autofix_redispatch_message(
    attempt: &AgentWorkspaceRepairAttempt,
    workspace: &AgentConversationWorkspace,
) -> String {
    let mut out = String::new();
    match workspace.publication_pr_number {
        Some(pr_number) => out.push_str(&format!(
            "RalphX is redelivering an interrupted PR fix for GitHub PR #{pr_number} on this agent workspace.\n\n"
        )),
        None => out
            .push_str("RalphX is redelivering an interrupted PR fix for this agent workspace.\n\n"),
    }
    out.push_str(
        "Re-observe the current state before changing anything: an earlier fixer generation may already have committed part of this work, and CI may have moved on since the failure was recorded. Inspect the live checks first, then fix only what is still broken.\n\n",
    );
    out.push_str(&format!(
        "Conversation ID: {}\n",
        workspace.conversation_id.as_str()
    ));
    out.push_str(&format!("Workspace branch: {}\n", workspace.branch_name));
    if let Some(pr_url) = workspace.publication_pr_url.as_deref() {
        out.push_str(&format!("Pull request: {pr_url}\n"));
    }
    if let Some(fingerprint) = attempt.pr_autofix_health_fingerprint.as_deref() {
        out.push_str(&format!(
            "Last observed failure fingerprint: {fingerprint}\n"
        ));
    }
    out.push_str(&format!(
        "Context: {}\n",
        human_repair_dispatch_context(attempt).unwrap_or(DEFAULT_REPAIR_DISPATCH_CONTEXT)
    ));
    out.push_str(
        "\nWhen you are done, call `complete_agent_workspace_pr_fix` with an honest resolution:\n",
    );
    out.push_str("- `fixed` with `fix_commit_sha` when you committed a real fix\n");
    out.push_str(
        "- `transient_ci` only for GitHub Actions infrastructure failures (runner cancellation, \
         infrastructure timeout, startup failure) — never for a real test, lint, coverage, or code failure\n",
    );
    out.push_str(
        "- `pre_existing_on_base` when the same failure already exists on the base branch and this PR did not cause it\n",
    );
    out.push_str("- `needs_human` when the problem needs a human decision\n");
    out.push_str(
        "Never report `fixed` without a commit, and never invent a change just to have something to report.\n",
    );
    out.push_str(
        "\nStart by calling `get_agent_workspace_pr_fix_context` with the conversation ID above.",
    );
    out
}
