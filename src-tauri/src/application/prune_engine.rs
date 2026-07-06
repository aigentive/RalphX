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

use std::str::FromStr;
use std::sync::Arc;

use tracing::warn;

use crate::application::interactive_process_registry::{
    InteractiveProcessKey, InteractiveProcessRegistry,
};
use crate::domain::entities::{
    AgentRun, AgentRunId, AgentRunStatus, ChatContextType, InternalStatus, TaskId,
};
use crate::domain::repositories::{AgentRunRepository, TaskRepository};
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

fn should_defer_terminal_settlement_prune(
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

/// Shared per-entry prune logic for the GC pruner and the reconciliation pruner.
///
/// Construct one `PruneEngine` per pruning pass and call [`PruneEngine::check_ipr_skip`]
/// followed by [`PruneEngine::evaluate_and_prune`] for each registry entry.
pub struct PruneEngine {
    running_agent_registry: Arc<dyn RunningAgentRegistry>,
    agent_run_repo: Arc<dyn AgentRunRepository>,
    task_repo: Arc<dyn TaskRepository>,
    interactive_process_registry: Option<Arc<InteractiveProcessRegistry>>,
}

impl PruneEngine {
    pub fn new(
        running_agent_registry: Arc<dyn RunningAgentRegistry>,
        agent_run_repo: Arc<dyn AgentRunRepository>,
        task_repo: Arc<dyn TaskRepository>,
        interactive_process_registry: Option<Arc<InteractiveProcessRegistry>>,
    ) -> Self {
        Self {
            running_agent_registry,
            agent_run_repo,
            task_repo,
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
        // Skip in-flight registrations: try_register writes pid=0/empty agent_run_id as a
        // placeholder; update_agent_process fills real values ~40ms later. Pruning here
        // would incorrectly discard a valid in-progress registration.
        if info.agent_run_id.is_empty() {
            tracing::debug!(
                context_type = key.context_type,
                context_id = key.context_id,
                "Skipping in-flight registry entry (no agent_run_id yet)"
            );
            return false;
        }

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

        // Execute prune: stop (if pid alive) or unregister, then cancel the agent_run.
        if pid_alive {
            let _ = self.running_agent_registry.stop(key).await;
        } else {
            let _ = self
                .running_agent_registry
                .unregister(key, &info.agent_run_id)
                .await;
        }

        if let Some(agent_run) = run {
            if agent_run.status == AgentRunStatus::Running {
                let _ = self
                    .agent_run_repo
                    .cancel(&AgentRunId::from_string(&info.agent_run_id))
                    .await;
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

        // Best-effort worktree cleanup for Merging contexts (Bug 5).
        // Merge worktrees should not persist on disk when the merger agent is pruned.
        if let Some(ChatContextType::Merge) = context_type {
            if let Some(path) = &info.worktree_path {
                if let Err(e) = tokio::fs::remove_dir_all(path).await {
                    warn!(
                        path = %path,
                        error = %e,
                        "Failed to remove merge worktree after prune"
                    );
                }
            }
        }

        true
    }
}

#[cfg(test)]
mod terminal_settlement_prune_tests {
    use super::*;
    use crate::domain::entities::ChatConversationId;

    fn running_info(last_active_at: Option<chrono::DateTime<chrono::Utc>>) -> RunningAgentInfo {
        RunningAgentInfo {
            pid: 12_345,
            conversation_id: "conversation-test".to_string(),
            agent_run_id: "run-test".to_string(),
            started_at: chrono::Utc::now(),
            worktree_path: None,
            cancellation_token: None,
            last_active_at,
            model: None,
        }
    }

    fn running_run() -> AgentRun {
        AgentRun::new(ChatConversationId::new())
    }

    #[test]
    fn terminal_settlement_prune_defers_recent_live_merge_status_mismatch() {
        let info = running_info(Some(chrono::Utc::now()));
        let run = running_run();

        assert!(should_defer_terminal_settlement_prune(
            Some(ChatContextType::Merge),
            &["task_status_mismatch"],
            &info,
            Some(&run),
            true,
        ));
    }

    #[test]
    fn terminal_settlement_prune_rejects_non_settlement_shapes() {
        let info = running_info(Some(chrono::Utc::now()));
        let missing_heartbeat = running_info(None);
        let run = running_run();

        assert!(!should_defer_terminal_settlement_prune(
            Some(ChatContextType::Merge),
            &["task_status_mismatch"],
            &info,
            Some(&run),
            false,
        ));
        assert!(!should_defer_terminal_settlement_prune(
            Some(ChatContextType::Merge),
            &["task_status_mismatch", "pid_missing"],
            &info,
            Some(&run),
            true,
        ));
        assert!(!should_defer_terminal_settlement_prune(
            Some(ChatContextType::TaskExecution),
            &["task_status_mismatch"],
            &info,
            Some(&run),
            true,
        ));
        assert!(!should_defer_terminal_settlement_prune(
            Some(ChatContextType::Review),
            &["task_status_mismatch"],
            &info,
            None,
            true,
        ));
        assert!(!should_defer_terminal_settlement_prune(
            Some(ChatContextType::Review),
            &["task_status_mismatch"],
            &missing_heartbeat,
            Some(&run),
            true,
        ));
    }
}
