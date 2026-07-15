//! Branch freshness checks for execution and review entry points.
//!
//! Ensures both plan←source and task←feature branches are fresh before
//! an agent is spawned. Stale branches activate a dedicated update operation.

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

use super::merge_coordination::{PlanUpdateResult, SourceUpdateResult};

pub(super) async fn observe_plan_freshness(
    repo_path: &Path,
    plan_branch_name: &str,
    base_branch: &str,
    _project: &Project,
    _task_id_str: &str,
    _event_sink: Option<&dyn ralphx_events::EventSink>,
) -> PlanUpdateResult {
    if let Err(error) = GitService::resolve_ref_sha(repo_path, base_branch).await {
        return PlanUpdateResult::Error(format!(
            "failed to resolve plan update source {base_branch}: {error}"
        ));
    }
    if let Err(error) = GitService::resolve_ref_sha(repo_path, plan_branch_name).await {
        return PlanUpdateResult::Error(format!(
            "failed to resolve plan update target {plan_branch_name}: {error}"
        ));
    }
    match GitService::is_ancestor(repo_path, base_branch, plan_branch_name).await {
        Ok(true) => PlanUpdateResult::AlreadyUpToDate,
        Ok(false) => PlanUpdateResult::Conflicts {
            conflict_files: Vec::new(),
        },
        Err(error) => PlanUpdateResult::Error(error.to_string()),
    }
}

async fn observe_source_freshness(
    repo_path: &Path,
    source_branch: &str,
    target_branch: &str,
) -> SourceUpdateResult {
    if GitService::resolve_ref_sha(repo_path, target_branch)
        .await
        .is_err()
    {
        return SourceUpdateResult::BranchMissing {
            branch: target_branch.to_string(),
        };
    }
    if GitService::resolve_ref_sha(repo_path, source_branch)
        .await
        .is_err()
    {
        return SourceUpdateResult::BranchMissing {
            branch: source_branch.to_string(),
        };
    }
    match GitService::is_ancestor(repo_path, target_branch, source_branch).await {
        Ok(true) => SourceUpdateResult::AlreadyUpToDate,
        Ok(false) => SourceUpdateResult::Conflicts {
            conflict_files: Vec::new(),
        },
        Err(error) => SourceUpdateResult::Error(error.to_string()),
    }
}

/// Typed metadata for branch freshness conflict tracking.
///
/// Stored in/extracted from task metadata JSON. Using a struct provides
/// compile-time validation of field names — prevents typos and stale keys.
///
/// Lifecycle:
/// - Initialized: defaults (absent from metadata)
/// - Incremented: once per `ensure_branches_fresh()` call that activates an update
/// - Reset: when freshness check passes without conflicts (via `reset_conflict_state()`)
/// - Cap: 5 (auto-reset once with extended cooldown; second cap → ExecutionBlocked)
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct FreshnessMetadata {
    /// True when a dedicated update was activated due to stale branches.
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

    /// Last durable plan-branch update receipt time.
    #[serde(default)]
    pub last_plan_freshness_check_at: Option<String>,

    /// Last durable task-branch update receipt time.
    #[serde(default)]
    pub last_task_freshness_check_at: Option<String>,

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
        "last_plan_freshness_check_at",
        "last_task_freshness_check_at",
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
        match &self.last_plan_freshness_check_at {
            Some(value) => obj.insert(
                "last_plan_freshness_check_at".to_owned(),
                Value::String(value.clone()),
            ),
            None => obj.remove("last_plan_freshness_check_at"),
        };
        match &self.last_task_freshness_check_at {
            Some(value) => obj.insert(
                "last_task_freshness_check_at".to_owned(),
                Value::String(value.clone()),
            ),
            None => obj.remove("last_task_freshness_check_at"),
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
    /// Stale branch detected — activate a dedicated update with freshness metadata.
    RouteToBranchUpdate {
        conflict_files: Vec<String>,
        conflict_type: &'static str, // "plan_update" | "source_update"
        freshness_metadata: Box<FreshnessMetadata>,
    },
    /// Fatal error or retry cap exceeded — task should fail.
    ExecutionBlocked {
        reason: String,
        branch_missing: Option<String>,
    },
}

fn record_freshness_conflict(
    mut freshness: FreshnessMetadata,
    config: &ReconciliationConfig,
) -> Result<FreshnessMetadata, String> {
    freshness.freshness_conflict_count = freshness.freshness_conflict_count.saturating_add(1);

    if freshness.freshness_conflict_count > config.freshness_max_conflict_retries {
        if freshness.freshness_auto_reset_count == 0 {
            freshness.freshness_conflict_count = 0;
            freshness.freshness_auto_reset_count = 1;
            freshness.freshness_backoff_until = Some(
                Utc::now()
                    + chrono::Duration::seconds(config.freshness_auto_reset_cooldown_secs as i64),
            );
            return Ok(freshness);
        }

        return Err(format!(
            "Branch freshness conflict retry cap exceeded after {} attempts",
            config.freshness_max_conflict_retries
        ));
    }

    freshness.freshness_backoff_until = FreshnessMetadata::compute_backoff(
        freshness.freshness_conflict_count,
        config.freshness_backoff_base_secs,
        config.freshness_backoff_max_secs,
    )
    .map(|duration| Utc::now() + duration);

    Ok(freshness)
}

/// Ensures both plan←source and task←feature branches are fresh.
///
/// Must be called BEFORE any agent process is spawned (before `send_message()`).
///
/// # Returns
/// - `Ok(updated_meta)` — both checks passed; caller should merge updated_meta into task metadata
/// - `Err(FreshnessAction::RouteToBranchUpdate)` — conflict; caller creates a dedicated update operation
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
    match is_worktree_dirty(repo_path).await {
        Ok(true)
            if super::automatic_commit_policy::protects_primary_checkout(project, repo_path) =>
        {
            info!(
                task_id = task_id_str,
                reason = "pr_mode_primary_checkout_protected",
                "Skipping emergency auto-commit for the PR-mode primary checkout"
            );
        }
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
                    let error = e.to_string();
                    if let FreshnessWorktreeGuardDecision::Block {
                        reason_code,
                        check,
                        reason,
                    } = dirty_worktree_autocommit_error_decision(origin_state, &error)
                    {
                        return Err(block_freshness_git_error(
                            activity_event_repo,
                            task_id_str,
                            reason_code,
                            check,
                            reason,
                            None,
                        )
                        .await);
                    } else {
                        warn!(
                            task_id = task_id_str,
                            origin_state,
                            error = %error,
                            "Emergency auto-commit failed outside execution spawn path — skipping freshness check"
                        );
                        return Ok(freshness);
                    }
                }
            }
        }
        Ok(false) => {}
        Err(e) => {
            if let FreshnessWorktreeGuardDecision::Block {
                reason_code,
                check,
                reason,
            } = worktree_status_error_decision(origin_state, &e)
            {
                return Err(block_freshness_git_error(
                    activity_event_repo,
                    task_id_str,
                    reason_code,
                    check,
                    reason,
                    None,
                )
                .await);
            } else {
                warn!(
                    task_id = task_id_str,
                    origin_state,
                    error = %e,
                    "Failed to check worktree status outside execution spawn path — skipping freshness check"
                );
                return Ok(freshness);
            }
        }
    }

    let freshness_timeout = std::time::Duration::from_secs(config.branch_freshness_timeout_secs);

    let base_branch =
        plan_source_branch.unwrap_or_else(|| project.base_branch.as_deref().unwrap_or("main"));

    // 6. Plan freshness check (plan←source branch)
    if let Some(plan_branch_name) = plan_branch.filter(|_| {
        !freshness_timestamp_is_recent(
            freshness.last_plan_freshness_check_at.as_deref(),
            config.freshness_skip_window_secs,
        )
    }) {
        // Heap-allocate the large update future to avoid overflowing tokio worker stacks
        // when startup reconciliation inlines deep async chains.
        let plan_result = tokio::time::timeout(
            freshness_timeout,
            Box::pin(observe_plan_freshness(
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
                    branch_missing: None,
                });
            }
            Ok(PlanUpdateResult::Conflicts { conflict_files }) => {
                let conflict_files_str: Vec<String> = conflict_files
                    .iter()
                    .map(|p| p.to_string_lossy().into_owned())
                    .collect();

                freshness.branch_freshness_conflict = true;
                freshness.freshness_origin_state = Some(origin_state.to_string());
                freshness.plan_update_conflict = true;
                freshness.source_update_conflict = false;
                freshness.conflict_files = conflict_files_str.clone();
                freshness.source_branch = Some(base_branch.to_string());
                freshness.target_branch = Some(plan_branch_name.to_string());

                freshness = match record_freshness_conflict(freshness, config) {
                    Ok(freshness) => freshness,
                    Err(reason) => {
                        return Err(block_freshness_update_error(
                            activity_event_repo,
                            task_id_str,
                            "plan_update",
                            reason,
                            None,
                        )
                        .await);
                    }
                };

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

                return Err(FreshnessAction::RouteToBranchUpdate {
                    conflict_files: conflict_files_str,
                    conflict_type: "plan_update",
                    freshness_metadata: Box::new(freshness),
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
                    Box::pin(observe_plan_freshness(
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
                        None,
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
    } else if freshness_timestamp_is_recent(
        freshness.last_task_freshness_check_at.as_deref(),
        config.freshness_skip_window_secs,
    ) {
        info!(
            task_id = task_id_str,
            "Task branch update receipt is still fresh"
        );
    } else {
        // Heap-allocate the large update future to avoid overflowing tokio worker stacks
        // when startup reconciliation inlines deep async chains.
        let source_result = tokio::time::timeout(
            freshness_timeout,
            Box::pin(observe_source_freshness(
                repo_path,
                source_branch,
                target_branch,
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
                    branch_missing: None,
                });
            }
            Ok(SourceUpdateResult::Conflicts { conflict_files }) => {
                let conflict_files_str: Vec<String> = conflict_files
                    .iter()
                    .map(|p| p.to_string_lossy().into_owned())
                    .collect();

                freshness.branch_freshness_conflict = true;
                freshness.freshness_origin_state = Some(origin_state.to_string());
                freshness.plan_update_conflict = false;
                freshness.source_update_conflict = true;
                freshness.conflict_files = conflict_files_str.clone();
                freshness.source_branch = Some(source_branch.to_string());
                freshness.target_branch = Some(target_branch.to_string());

                freshness = match record_freshness_conflict(freshness, config) {
                    Ok(freshness) => freshness,
                    Err(reason) => {
                        return Err(block_freshness_update_error(
                            activity_event_repo,
                            task_id_str,
                            "source_update",
                            reason,
                            None,
                        )
                        .await);
                    }
                };

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

                return Err(FreshnessAction::RouteToBranchUpdate {
                    conflict_files: conflict_files_str,
                    conflict_type: "source_update",
                    freshness_metadata: Box::new(freshness),
                });
            }
            Ok(SourceUpdateResult::BranchMissing { branch }) => {
                return Err(block_freshness_update_error(
                    activity_event_repo,
                    task_id_str,
                    "source_update",
                    format!("branch missing before source update: {}", branch),
                    Some(branch),
                )
                .await);
            }
            Ok(SourceUpdateResult::Error(e)) => {
                warn!(
                    task_id = task_id_str,
                    error = %e,
                    "update_source_from_target failed — retrying once before blocking execution"
                );
                let retry_result = tokio::time::timeout(
                    freshness_timeout,
                    Box::pin(observe_source_freshness(
                        repo_path,
                        source_branch,
                        target_branch,
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
                        None,
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

fn freshness_timestamp_is_recent(value: Option<&str>, window_secs: u64) -> bool {
    value
        .and_then(|value| value.parse::<DateTime<Utc>>().ok())
        .is_some_and(|checked_at| {
            let elapsed = Utc::now() - checked_at;
            elapsed.num_seconds() >= 0 && elapsed.num_seconds() < window_secs as i64
        })
}

#[derive(Debug, PartialEq, Eq)]
enum FreshnessRetryDecision {
    Continue,
    Block { reason: String },
}

#[derive(Debug, PartialEq, Eq)]
enum FreshnessWorktreeGuardDecision {
    Skip,
    Block {
        reason_code: &'static str,
        check: &'static str,
        reason: String,
    },
}

fn should_fail_closed_on_worktree_status(origin_state: &str) -> bool {
    matches!(origin_state, "executing" | "re_executing")
}

fn dirty_worktree_autocommit_error_decision(
    origin_state: &str,
    error: &str,
) -> FreshnessWorktreeGuardDecision {
    if should_fail_closed_on_worktree_status(origin_state) {
        FreshnessWorktreeGuardDecision::Block {
            reason_code: "dirty_worktree_autocommit_failed",
            check: "dirty_worktree_autocommit",
            reason: format!("Emergency auto-commit failed before freshness check: {error}"),
        }
    } else {
        FreshnessWorktreeGuardDecision::Skip
    }
}

fn worktree_status_error_decision(
    origin_state: &str,
    error: &str,
) -> FreshnessWorktreeGuardDecision {
    if should_fail_closed_on_worktree_status(origin_state) {
        FreshnessWorktreeGuardDecision::Block {
            reason_code: "worktree_status_unreadable",
            check: "worktree_status",
            reason: format!("Failed to check worktree status before freshness check: {error}"),
        }
    } else {
        FreshnessWorktreeGuardDecision::Skip
    }
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
        Some(SourceUpdateResult::BranchMissing { branch }) => FreshnessRetryDecision::Block {
            reason: format!("branch missing before source update retry: {}", branch),
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
    branch_missing: Option<String>,
) -> FreshnessAction {
    block_freshness_git_error(
        activity_event_repo,
        task_id_str,
        "update_error_after_retry",
        check,
        reason,
        branch_missing,
    )
    .await
}

async fn block_freshness_git_error(
    activity_event_repo: Option<&Arc<dyn ActivityEventRepository>>,
    task_id_str: &str,
    reason_code: &'static str,
    check: &'static str,
    reason: String,
    branch_missing: Option<String>,
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
    FreshnessAction::ExecutionBlocked {
        reason,
        branch_missing,
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
#[path = "freshness_tests.rs"]
mod field_sync_tests;
