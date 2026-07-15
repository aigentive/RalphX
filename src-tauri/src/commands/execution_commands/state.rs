use super::*;
use ralphx_events::EventSink;

/// Statuses where an agent is actively running.
/// Tasks in these states need to be cancelled when stop is called,
/// and resumed when the app restarts.
///
/// NOTE: `Paused` is intentionally excluded. Paused tasks persist across restarts
/// with their metadata intact (pause_reason in SQLite). Startup recovery does not
/// touch them — they stay visible in the UI for manual or auto-resume.
///
/// Used by:
/// - `stop_execution` command to find tasks to cancel
/// - `StartupJobRunner` to find tasks to resume on app restart
pub const AGENT_ACTIVE_STATUSES: &[InternalStatus] = &[
    InternalStatus::Executing,
    InternalStatus::QaRefining,
    InternalStatus::QaTesting,
    InternalStatus::Reviewing,
    InternalStatus::ReExecuting,
    InternalStatus::Merging,      // spawns merger agent
    InternalStatus::PendingMerge, // runs attempt_programmatic_merge async side effect
    InternalStatus::UpdatingPlanBranch,
    InternalStatus::UpdatingTaskBranch,
];

/// States that have automatic transitions on entry.
/// Tasks stuck in these states on startup should have their entry actions
/// re-triggered to complete the auto-transition.
///
/// Used by:
/// - `StartupJobRunner` to find tasks needing auto-transition recovery
pub const AUTO_TRANSITION_STATES: &[InternalStatus] = &[
    InternalStatus::QaPassed,       // → PendingReview
    InternalStatus::PendingReview,  // → Reviewing (spawns reviewer)
    InternalStatus::RevisionNeeded, // → ReExecuting (spawns worker)
    InternalStatus::Approved,       // → PendingMerge (programmatic merge)
    InternalStatus::PendingMerge,   // attempt_programmatic_merge() (→ Merged or → Merging)
];

// ========================================
// Phase 82: Active Project State
// ========================================

/// Tracks the currently active project for execution scoping.
/// Commands without explicit project_id use the active project.
/// Phase 90: Simple RwLock — DB persistence eliminates the startup race condition.
pub struct ActiveProjectState {
    /// The currently active project, if any
    current: RwLock<Option<ProjectId>>,
}

impl std::fmt::Debug for ActiveProjectState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ActiveProjectState")
            .field("current", &self.current)
            .finish()
    }
}

impl Default for ActiveProjectState {
    fn default() -> Self {
        Self::new()
    }
}

impl ActiveProjectState {
    /// Create a new ActiveProjectState with no active project
    pub fn new() -> Self {
        Self {
            current: RwLock::new(None),
        }
    }

    /// Get the current active project ID
    pub async fn get(&self) -> Option<ProjectId> {
        self.current.read().await.clone()
    }

    /// Set the active project
    pub async fn set(&self, project_id: Option<ProjectId>) {
        *self.current.write().await = project_id;
    }

    /// Set the active project only when it changed.
    pub async fn set_if_changed(&self, project_id: Option<ProjectId>) -> bool {
        let mut current = self.current.write().await;
        if *current == project_id {
            return false;
        }
        *current = project_id;
        true
    }
}

/// Global execution state managed atomically for thread safety
pub struct ExecutionState {
    /// Whether the app is shutting down. Set as the FIRST operation in RunEvent::Exit
    /// (before agent cleanup) so stream handlers can skip escalation during clean shutdown.
    /// Resets automatically on restart (AtomicBool in memory — not persisted).
    pub is_shutting_down: AtomicBool,
    /// Whether execution is paused (stops picking up new tasks)
    is_paused: AtomicBool,
    /// Number of currently running tasks
    running_count: AtomicU32,
    /// Maximum concurrent tasks allowed (per-project)
    max_concurrent: AtomicU32,
    /// Global maximum concurrent tasks across ALL projects (Phase 82)
    /// Default 20, hard cap 50. Enforced alongside per-project max.
    global_max_concurrent: AtomicU32,
    /// Global maximum concurrent workspace main agents.
    workspace_max_concurrent: AtomicU32,
    /// Global maximum concurrent ideation sessions allowed to actively generate.
    /// This is a pipeline cap inside the global hard cap so ideation cannot consume
    /// all slots and starve task/review/merge execution.
    global_ideation_max: AtomicU32,
    /// Per-project maximum concurrent ideation sessions (synced from project execution settings).
    project_ideation_max: AtomicU32,
    /// When true, ideation may exceed `global_ideation_max` only if there is still
    /// total capacity available and no runnable execution work is waiting.
    allow_ideation_borrow_idle_execution: AtomicBool,
    /// Provider rate limit backpressure: epoch seconds until which all spawns are blocked.
    /// 0 = no active rate limit. When any agent detects a provider rate limit,
    /// this is set to the retry_after timestamp so ALL subsequent spawns are gated.
    rate_limited_until: AtomicU64,
    /// Set of task IDs that currently have an `attempt_merge_auto_complete` call in flight.
    /// Prevents duplicate auto-complete calls (e.g. from agent completion + error handler)
    /// from running validation concurrently in the same worktree.
    /// Uses std::sync::Mutex (not tokio) so Drop-based guards work synchronously.
    auto_completes_in_flight: std::sync::Mutex<HashSet<String>>,
    /// Set of task IDs currently being scheduled (transition_task in progress).
    /// Prevents two concurrent scheduler invocations from double-scheduling the same task
    /// when `on_enter(Executing)` is async and a stale DB read races with the first caller.
    /// Uses std::sync::Mutex for synchronous check-and-insert atomicity.
    scheduling_in_flight: std::sync::Mutex<HashSet<String>>,
    /// Set of task IDs currently running execution entry actions.
    /// Protects direct entry-action callers (manual resume, startup recovery, reconciliation)
    /// from mutating the same task worktree concurrently.
    execution_entries_in_flight: std::sync::Mutex<HashSet<String>>,
    /// Set of interactive process context keys (format: "{context_type}/{context_id}")
    /// whose execution slot has been released by TurnComplete (process is idle between turns).
    /// Used to prevent double-increment when multiple messages arrive while the agent is active.
    /// - `mark_interactive_idle(key)`: called by TurnComplete after decrementing
    /// - `claim_interactive_slot(key)`: called by send_message/queue_message fast-path;
    ///   returns true if slot was idle (caller should increment), false if already active
    /// - `remove_interactive_slot(key)`: called on process exit cleanup
    interactive_idle_slots: std::sync::Mutex<HashSet<String>>,
}

impl ExecutionState {
    /// Create a new ExecutionState with defaults
    pub fn new() -> Self {
        Self {
            is_shutting_down: AtomicBool::new(false),
            is_paused: AtomicBool::new(false),
            running_count: AtomicU32::new(0),
            max_concurrent: AtomicU32::new(10),
            global_max_concurrent: AtomicU32::new(20),
            workspace_max_concurrent: AtomicU32::new(DEFAULT_WORKSPACE_MAX_CONCURRENT),
            global_ideation_max: AtomicU32::new(10),
            project_ideation_max: AtomicU32::new(5),
            allow_ideation_borrow_idle_execution: AtomicBool::new(false),
            rate_limited_until: AtomicU64::new(0),
            auto_completes_in_flight: std::sync::Mutex::new(HashSet::new()),
            scheduling_in_flight: std::sync::Mutex::new(HashSet::new()),
            execution_entries_in_flight: std::sync::Mutex::new(HashSet::new()),
            interactive_idle_slots: std::sync::Mutex::new(HashSet::new()),
        }
    }

    /// Create ExecutionState with custom max concurrent
    pub fn with_max_concurrent(max: u32) -> Self {
        Self {
            is_shutting_down: AtomicBool::new(false),
            is_paused: AtomicBool::new(false),
            running_count: AtomicU32::new(0),
            max_concurrent: AtomicU32::new(max),
            global_max_concurrent: AtomicU32::new(20),
            workspace_max_concurrent: AtomicU32::new(DEFAULT_WORKSPACE_MAX_CONCURRENT),
            global_ideation_max: AtomicU32::new(10),
            project_ideation_max: AtomicU32::new(5),
            allow_ideation_borrow_idle_execution: AtomicBool::new(false),
            rate_limited_until: AtomicU64::new(0),
            auto_completes_in_flight: std::sync::Mutex::new(HashSet::new()),
            scheduling_in_flight: std::sync::Mutex::new(HashSet::new()),
            execution_entries_in_flight: std::sync::Mutex::new(HashSet::new()),
            interactive_idle_slots: std::sync::Mutex::new(HashSet::new()),
        }
    }

    /// Check if execution is paused
    pub fn is_paused(&self) -> bool {
        self.is_paused.load(Ordering::SeqCst)
    }

    /// Pause execution (stops picking up new tasks)
    pub fn pause(&self) {
        self.is_paused.store(true, Ordering::SeqCst);
    }

    /// Resume execution
    pub fn resume(&self) {
        self.is_paused.store(false, Ordering::SeqCst);
    }

    /// Get current running task count
    pub fn running_count(&self) -> u32 {
        self.running_count.load(Ordering::SeqCst)
    }

    /// Increment running count (when a task starts)
    pub fn increment_running(&self) -> u32 {
        self.running_count.fetch_add(1, Ordering::SeqCst) + 1
    }

    /// Decrement running count (when a task completes).
    /// Uses `fetch_update` with `saturating_sub` to prevent atomic underflow.
    pub fn decrement_running(&self) -> u32 {
        let prev = self
            .running_count
            .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |v| {
                Some(v.saturating_sub(1))
            })
            .unwrap(); // fetch_update with Some always succeeds
        if prev == 0 {
            tracing::warn!("decrement_running: running_count underflow prevented (was 0)");
        }
        prev.saturating_sub(1)
    }

    /// Atomically decrement running count AND mark the interactive slot as idle.
    /// Prevents race where a concurrent `claim_interactive_slot` call between
    /// separate `decrement_running()` and `mark_interactive_idle()` sees the slot
    /// as not-yet-idle, skips increment, and leaks a count.
    pub fn decrement_and_mark_idle(&self, key: &str) -> u32 {
        let mut idle_slots = self
            .interactive_idle_slots
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        idle_slots.insert(key.to_string());
        // Decrement while holding the lock so claim_interactive_slot can't race
        let new_count = self
            .running_count
            .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |v| {
                Some(v.saturating_sub(1))
            })
            .unwrap()
            .saturating_sub(1);
        new_count
    }

    /// Force-set running count (used for reconciliation with registry)
    pub fn set_running_count(&self, count: u32) {
        self.running_count.store(count, Ordering::SeqCst);
    }

    /// Get max concurrent tasks (per-project)
    pub fn max_concurrent(&self) -> u32 {
        self.max_concurrent.load(Ordering::SeqCst)
    }

    /// Set max concurrent tasks (per-project)
    pub fn set_max_concurrent(&self, max: u32) {
        self.max_concurrent.store(max, Ordering::SeqCst);
    }

    /// Get global max concurrent tasks (Phase 82)
    pub fn global_max_concurrent(&self) -> u32 {
        self.global_max_concurrent.load(Ordering::SeqCst)
    }

    /// Set global max concurrent tasks (Phase 82)
    /// Clamped to [1, 50] range
    pub fn set_global_max_concurrent(&self, max: u32) {
        let clamped = max.clamp(1, 50);
        self.global_max_concurrent.store(clamped, Ordering::SeqCst);
        let ideation_cap = self.global_ideation_max();
        if ideation_cap > clamped {
            self.global_ideation_max.store(clamped, Ordering::SeqCst);
        }
    }

    /// Get global max concurrent workspace main agents.
    pub fn workspace_max_concurrent(&self) -> u32 {
        self.workspace_max_concurrent.load(Ordering::SeqCst)
    }

    /// Set global max concurrent workspace main agents.
    /// Clamped to [1, 50].
    pub fn set_workspace_max_concurrent(&self, max: u32) {
        self.workspace_max_concurrent
            .store(max.clamp(1, 50), Ordering::SeqCst);
    }

    /// Get global max concurrent ideation sessions.
    pub fn global_ideation_max(&self) -> u32 {
        self.global_ideation_max.load(Ordering::SeqCst)
    }

    /// Set global max concurrent ideation sessions.
    /// Clamped to [1, global_max_concurrent].
    pub fn set_global_ideation_max(&self, max: u32) {
        let clamped = max.clamp(1, self.global_max_concurrent());
        self.global_ideation_max.store(clamped, Ordering::SeqCst);
    }

    /// Get per-project max concurrent ideation sessions.
    pub fn project_ideation_max(&self) -> u32 {
        self.project_ideation_max.load(Ordering::SeqCst)
    }

    /// Set per-project max concurrent ideation sessions (synced from project settings).
    pub fn set_project_ideation_max(&self, max: u32) {
        self.project_ideation_max.store(max, Ordering::SeqCst);
    }

    /// Check whether ideation may borrow idle execution capacity.
    pub fn allow_ideation_borrow_idle_execution(&self) -> bool {
        self.allow_ideation_borrow_idle_execution
            .load(Ordering::SeqCst)
    }

    /// Enable or disable ideation borrowing of idle execution capacity.
    pub fn set_allow_ideation_borrow_idle_execution(&self, allow: bool) {
        self.allow_ideation_borrow_idle_execution
            .store(allow, Ordering::SeqCst);
    }

    /// Check if the provider rate limit is currently active (blocking all spawns)
    pub fn is_provider_blocked(&self) -> bool {
        let until = self.rate_limited_until.load(Ordering::SeqCst);
        if until == 0 {
            return false;
        }
        let now = chrono::Utc::now().timestamp() as u64;
        now < until
    }

    /// Set the provider rate limit backpressure until a specific epoch second.
    /// All agent spawns will be blocked until this time.
    pub fn set_provider_blocked_until(&self, until_epoch_secs: u64) {
        self.rate_limited_until
            .store(until_epoch_secs, Ordering::SeqCst);
    }

    /// Clear the provider rate limit backpressure (set to 0 = no limit).
    pub fn clear_provider_block(&self) {
        self.rate_limited_until.store(0, Ordering::SeqCst);
    }

    /// Get the raw epoch seconds value for provider_blocked_until (0 = no limit).
    pub fn provider_blocked_until_epoch(&self) -> u64 {
        self.rate_limited_until.load(Ordering::SeqCst)
    }

    /// Check if we can start a new task
    /// Enforces pause, provider rate limit, per-project max, and global cap (Phase 82)
    pub fn can_start_task(&self) -> bool {
        if self.is_paused() {
            return false;
        }
        if self.is_provider_blocked() {
            return false;
        }
        let running = self.running_count();
        running < self.max_concurrent() && running < self.global_max_concurrent()
    }

    /// Check if any new execution-side context may start from the global perspective.
    /// This ignores per-project limits; callers that know the project must apply those separately.
    pub fn can_start_any_execution_context(&self) -> bool {
        if self.is_paused() {
            return false;
        }
        if self.is_provider_blocked() {
            return false;
        }
        self.running_count() < self.global_max_concurrent()
    }

    /// Check if we can start a new execution-side slot consumer (task/review/merge)
    /// for a specific project without violating the global or per-project caps.
    pub fn can_start_execution_context(
        &self,
        running_project_total: u32,
        project_max_concurrent: u32,
    ) -> bool {
        if self.is_paused() {
            return false;
        }
        if self.is_provider_blocked() {
            return false;
        }

        let running = self.running_count();
        if running >= self.global_max_concurrent() {
            return false;
        }

        running_project_total < project_max_concurrent
    }

    /// Check if we can start a new ideation session without starving execution.
    ///
    /// This uses the global hard cap plus a global ideation sub-cap. Per-project
    /// ideation allocation lands in a later milestone once those settings are
    /// persisted and threaded into all ideation entry points.
    pub fn can_start_ideation(
        &self,
        running_global_ideation: u32,
        running_project_ideation: u32,
        running_project_total: u32,
        project_max_concurrent: u32,
        project_ideation_max: u32,
        runnable_execution_waiting: bool,
        project_execution_waiting: bool,
    ) -> bool {
        if self.is_paused() {
            return false;
        }
        if self.is_provider_blocked() {
            return false;
        }

        let running = self.running_count();
        if running >= self.global_max_concurrent() {
            return false;
        }

        if running_project_total >= project_max_concurrent {
            return false;
        }

        let global_allows = if running_global_ideation < self.global_ideation_max() {
            true
        } else {
            self.allow_ideation_borrow_idle_execution() && !runnable_execution_waiting
        };

        if !global_allows {
            return false;
        }

        if project_ideation_max == 0 {
            return false;
        }

        if running_project_ideation < project_ideation_max {
            return true;
        }

        self.allow_ideation_borrow_idle_execution() && !project_execution_waiting
    }

    /// Try to mark a task as having an auto-complete in flight.
    /// Returns `true` if the task was newly inserted (caller should proceed).
    /// Returns `false` if the task was already in the set (caller should skip — duplicate).
    pub fn try_start_auto_complete(&self, task_id: &str) -> bool {
        let mut set = self
            .auto_completes_in_flight
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        set.insert(task_id.to_string())
    }

    /// Remove a task from the auto-completes-in-flight set.
    /// Called when auto-complete finishes (success or failure).
    pub fn finish_auto_complete(&self, task_id: &str) {
        let mut set = self
            .auto_completes_in_flight
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        set.remove(task_id);
    }

    /// Check if a task currently has an auto-complete in flight.
    /// Used by the reconciler to skip reconciliation when auto-complete is already running
    /// (prevents misinterpreting the dedup guard's skip as a failure).
    pub fn is_auto_complete_in_flight(&self, task_id: &str) -> bool {
        let set = self
            .auto_completes_in_flight
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        set.contains(task_id)
    }

    /// Try to claim a task for scheduling (per-task concurrency guard).
    /// Returns `true` if the task was newly inserted (caller should proceed with transition).
    /// Returns `false` if the task is already being scheduled (caller should skip).
    pub fn try_start_scheduling(&self, task_id: &str) -> bool {
        let mut set = self
            .scheduling_in_flight
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        set.insert(task_id.to_string())
    }

    /// Remove a task from the scheduling-in-flight set.
    /// Must be called after `transition_task` completes (success or error).
    pub fn finish_scheduling(&self, task_id: &str) {
        let mut set = self
            .scheduling_in_flight
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        set.remove(task_id);
    }

    /// Try to claim execution entry actions for a task.
    /// Returns `true` when the caller should proceed with `on_enter(Executing/ReExecuting)`.
    pub fn try_start_execution_entry(&self, task_id: &str) -> bool {
        let mut set = self
            .execution_entries_in_flight
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        set.insert(task_id.to_string())
    }

    /// Release the execution entry-action guard for a task.
    pub fn finish_execution_entry(&self, task_id: &str) {
        let mut set = self
            .execution_entries_in_flight
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        set.remove(task_id);
    }

    /// Check whether execution entry actions are in flight for a task.
    pub fn is_execution_entry_in_flight(&self, task_id: &str) -> bool {
        let set = self
            .execution_entries_in_flight
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        set.contains(task_id)
    }

    /// Mark an interactive process as idle (execution slot released by TurnComplete).
    /// The key format is "{context_type}/{context_id}".
    pub fn mark_interactive_idle(&self, key: &str) {
        let mut set = self
            .interactive_idle_slots
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        set.insert(key.to_string());
    }

    /// Atomically claim an interactive slot if the process is idle.
    /// Returns `true` if the slot was idle (removed from set — caller should increment running).
    /// Returns `false` if the slot was already active (not in set — caller should NOT increment).
    pub fn claim_interactive_slot(&self, key: &str) -> bool {
        let mut set = self
            .interactive_idle_slots
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        set.remove(key)
    }

    /// Remove an interactive slot from tracking (process exited).
    pub fn remove_interactive_slot(&self, key: &str) {
        let mut set = self
            .interactive_idle_slots
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        set.remove(key);
    }

    /// Check if a specific interactive slot is currently idle (between turns).
    /// Used by reconciliation to avoid overcounting idle processes as active.
    pub fn is_interactive_idle(&self, key: &str) -> bool {
        let set = self
            .interactive_idle_slots
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        set.contains(key)
    }

    /// Count how many interactive slots are currently idle.
    /// Used by `get_execution_status` to compute active count = registry_count - idle_count.
    pub fn interactive_idle_count(&self) -> usize {
        self.interactive_idle_slots
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .len()
    }

    fn status_changed_payload(&self, reason: &str) -> serde_json::Value {
        let blocked_until = self.provider_blocked_until_epoch();
        serde_json::json!({
            "isPaused": self.is_paused(),
            "runningCount": self.running_count(),
            "maxConcurrent": self.max_concurrent(),
            "globalMaxConcurrent": self.global_max_concurrent(),
            "workspaceMaxConcurrent": self.workspace_max_concurrent(),
            "providerBlocked": self.is_provider_blocked(),
            "providerBlockedUntil": if blocked_until > 0 { Some(blocked_until) } else { None::<u64> },
            "reason": reason,
            "timestamp": chrono::Utc::now().to_rfc3339(),
        })
    }

    /// Emit execution:status_changed event with current state through EventSink.
    pub fn emit_status_changed_to_sink(&self, sink: &dyn EventSink, reason: &str) {
        sink.emit(
            "execution:status_changed",
            self.status_changed_payload(reason),
        );
    }

    /// Emit execution:status_changed event with current state
    pub fn emit_status_changed<R: Runtime>(&self, handle: &AppHandle<R>, reason: &str) {
        let _ = handle.emit(
            "execution:status_changed",
            self.status_changed_payload(reason),
        );
    }
}

impl Default for ExecutionState {
    fn default() -> Self {
        Self::new()
    }
}

pub use crate::domain::execution::{ExecutionCommandResponse, ExecutionStatusResponse};

/// Response for execution settings queries
#[derive(Debug, Serialize)]
pub struct ExecutionSettingsResponse {
    /// Maximum number of concurrent tasks
    pub max_concurrent_tasks: u32,
    /// Maximum number of concurrent ideation sessions for this project
    pub project_ideation_max: u32,
    /// Whether to auto-commit changes after successful task completion
    pub auto_commit: bool,
    /// Whether to pause execution when a task fails
    pub pause_on_failure: bool,
    /// Default Autofix CI & Reviews setting for new agent workspaces
    pub agent_workspace_pr_autofix_default: bool,
    /// Default GitHub auto-merge setting for new agent workspaces
    pub agent_workspace_pr_auto_merge_default: bool,
}

impl From<ExecutionSettings> for ExecutionSettingsResponse {
    fn from(settings: ExecutionSettings) -> Self {
        Self {
            max_concurrent_tasks: settings.max_concurrent_tasks,
            project_ideation_max: settings.project_ideation_max,
            auto_commit: settings.auto_commit,
            pause_on_failure: settings.pause_on_failure,
            agent_workspace_pr_autofix_default: settings.agent_workspace_pr_autofix_default,
            agent_workspace_pr_auto_merge_default: settings.agent_workspace_pr_auto_merge_default,
        }
    }
}

/// Input for updating execution settings
#[derive(Debug, Deserialize)]
pub struct UpdateExecutionSettingsInput {
    /// Maximum number of concurrent tasks
    pub max_concurrent_tasks: u32,
    /// Maximum number of concurrent ideation sessions for this project
    pub project_ideation_max: u32,
    /// Whether to auto-commit changes after successful task completion
    pub auto_commit: bool,
    /// Whether to pause execution when a task fails
    pub pause_on_failure: bool,
    /// Default Autofix CI & Reviews setting for new agent workspaces
    pub agent_workspace_pr_autofix_default: bool,
    /// Default GitHub auto-merge setting for new agent workspaces
    pub agent_workspace_pr_auto_merge_default: bool,
}

/// Response for global execution settings queries
/// Phase 82: Global concurrency cap across all projects
#[derive(Debug, Serialize)]
pub struct GlobalExecutionSettingsResponse {
    /// Maximum total concurrent tasks across ALL projects
    pub global_max_concurrent: u32,
    /// Maximum concurrent workspace main agents across all projects
    pub workspace_max_concurrent: u32,
    /// Maximum total concurrent ideation sessions across all projects
    pub global_ideation_max: u32,
    /// Whether ideation may borrow idle execution capacity
    pub allow_ideation_borrow_idle_execution: bool,
}

impl From<crate::domain::execution::GlobalExecutionSettings> for GlobalExecutionSettingsResponse {
    fn from(settings: crate::domain::execution::GlobalExecutionSettings) -> Self {
        Self {
            global_max_concurrent: settings.global_max_concurrent,
            workspace_max_concurrent: settings.workspace_max_concurrent,
            global_ideation_max: settings.global_ideation_max,
            allow_ideation_borrow_idle_execution: settings.allow_ideation_borrow_idle_execution,
        }
    }
}

/// Input for updating global execution settings
#[derive(Debug, Deserialize)]
pub struct UpdateGlobalExecutionSettingsInput {
    /// Maximum total concurrent tasks across ALL projects (max: 50)
    pub global_max_concurrent: u32,
    /// Maximum concurrent workspace main agents across ALL projects (max: 50)
    #[serde(default = "default_workspace_update_max_concurrent")]
    pub workspace_max_concurrent: u32,
    /// Maximum total concurrent ideation sessions across ALL projects (max: 50)
    pub global_ideation_max: u32,
    /// Whether ideation may borrow idle execution capacity
    pub allow_ideation_borrow_idle_execution: bool,
}

fn default_workspace_update_max_concurrent() -> u32 {
    DEFAULT_WORKSPACE_MAX_CONCURRENT
}

// ========================================
// Quota Sync Helper
// ========================================

/// Result of syncing project quota
/// Contains the resolved project ID and the max concurrent value that was applied
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct ProjectQuotaSync {
    /// The resolved project ID (None if global/no project)
    pub project_id: Option<ProjectId>,
    /// The max concurrent tasks value that was synced to execution_state
    pub max_concurrent: u32,
}

/// Syncs runtime ExecutionState max_concurrent with persisted project settings.
/// Returns the resolved project ID and the effective max_concurrent value.
///
/// Resolution order:
/// 1. Explicit project_id parameter
/// 2. Active project from active_project_state
/// 3. None (uses global default settings)
///
/// This helper ensures the runtime quota always reflects the active project's
/// persisted settings, preventing drift when switching projects or querying status.
pub(super) async fn sync_quota_from_project(
    project_id: Option<ProjectId>,
    active_project_state: &Arc<ActiveProjectState>,
    execution_state: &Arc<ExecutionState>,
    app_state: &AppState,
) -> Result<(Option<ProjectId>, u32), String> {
    // Determine effective project_id: explicit param > active project > none
    let effective_project_id = match project_id {
        Some(id) => Some(id),
        None => active_project_state.get().await,
    };

    // Load execution settings for the effective project
    let settings = app_state
        .execution_settings_repo
        .get_settings(effective_project_id.as_ref())
        .await
        .map_err(|e| e.to_string())?;

    // Sync runtime ExecutionState with persisted project settings
    execution_state.set_max_concurrent(settings.max_concurrent_tasks);
    execution_state.set_project_ideation_max(settings.project_ideation_max);

    Ok((effective_project_id, settings.max_concurrent_tasks))
}

/// Wrapper that returns a `ProjectQuotaSync` struct instead of a tuple.
/// Delegates to `sync_quota_from_project` for the actual logic.
#[allow(dead_code)]
pub(super) async fn sync_project_quota(
    explicit_project_id: Option<ProjectId>,
    active_project_state: &Arc<ActiveProjectState>,
    execution_state: &Arc<ExecutionState>,
    app_state: &AppState,
) -> Result<ProjectQuotaSync, String> {
    let (project_id, max_concurrent) = sync_quota_from_project(
        explicit_project_id,
        active_project_state,
        execution_state,
        app_state,
    )
    .await?;

    Ok(ProjectQuotaSync {
        project_id,
        max_concurrent,
    })
}
