use serde::Deserialize;
use tracing::warn;

// ── Top-level wrapper ────────────────────────────────────────────────────

/// All runtime configuration collected from config/ralphx.yaml + env overrides.
#[derive(Debug, Clone)]
pub struct AllRuntimeConfig {
    pub stream: StreamTimeoutsConfig,
    pub reconciliation: ReconciliationConfig,
    pub git: GitRuntimeConfig,
    pub scheduler: SchedulerConfig,
    pub supervisor: SupervisorRuntimeConfig,
    pub limits: LimitsConfig,
    pub verification: VerificationConfig,
    pub external_mcp: ExternalMcpConfig,
    /// Seconds of inactivity before an agent is considered "likely_waiting" vs "likely_generating".
    /// Used by get_child_session_status to derive estimated_status. Default: 10.
    pub child_session_activity_threshold_secs: Option<u64>,
    /// UI feature flags (page visibility). Defaults to all enabled.
    pub ui_feature_flags: super::ui_config::UiFeatureFlagsConfig,
}

/// A specialist agent entry in the verification pipeline.
#[derive(Debug, Clone, Deserialize, Default)]
pub struct SpecialistEntry {
    /// Unique agent name (matches config/ralphx.yaml agent name, e.g. "ralphx-ideation-specialist-code-quality").
    pub name: String,
    /// Human-readable display name shown in the UI.
    pub display_name: String,
    /// Brief description of what this specialist analyzes.
    pub description: String,
    /// When this specialist is dispatched: "pre_round" (once before the loop) or "per_round" (each round).
    pub dispatch_mode: String,
    /// Whether this specialist is selected by default in the confirmation dialog.
    pub enabled_by_default: bool,
}

/// Configuration for the plan verification feature.
///
/// All fields required in config/ralphx.yaml under `ideation.verification:`.
/// `Default` impl retained only for fallback/test use.
#[derive(Debug, Clone, Deserialize)]
pub struct VerificationConfig {
    /// Maximum number of adversarial review rounds [1, 10]. Hard cap — always terminates.
    pub max_rounds: u32,
    /// If true, verification starts automatically when a plan is created.
    pub auto_verify: bool,
    /// Deprecated: gate is now driven by DB-backed `IdeationSettings`; this YAML field is ignored.
    #[serde(skip)]
    #[allow(dead_code)]
    pub require_verification_for_accept: bool,
    /// Minimum number of proposal tasks before auto-verification triggers (if `auto_verify=true`).
    pub complexity_threshold: u32,
    /// Sessions stuck in `verification_in_progress=1` for longer than this are reset by
    /// the reconciliation service (seconds). Default: 5400 (90 min). For manual verify sessions.
    pub reconciliation_stale_after_secs: u64,
    /// How often the verification reconciliation service scans for stuck sessions (seconds).
    pub reconciliation_interval_secs: u64,
    /// Stale threshold for auto-verify sessions (generation > 0). Default: 600s (10 minutes).
    #[serde(default = "default_auto_verify_stale_secs")]
    pub auto_verify_stale_secs: u64,
    /// Retry self-heal threshold for orphaned execution plans left behind by failed accept-plan
    /// attempts. If an active execution plan has no tasks or linked proposals after this many
    /// seconds, a later retry supersedes it and starts fresh. Default: 30s.
    #[serde(default = "default_accept_stale_execution_plan_secs")]
    pub accept_stale_execution_plan_secs: u64,
    /// Specialist agents available in the verification pipeline.
    /// Loaded from `verification.specialists` in `config/ralphx.yaml`.
    #[serde(default)]
    pub specialists: Vec<SpecialistEntry>,
}

fn default_auto_verify_stale_secs() -> u64 {
    600
}

fn default_accept_stale_execution_plan_secs() -> u64 {
    30
}

fn default_external_session_stale_secs() -> u64 {
    7200 // 2 hours
}

fn default_external_message_queue_cap() -> u32 {
    10
}

fn default_external_session_similarity_threshold() -> f64 {
    0.7
}

impl Default for VerificationConfig {
    fn default() -> Self {
        Self {
            max_rounds: 4,
            auto_verify: false,
            require_verification_for_accept: true,
            complexity_threshold: 3,
            reconciliation_stale_after_secs: 5400, // 90 minutes
            reconciliation_interval_secs: 300,     // 5 minutes
            auto_verify_stale_secs: 600,           // 10 minutes
            accept_stale_execution_plan_secs: 30,  // 30 seconds
            specialists: vec![],
        }
    }
}

// ── ExternalMcpConfig ─────────────────────────────────────────────────────

/// Configuration for the external MCP server feature.
///
/// All fields have defaults via `#[serde(default)]` — no YAML entry required.
#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct ExternalMcpConfig {
    /// Enable the external MCP server. Default: false.
    pub enabled: bool,
    /// Port the external MCP server listens on. Default: 3848.
    pub port: u16,
    /// Host the external MCP server binds to. Default: "127.0.0.1".
    pub host: String,
    /// Maximum restart attempts before giving up. Default: 3.
    pub max_restart_attempts: u32,
    /// Delay between restart attempts in milliseconds. Default: 2000.
    pub restart_delay_ms: u64,
    /// Backend deadline for human-in-the-loop MCP waits (question/team-plan).
    /// Must stay below the effective MCP tool ceiling so backend 408 responses win.
    pub human_wait_timeout_secs: u64,
    /// Optional auth token for the external MCP server (placeholder for future use).
    #[serde(default)]
    pub auth_token: Option<String>,
    /// Path to the Node.js binary. Resolved from `RALPHX_NODE_PATH` env var if not set.
    #[serde(default)]
    pub node_path: Option<String>,
    /// **Deprecated** — no longer enforced. The session-gate was removed in favour of
    /// always-create-session-first behaviour. Field retained permanently for backward-compatible
    /// YAML parsing. Value is ignored at runtime.
    pub max_external_ideation_sessions: u32,
    /// Seconds of inactivity before an external session is considered stale and archived.
    /// External sessions older than this with no proposals and idle agent are archived.
    /// Default: 7200 (2 hours).
    #[serde(default = "default_external_session_stale_secs")]
    pub external_session_stale_secs: u64,
    /// Maximum number of queued messages per external session.
    /// When queue depth reaches this limit, new messages return 429.
    /// Default: 10.
    #[serde(default = "default_external_message_queue_cap")]
    pub external_message_queue_cap: u32,
    /// Jaccard similarity threshold for session dedup.
    /// Prompts/titles with similarity >= this value are treated as duplicates.
    /// Default: 0.7. Range [0.0, 1.0]. Set to 0.0 to disable dedup; 1.0 for exact match only.
    #[serde(default = "default_external_session_similarity_threshold")]
    pub external_session_similarity_threshold: f64,
    /// Separate TTL for cold-boot external session archival (seconds).
    /// When set, used instead of `external_session_stale_secs` during startup scans,
    /// allowing a longer grace period on first boot without changing the periodic TTL.
    /// When `None` (default), falls back to `external_session_stale_secs`.
    #[serde(default)]
    pub external_session_startup_grace_secs: Option<u64>,
}

impl Default for ExternalMcpConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            port: 3848,
            host: "127.0.0.1".to_string(),
            max_restart_attempts: 3,
            restart_delay_ms: 2000,
            human_wait_timeout_secs: 285,
            auth_token: None,
            node_path: None,
            max_external_ideation_sessions: 1,
            external_session_stale_secs: 7200,
            external_message_queue_cap: 10,
            external_session_similarity_threshold: 0.7,
            external_session_startup_grace_secs: None,
        }
    }
}

/// YAML wrapper for nested `ideation:` key.
#[derive(Debug, Clone, Deserialize, Default)]
pub(crate) struct IdeationConfigWrapper {
    #[serde(default)]
    pub verification: VerificationConfig,
    #[serde(default)]
    pub child_session_activity_threshold_secs: Option<u64>,
}

// ── YAML wrapper for nested `timeouts:` key ──────────────────────────────

#[derive(Debug, Clone, Deserialize, Default)]
pub(crate) struct TimeoutsWrapper {
    #[serde(default)]
    pub stream: StreamTimeoutsConfig,
}

// ── Individual config structs ────────────────────────────────────────────

/// All fields required in config/ralphx.yaml except backward-compatible timeout fields
/// with serde defaults (`max_wall_clock_secs`, `completion_grace_secs`).
/// `Default` impl retained only for fallback/test use.
#[derive(Debug, Clone, Deserialize)]
pub struct StreamTimeoutsConfig {
    pub merge_line_read_secs: u64,
    pub merge_parse_stall_secs: u64,
    pub review_line_read_secs: u64,
    pub review_parse_stall_secs: u64,
    pub default_line_read_secs: u64,
    pub default_parse_stall_secs: u64,
    pub team_line_read_secs: u64,
    pub team_parse_stall_secs: u64,
    #[serde(default = "default_max_wall_clock_secs")]
    pub max_wall_clock_secs: u64,
    #[serde(default = "default_completion_grace_secs")]
    pub completion_grace_secs: u64,
}

fn default_max_wall_clock_secs() -> u64 {
    1800
}

fn default_completion_grace_secs() -> u64 {
    30
}

impl Default for StreamTimeoutsConfig {
    fn default() -> Self {
        Self {
            merge_line_read_secs: 600,
            merge_parse_stall_secs: 180,
            review_line_read_secs: 600,
            review_parse_stall_secs: 120,
            default_line_read_secs: 600,
            default_parse_stall_secs: 180,
            team_line_read_secs: 3600,
            team_parse_stall_secs: 3600,
            max_wall_clock_secs: 1800,
            completion_grace_secs: 30,
        }
    }
}

/// All fields required in config/ralphx.yaml — no serde defaults.
/// `Default` impl retained only for fallback/test use.
#[derive(Debug, Clone, Deserialize)]
pub struct ReconciliationConfig {
    pub merger_timeout_secs: u64,
    pub merging_max_retries: u64,
    pub pending_merge_stale_minutes: u64,
    pub qa_stale_minutes: u64,
    pub merge_incomplete_retry_base_secs: u64,
    pub merge_incomplete_retry_max_secs: u64,
    pub merge_incomplete_max_retries: u64,
    pub validation_revert_max_count: u64,
    pub merge_conflict_retry_base_secs: u64,
    pub merge_conflict_retry_max_secs: u64,
    pub merge_conflict_max_retries: u64,
    pub executing_max_retries: u64,
    pub reviewing_max_retries: u64,
    pub qa_max_retries: u64,
    pub executing_max_wall_clock_minutes: u64,
    pub reviewing_max_wall_clock_minutes: u64,
    pub qa_max_wall_clock_minutes: u64,
    /// Maximum wall-clock seconds for `pre_merge_cleanup` before the merge proceeds anyway.
    /// Cleanup is best-effort; if it hangs (e.g. lsof on large target/), we skip it.
    pub pre_merge_cleanup_timeout_secs: u64,
    /// Maximum wall-clock seconds for the entire programmatic merge attempt
    /// (cleanup + freshness + strategy dispatch), measured from function entry.
    /// If exceeded, task transitions to MergeIncomplete. Also used as auto-expiry
    /// for the `merge_pipeline_active` metadata flag.
    pub attempt_merge_deadline_secs: u64,
    /// Maximum wall-clock seconds for post-merge validation commands.
    /// Separate from `attempt_merge_deadline_secs` so git operations stay bounded
    /// while long-running validation (e.g. `cargo test`) gets adequate time.
    pub validation_deadline_secs: u64,
    /// Grace period (seconds) after a merge agent run is created before the reconciler
    /// checks for run-state vs registry mismatches. Covers agent startup latency.
    pub merge_registry_grace_period_secs: u64,
    /// Minimum cooldown (seconds) after a validation failure before the reconciler retries.
    /// Prevents rapid retry loops when validation consistently fails.
    pub validation_retry_min_cooldown_secs: u64,
    /// After this many consecutive validation failures, stop auto-retrying entirely
    /// and leave for human intervention.
    pub validation_failure_circuit_breaker_count: u64,
    /// Starvation guard: skip a MergeIncomplete task if it was retried within this many
    /// seconds, giving other tasks a turn in the reconciliation cycle.
    pub merge_starvation_guard_secs: u64,
    /// Maximum seconds for branch freshness updates (update_plan_from_main, update_source_from_target).
    /// If exceeded, the merge aborts to MergeIncomplete instead of hanging indefinitely.
    pub branch_freshness_timeout_secs: u64,
    /// Initial grace period (seconds) before the merge completion watcher starts polling.
    /// Gives the merger agent time to begin work before checking git state.
    pub merge_watcher_grace_secs: u64,
    /// Poll interval (seconds) for the merge completion watcher to check git state.
    pub merge_watcher_poll_secs: u64,
    /// Max auto-retry attempts for Failed tasks with transient execution failures (timeout/crash/stall).
    /// Independent of `executing_max_retries` (which tracks in-flight agent deaths).
    pub execution_failed_max_retries: u64,
    /// Initial backoff before retrying a Failed execution task (exponential base, seconds).
    pub execution_failed_retry_base_secs: u64,
    /// Cap on execution retry exponential backoff (seconds).
    pub execution_failed_retry_max_secs: u64,
    /// Number of same-source failures in the window before circuit breaker fires (default: 3)
    #[serde(default = "default_merge_circuit_breaker_threshold")]
    pub merge_circuit_breaker_threshold: u64,
    /// Window size (number of recent failure events) for circuit breaker evaluation (default: 5)
    #[serde(default = "default_merge_circuit_breaker_window")]
    pub merge_circuit_breaker_window: u64,
    /// Enable branch freshness checks before execution/review agent spawn. Default: true.
    #[serde(default = "default_true")]
    pub execution_freshness_enabled: bool,
    /// Skip freshness check if it was run within this many seconds. Default: 30.
    #[serde(default = "default_freshness_skip_window_secs")]
    pub freshness_skip_window_secs: u64,
    /// Max number of freshness conflict retries before blocking execution. Default: 5.
    #[serde(default = "default_freshness_max_conflict_retries")]
    pub freshness_max_conflict_retries: u32,
    /// Base backoff (seconds) between freshness conflict retries (exponential). Default: 60.
    #[serde(default = "default_freshness_backoff_base_secs")]
    pub freshness_backoff_base_secs: u64,
    /// Maximum backoff cap (seconds) for freshness conflict retries. Default: 600.
    #[serde(default = "default_freshness_backoff_max_secs")]
    pub freshness_backoff_max_secs: u64,
    /// Cooldown (seconds) before auto-resetting a freshness-blocked task. Default: 600.
    #[serde(default = "default_freshness_auto_reset_cooldown_secs")]
    pub freshness_auto_reset_cooldown_secs: u64,
    /// Initial backoff before retrying a git-isolation Failed task (seconds). Default: 5.
    /// Shorter than `execution_failed_retry_base_secs` because git transient issues resolve quickly.
    #[serde(default = "default_git_isolation_retry_base_secs")]
    pub git_isolation_retry_base_secs: u64,
    /// Max auto-retry attempts for Failed tasks with git isolation failures. Default: 3.
    /// Independent budget from `execution_failed_max_retries` (timeout/crash retries).
    #[serde(default = "default_git_isolation_max_retries")]
    pub git_isolation_max_retries: u32,
    /// Tasks whose `failed_at` metadata timestamp is older than this many seconds are skipped
    /// by both startup recovery and the reconciler retry loop. Default: 86400 (24 hours).
    #[serde(default = "default_recovery_staleness_secs")]
    pub recovery_staleness_secs: u64,
}

fn default_merge_circuit_breaker_threshold() -> u64 {
    3
}
fn default_merge_circuit_breaker_window() -> u64 {
    5
}
fn default_true() -> bool {
    true
}
fn default_freshness_skip_window_secs() -> u64 {
    30
}
fn default_freshness_max_conflict_retries() -> u32 {
    5
}
fn default_freshness_backoff_base_secs() -> u64 {
    60
}
fn default_freshness_backoff_max_secs() -> u64 {
    600
}
fn default_freshness_auto_reset_cooldown_secs() -> u64 {
    600
}
fn default_git_isolation_retry_base_secs() -> u64 {
    5
}
fn default_git_isolation_max_retries() -> u32 {
    3
}
fn default_recovery_staleness_secs() -> u64 {
    86400 // 24 hours
}

impl Default for ReconciliationConfig {
    fn default() -> Self {
        Self {
            merger_timeout_secs: 1200,
            merging_max_retries: 3,
            pending_merge_stale_minutes: 2,
            qa_stale_minutes: 5,
            merge_incomplete_retry_base_secs: 5,
            merge_incomplete_retry_max_secs: 1800,
            merge_incomplete_max_retries: 5,
            validation_revert_max_count: 2,
            merge_conflict_retry_base_secs: 60,
            merge_conflict_retry_max_secs: 600,
            merge_conflict_max_retries: 3,
            executing_max_retries: 5,
            reviewing_max_retries: 3,
            qa_max_retries: 3,
            executing_max_wall_clock_minutes: 60,
            reviewing_max_wall_clock_minutes: 30,
            qa_max_wall_clock_minutes: 15,
            pre_merge_cleanup_timeout_secs: 60,
            attempt_merge_deadline_secs: 120,
            validation_deadline_secs: 1200,
            merge_registry_grace_period_secs: 60,
            validation_retry_min_cooldown_secs: 120,
            validation_failure_circuit_breaker_count: 3,
            merge_starvation_guard_secs: 60,
            branch_freshness_timeout_secs: 60,
            merge_watcher_grace_secs: 30,
            merge_watcher_poll_secs: 15,
            execution_failed_max_retries: 3,
            execution_failed_retry_base_secs: 30,
            execution_failed_retry_max_secs: 600,
            merge_circuit_breaker_threshold: 3,
            merge_circuit_breaker_window: 5,
            execution_freshness_enabled: true,
            freshness_skip_window_secs: 30,
            freshness_max_conflict_retries: 5,
            freshness_backoff_base_secs: 60,
            freshness_backoff_max_secs: 600,
            freshness_auto_reset_cooldown_secs: 600,
            git_isolation_retry_base_secs: 5,
            git_isolation_max_retries: 3,
            recovery_staleness_secs: 86400,
        }
    }
}

/// All fields required in config/ralphx.yaml — no serde defaults.
/// `Default` impl retained only for fallback/test use.
#[derive(Debug, Clone, Deserialize)]
pub struct GitRuntimeConfig {
    pub cmd_timeout_secs: u64,
    pub max_retries: u64,
    pub retry_backoff_secs: Vec<u64>,
    pub index_lock_stale_secs: u64,
    /// Seconds to wait after SIGTERM for process tree cleanup before worktree deletion.
    pub agent_kill_settle_secs: u64,
    /// Timeout in seconds for each stop_agent() call in pre-merge cleanup step 0.
    pub agent_stop_timeout_secs: u64,
    /// Timeout in seconds for deleting the task worktree during pre-merge cleanup.
    pub cleanup_worktree_timeout_secs: u64,
    /// Timeout in seconds for merge/rebase worktree deletion and git clean during pre-merge cleanup.
    pub cleanup_git_op_timeout_secs: u64,
    /// Timeout in seconds for the `lsof +D` scan in `kill_worktree_processes_async`.
    /// On large worktrees (with `target/` dirs), lsof can block for minutes.
    pub worktree_lsof_timeout_secs: u64,
    /// Outer timeout in seconds for the entire step 0b kill phase
    /// (`kill_worktree_processes_async`). Defense in depth — bounds the step even if
    /// the inner lsof timeout fails due to tokio timer driver starvation.
    pub step_0b_kill_timeout_secs: u64,
}

impl Default for GitRuntimeConfig {
    fn default() -> Self {
        Self {
            cmd_timeout_secs: 60,
            max_retries: 3,
            retry_backoff_secs: vec![1, 2, 4],
            index_lock_stale_secs: 5,
            agent_kill_settle_secs: 0,
            agent_stop_timeout_secs: 3,
            cleanup_worktree_timeout_secs: 15,
            cleanup_git_op_timeout_secs: 30,
            worktree_lsof_timeout_secs: 10,
            step_0b_kill_timeout_secs: 5,
        }
    }
}

/// All fields required in config/ralphx.yaml — no serde defaults.
/// `Default` impl retained only for fallback/test use.
#[derive(Debug, Clone, Deserialize)]
pub struct SchedulerConfig {
    pub watchdog_interval_secs: u64,
    pub watchdog_stale_threshold_secs: u64,
    pub max_contention_retries: u64,
    pub contention_retry_delay_ms: u64,
    pub ready_settle_ms: u64,
    pub merge_settle_ms: u64,
}

impl Default for SchedulerConfig {
    fn default() -> Self {
        Self {
            watchdog_interval_secs: 60,
            watchdog_stale_threshold_secs: 30,
            max_contention_retries: 3,
            contention_retry_delay_ms: 200,
            ready_settle_ms: 300,
            merge_settle_ms: 100,
        }
    }
}

/// All fields required in config/ralphx.yaml — no serde defaults.
/// `Default` impl retained only for fallback/test use.
#[derive(Debug, Clone, Deserialize)]
pub struct SupervisorRuntimeConfig {
    pub time_threshold_secs: u64,
    pub token_threshold: u64,
    pub max_tokens: u64,
    pub progress_interval_secs: u64,
    pub loop_threshold: u64,
    pub stuck_threshold: u64,
}

impl Default for SupervisorRuntimeConfig {
    fn default() -> Self {
        Self {
            time_threshold_secs: 600,
            token_threshold: 50000,
            max_tokens: 100000,
            progress_interval_secs: 30,
            loop_threshold: 3,
            stuck_threshold: 5,
        }
    }
}

/// All fields required in config/ralphx.yaml — no serde defaults.
/// `Default` impl retained only for fallback/test use.
#[derive(Debug, Clone, Deserialize)]
pub struct LimitsConfig {
    pub max_resume_attempts: u64,
}

impl Default for LimitsConfig {
    fn default() -> Self {
        Self {
            max_resume_attempts: 5,
        }
    }
}

// ── Env overrides ────────────────────────────────────────────────────────

pub fn apply_env_overrides(cfg: &mut AllRuntimeConfig) {
    apply_env_overrides_with(cfg, &|name| std::env::var(name).ok());
}

/// Test-only entry point: apply env overrides using a custom lookup function.
#[cfg(test)]
pub(crate) fn apply_env_overrides_with_lookup(
    cfg: &mut AllRuntimeConfig,
    lookup: &dyn Fn(&str) -> Option<String>,
) {
    apply_env_overrides_with(cfg, lookup);
}

fn apply_env_overrides_with(cfg: &mut AllRuntimeConfig, lookup: &dyn Fn(&str) -> Option<String>) {
    macro_rules! env_u64 {
        ($field:expr, $key:expr) => {
            if let Some(v) = lookup($key) {
                if let Ok(n) = v.parse::<u64>() {
                    $field = n;
                }
            }
        };
    }

    // Stream timeouts
    env_u64!(
        cfg.stream.merge_line_read_secs,
        "RALPHX_STREAM_MERGE_LINE_READ_SECS"
    );
    env_u64!(
        cfg.stream.merge_parse_stall_secs,
        "RALPHX_STREAM_MERGE_PARSE_STALL_SECS"
    );
    env_u64!(
        cfg.stream.review_line_read_secs,
        "RALPHX_STREAM_REVIEW_LINE_READ_SECS"
    );
    env_u64!(
        cfg.stream.review_parse_stall_secs,
        "RALPHX_STREAM_REVIEW_PARSE_STALL_SECS"
    );
    env_u64!(
        cfg.stream.default_line_read_secs,
        "RALPHX_STREAM_DEFAULT_LINE_READ_SECS"
    );
    env_u64!(
        cfg.stream.default_parse_stall_secs,
        "RALPHX_STREAM_DEFAULT_PARSE_STALL_SECS"
    );
    env_u64!(
        cfg.stream.team_line_read_secs,
        "RALPHX_STREAM_TEAM_LINE_READ_SECS"
    );
    env_u64!(
        cfg.stream.team_parse_stall_secs,
        "RALPHX_STREAM_TEAM_PARSE_STALL_SECS"
    );
    env_u64!(
        cfg.stream.max_wall_clock_secs,
        "RALPHX_STREAM_MAX_WALL_CLOCK_SECS"
    );
    env_u64!(
        cfg.stream.completion_grace_secs,
        "RALPHX_STREAM_COMPLETION_GRACE_SECS"
    );

    // Reconciliation
    // Backward compat: old env key
    env_u64!(
        cfg.reconciliation.merger_timeout_secs,
        "RALPHX_MERGER_TIMEOUT_SECS"
    );
    // New canonical key (takes precedence if both set)
    env_u64!(cfg.reconciliation.merger_timeout_secs, "RALPHX_RECONCILIATION_MERGER_TIMEOUT_SECS");
    env_u64!(cfg.reconciliation.merging_max_retries, "RALPHX_RECONCILIATION_MERGING_MAX_RETRIES");
    env_u64!(cfg.reconciliation.pending_merge_stale_minutes, "RALPHX_RECONCILIATION_PENDING_MERGE_STALE_MINUTES");
    env_u64!(cfg.reconciliation.qa_stale_minutes, "RALPHX_RECONCILIATION_QA_STALE_MINUTES");
    env_u64!(cfg.reconciliation.merge_incomplete_retry_base_secs, "RALPHX_RECONCILIATION_MERGE_INCOMPLETE_RETRY_BASE_SECS");
    env_u64!(cfg.reconciliation.merge_incomplete_retry_max_secs, "RALPHX_RECONCILIATION_MERGE_INCOMPLETE_RETRY_MAX_SECS");
    env_u64!(cfg.reconciliation.merge_incomplete_max_retries, "RALPHX_RECONCILIATION_MERGE_INCOMPLETE_MAX_RETRIES");
    env_u64!(cfg.reconciliation.validation_revert_max_count, "RALPHX_RECONCILIATION_VALIDATION_REVERT_MAX_COUNT");
    env_u64!(cfg.reconciliation.merge_conflict_retry_base_secs, "RALPHX_RECONCILIATION_MERGE_CONFLICT_RETRY_BASE_SECS");
    env_u64!(cfg.reconciliation.merge_conflict_retry_max_secs, "RALPHX_RECONCILIATION_MERGE_CONFLICT_RETRY_MAX_SECS");
    env_u64!(cfg.reconciliation.merge_conflict_max_retries, "RALPHX_RECONCILIATION_MERGE_CONFLICT_MAX_RETRIES");
    env_u64!(cfg.reconciliation.executing_max_retries, "RALPHX_RECONCILIATION_EXECUTING_MAX_RETRIES");
    env_u64!(cfg.reconciliation.reviewing_max_retries, "RALPHX_RECONCILIATION_REVIEWING_MAX_RETRIES");
    env_u64!(cfg.reconciliation.qa_max_retries, "RALPHX_RECONCILIATION_QA_MAX_RETRIES");
    env_u64!(cfg.reconciliation.executing_max_wall_clock_minutes, "RALPHX_RECONCILIATION_EXECUTING_MAX_WALL_CLOCK_MINUTES");
    env_u64!(cfg.reconciliation.reviewing_max_wall_clock_minutes, "RALPHX_RECONCILIATION_REVIEWING_MAX_WALL_CLOCK_MINUTES");
    env_u64!(cfg.reconciliation.qa_max_wall_clock_minutes, "RALPHX_RECONCILIATION_QA_MAX_WALL_CLOCK_MINUTES");
    env_u64!(cfg.reconciliation.pre_merge_cleanup_timeout_secs, "RALPHX_RECONCILIATION_PRE_MERGE_CLEANUP_TIMEOUT_SECS");
    env_u64!(cfg.reconciliation.attempt_merge_deadline_secs, "RALPHX_RECONCILIATION_ATTEMPT_MERGE_DEADLINE_SECS");
    env_u64!(cfg.reconciliation.validation_deadline_secs, "RALPHX_RECONCILIATION_VALIDATION_DEADLINE_SECS");
    env_u64!(cfg.reconciliation.merge_registry_grace_period_secs, "RALPHX_RECONCILIATION_MERGE_REGISTRY_GRACE_PERIOD_SECS");
    env_u64!(cfg.reconciliation.validation_retry_min_cooldown_secs, "RALPHX_RECONCILIATION_VALIDATION_RETRY_MIN_COOLDOWN_SECS");
    env_u64!(cfg.reconciliation.validation_failure_circuit_breaker_count, "RALPHX_RECONCILIATION_VALIDATION_FAILURE_CIRCUIT_BREAKER_COUNT");
    env_u64!(cfg.reconciliation.merge_starvation_guard_secs, "RALPHX_RECONCILIATION_MERGE_STARVATION_GUARD_SECS");
    env_u64!(cfg.reconciliation.branch_freshness_timeout_secs, "RALPHX_RECONCILIATION_BRANCH_FRESHNESS_TIMEOUT_SECS");
    env_u64!(cfg.reconciliation.merge_watcher_grace_secs, "RALPHX_RECONCILIATION_MERGE_WATCHER_GRACE_SECS");
    env_u64!(cfg.reconciliation.merge_watcher_poll_secs, "RALPHX_RECONCILIATION_MERGE_WATCHER_POLL_SECS");
    env_u64!(cfg.reconciliation.execution_failed_max_retries, "RALPHX_RECONCILIATION_EXECUTION_FAILED_MAX_RETRIES");
    env_u64!(cfg.reconciliation.execution_failed_retry_base_secs, "RALPHX_RECONCILIATION_EXECUTION_FAILED_RETRY_BASE_SECS");
    env_u64!(cfg.reconciliation.execution_failed_retry_max_secs, "RALPHX_RECONCILIATION_EXECUTION_FAILED_RETRY_MAX_SECS");
    env_u64!(cfg.reconciliation.recovery_staleness_secs, "RALPHX_RECONCILIATION_RECOVERY_STALENESS_SECS");
    env_u64!(cfg.reconciliation.merge_circuit_breaker_threshold, "RALPHX_MERGE_CIRCUIT_BREAKER_THRESHOLD");
    env_u64!(cfg.reconciliation.merge_circuit_breaker_window, "RALPHX_MERGE_CIRCUIT_BREAKER_WINDOW");
    env_u64!(cfg.reconciliation.freshness_backoff_base_secs, "RALPHX_RECONCILIATION_FRESHNESS_BACKOFF_BASE_SECS");
    env_u64!(cfg.reconciliation.freshness_backoff_max_secs, "RALPHX_RECONCILIATION_FRESHNESS_BACKOFF_MAX_SECS");
    env_u64!(cfg.reconciliation.freshness_auto_reset_cooldown_secs, "RALPHX_RECONCILIATION_FRESHNESS_AUTO_RESET_COOLDOWN_SECS");
    if let Some(v) = lookup("RALPHX_RECONCILIATION_FRESHNESS_MAX_CONFLICT_RETRIES") {
        if let Ok(n) = v.parse::<u32>() {
            cfg.reconciliation.freshness_max_conflict_retries = n;
        }
    }
    env_u64!(cfg.reconciliation.git_isolation_retry_base_secs, "RALPHX_RECONCILIATION_GIT_ISOLATION_RETRY_BASE_SECS");
    if let Some(v) = lookup("RALPHX_RECONCILIATION_GIT_ISOLATION_MAX_RETRIES") {
        if let Ok(n) = v.parse::<u32>() {
            cfg.reconciliation.git_isolation_max_retries = n;
        }
    }

    validate_reconciliation_config(&mut cfg.reconciliation);

    // Git
    env_u64!(cfg.git.cmd_timeout_secs, "RALPHX_GIT_CMD_TIMEOUT_SECS");
    env_u64!(cfg.git.max_retries, "RALPHX_GIT_MAX_RETRIES");
    env_u64!(
        cfg.git.index_lock_stale_secs,
        "RALPHX_GIT_INDEX_LOCK_STALE_SECS"
    );
    env_u64!(
        cfg.git.agent_kill_settle_secs,
        "RALPHX_GIT_AGENT_KILL_SETTLE_SECS"
    );
    env_u64!(
        cfg.git.agent_stop_timeout_secs,
        "RALPHX_GIT_AGENT_STOP_TIMEOUT_SECS"
    );
    env_u64!(
        cfg.git.cleanup_worktree_timeout_secs,
        "RALPHX_GIT_CLEANUP_WORKTREE_TIMEOUT_SECS"
    );
    env_u64!(
        cfg.git.cleanup_git_op_timeout_secs,
        "RALPHX_GIT_CLEANUP_GIT_OP_TIMEOUT_SECS"
    );
    env_u64!(
        cfg.git.worktree_lsof_timeout_secs,
        "RALPHX_GIT_WORKTREE_LSOF_TIMEOUT_SECS"
    );
    env_u64!(
        cfg.git.step_0b_kill_timeout_secs,
        "RALPHX_GIT_STEP_0B_KILL_TIMEOUT_SECS"
    );
    // retry_backoff_secs: comma-separated list
    if let Some(v) = lookup("RALPHX_GIT_RETRY_BACKOFF_SECS") {
        let parsed: Vec<u64> = v
            .split(',')
            .filter_map(|s| s.trim().parse::<u64>().ok())
            .collect();
        if !parsed.is_empty() {
            cfg.git.retry_backoff_secs = parsed;
        }
    }

    // Scheduler
    env_u64!(
        cfg.scheduler.watchdog_interval_secs,
        "RALPHX_SCHEDULER_WATCHDOG_INTERVAL_SECS"
    );
    env_u64!(
        cfg.scheduler.watchdog_stale_threshold_secs,
        "RALPHX_SCHEDULER_WATCHDOG_STALE_THRESHOLD_SECS"
    );
    env_u64!(
        cfg.scheduler.max_contention_retries,
        "RALPHX_SCHEDULER_MAX_CONTENTION_RETRIES"
    );
    env_u64!(
        cfg.scheduler.contention_retry_delay_ms,
        "RALPHX_SCHEDULER_CONTENTION_RETRY_DELAY_MS"
    );
    env_u64!(
        cfg.scheduler.ready_settle_ms,
        "RALPHX_SCHEDULER_READY_SETTLE_MS"
    );
    env_u64!(
        cfg.scheduler.merge_settle_ms,
        "RALPHX_SCHEDULER_MERGE_SETTLE_MS"
    );

    // Supervisor
    env_u64!(
        cfg.supervisor.time_threshold_secs,
        "RALPHX_SUPERVISOR_TIME_THRESHOLD_SECS"
    );
    env_u64!(
        cfg.supervisor.token_threshold,
        "RALPHX_SUPERVISOR_TOKEN_THRESHOLD"
    );
    env_u64!(cfg.supervisor.max_tokens, "RALPHX_SUPERVISOR_MAX_TOKENS");
    env_u64!(
        cfg.supervisor.progress_interval_secs,
        "RALPHX_SUPERVISOR_PROGRESS_INTERVAL_SECS"
    );
    env_u64!(
        cfg.supervisor.loop_threshold,
        "RALPHX_SUPERVISOR_LOOP_THRESHOLD"
    );
    env_u64!(
        cfg.supervisor.stuck_threshold,
        "RALPHX_SUPERVISOR_STUCK_THRESHOLD"
    );

    // Limits
    env_u64!(
        cfg.limits.max_resume_attempts,
        "RALPHX_LIMITS_MAX_RESUME_ATTEMPTS"
    );

    // Verification
    env_u64!(
        cfg.verification.reconciliation_stale_after_secs,
        "RALPHX_VERIFICATION_RECONCILIATION_STALE_AFTER_SECS"
    );
    env_u64!(
        cfg.verification.reconciliation_interval_secs,
        "RALPHX_VERIFICATION_RECONCILIATION_INTERVAL_SECS"
    );
    env_u64!(
        cfg.verification.auto_verify_stale_secs,
        "RALPHX_VERIFICATION_AUTO_VERIFY_STALE_SECS"
    );
    env_u64!(
        cfg.verification.accept_stale_execution_plan_secs,
        "RALPHX_VERIFICATION_ACCEPT_STALE_EXECUTION_PLAN_SECS"
    );
    if let Some(v) = lookup("RALPHX_VERIFICATION_MAX_ROUNDS") {
        if let Ok(n) = v.parse::<u32>() {
            cfg.verification.max_rounds = n;
        }
    }
    if let Some(v) = lookup("RALPHX_VERIFICATION_COMPLEXITY_THRESHOLD") {
        if let Ok(n) = v.parse::<u32>() {
            cfg.verification.complexity_threshold = n;
        }
    }

    validate_verification_config(&mut cfg.verification);

    // External MCP
    if let Some(v) = lookup("RALPHX_EXTERNAL_MCP_ENABLED") {
        cfg.external_mcp.enabled = matches!(v.to_lowercase().as_str(), "true" | "1");
    }
    if let Some(v) = lookup("RALPHX_EXTERNAL_MCP_PORT") {
        if let Ok(n) = v.parse::<u16>() {
            cfg.external_mcp.port = n;
        }
    }
    if let Some(v) = lookup("RALPHX_EXTERNAL_MCP_HOST") {
        cfg.external_mcp.host = v;
    }
    env_u64!(
        cfg.external_mcp.human_wait_timeout_secs,
        "RALPHX_EXTERNAL_MCP_HUMAN_WAIT_TIMEOUT_SECS"
    );
    if let Some(v) = lookup("RALPHX_NODE_PATH") {
        cfg.external_mcp.node_path = Some(v);
    }
    if let Some(_v) = lookup("RALPHX_EXTERNAL_MCP_MAX_IDEATION_SESSIONS") {
        warn!(
            "RALPHX_EXTERNAL_MCP_MAX_IDEATION_SESSIONS is deprecated and has no effect. \
             The session gate was removed; sessions are always created. Remove this env var."
        );
    }

    // Ideation
    if let Some(v) = lookup("RALPHX_IDEATION_ACTIVITY_THRESHOLD_SECS") {
        if let Ok(n) = v.parse::<u64>() {
            cfg.child_session_activity_threshold_secs = Some(n);
        }
    }

    // UI feature flags
    if let Some(v) = lookup("RALPHX_UI_ACTIVITY_PAGE") {
        cfg.ui_feature_flags.activity_page = matches!(v.to_lowercase().as_str(), "true" | "1");
    }
    if let Some(v) = lookup("RALPHX_UI_EXTENSIBILITY_PAGE") {
        cfg.ui_feature_flags.extensibility_page =
            matches!(v.to_lowercase().as_str(), "true" | "1");
    }
    if let Some(v) = lookup("RALPHX_UI_BATTLE_MODE") {
        cfg.ui_feature_flags.battle_mode = matches!(v.to_lowercase().as_str(), "true" | "1");
    }
    if let Some(v) = lookup("RALPHX_UI_TEAM_MODE") {
        cfg.ui_feature_flags.team_mode =
            matches!(v.to_lowercase().as_str(), "true" | "1");
    }
}

/// Validate ReconciliationConfig fields and clamp to safe defaults on invalid values (GAP M7).
/// Called after env overrides are applied so invalid YAML or env vars are caught.
pub fn validate_reconciliation_config(cfg: &mut ReconciliationConfig) {
    const DEFAULT_BASE: u64 = 30;
    const DEFAULT_MAX: u64 = 600;
    const DEFAULT_MAX_RETRIES: u64 = 3;

    if cfg.execution_failed_max_retries == 0 {
        warn!(
            "execution_failed_max_retries must be > 0, got 0; clamping to {}",
            DEFAULT_MAX_RETRIES
        );
        cfg.execution_failed_max_retries = DEFAULT_MAX_RETRIES;
    }

    if cfg.execution_failed_retry_base_secs > cfg.execution_failed_retry_max_secs {
        warn!(
            "execution_failed_retry_base_secs ({}) > execution_failed_retry_max_secs ({}); \
             clamping to defaults ({}/{})",
            cfg.execution_failed_retry_base_secs,
            cfg.execution_failed_retry_max_secs,
            DEFAULT_BASE,
            DEFAULT_MAX,
        );
        cfg.execution_failed_retry_base_secs = DEFAULT_BASE;
        cfg.execution_failed_retry_max_secs = DEFAULT_MAX;
    }

    if cfg.git_isolation_max_retries == 0 {
        warn!(
            "git_isolation_max_retries must be > 0, got 0; clamping to {}",
            DEFAULT_MAX_RETRIES
        );
        cfg.git_isolation_max_retries = DEFAULT_MAX_RETRIES as u32;
    }

    if cfg.git_isolation_retry_base_secs == 0 {
        warn!("git_isolation_retry_base_secs must be > 0, got 0; clamping to 5");
        cfg.git_isolation_retry_base_secs = 5;
    }
}

/// Validate VerificationConfig fields and clamp to safe defaults on invalid values.
pub fn validate_verification_config(cfg: &mut VerificationConfig) {
    const MIN_ROUNDS: u32 = 1;
    const MAX_ROUNDS: u32 = 10;
    const MIN_INTERVAL_SECS: u64 = 60;

    if cfg.max_rounds < MIN_ROUNDS || cfg.max_rounds > MAX_ROUNDS {
        warn!(
            "verification.max_rounds must be [{}, {}], got {}; clamping",
            MIN_ROUNDS, MAX_ROUNDS, cfg.max_rounds
        );
        cfg.max_rounds = cfg.max_rounds.clamp(MIN_ROUNDS, MAX_ROUNDS);
    }

    if cfg.reconciliation_interval_secs < MIN_INTERVAL_SECS {
        warn!(
            "verification.reconciliation_interval_secs must be >= {}s, got {}; clamping",
            MIN_INTERVAL_SECS, cfg.reconciliation_interval_secs
        );
        cfg.reconciliation_interval_secs = MIN_INTERVAL_SECS;
    }

    if cfg.reconciliation_stale_after_secs == 0 {
        warn!("verification.reconciliation_stale_after_secs must be > 0; clamping to 5400");
        cfg.reconciliation_stale_after_secs = 5400;
    }

    if cfg.auto_verify_stale_secs == 0 {
        warn!("verification.auto_verify_stale_secs must be > 0; clamping to 600");
        cfg.auto_verify_stale_secs = 600;
    }
    if cfg.accept_stale_execution_plan_secs == 0 {
        warn!(
            "verification.accept_stale_execution_plan_secs must be > 0; clamping to 30"
        );
        cfg.accept_stale_execution_plan_secs = 30;
    }
    if cfg.auto_verify_stale_secs >= cfg.reconciliation_stale_after_secs {
        warn!(
            "verification.auto_verify_stale_secs ({}) >= reconciliation_stale_after_secs ({}); \
             auto_verify threshold should be shorter",
            cfg.auto_verify_stale_secs, cfg.reconciliation_stale_after_secs
        );
    }
}

/// Validate ExternalMcpConfig fields.
///
/// # Errors
///
/// Returns an error message if:
/// - `port` is 0 (invalid port number)
/// - `host` is empty
/// - `enabled` is true, host is not local, and TLS env vars are missing
#[allow(dead_code)]
pub fn validate_external_mcp_config(cfg: &ExternalMcpConfig) -> Result<(), String> {
    if cfg.port == 0 {
        return Err("external_mcp.port must be in range 1-65535, got 0".to_string());
    }
    if cfg.host.is_empty() {
        return Err("external_mcp.host must not be empty".to_string());
    }
    if cfg.human_wait_timeout_secs == 0 {
        return Err(
            "external_mcp.human_wait_timeout_secs must be greater than 0".to_string(),
        );
    }
    if cfg.enabled {
        let is_local = cfg.host == "localhost" || cfg.host == "127.0.0.1";
        if !is_local {
            let tls_cert = std::env::var("EXTERNAL_MCP_TLS_CERT").ok();
            let tls_key = std::env::var("EXTERNAL_MCP_TLS_KEY").ok();
            if tls_cert.is_none() || tls_key.is_none() {
                return Err(format!(
                    "external_mcp is enabled with non-local host '{}'; \
                     EXTERNAL_MCP_TLS_CERT and EXTERNAL_MCP_TLS_KEY must be set",
                    cfg.host
                ));
            }
        }
    }
    Ok(())
}

#[cfg(test)]
#[path = "runtime_config_tests.rs"]
mod tests;
