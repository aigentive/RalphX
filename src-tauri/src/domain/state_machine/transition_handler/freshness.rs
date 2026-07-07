//! Branch freshness checks for execution and review entry points.
//!
//! Ensures both plan←source and task←feature branches are fresh before
//! an agent is spawned. On conflict, routes to Merging state for resolution.

// Callers in on_enter_states.rs and side_effects.rs are added in subsequent steps.

use std::path::Path;
use std::sync::Arc;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tracing::{info, warn};

use crate::application::git_service::git_cmd;
use crate::application::GitService;
use crate::domain::entities::{
    ActivityEvent, ActivityEventRole, ActivityEventType, Project, Task, TaskId,
};
use crate::domain::repositories::ActivityEventRepository;
use crate::infrastructure::agents::claude::ReconciliationConfig;

use super::merge_coordination::{
    update_plan_from_main, update_source_from_target, PlanUpdateResult, SourceUpdateResult,
};

/// Typed metadata for branch freshness conflict tracking.
///
/// Stored in/extracted from task metadata JSON. Using a struct provides
/// compile-time validation of field names — prevents typos and stale keys.
///
/// Lifecycle:
/// - Initialized: defaults (absent from metadata)
/// - Incremented: once per `ensure_branches_fresh()` call that routes to Merging
/// - Reset: when freshness check passes without conflicts (via `reset_conflict_state()`)
/// - Cap: 5 (auto-reset once with extended cooldown; second cap → ExecutionBlocked)
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct FreshnessMetadata {
    /// True when task was routed to Merging due to stale branches.
    #[serde(default)]
    pub branch_freshness_conflict: bool,

    /// The state from which the freshness conflict was detected.
    /// Values: "executing" | "re_executing" | "reviewing"
    #[serde(default)]
    pub freshness_origin_state: Option<String>,

    /// Number of times freshness routing has occurred for this task.
    /// Incremented once per `ensure_branches_fresh()` call (not per conflict within a call).
    /// Reset to 0 when freshness check passes without conflicts (via `reset_conflict_state()`).
    #[serde(default)]
    pub freshness_conflict_count: u32,

    /// True when the plan←source update had conflicts requiring merger agent resolution.
    #[serde(default)]
    pub plan_update_conflict: bool,

    /// True when the task←feature update had conflicts requiring merger agent resolution.
    #[serde(default)]
    pub source_update_conflict: bool,

    /// RFC3339 timestamp of the last successful freshness check.
    /// Used for skip-if-recently-checked optimization (default window: 30s).
    #[serde(default)]
    pub last_freshness_check_at: Option<String>,

    /// Files involved in the freshness conflict (from git conflict output).
    #[serde(default)]
    pub conflict_files: Vec<String>,

    /// The task/source branch that was being updated (task←feature direction).
    #[serde(default)]
    pub source_branch: Option<String>,

    /// The plan/target branch that was the merge target (task←feature direction).
    #[serde(default)]
    pub target_branch: Option<String>,

    /// Timestamp until which the reconciler should not re-queue this task.
    /// Set after a freshness conflict to implement exponential backoff.
    #[serde(default)]
    pub freshness_backoff_until: Option<DateTime<Utc>>,

    /// Number of times the auto-reset has been triggered after hitting the cap.
    /// 0 = never auto-reset; 1 = auto-reset once (second cap → ExecutionBlocked).
    #[serde(default)]
    pub freshness_auto_reset_count: u8,

    /// Signals which code path already incremented freshness_conflict_count.
    /// Set to Some("ensure_branches_fresh") by apply_freshness_result() when the
    /// normal freshness check path is used. Absent for the conflict marker scan path.
    /// Used by the BranchFreshnessConflict handler to avoid double-counting.
    #[serde(default)]
    pub freshness_count_incremented_by: Option<String>,
}

/// Scope of cleanup to perform on freshness metadata.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FreshnessCleanupScope {
    /// Clear only routing flags (branch_freshness_conflict, freshness_origin_state,
    /// plan_update_conflict, source_update_conflict, conflict_files, source_branch,
    /// target_branch). Preserves conflict count, backoff_until, and auto_reset_count.
    RoutingOnly,
    /// Clear conflict state (freshness_conflict_count=0, backoff_until=None,
    /// auto_reset_count=0). Does NOT clear routing flags.
    ConflictState,
    /// Full clear: all freshness keys removed from metadata JSON.
    Full,
}

impl FreshnessMetadata {
    /// All JSON keys managed by FreshnessMetadata.
    pub(crate) const KEYS: &'static [&'static str] = &[
        "branch_freshness_conflict",
        "freshness_origin_state",
        "freshness_conflict_count",
        "plan_update_conflict",
        "source_update_conflict",
        "last_freshness_check_at",
        "conflict_files",
        "source_branch",
        "target_branch",
        "freshness_backoff_until",
        "freshness_auto_reset_count",
        "freshness_count_incremented_by",
    ];

    /// Dispatch cleanup by scope.
    ///
    /// - `RoutingOnly` → clears routing flags, preserves conflict count/backoff/auto_reset_count
    /// - `ConflictState` → resets conflict state (count=0, backoff_until=None, auto_reset_count=0)
    /// - `Full` → removes all freshness keys from JSON
    pub fn cleanup(scope: FreshnessCleanupScope, meta: &mut Value) {
        match scope {
            FreshnessCleanupScope::RoutingOnly => {
                let mut freshness = Self::from_task_metadata(meta);
                freshness.clear_routing_flags();
                freshness.merge_into(meta);
            }
            FreshnessCleanupScope::ConflictState => {
                let mut freshness = Self::from_task_metadata(meta);
                freshness.reset_conflict_state();
                freshness.merge_into(meta);
            }
            FreshnessCleanupScope::Full => {
                Self::clear_from(meta);
            }
        }
    }

    /// Clear routing flags only.
    ///
    /// Clears: branch_freshness_conflict, freshness_origin_state, plan_update_conflict,
    /// source_update_conflict, conflict_files, source_branch, target_branch.
    ///
    /// Preserves: freshness_conflict_count, freshness_backoff_until, freshness_auto_reset_count.
    pub fn clear_routing_flags(&mut self) {
        self.branch_freshness_conflict = false;
        self.freshness_origin_state = None;
        self.plan_update_conflict = false;
        self.source_update_conflict = false;
        self.conflict_files = Vec::new();
        self.source_branch = None;
        self.target_branch = None;
        self.freshness_count_incremented_by = None;
    }

    /// Reset conflict state (count=0, backoff_until=None, auto_reset_count=0).
    ///
    /// Does NOT clear routing flags — use `clear_routing_flags()` for that.
    pub fn reset_conflict_state(&mut self) {
        self.freshness_conflict_count = 0;
        self.freshness_backoff_until = None;
        self.freshness_auto_reset_count = 0;
    }

    /// Extract FreshnessMetadata from task metadata JSON.
    /// Returns struct with defaults for any missing fields.
    pub fn from_task_metadata(metadata: &Value) -> Self {
        serde_json::from_value(metadata.clone()).unwrap_or_default()
    }

    /// Merge freshness fields back into task metadata JSON.
    /// Preserves existing non-freshness keys. Explicitly handles Option
    /// fields by removing keys when None.
    pub fn merge_into(&self, metadata: &mut Value) {
        let Some(obj) = metadata.as_object_mut() else {
            return;
        };

        obj.insert(
            "branch_freshness_conflict".to_owned(),
            Value::Bool(self.branch_freshness_conflict),
        );
        match &self.freshness_origin_state {
            Some(s) => obj.insert(
                "freshness_origin_state".to_owned(),
                Value::String(s.clone()),
            ),
            None => obj.remove("freshness_origin_state"),
        };
        obj.insert(
            "freshness_conflict_count".to_owned(),
            Value::Number(self.freshness_conflict_count.into()),
        );
        obj.insert(
            "plan_update_conflict".to_owned(),
            Value::Bool(self.plan_update_conflict),
        );
        obj.insert(
            "source_update_conflict".to_owned(),
            Value::Bool(self.source_update_conflict),
        );
        match &self.last_freshness_check_at {
            Some(s) => obj.insert(
                "last_freshness_check_at".to_owned(),
                Value::String(s.clone()),
            ),
            None => obj.remove("last_freshness_check_at"),
        };
        obj.insert(
            "conflict_files".to_owned(),
            Value::Array(
                self.conflict_files
                    .iter()
                    .map(|f| Value::String(f.clone()))
                    .collect(),
            ),
        );
        match &self.source_branch {
            Some(s) => obj.insert("source_branch".to_owned(), Value::String(s.clone())),
            None => obj.remove("source_branch"),
        };
        match &self.target_branch {
            Some(s) => obj.insert("target_branch".to_owned(), Value::String(s.clone())),
            None => obj.remove("target_branch"),
        };
        match &self.freshness_backoff_until {
            Some(dt) => obj.insert(
                "freshness_backoff_until".to_owned(),
                Value::String(dt.to_rfc3339()),
            ),
            None => obj.remove("freshness_backoff_until"),
        };
        obj.insert(
            "freshness_auto_reset_count".to_owned(),
            Value::Number(self.freshness_auto_reset_count.into()),
        );
        match &self.freshness_count_incremented_by {
            Some(s) => obj.insert(
                "freshness_count_incremented_by".to_owned(),
                Value::String(s.clone()),
            ),
            None => obj.remove("freshness_count_incremented_by"),
        };
    }

    /// Remove all freshness keys from task metadata JSON.
    ///
    /// Use when task completes or is fully cleaned up. For partial cleanup, use `cleanup(scope)`.
    pub fn clear_from(metadata: &mut Value) {
        if let Some(obj) = metadata.as_object_mut() {
            for key in Self::KEYS {
                obj.remove(*key);
            }
        }
    }

    /// Compute exponential backoff duration: min(base * 2^(count-1), max).
    /// Returns None if count is 0.
    pub fn compute_backoff(count: u32, base_secs: u64, max_secs: u64) -> Option<chrono::Duration> {
        if count == 0 {
            return None;
        }
        let exponent = count.saturating_sub(1);
        let multiplier = 1u64.checked_shl(exponent).unwrap_or(u64::MAX);
        let secs = base_secs.saturating_mul(multiplier).min(max_secs);
        Some(chrono::Duration::seconds(secs as i64))
    }

    /// Returns true if the task is currently in backoff (backoff_until is in the future).
    pub fn is_in_backoff(&self) -> bool {
        match self.freshness_backoff_until {
            Some(until) => Utc::now() < until,
            None => false,
        }
    }
}

/// Action returned by `ensure_branches_fresh()` when branches are not clean.
#[derive(Debug)]
pub enum FreshnessAction {
    /// Branch conflict detected — route to Merging with freshness metadata.
    RouteToMerging {
        conflict_files: Vec<String>,
        conflict_type: &'static str, // "plan_update" | "source_update"
        freshness_metadata: FreshnessMetadata,
    },
    /// Fatal error or retry cap exceeded — task should fail.
    ExecutionBlocked { reason: String },
}

/// Ensures both plan←source and task←feature branches are fresh.
///
/// Must be called BEFORE any agent process is spawned (before `send_message()`).
///
/// # Returns
/// - `Ok(updated_meta)` — both checks passed; caller should merge updated_meta into task metadata
/// - `Err(FreshnessAction::RouteToMerging)` — conflict; caller sets metadata + transitions to Merging
/// - `Err(FreshnessAction::ExecutionBlocked)` — timeout or retry cap exceeded
///
/// # Errors
/// Returns `Err(FreshnessAction)` when a conflict or execution-blocking condition is detected.
pub async fn ensure_branches_fresh(
    repo_path: &Path,
    task: &Task,
    project: &Project,
    task_id_str: &str,
    plan_branch: Option<&str>,
    plan_source_branch: Option<&str>,
    event_sink: Option<&dyn ralphx_events::EventSink>,
    activity_event_repo: Option<&Arc<dyn ActivityEventRepository>>,
    origin_state: &str,
    config: &ReconciliationConfig,
) -> Result<FreshnessMetadata, FreshnessAction> {
    // 1. Config toggle
    if !config.execution_freshness_enabled {
        info!(
            task_id = task_id_str,
            "Freshness check disabled via config (execution_freshness_enabled=false)"
        );
        return Ok(FreshnessMetadata::default());
    }

    // 2. Parse current freshness metadata
    let task_metadata_val: serde_json::Value = task
        .metadata
        .as_deref()
        .and_then(|s| serde_json::from_str(s).ok())
        .unwrap_or_else(|| serde_json::json!({}));
    let mut freshness = FreshnessMetadata::from_task_metadata(&task_metadata_val);

    // 3. Backoff check — skip re-queuing if still in cooldown window
    if freshness.is_in_backoff() {
        let until = freshness.freshness_backoff_until.expect("checked above");
        let remaining = (until - Utc::now()).num_seconds().max(0);
        warn!(
            task_id = task_id_str,
            backoff_until = %until.to_rfc3339(),
            remaining_secs = remaining,
            conflict_count = freshness.freshness_conflict_count,
            "Skipping freshness check — task in backoff window"
        );
        emit_freshness_activity(
            activity_event_repo,
            task_id_str,
            "branch_freshness_skipped",
            serde_json::json!({
                "reason": "backoff",
                "backoff_until": until.to_rfc3339(),
                "remaining_secs": remaining,
            }),
        )
        .await;
        return Ok(freshness);
    }

    // 4. Skip-if-recently-checked
    if let Some(ref last_check_str) = freshness.last_freshness_check_at.clone() {
        if let Ok(last_check) = last_check_str.parse::<DateTime<Utc>>() {
            let elapsed = Utc::now() - last_check;
            let skip_window = config.freshness_skip_window_secs as i64;
            if elapsed.num_seconds() < skip_window {
                info!(
                    task_id = task_id_str,
                    elapsed_secs = elapsed.num_seconds(),
                    skip_window_secs = skip_window,
                    "Skipping freshness check — last checked recently"
                );
                emit_freshness_activity(
                    activity_event_repo,
                    task_id_str,
                    "branch_freshness_skipped",
                    serde_json::json!({
                        "reason": "recently_checked",
                        "last_check_secs_ago": elapsed.num_seconds(),
                    }),
                )
                .await;
                return Ok(freshness);
            }
        }
    }

    // 5. Dirty worktree guard
    let fail_closed_on_worktree_status = matches!(origin_state, "executing" | "re_executing");
    match is_worktree_dirty(repo_path).await {
        Ok(true) => {
            warn!(
                task_id = task_id_str,
                "Dirty worktree detected before freshness check — attempting emergency auto-commit"
            );
            match GitService::commit_all_including_deletions(
                repo_path,
                "chore: auto-commit before freshness check",
            )
            .await
            {
                Ok(Some(sha)) => {
                    info!(
                        task_id = task_id_str,
                        sha = %sha,
                        "Emergency auto-commit succeeded"
                    );
                }
                Ok(None) => {
                    info!(
                        task_id = task_id_str,
                        "Emergency auto-commit: nothing to commit (race condition)"
                    );
                }
                Err(e) => {
                    if !fail_closed_on_worktree_status {
                        warn!(
                            task_id = task_id_str,
                            origin_state,
                            error = %e,
                            "Emergency auto-commit failed outside execution spawn path — skipping freshness check"
                        );
                        return Ok(freshness);
                    }
                    return Err(block_freshness_git_error(
                        activity_event_repo,
                        task_id_str,
                        "dirty_worktree_autocommit_failed",
                        "dirty_worktree_autocommit",
                        format!("Emergency auto-commit failed before freshness check: {}", e),
                    )
                    .await);
                }
            }
        }
        Ok(false) => {}
        Err(e) => {
            if !fail_closed_on_worktree_status {
                warn!(
                    task_id = task_id_str,
                    origin_state,
                    error = %e,
                    "Failed to check worktree status outside execution spawn path — skipping freshness check"
                );
                return Ok(freshness);
            }
            return Err(block_freshness_git_error(
                activity_event_repo,
                task_id_str,
                "worktree_status_unreadable",
                "worktree_status",
                format!(
                    "Failed to check worktree status before freshness check: {}",
                    e
                ),
            )
            .await);
        }
    }

    let freshness_timeout = std::time::Duration::from_secs(config.branch_freshness_timeout_secs);

    let base_branch =
        plan_source_branch.unwrap_or_else(|| project.base_branch.as_deref().unwrap_or("main"));

    // 6. Plan freshness check (plan←source branch)
    if let Some(plan_branch_name) = plan_branch {
        // Heap-allocate the large update future to avoid overflowing tokio worker stacks
        // when startup reconciliation inlines deep async chains.
        let plan_result = tokio::time::timeout(
            freshness_timeout,
            Box::pin(update_plan_from_main(
                repo_path,
                plan_branch_name,
                base_branch,
                project,
                task_id_str,
                event_sink,
            )),
        )
        .await;

        match plan_result {
            Err(_timeout) => {
                emit_freshness_activity(
                    activity_event_repo,
                    task_id_str,
                    "branch_freshness_blocked",
                    serde_json::json!({
                        "reason": "timeout",
                        "check": "plan_update",
                        "conflict_count": freshness.freshness_conflict_count,
                    }),
                )
                .await;
                return Err(FreshnessAction::ExecutionBlocked {
                    reason: format!(
                        "update_plan_from_main timed out after {}s",
                        config.branch_freshness_timeout_secs
                    ),
                });
            }
            Ok(PlanUpdateResult::Conflicts { conflict_files }) => {
                // Single-increment per ensure_branches_fresh() call
                freshness.freshness_conflict_count += 1;

                let conflict_files_str: Vec<String> = conflict_files
                    .iter()
                    .map(|p| p.to_string_lossy().into_owned())
                    .collect();

                // Auto-recovery at cap (or block on second cap)
                if let Some(action) = handle_cap_if_needed(
                    &mut freshness,
                    &conflict_files_str,
                    task_id_str,
                    activity_event_repo,
                    config,
                )
                .await
                {
                    return Err(action);
                }

                // Set exponential backoff for next attempt
                set_backoff(&mut freshness, config);

                freshness.branch_freshness_conflict = true;
                freshness.freshness_origin_state = Some(origin_state.to_string());
                freshness.plan_update_conflict = true;
                freshness.source_update_conflict = false;
                freshness.conflict_files = conflict_files_str.clone();
                freshness.source_branch = task.task_branch.clone();
                freshness.target_branch = Some(plan_branch_name.to_string());

                emit_freshness_activity(
                    activity_event_repo,
                    task_id_str,
                    "branch_freshness_conflict",
                    serde_json::json!({
                        "conflict_type": "plan_update",
                        "conflict_files": conflict_files_str,
                        "retry_count": freshness.freshness_conflict_count,
                        "origin_state": origin_state,
                    }),
                )
                .await;

                return Err(FreshnessAction::RouteToMerging {
                    conflict_files: conflict_files_str,
                    conflict_type: "plan_update",
                    freshness_metadata: freshness,
                });
            }
            Ok(PlanUpdateResult::Error(e)) => {
                warn!(
                    task_id = task_id_str,
                    error = %e,
                    "update_plan_from_main failed — retrying once before blocking execution"
                );
                let retry_result = tokio::time::timeout(
                    freshness_timeout,
                    Box::pin(update_plan_from_main(
                        repo_path,
                        plan_branch_name,
                        base_branch,
                        project,
                        task_id_str,
                        event_sink,
                    )),
                )
                .await;
                if let FreshnessRetryDecision::Block { reason } = plan_retry_decision_after_error(
                    retry_result.ok(),
                    config.branch_freshness_timeout_secs,
                ) {
                    return Err(block_freshness_update_error(
                        activity_event_repo,
                        task_id_str,
                        "plan_update",
                        reason,
                    )
                    .await);
                }
            }
            Ok(
                PlanUpdateResult::AlreadyUpToDate
                | PlanUpdateResult::Updated
                | PlanUpdateResult::NotPlanBranch,
            ) => {
                // Plan is fresh (or not applicable) — continue to source check
            }
        }
    }

    // 7. Source freshness check (task←plan)
    let source_branch = task.task_branch.as_deref().unwrap_or("");
    let target_branch = plan_branch.unwrap_or(base_branch);

    if source_branch.is_empty() {
        // No task branch assigned yet — skip source check
        info!(
            task_id = task_id_str,
            "No task branch set — skipping source freshness check"
        );
    } else {
        // Heap-allocate the large update future to avoid overflowing tokio worker stacks
        // when startup reconciliation inlines deep async chains.
        let source_result = tokio::time::timeout(
            freshness_timeout,
            Box::pin(update_source_from_target(
                repo_path,
                source_branch,
                target_branch,
                project,
                task_id_str,
                event_sink,
            )),
        )
        .await;

        match source_result {
            Err(_timeout) => {
                emit_freshness_activity(
                    activity_event_repo,
                    task_id_str,
                    "branch_freshness_blocked",
                    serde_json::json!({
                        "reason": "timeout",
                        "check": "source_update",
                        "conflict_count": freshness.freshness_conflict_count,
                    }),
                )
                .await;
                return Err(FreshnessAction::ExecutionBlocked {
                    reason: format!(
                        "update_source_from_target timed out after {}s",
                        config.branch_freshness_timeout_secs
                    ),
                });
            }
            Ok(SourceUpdateResult::Conflicts { conflict_files }) => {
                // Single-increment per ensure_branches_fresh() call
                freshness.freshness_conflict_count += 1;

                let conflict_files_str: Vec<String> = conflict_files
                    .iter()
                    .map(|p| p.to_string_lossy().into_owned())
                    .collect();

                // Auto-recovery at cap (or block on second cap)
                if let Some(action) = handle_cap_if_needed(
                    &mut freshness,
                    &conflict_files_str,
                    task_id_str,
                    activity_event_repo,
                    config,
                )
                .await
                {
                    return Err(action);
                }

                // Set exponential backoff for next attempt
                set_backoff(&mut freshness, config);

                freshness.branch_freshness_conflict = true;
                freshness.freshness_origin_state = Some(origin_state.to_string());
                freshness.plan_update_conflict = false;
                freshness.source_update_conflict = true;
                freshness.conflict_files = conflict_files_str.clone();
                freshness.source_branch = Some(source_branch.to_string());
                freshness.target_branch = Some(target_branch.to_string());

                emit_freshness_activity(
                    activity_event_repo,
                    task_id_str,
                    "branch_freshness_conflict",
                    serde_json::json!({
                        "conflict_type": "source_update",
                        "conflict_files": conflict_files_str,
                        "retry_count": freshness.freshness_conflict_count,
                        "origin_state": origin_state,
                    }),
                )
                .await;

                return Err(FreshnessAction::RouteToMerging {
                    conflict_files: conflict_files_str,
                    conflict_type: "source_update",
                    freshness_metadata: freshness,
                });
            }
            Ok(SourceUpdateResult::Error(e)) => {
                warn!(
                    task_id = task_id_str,
                    error = %e,
                    "update_source_from_target failed — retrying once before blocking execution"
                );
                let retry_result = tokio::time::timeout(
                    freshness_timeout,
                    Box::pin(update_source_from_target(
                        repo_path,
                        source_branch,
                        target_branch,
                        project,
                        task_id_str,
                        event_sink,
                    )),
                )
                .await;
                if let FreshnessRetryDecision::Block { reason } = source_retry_decision_after_error(
                    retry_result.ok(),
                    config.branch_freshness_timeout_secs,
                ) {
                    return Err(block_freshness_update_error(
                        activity_event_repo,
                        task_id_str,
                        "source_update",
                        reason,
                    )
                    .await);
                }
            }
            Ok(SourceUpdateResult::AlreadyUpToDate | SourceUpdateResult::Updated) => {
                // Source is fresh — continue
            }
        }
    }

    // 8. Both checks passed — update timestamp and reset conflict state
    info!(
        task_id = task_id_str,
        origin_state = origin_state,
        "Freshness checks passed — branches are up-to-date"
    );
    freshness.last_freshness_check_at = Some(Utc::now().to_rfc3339());
    freshness.branch_freshness_conflict = false;
    freshness.reset_conflict_state();
    freshness.clear_routing_flags(); // clear stale routing flags after successful freshness check

    emit_freshness_activity(
        activity_event_repo,
        task_id_str,
        "branch_freshness_checked",
        serde_json::json!({ "status": "passed" }),
    )
    .await;

    Ok(freshness)
}

#[derive(Debug, PartialEq, Eq)]
enum FreshnessRetryDecision {
    Continue,
    Block { reason: String },
}

fn plan_retry_decision_after_error(
    retry_result: Option<PlanUpdateResult>,
    timeout_secs: u64,
) -> FreshnessRetryDecision {
    match retry_result {
        Some(
            PlanUpdateResult::AlreadyUpToDate
            | PlanUpdateResult::Updated
            | PlanUpdateResult::NotPlanBranch,
        ) => FreshnessRetryDecision::Continue,
        Some(PlanUpdateResult::Conflicts { conflict_files }) => FreshnessRetryDecision::Block {
            reason: format!(
                "update_plan_from_main returned conflicts after retry following error: {:?}",
                conflict_files
            ),
        },
        Some(PlanUpdateResult::Error(retry_error)) => FreshnessRetryDecision::Block {
            reason: format!("update_plan_from_main failed after retry: {}", retry_error),
        },
        None => FreshnessRetryDecision::Block {
            reason: format!("update_plan_from_main retry timed out after {timeout_secs}s"),
        },
    }
}

fn source_retry_decision_after_error(
    retry_result: Option<SourceUpdateResult>,
    timeout_secs: u64,
) -> FreshnessRetryDecision {
    match retry_result {
        Some(SourceUpdateResult::AlreadyUpToDate | SourceUpdateResult::Updated) => {
            FreshnessRetryDecision::Continue
        }
        Some(SourceUpdateResult::Conflicts { conflict_files }) => FreshnessRetryDecision::Block {
            reason: format!(
                "update_source_from_target returned conflicts after retry following error: {:?}",
                conflict_files
            ),
        },
        Some(SourceUpdateResult::Error(retry_error)) => FreshnessRetryDecision::Block {
            reason: format!(
                "update_source_from_target failed after retry: {}",
                retry_error
            ),
        },
        None => FreshnessRetryDecision::Block {
            reason: format!("update_source_from_target retry timed out after {timeout_secs}s"),
        },
    }
}

async fn block_freshness_update_error(
    activity_event_repo: Option<&Arc<dyn ActivityEventRepository>>,
    task_id_str: &str,
    check: &'static str,
    reason: String,
) -> FreshnessAction {
    block_freshness_git_error(
        activity_event_repo,
        task_id_str,
        "update_error_after_retry",
        check,
        reason,
    )
    .await
}

async fn block_freshness_git_error(
    activity_event_repo: Option<&Arc<dyn ActivityEventRepository>>,
    task_id_str: &str,
    reason_code: &'static str,
    check: &'static str,
    reason: String,
) -> FreshnessAction {
    warn!(
        task_id = task_id_str,
        check,
        reason = %reason,
        reason_code,
        "Freshness git error blocks execution"
    );
    emit_freshness_activity(
        activity_event_repo,
        task_id_str,
        "branch_freshness_blocked",
        serde_json::json!({
            "reason": reason_code,
            "check": check,
            "message": reason,
        }),
    )
    .await;
    FreshnessAction::ExecutionBlocked { reason }
}

/// Set exponential backoff on the freshness metadata after a conflict.
/// backoff = min(base * 2^(count-1), max)
fn set_backoff(freshness: &mut FreshnessMetadata, config: &ReconciliationConfig) {
    if let Some(duration) = FreshnessMetadata::compute_backoff(
        freshness.freshness_conflict_count,
        config.freshness_backoff_base_secs,
        config.freshness_backoff_max_secs,
    ) {
        freshness.freshness_backoff_until = Some(Utc::now() + duration);
    }
}

/// Handle auto-recovery or block when retry cap is exceeded.
///
/// Returns `Some(FreshnessAction::ExecutionBlocked)` if the task should be blocked,
/// or `None` if the call should continue (auto-reset occurred or cap not reached).
async fn handle_cap_if_needed(
    freshness: &mut FreshnessMetadata,
    conflict_files: &[String],
    task_id_str: &str,
    activity_event_repo: Option<&Arc<dyn ActivityEventRepository>>,
    config: &ReconciliationConfig,
) -> Option<FreshnessAction> {
    if freshness.freshness_conflict_count <= config.freshness_max_conflict_retries {
        return None;
    }

    let total = freshness.freshness_conflict_count;
    let files = conflict_files.join(", ");

    if freshness.freshness_auto_reset_count == 0 {
        // First cap: auto-reset with extended cooldown
        let cooldown_secs = config.freshness_auto_reset_cooldown_secs;
        let cooldown_minutes = cooldown_secs / 60;
        freshness.freshness_conflict_count = 0;
        freshness.freshness_backoff_until =
            Some(Utc::now() + chrono::Duration::seconds(cooldown_secs as i64));
        freshness.freshness_auto_reset_count = 1;

        warn!(
            task_id = task_id_str,
            total_conflicts = total,
            cooldown_minutes = cooldown_minutes,
            conflict_files = %files,
            "Freshness conflict cap reached — auto-resetting with extended cooldown"
        );

        emit_freshness_activity(
            activity_event_repo,
            task_id_str,
            "branch_freshness_auto_reset",
            serde_json::json!({
                "total_conflicts": total,
                "cooldown_minutes": cooldown_minutes,
                "conflict_files": conflict_files,
                "auto_reset_count": 1,
            }),
        )
        .await;

        // Return None: after auto-reset, caller proceeds to RouteToMerging with reset state
        // The backoff_until we set above will prevent immediate re-queuing
        None
    } else {
        // Second cap: ExecutionBlocked
        let minutes = config.freshness_auto_reset_cooldown_secs / 60;
        let msg = format!(
            "FRESHNESS_BLOCKED|{}|{}|{}|Persistent freshness conflicts after auto-reset",
            total, minutes, files
        );

        warn!(
            task_id = task_id_str,
            total_conflicts = total,
            conflict_files = %files,
            "Freshness conflict cap exceeded after auto-reset — blocking execution"
        );

        emit_freshness_activity(
            activity_event_repo,
            task_id_str,
            "branch_freshness_blocked",
            serde_json::json!({
                "reason": "retry_cap_exceeded_after_auto_reset",
                "conflict_count": total,
                "max_retries": config.freshness_max_conflict_retries,
                "conflict_files": conflict_files,
            }),
        )
        .await;

        Some(FreshnessAction::ExecutionBlocked { reason: msg })
    }
}

/// Returns true if the git worktree has uncommitted changes.
async fn is_worktree_dirty(path: &Path) -> Result<bool, String> {
    match git_cmd::run(&["status", "--porcelain", "-z"], path).await {
        Ok(output) => Ok(!output.stdout.is_empty()),
        Err(e) => {
            warn!(path = %path.display(), error = %e, "Failed to check worktree status");
            Err(e.to_string())
        }
    }
}

/// Emit a freshness-related activity event. Non-fatal: logs warning on failure.
async fn emit_freshness_activity(
    activity_event_repo: Option<&Arc<dyn ActivityEventRepository>>,
    task_id_str: &str,
    event_kind: &str,
    metadata: serde_json::Value,
) {
    let Some(repo) = activity_event_repo else {
        return;
    };
    let tid = TaskId::from_string(task_id_str.to_string());
    let content = match event_kind {
        "branch_freshness_checked" => "Branch freshness check passed".to_string(),
        "branch_freshness_conflict" => format!(
            "Branch freshness conflict detected ({})",
            metadata
                .get("conflict_type")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown")
        ),
        "branch_freshness_skipped" => {
            "Branch freshness check skipped (recently checked)".to_string()
        }
        "branch_freshness_blocked" => format!(
            "Branch freshness blocked: {}",
            metadata
                .get("reason")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown")
        ),
        "branch_freshness_auto_reset" => "Branch freshness auto-reset after cap".to_string(),
        _ => event_kind.to_string(),
    };
    let metadata_str = serde_json::json!({
        "event_kind": event_kind,
        "details": metadata,
    })
    .to_string();
    let event = ActivityEvent::new_task_event(tid, ActivityEventType::System, content)
        .with_role(ActivityEventRole::System)
        .with_metadata(metadata_str);
    if let Err(e) = repo.save(event).await {
        tracing::warn!(
            task_id = task_id_str,
            event_kind = event_kind,
            error = %e,
            "Failed to save freshness activity event (non-fatal)"
        );
    }
}

#[cfg(test)]
mod field_sync_tests {
    use super::*;

    /// Verify that KEYS contains exactly the fields in FreshnessMetadata.
    /// If this test fails, KEYS is out of sync with the struct fields.
    #[test]
    fn keys_matches_struct_fields() {
        // Use a fully-populated instance (all Options set to Some) to ensure all keys appear.
        let meta = FreshnessMetadata {
            branch_freshness_conflict: true,
            freshness_origin_state: Some("executing".to_string()),
            freshness_conflict_count: 1,
            plan_update_conflict: true,
            source_update_conflict: false,
            last_freshness_check_at: Some("2026-01-01T00:00:00Z".to_string()),
            conflict_files: vec!["foo.rs".to_string()],
            source_branch: Some("task/foo".to_string()),
            target_branch: Some("plan/foo".to_string()),
            freshness_backoff_until: Some(Utc::now()),
            freshness_auto_reset_count: 0,
            freshness_count_incremented_by: Some("ensure_branches_fresh".to_string()),
        };
        let mut json = serde_json::json!({});
        meta.merge_into(&mut json);
        let obj = json.as_object().unwrap();

        // Every KEYS entry should appear in merge_into() output (with Some values)
        for key in FreshnessMetadata::KEYS {
            assert!(
                obj.contains_key(*key),
                "KEYS entry '{key}' not found in merge_into() output — KEYS is out of sync"
            );
        }

        // Field count: update this when adding fields to FreshnessMetadata
        assert_eq!(
            FreshnessMetadata::KEYS.len(),
            12,
            "KEYS length mismatch — update this assertion when adding fields"
        );
    }

    #[test]
    fn compute_backoff_exponential() {
        // count=1: base * 2^0 = base = 60
        let d = FreshnessMetadata::compute_backoff(1, 60, 600).unwrap();
        assert_eq!(d.num_seconds(), 60);

        // count=2: base * 2^1 = 120
        let d = FreshnessMetadata::compute_backoff(2, 60, 600).unwrap();
        assert_eq!(d.num_seconds(), 120);

        // count=4: base * 2^3 = 480
        let d = FreshnessMetadata::compute_backoff(4, 60, 600).unwrap();
        assert_eq!(d.num_seconds(), 480);

        // count=5: base * 2^4 = 960 → capped at 600
        let d = FreshnessMetadata::compute_backoff(5, 60, 600).unwrap();
        assert_eq!(d.num_seconds(), 600);

        // count=0: None
        assert!(FreshnessMetadata::compute_backoff(0, 60, 600).is_none());
    }

    #[test]
    fn clear_routing_flags_preserves_conflict_state() {
        let mut meta = FreshnessMetadata {
            branch_freshness_conflict: true,
            freshness_origin_state: Some("executing".to_string()),
            freshness_conflict_count: 3,
            plan_update_conflict: true,
            source_update_conflict: false,
            conflict_files: vec!["foo.rs".to_string()],
            source_branch: Some("task/foo".to_string()),
            target_branch: Some("plan/foo".to_string()),
            freshness_backoff_until: Some(Utc::now() + chrono::Duration::seconds(60)),
            freshness_auto_reset_count: 1,
            last_freshness_check_at: None,
            freshness_count_incremented_by: Some("ensure_branches_fresh".to_string()),
        };
        meta.clear_routing_flags();

        assert!(!meta.branch_freshness_conflict);
        assert!(meta.freshness_origin_state.is_none());
        assert!(!meta.plan_update_conflict);
        assert!(!meta.source_update_conflict);
        assert!(meta.conflict_files.is_empty());
        assert!(meta.source_branch.is_none());
        assert!(meta.target_branch.is_none());
        assert!(meta.freshness_count_incremented_by.is_none());
        // Preserved:
        assert_eq!(meta.freshness_conflict_count, 3);
        assert!(meta.freshness_backoff_until.is_some());
        assert_eq!(meta.freshness_auto_reset_count, 1);
    }

    #[test]
    fn reset_conflict_state_clears_count_and_backoff() {
        let mut meta = FreshnessMetadata {
            freshness_conflict_count: 5,
            freshness_backoff_until: Some(Utc::now() + chrono::Duration::seconds(60)),
            freshness_auto_reset_count: 1,
            branch_freshness_conflict: true,
            ..Default::default()
        };
        meta.reset_conflict_state();

        assert_eq!(meta.freshness_conflict_count, 0);
        assert!(meta.freshness_backoff_until.is_none());
        assert_eq!(meta.freshness_auto_reset_count, 0);
        // Routing flags NOT cleared:
        assert!(meta.branch_freshness_conflict);
    }

    #[test]
    fn plan_retry_decision_after_error_covers_all_outcomes() {
        assert_eq!(
            plan_retry_decision_after_error(Some(PlanUpdateResult::AlreadyUpToDate), 7),
            FreshnessRetryDecision::Continue
        );
        assert_eq!(
            plan_retry_decision_after_error(Some(PlanUpdateResult::Updated), 7),
            FreshnessRetryDecision::Continue
        );
        assert_eq!(
            plan_retry_decision_after_error(Some(PlanUpdateResult::NotPlanBranch), 7),
            FreshnessRetryDecision::Continue
        );

        let conflict = plan_retry_decision_after_error(
            Some(PlanUpdateResult::Conflicts {
                conflict_files: vec![std::path::PathBuf::from("src/lib.rs")],
            }),
            7,
        );
        assert!(
            matches!(conflict, FreshnessRetryDecision::Block { ref reason } if reason.contains("src/lib.rs")),
            "conflict retry must block with conflict file context: {conflict:?}"
        );

        assert_eq!(
            plan_retry_decision_after_error(Some(PlanUpdateResult::Error("boom".into())), 7),
            FreshnessRetryDecision::Block {
                reason: "update_plan_from_main failed after retry: boom".to_string()
            }
        );
        assert_eq!(
            plan_retry_decision_after_error(None, 7),
            FreshnessRetryDecision::Block {
                reason: "update_plan_from_main retry timed out after 7s".to_string()
            }
        );
    }

    #[test]
    fn source_retry_decision_after_error_covers_all_outcomes() {
        assert_eq!(
            source_retry_decision_after_error(Some(SourceUpdateResult::AlreadyUpToDate), 11),
            FreshnessRetryDecision::Continue
        );
        assert_eq!(
            source_retry_decision_after_error(Some(SourceUpdateResult::Updated), 11),
            FreshnessRetryDecision::Continue
        );

        let conflict = source_retry_decision_after_error(
            Some(SourceUpdateResult::Conflicts {
                conflict_files: vec![std::path::PathBuf::from("src/main.rs")],
            }),
            11,
        );
        assert!(
            matches!(conflict, FreshnessRetryDecision::Block { ref reason } if reason.contains("src/main.rs")),
            "source conflict retry must block with conflict file context: {conflict:?}"
        );

        assert_eq!(
            source_retry_decision_after_error(Some(SourceUpdateResult::Error("again".into())), 11),
            FreshnessRetryDecision::Block {
                reason: "update_source_from_target failed after retry: again".to_string()
            }
        );
        assert_eq!(
            source_retry_decision_after_error(None, 11),
            FreshnessRetryDecision::Block {
                reason: "update_source_from_target retry timed out after 11s".to_string()
            }
        );
    }

    #[tokio::test]
    async fn block_freshness_update_error_returns_execution_blocked_action() {
        let action =
            block_freshness_update_error(None, "task-1", "plan_update", "retry failed".to_string())
                .await;

        assert!(
            matches!(action, FreshnessAction::ExecutionBlocked { ref reason } if reason == "retry failed"),
            "freshness update errors must block execution after retry: {action:?}"
        );
    }
}
