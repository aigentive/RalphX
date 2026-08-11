//! Shared prune logic for the GC pruner and reconciliation pruner.
//!
//! Both `prune_stale_execution_registry_entries` (GC, high-frequency) and
//! `prune_stale_running_registry_entries` (reconciler, 30s) need identical
//! per-entry staleness evaluation and prune actions. `PruneEngine` centralises
//! this logic so the two pruners cannot diverge again.
//!
//! # Capabilities
//! - PID-verified IPR check — skips alive interactive processes, removes stale IPR entries
//! - Staleness evaluation — reasons: pid_missing | run_not_running | run_missing |
//!   task_status_mismatch | task_missing
//! - Prune action — unregister/stop + cancel agent_run + structured `warn!` logging
//!
//! # What stays in each caller
//! Post-loop concerns differ between the high-frequency GC and the 30s reconciler:
//! - GC: removes interactive idle slot tracking, updates running count only when pruned
//! - Reconciler: always recalculates running count from remaining entries, emits event

use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::sync::Arc;

use tracing::warn;

use crate::application::interactive_process_registry::{
    InteractiveProcessKey, InteractiveProcessRegistry,
};
use crate::domain::entities::{
    AgentRun, AgentRunId, AgentRunStatus, ChatContextType, InternalStatus, TaskId,
};
use crate::domain::repositories::{
    AgentRunRepository, ProjectRepository, TaskRepository, PRUNED_STALE_AGENT_RUN,
};
use crate::domain::services::{RunningAgentInfo, RunningAgentKey, RunningAgentRegistry};
use crate::infrastructure::agents::claude::stream_timeouts;

/// Returns true when a task-backed context has a status consistent with an active agent.
///
/// Used only for `TaskExecution | Review | Merge` — Ideation entries skip the task lookup
/// because their context IDs are session IDs, not task IDs.
fn context_matches_running_status(context_type: ChatContextType, status: InternalStatus) -> bool {
    match context_type {
        ChatContextType::TaskExecution => {
            status == InternalStatus::Executing || status == InternalStatus::ReExecuting
        }
        ChatContextType::Review => status == InternalStatus::Reviewing,
        ChatContextType::Merge => status == InternalStatus::Merging,
        _ => false,
    }
}

pub(super) fn should_defer_terminal_settlement_prune(
    context_type: Option<ChatContextType>,
    reasons: &[&'static str],
    info: &RunningAgentInfo,
    run: Option<&AgentRun>,
    pid_alive: bool,
) -> bool {
    if !pid_alive || reasons.len() != 1 || reasons[0] != "task_status_mismatch" {
        return false;
    }
    if !matches!(
        context_type,
        Some(ChatContextType::Merge | ChatContextType::Review)
    ) {
        return false;
    }
    if !matches!(run, Some(agent_run) if agent_run.status == AgentRunStatus::Running) {
        return false;
    }

    let Some(last_active_at) = info.last_active_at else {
        return false;
    };
    let grace_secs = i64::try_from(stream_timeouts().completion_grace_secs).unwrap_or(i64::MAX);
    let age = chrono::Utc::now().signed_duration_since(last_active_at);
    age.num_seconds() <= grace_secs
}

pub(super) fn should_defer_pid_missing_prune(
    reasons: &[&'static str],
    info: &RunningAgentInfo,
    run: Option<&AgentRun>,
) -> bool {
    if reasons != ["pid_missing"] {
        return false;
    }
    if !matches!(run, Some(agent_run) if agent_run.status == AgentRunStatus::Running) {
        return false;
    }

    let grace_secs = i64::try_from(stream_timeouts().completion_grace_secs).unwrap_or(i64::MAX);
    let last_activity = info.last_active_at.unwrap_or(info.started_at);
    let age = chrono::Utc::now().signed_duration_since(last_activity);
    age.num_seconds() <= grace_secs
}

/// Shared per-entry prune logic for the GC pruner and the reconciliation pruner.
///
/// Construct one `PruneEngine` per pruning pass and call [`PruneEngine::check_ipr_skip`]
/// followed by [`PruneEngine::evaluate_and_prune`] for each registry entry.
pub struct PruneEngine {
    running_agent_registry: Arc<dyn RunningAgentRegistry>,
    agent_run_repo: Arc<dyn AgentRunRepository>,
    task_repo: Arc<dyn TaskRepository>,
    project_repo: Arc<dyn ProjectRepository>,
    interactive_process_registry: Option<Arc<InteractiveProcessRegistry>>,
}

impl PruneEngine {
    pub fn new(
        running_agent_registry: Arc<dyn RunningAgentRegistry>,
        agent_run_repo: Arc<dyn AgentRunRepository>,
        task_repo: Arc<dyn TaskRepository>,
        project_repo: Arc<dyn ProjectRepository>,
        interactive_process_registry: Option<Arc<InteractiveProcessRegistry>>,
    ) -> Self {
        Self {
            running_agent_registry,
            agent_run_repo,
            task_repo,
            project_repo,
            interactive_process_registry,
        }
    }

    /// PID-verified IPR skip check.
    ///
    /// Returns `true` if the entry has a live interactive process (caller should skip it).
    ///
    /// If the IPR has an entry but the PID is dead, the stale IPR entry is removed and
    /// `false` is returned so pruning can proceed. This mirrors the reconciler's
    /// `is_ipr_process_alive` but operates on the pre-computed `pid_alive` value to
    /// avoid redundant non-blocking syscalls.
    pub async fn check_ipr_skip(&self, key: &RunningAgentKey, pid_alive: bool) -> bool {
        let ipr = match self.interactive_process_registry.as_ref() {
            Some(ipr) => ipr,
            None => return false,
        };

        let ipr_key = InteractiveProcessKey::new(&key.context_type, &key.context_id);
        if !ipr.has_process(&ipr_key).await {
            return false;
        }

        if pid_alive {
            tracing::debug!(
                context_type = key.context_type,
                context_id = key.context_id,
                "Skipping prune for interactive process"
            );
            return true;
        }

        // IPR has an entry but the PID is dead → stale entry, remove it so
        // reconciliation is not blocked forever.
        warn!(
            context_type = key.context_type,
            context_id = key.context_id,
            "Removing stale IPR entry during prune — PID no longer alive"
        );
        ipr.remove(&ipr_key).await;
        false
    }

    /// Evaluate staleness and, if stale, execute the prune action for one registry entry.
    ///
    /// Returns `true` if the entry was pruned, `false` if it was kept.
    ///
    /// # Staleness reasons
    /// - `pid_missing` — process is no longer alive
    /// - `run_not_running` — agent_run row exists but status ≠ Running
    /// - `run_missing` — agent_run row does not exist
    /// - `task_status_mismatch` — task is not in an agent-active status for this context type
    /// - `task_missing` — task row does not exist
    ///
    /// # Error handling
    /// On DB error loading agent_run or task the entry is kept (returns `false`) to avoid
    /// discarding entries during transient DB failures. A `warn!` is emitted for each.
    pub async fn evaluate_and_prune(
        &self,
        key: &RunningAgentKey,
        info: &RunningAgentInfo,
        pid_alive: bool,
    ) -> bool {
        let run = match self
            .agent_run_repo
            .get_by_id(&AgentRunId::from_string(&info.agent_run_id))
            .await
        {
            Ok(run) => run,
            Err(e) => {
                warn!(
                    context_type = key.context_type,
                    context_id = key.context_id,
                    run_id = info.agent_run_id,
                    error = %e,
                    "Failed to load agent_run while pruning running registry; keeping entry"
                );
                return false;
            }
        };

        if info.pid == 0 {
            let lease_secs =
                i64::try_from(stream_timeouts().launch_reservation_lease_secs).unwrap_or(i64::MAX);
            let renewed_at = info.last_active_at.unwrap_or(info.started_at);
            let lease_fresh = chrono::Utc::now().signed_duration_since(renewed_at)
                <= chrono::Duration::seconds(lease_secs);
            let owned_run_terminal = matches!(
                run.as_ref(),
                Some(agent_run) if agent_run.status != AgentRunStatus::Running
            );

            if lease_fresh && !owned_run_terminal {
                tracing::debug!(
                    context_type = key.context_type,
                    context_id = key.context_id,
                    run_id = info.agent_run_id,
                    "Keeping fresh launch reservation"
                );
                return false;
            }

            let removed = self
                .running_agent_registry
                .unregister(key, &info.agent_run_id)
                .await;
            if removed.is_none() {
                return false;
            }
            if let Some(agent_run) = run {
                if agent_run.status == AgentRunStatus::Running {
                    let _ = self
                        .agent_run_repo
                        .cancel_with_reason(
                            &AgentRunId::from_string(&info.agent_run_id),
                            PRUNED_STALE_AGENT_RUN,
                        )
                        .await;
                }
            }
            warn!(
                context_type = key.context_type,
                context_id = key.context_id,
                run_id = info.agent_run_id,
                lease_fresh,
                owned_run_terminal,
                "Pruned expired or terminal launch reservation"
            );
            return true;
        }

        let mut reasons: Vec<&'static str> = Vec::new();

        if !pid_alive {
            reasons.push("pid_missing");
        }
        match run.as_ref() {
            Some(r) if r.status != AgentRunStatus::Running => reasons.push("run_not_running"),
            None => reasons.push("run_missing"),
            _ => {}
        }

        // Task status check for task-backed contexts.
        // Ideation uses session IDs (not task IDs) — skip the task lookup to avoid
        // mis-routing a session ID through the task repository.
        let context_type = ChatContextType::from_str(&key.context_type).ok();
        let mut merge_cleanup_path: Option<PathBuf> = None;
        if let Some(ctx) = context_type {
            if matches!(
                ctx,
                ChatContextType::TaskExecution | ChatContextType::Review | ChatContextType::Merge
            ) {
                let task_id = TaskId::from_string(key.context_id.clone());
                match self.task_repo.get_by_id(&task_id).await {
                    Ok(Some(task)) => {
                        if !context_matches_running_status(ctx, task.internal_status) {
                            reasons.push("task_status_mismatch");
                        }
                        if ctx == ChatContextType::Merge {
                            merge_cleanup_path = self
                                .validated_merge_cleanup_path(&task, info.worktree_path.as_deref())
                                .await;
                        }
                    }
                    Ok(None) => reasons.push("task_missing"),
                    Err(e) => {
                        warn!(
                            context_type = key.context_type,
                            context_id = key.context_id,
                            error = %e,
                            "Failed to load task while pruning running registry; keeping entry"
                        );
                        return false;
                    }
                }
            }
        }

        if reasons.is_empty() {
            return false;
        }

        if should_defer_terminal_settlement_prune(
            context_type,
            &reasons,
            info,
            run.as_ref(),
            pid_alive,
        ) {
            tracing::debug!(
                context_type = key.context_type,
                context_id = key.context_id,
                run_id = info.agent_run_id,
                "Deferring prune for live terminal-tool settlement"
            );
            return false;
        }

        if should_defer_pid_missing_prune(&reasons, info, run.as_ref()) {
            tracing::debug!(
                context_type = key.context_type,
                context_id = key.context_id,
                run_id = info.agent_run_id,
                "Deferring pid_missing prune within completion grace"
            );
            return false;
        }

        // Merge cleanup retains the exact owner reservation until the worktree side
        // effect is complete, so a replacement cannot claim and reuse the path.
        let merge_cleanup = context_type == Some(ChatContextType::Merge);
        let removed = if merge_cleanup {
            self.running_agent_registry
                .quiesce_if_owned(key, &info.agent_run_id)
                .await
                .ok()
                .flatten()
        } else if pid_alive {
            self.running_agent_registry
                .stop_if_owned(key, &info.agent_run_id)
                .await
                .ok()
                .flatten()
        } else {
            self.running_agent_registry
                .unregister(key, &info.agent_run_id)
                .await
        };
        if removed.is_none() {
            return false;
        }

        if let Some(agent_run) = run {
            if agent_run.status == AgentRunStatus::Running {
                let run_id = AgentRunId::from_string(&info.agent_run_id);
                if !merge_cleanup && !pid_alive {
                    let _ = self
                        .agent_run_repo
                        .cancel_with_reason(&run_id, PRUNED_STALE_AGENT_RUN)
                        .await;
                } else {
                    let _ = self.agent_run_repo.cancel(&run_id).await;
                }
            }
        }

        warn!(
            context_type = key.context_type,
            context_id = key.context_id,
            pid = info.pid,
            run_id = info.agent_run_id,
            reasons = reasons.join(","),
            "Pruned stale running agent registry entry"
        );

        if merge_cleanup {
            if let Some(path) = merge_cleanup_path {
                if let Err(error) = crate::utils::path_safety::checked_remove_dir_all(
                    &path,
                    "pruned merge worktree",
                )
                .await
                {
                    warn!(path = %path.display(), %error, "Failed to remove merge worktree after prune");
                }
            }
            self.running_agent_registry
                .unregister(key, &info.agent_run_id)
                .await;
        }

        true
    }

    async fn validated_merge_cleanup_path(
        &self,
        task: &crate::domain::entities::Task,
        candidate: Option<&str>,
    ) -> Option<PathBuf> {
        let candidate = Path::new(candidate?);
        let candidate = match crate::utils::path_safety::validate_absolute_non_root_path(
            candidate,
            "pruned merge worktree",
        ) {
            Ok(path) => path,
            Err(error) => {
                warn!(path = %candidate.display(), %error, "Refusing unsafe merge worktree cleanup path");
                return None;
            }
        };
        let project = match self.project_repo.get_by_id(&task.project_id).await {
            Ok(Some(project)) => project,
            Ok(None) => {
                warn!(task_id = %task.id, "Skipping merge worktree cleanup because project is missing");
                return None;
            }
            Err(error) => {
                warn!(task_id = %task.id, %error, "Skipping merge worktree cleanup because project lookup failed");
                return None;
            }
        };
        let expected = project.task_worktree_path(task.id.as_str());
        let expected = match crate::utils::path_safety::validate_absolute_non_root_path(
            &expected,
            "expected merge worktree",
        ) {
            Ok(path) => path,
            Err(error) => {
                warn!(task_id = %task.id, %error, "Skipping merge worktree cleanup because the canonical path is unsafe");
                return None;
            }
        };
        if candidate != expected {
            warn!(
                task_id = %task.id,
                candidate = %candidate.display(),
                expected = %expected.display(),
                "Skipping merge worktree cleanup outside the canonical task path"
            );
            return None;
        }
        Some(candidate)
    }
}
