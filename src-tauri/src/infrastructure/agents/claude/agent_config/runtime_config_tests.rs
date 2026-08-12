use super::*;

#[test]
fn test_all_defaults_are_sensible() {
    let cfg = AllRuntimeConfig {
        stream: StreamTimeoutsConfig::default(),
        reconciliation: ReconciliationConfig::default(),
        git: GitRuntimeConfig::default(),
        scheduler: SchedulerConfig::default(),
        supervisor: SupervisorRuntimeConfig::default(),
        limits: LimitsConfig::default(),
        verification: VerificationConfig::default(),
        external_mcp: ExternalMcpConfig::default(),
        child_session_activity_threshold_secs: None,
        ui_feature_flags: Default::default(),
        database_maintenance: DatabaseMaintenanceConfig::default(),
        delegation: DelegationConfig::default(),
    };
    assert_eq!(cfg.stream.merge_line_read_secs, 600);
    assert_eq!(cfg.stream.completion_grace_secs, 30);
    assert_eq!(cfg.stream.agent_completion_correlation_ttl_secs, 60);
    assert_eq!(cfg.stream.agent_completion_correlation_capacity, 1_024);
    assert_eq!(cfg.stream.agent_completion_processed_ttl_secs, 900);
    assert_eq!(cfg.stream.agent_completion_processed_capacity, 4_096);
    assert_eq!(cfg.stream.launch_reservation_lease_secs, 30);
    assert_eq!(cfg.stream.execution_attempt_start_tolerance_secs, 1);
    assert_eq!(cfg.stream.notification_retention_read_days, 30);
    assert_eq!(cfg.stream.notification_retention_max_rows, 1000);
    assert!(cfg.stream.chat_payload_retention_enabled);
    assert_eq!(cfg.stream.chat_payload_retention_days, 90);
    assert_eq!(cfg.stream.chat_payload_retention_archived_days, 7);
    assert_eq!(cfg.stream.chat_payload_retention_batch_rows, 500);
    assert_eq!(
        cfg.stream.chat_payload_size_budget_recommended_bytes,
        5_368_709_120
    );
    assert_eq!(
        cfg.stream.chat_payload_advisory_threshold_bytes,
        10_737_418_240
    );
    assert_eq!(cfg.stream.chat_payload_retention_interval_hours, 6);
    assert_eq!(cfg.stream.chat_payload_retention_batch_pause_ms, 50);
    assert_eq!(cfg.stream.chat_payload_retention_checkpoint_batches, 50);
    assert_eq!(cfg.stream.db_lock_wait_warn_ms, 100);
    assert_eq!(cfg.stream.db_lock_hold_warn_ms, 250);
    assert!(cfg.database_maintenance.db_auto_compact_enabled);
    assert_eq!(
        cfg.database_maintenance.db_auto_compact_max_db_bytes,
        2_147_483_648
    );
    assert_eq!(
        cfg.database_maintenance
            .db_auto_compact_min_freelist_percent,
        20
    );
    assert_eq!(cfg.reconciliation.merger_timeout_secs, 1200);
    assert_eq!(cfg.reconciliation.validation_deadline_secs, 1200);
    assert_eq!(cfg.reconciliation.branch_freshness_timeout_secs, 60);
    assert_eq!(cfg.git.cmd_timeout_secs, 60);
    assert_eq!(cfg.git.startup_auth_preflight_timeout_secs, 10);
    assert_eq!(cfg.git.retry_backoff_secs, vec![1, 2, 4]);
    assert_eq!(cfg.git.provider_probe_cache_ttl_secs, 300);
    assert_eq!(cfg.git.workspace_freshness_cache_ttl_ms, 2_000);
    // Full scope fetches origin and reads PR status per PR-as-base workspace, so it gets a much
    // longer window than the cheap local scope above.
    assert_eq!(cfg.git.workspace_freshness_full_scope_cache_ttl_ms, 30_000);
    assert_eq!(cfg.git.workspace_pr_poll_base_secs, 60);
    assert_eq!(cfg.git.workspace_pr_poll_max_secs, 300);
    assert!(
        cfg.git.workspace_pr_poll_base_secs <= cfg.git.workspace_pr_poll_max_secs,
        "the adaptive poll ceiling must not sit below its base"
    );
    assert_eq!(cfg.git.github_rate_limit_probe_interval_secs, 300);
    assert_eq!(cfg.git.workspace_review_cache_ttl_ms, 2_000);
    assert_eq!(cfg.git.workspace_pr_description_cache_ttl_ms, 300_000);
    assert_eq!(cfg.git.workspace_pr_annotations_cache_ttl_ms, 30_000);
    assert_eq!(cfg.git.workspace_pr_annotations_check_run_fetch_limit, 10);
    assert_eq!(
        cfg.git.agent_workspace_pr_reconciliation_cache_ttl_ms,
        30_000
    );
    assert_eq!(cfg.git.agent_workspace_publish_lease_stale_secs, 300);
    assert_eq!(
        cfg.git
            .agent_workspace_publish_lease_heartbeat_interval_secs,
        30
    );
    assert_eq!(cfg.git.agent_workspace_publish_recovery_interval_secs, 120);
    assert_eq!(
        cfg.git
            .agent_workspace_repair_reconciliation_scan_interval_secs,
        60
    );
    assert_eq!(default_agent_workspace_publish_lease_stale_secs(), 300);
    assert_eq!(
        default_agent_workspace_publish_lease_heartbeat_interval_secs(),
        30
    );
    assert_eq!(
        default_agent_workspace_publish_recovery_interval_secs(),
        120
    );
    assert_eq!(
        default_agent_workspace_repair_reconciliation_scan_interval_secs(),
        60
    );
    assert_eq!(cfg.git.terminal_pr_local_cleanup_interval_secs, 900);
    assert_eq!(cfg.git.terminal_pr_local_cleanup_retry_secs, 3_600);
    assert_eq!(cfg.git.orphan_worktree_cleanup_marker_retry_secs, 86_400);
    assert_eq!(cfg.git.orphan_worktree_cleanup_interval_secs, 900);
    assert_eq!(cfg.scheduler.watchdog_interval_secs, 60);
    assert_eq!(cfg.supervisor.time_threshold_secs, 600);
    assert_eq!(cfg.limits.max_resume_attempts, 5);
}

/// Merge speed overhaul: verify reduced timeout defaults for faster merge pipeline.
#[test]
fn test_merge_speed_defaults() {
    let recon = ReconciliationConfig::default();
    let git = GitRuntimeConfig::default();

    // Reconciliation — merge-speed targets
    assert_eq!(
        recon.attempt_merge_deadline_secs, 120,
        "merge deadline: 600→120"
    );
    assert_eq!(
        recon.merge_incomplete_retry_base_secs, 5,
        "retry base: 30→5"
    );

    // Git — agent cleanup speed targets
    assert_eq!(git.agent_stop_timeout_secs, 3, "agent stop: 10→3");
    assert_eq!(git.agent_kill_settle_secs, 0, "kill settle: 1→0");
    assert_eq!(
        git.cleanup_worktree_timeout_secs, 15,
        "worktree cleanup: 5→15 for TOCTOU fix"
    );
    assert_eq!(git.step_0b_kill_timeout_secs, 5, "step 0b kill: 20→5");
}

#[test]
fn test_merge_attempt_deadline_exceeds_single_git_command_timeout() {
    let recon = ReconciliationConfig::default();
    let git = GitRuntimeConfig::default();

    assert!(
        recon.attempt_merge_deadline_secs > git.cmd_timeout_secs,
        "outer merge attempt deadline must exceed one git command timeout; rebase-squash runs multiple git commands"
    );
}

/// Verify env overrides still work for the changed merge-speed fields.
#[test]
fn test_merge_speed_env_overrides() {
    let mut cfg = AllRuntimeConfig {
        stream: StreamTimeoutsConfig::default(),
        reconciliation: ReconciliationConfig::default(),
        git: GitRuntimeConfig::default(),
        scheduler: SchedulerConfig::default(),
        supervisor: SupervisorRuntimeConfig::default(),
        limits: LimitsConfig::default(),
        verification: VerificationConfig::default(),
        external_mcp: ExternalMcpConfig::default(),
        child_session_activity_threshold_secs: None,
        ui_feature_flags: Default::default(),
        database_maintenance: DatabaseMaintenanceConfig::default(),
        delegation: DelegationConfig::default(),
    };

    apply_env_overrides_with(&mut cfg, &|name| match name {
        "RALPHX_RECONCILIATION_ATTEMPT_MERGE_DEADLINE_SECS" => Some("90".to_string()),
        "RALPHX_RECONCILIATION_MERGE_INCOMPLETE_RETRY_BASE_SECS" => Some("10".to_string()),
        "RALPHX_GIT_AGENT_STOP_TIMEOUT_SECS" => Some("7".to_string()),
        "RALPHX_GIT_AGENT_KILL_SETTLE_SECS" => Some("2".to_string()),
        "RALPHX_GIT_CLEANUP_WORKTREE_TIMEOUT_SECS" => Some("8".to_string()),
        "RALPHX_GIT_STEP_0B_KILL_TIMEOUT_SECS" => Some("12".to_string()),
        _ => None,
    });

    assert_eq!(cfg.reconciliation.attempt_merge_deadline_secs, 90);
    assert_eq!(cfg.reconciliation.merge_incomplete_retry_base_secs, 10);
    assert_eq!(cfg.git.agent_stop_timeout_secs, 7);
    assert_eq!(cfg.git.agent_kill_settle_secs, 2);
    assert_eq!(cfg.git.cleanup_worktree_timeout_secs, 8);
    assert_eq!(cfg.git.step_0b_kill_timeout_secs, 12);
}

#[test]
fn test_env_overrides_apply() {
    let mut cfg = AllRuntimeConfig {
        stream: StreamTimeoutsConfig::default(),
        reconciliation: ReconciliationConfig::default(),
        git: GitRuntimeConfig::default(),
        scheduler: SchedulerConfig::default(),
        supervisor: SupervisorRuntimeConfig::default(),
        limits: LimitsConfig::default(),
        verification: VerificationConfig::default(),
        external_mcp: ExternalMcpConfig::default(),
        child_session_activity_threshold_secs: None,
        ui_feature_flags: Default::default(),
        database_maintenance: DatabaseMaintenanceConfig::default(),
        delegation: DelegationConfig::default(),
    };

    apply_env_overrides_with(&mut cfg, &|name| match name {
        "RALPHX_STREAM_MERGE_LINE_READ_SECS" => Some("999".to_string()),
        "RALPHX_STREAM_COMPLETION_GRACE_SECS" => Some("45".to_string()),
        "RALPHX_STREAM_AGENT_COMPLETION_CORRELATION_TTL_SECS" => Some("75".to_string()),
        "RALPHX_STREAM_AGENT_COMPLETION_CORRELATION_CAPACITY" => Some("512".to_string()),
        "RALPHX_STREAM_AGENT_COMPLETION_PROCESSED_TTL_SECS" => Some("600".to_string()),
        "RALPHX_STREAM_AGENT_COMPLETION_PROCESSED_CAPACITY" => Some("2048".to_string()),
        "RALPHX_STREAM_LAUNCH_RESERVATION_LEASE_SECS" => Some("60".to_string()),
        "RALPHX_STREAM_NOTIFICATION_RETENTION_READ_DAYS" => Some("14".to_string()),
        "RALPHX_STREAM_NOTIFICATION_RETENTION_MAX_ROWS" => Some("250".to_string()),
        "RALPHX_STREAM_CHAT_PAYLOAD_RETENTION_ENABLED" => Some("false".to_string()),
        "RALPHX_STREAM_CHAT_PAYLOAD_RETENTION_DAYS" => Some("21".to_string()),
        "RALPHX_STREAM_CHAT_PAYLOAD_RETENTION_ARCHIVED_DAYS" => Some("3".to_string()),
        "RALPHX_STREAM_CHAT_PAYLOAD_RETENTION_BATCH_ROWS" => Some("17".to_string()),
        "RALPHX_STREAM_CHAT_PAYLOAD_SIZE_BUDGET_RECOMMENDED_BYTES" => Some("268435456".to_string()),
        "RALPHX_STREAM_CHAT_PAYLOAD_ADVISORY_THRESHOLD_BYTES" => Some("536870912".to_string()),
        "RALPHX_STREAM_CHAT_PAYLOAD_RETENTION_INTERVAL_HOURS" => Some("2".to_string()),
        "RALPHX_STREAM_CHAT_PAYLOAD_RETENTION_BATCH_PAUSE_MS" => Some("5".to_string()),
        "RALPHX_STREAM_CHAT_PAYLOAD_RETENTION_CHECKPOINT_BATCHES" => Some("9".to_string()),
        "RALPHX_STREAM_DB_LOCK_WAIT_WARN_MS" => Some("25".to_string()),
        "RALPHX_STREAM_DB_LOCK_HOLD_WARN_MS" => Some("75".to_string()),
        "RALPHX_DB_AUTO_COMPACT_ENABLED" => Some("false".to_string()),
        "RALPHX_DB_AUTO_COMPACT_MAX_DB_BYTES" => Some("1048576".to_string()),
        "RALPHX_DB_AUTO_COMPACT_MIN_FREELIST_PERCENT" => Some("35".to_string()),
        "RALPHX_RECONCILIATION_MERGER_TIMEOUT_SECS" => Some("2400".to_string()),
        "RALPHX_GIT_CMD_TIMEOUT_SECS" => Some("120".to_string()),
        "RALPHX_GIT_STARTUP_AUTH_PREFLIGHT_TIMEOUT_SECS" => Some("9".to_string()),
        "RALPHX_GIT_RETRY_BACKOFF_SECS" => Some("2,4,8,16".to_string()),
        "RALPHX_GIT_PROVIDER_PROBE_CACHE_TTL_SECS" => Some("120".to_string()),
        "RALPHX_GIT_WORKSPACE_FRESHNESS_CACHE_TTL_MS" => Some("750".to_string()),
        "RALPHX_GIT_WORKSPACE_REVIEW_CACHE_TTL_MS" => Some("900".to_string()),
        "RALPHX_GIT_WORKSPACE_PR_DESCRIPTION_CACHE_TTL_MS" => Some("1200".to_string()),
        "RALPHX_GIT_WORKSPACE_PR_ANNOTATIONS_CACHE_TTL_MS" => Some("45000".to_string()),
        "RALPHX_GIT_WORKSPACE_PR_ANNOTATIONS_CHECK_RUN_FETCH_LIMIT" => Some("7".to_string()),
        "RALPHX_GIT_AGENT_WORKSPACE_PR_RECONCILIATION_CACHE_TTL_MS" => Some("45000".to_string()),
        "RALPHX_GIT_AGENT_WORKSPACE_PUBLISH_LEASE_STALE_SECS" => Some("600".to_string()),
        "RALPHX_GIT_AGENT_WORKSPACE_PUBLISH_LEASE_HEARTBEAT_INTERVAL_SECS" => {
            Some("45".to_string())
        }
        "RALPHX_GIT_AGENT_WORKSPACE_PUBLISH_RECOVERY_INTERVAL_SECS" => Some("180".to_string()),
        "RALPHX_GIT_AGENT_WORKSPACE_REPAIR_RECONCILIATION_SCAN_INTERVAL_SECS" => {
            Some("90".to_string())
        }
        "RALPHX_GIT_TERMINAL_PR_LOCAL_CLEANUP_INTERVAL_SECS" => Some("300".to_string()),
        "RALPHX_GIT_TERMINAL_PR_LOCAL_CLEANUP_RETRY_SECS" => Some("1800".to_string()),
        "RALPHX_GIT_ORPHAN_WORKTREE_CLEANUP_MARKER_RETRY_SECS" => Some("3600".to_string()),
        "RALPHX_GIT_ORPHAN_WORKTREE_CLEANUP_INTERVAL_SECS" => Some("600".to_string()),
        "RALPHX_SCHEDULER_READY_SETTLE_MS" => Some("500".to_string()),
        "RALPHX_SUPERVISOR_MAX_TOKENS" => Some("200000".to_string()),
        "RALPHX_LIMITS_MAX_RESUME_ATTEMPTS" => Some("10".to_string()),
        _ => None,
    });

    assert_eq!(cfg.stream.merge_line_read_secs, 999);
    assert_eq!(cfg.stream.completion_grace_secs, 45);
    assert_eq!(cfg.stream.agent_completion_correlation_ttl_secs, 75);
    assert_eq!(cfg.stream.agent_completion_correlation_capacity, 512);
    assert_eq!(cfg.stream.agent_completion_processed_ttl_secs, 600);
    assert_eq!(cfg.stream.agent_completion_processed_capacity, 2_048);
    assert_eq!(cfg.stream.launch_reservation_lease_secs, 60);
    assert_eq!(cfg.stream.notification_retention_read_days, 14);
    assert_eq!(cfg.stream.notification_retention_max_rows, 250);
    assert!(!cfg.stream.chat_payload_retention_enabled);
    assert_eq!(cfg.stream.chat_payload_retention_days, 21);
    assert_eq!(cfg.stream.chat_payload_retention_archived_days, 3);
    assert_eq!(cfg.stream.chat_payload_retention_batch_rows, 17);
    assert_eq!(
        cfg.stream.chat_payload_size_budget_recommended_bytes,
        268_435_456
    );
    assert_eq!(
        cfg.stream.chat_payload_advisory_threshold_bytes,
        536_870_912
    );
    assert_eq!(cfg.stream.chat_payload_retention_interval_hours, 2);
    assert_eq!(cfg.stream.chat_payload_retention_batch_pause_ms, 5);
    assert_eq!(cfg.stream.chat_payload_retention_checkpoint_batches, 9);
    assert_eq!(cfg.stream.db_lock_wait_warn_ms, 25);
    assert_eq!(cfg.stream.db_lock_hold_warn_ms, 75);
    assert!(!cfg.database_maintenance.db_auto_compact_enabled);
    assert_eq!(
        cfg.database_maintenance.db_auto_compact_max_db_bytes,
        1_048_576
    );
    assert_eq!(
        cfg.database_maintenance
            .db_auto_compact_min_freelist_percent,
        35
    );
    assert_eq!(cfg.reconciliation.merger_timeout_secs, 2400);
    // validation_deadline_secs not overridden — should keep default
    assert_eq!(cfg.reconciliation.validation_deadline_secs, 1200);
    assert_eq!(cfg.git.cmd_timeout_secs, 120);
    assert_eq!(cfg.git.startup_auth_preflight_timeout_secs, 9);
    assert_eq!(cfg.git.retry_backoff_secs, vec![2, 4, 8, 16]);
    assert_eq!(cfg.git.provider_probe_cache_ttl_secs, 120);
    assert_eq!(cfg.git.workspace_freshness_cache_ttl_ms, 750);
    assert_eq!(cfg.git.workspace_review_cache_ttl_ms, 900);
    assert_eq!(cfg.git.workspace_pr_description_cache_ttl_ms, 1200);
    assert_eq!(cfg.git.workspace_pr_annotations_cache_ttl_ms, 45_000);
    assert_eq!(cfg.git.workspace_pr_annotations_check_run_fetch_limit, 7);
    assert_eq!(
        cfg.git.agent_workspace_pr_reconciliation_cache_ttl_ms,
        45_000
    );
    assert_eq!(cfg.git.agent_workspace_publish_lease_stale_secs, 600);
    assert_eq!(
        cfg.git
            .agent_workspace_publish_lease_heartbeat_interval_secs,
        45
    );
    assert_eq!(cfg.git.agent_workspace_publish_recovery_interval_secs, 180);
    assert_eq!(
        cfg.git
            .agent_workspace_repair_reconciliation_scan_interval_secs,
        90
    );
    assert_eq!(cfg.git.terminal_pr_local_cleanup_interval_secs, 300);
    assert_eq!(cfg.git.terminal_pr_local_cleanup_retry_secs, 1800);
    assert_eq!(cfg.git.orphan_worktree_cleanup_marker_retry_secs, 3600);
    assert_eq!(cfg.git.orphan_worktree_cleanup_interval_secs, 600);
    assert_eq!(cfg.scheduler.ready_settle_ms, 500);
    assert_eq!(cfg.supervisor.max_tokens, 200000);
    assert_eq!(cfg.limits.max_resume_attempts, 10);
}

#[test]
fn test_backward_compat_merger_timeout_env() {
    let mut cfg = AllRuntimeConfig {
        stream: StreamTimeoutsConfig::default(),
        reconciliation: ReconciliationConfig::default(),
        git: GitRuntimeConfig::default(),
        scheduler: SchedulerConfig::default(),
        supervisor: SupervisorRuntimeConfig::default(),
        limits: LimitsConfig::default(),
        verification: VerificationConfig::default(),
        external_mcp: ExternalMcpConfig::default(),
        child_session_activity_threshold_secs: None,
        ui_feature_flags: Default::default(),
        database_maintenance: DatabaseMaintenanceConfig::default(),
        delegation: DelegationConfig::default(),
    };

    // Old key only
    apply_env_overrides_with(&mut cfg, &|name| match name {
        "RALPHX_MERGER_TIMEOUT_SECS" => Some("1800".to_string()),
        _ => None,
    });
    assert_eq!(cfg.reconciliation.merger_timeout_secs, 1800);
}

#[test]
fn test_new_key_takes_precedence_over_old() {
    let mut cfg = AllRuntimeConfig {
        stream: StreamTimeoutsConfig::default(),
        reconciliation: ReconciliationConfig::default(),
        git: GitRuntimeConfig::default(),
        scheduler: SchedulerConfig::default(),
        supervisor: SupervisorRuntimeConfig::default(),
        limits: LimitsConfig::default(),
        verification: VerificationConfig::default(),
        external_mcp: ExternalMcpConfig::default(),
        child_session_activity_threshold_secs: None,
        ui_feature_flags: Default::default(),
        database_maintenance: DatabaseMaintenanceConfig::default(),
        delegation: DelegationConfig::default(),
    };

    // Both keys set — new one should win (applied second)
    apply_env_overrides_with(&mut cfg, &|name| match name {
        "RALPHX_MERGER_TIMEOUT_SECS" => Some("1800".to_string()),
        "RALPHX_RECONCILIATION_MERGER_TIMEOUT_SECS" => Some("2400".to_string()),
        _ => None,
    });
    assert_eq!(cfg.reconciliation.merger_timeout_secs, 2400);
}

#[test]
fn test_invalid_env_values_ignored() {
    let mut cfg = AllRuntimeConfig {
        stream: StreamTimeoutsConfig::default(),
        reconciliation: ReconciliationConfig::default(),
        git: GitRuntimeConfig::default(),
        scheduler: SchedulerConfig::default(),
        supervisor: SupervisorRuntimeConfig::default(),
        limits: LimitsConfig::default(),
        verification: VerificationConfig::default(),
        external_mcp: ExternalMcpConfig::default(),
        child_session_activity_threshold_secs: None,
        ui_feature_flags: Default::default(),
        database_maintenance: DatabaseMaintenanceConfig::default(),
        delegation: DelegationConfig::default(),
    };

    apply_env_overrides_with(&mut cfg, &|name| match name {
        "RALPHX_STREAM_MERGE_LINE_READ_SECS" => Some("not_a_number".to_string()),
        "RALPHX_GIT_RETRY_BACKOFF_SECS" => Some("".to_string()),
        _ => None,
    });

    // Should keep defaults
    assert_eq!(cfg.stream.merge_line_read_secs, 600);
    assert_eq!(cfg.git.retry_backoff_secs, vec![1, 2, 4]);
}

#[test]
fn test_validation_deadline_env_override() {
    let mut cfg = AllRuntimeConfig {
        stream: StreamTimeoutsConfig::default(),
        reconciliation: ReconciliationConfig::default(),
        git: GitRuntimeConfig::default(),
        scheduler: SchedulerConfig::default(),
        supervisor: SupervisorRuntimeConfig::default(),
        limits: LimitsConfig::default(),
        verification: VerificationConfig::default(),
        external_mcp: ExternalMcpConfig::default(),
        child_session_activity_threshold_secs: None,
        ui_feature_flags: Default::default(),
        database_maintenance: DatabaseMaintenanceConfig::default(),
        delegation: DelegationConfig::default(),
    };

    apply_env_overrides_with(&mut cfg, &|name| match name {
        "RALPHX_RECONCILIATION_VALIDATION_DEADLINE_SECS" => Some("900".to_string()),
        _ => None,
    });

    assert_eq!(cfg.reconciliation.validation_deadline_secs, 900);
    // merge deadline should remain unchanged
    assert_eq!(cfg.reconciliation.attempt_merge_deadline_secs, 120);
}

#[test]
fn test_yaml_deserialization_requires_all_fields() {
    // Partial YAML should fail — all fields are required (no serde defaults)
    let partial_yaml = "merge_line_read_secs: 900";
    let result: Result<StreamTimeoutsConfig, _> = serde_yaml::from_str(partial_yaml);
    assert!(
        result.is_err(),
        "partial YAML should fail without serde defaults"
    );
}

#[test]
fn test_yaml_deserialization_with_all_fields() {
    let yaml = r#"
merge_line_read_secs: 900
merge_parse_stall_secs: 180
review_line_read_secs: 600
review_parse_stall_secs: 120
default_line_read_secs: 600
default_parse_stall_secs: 180
"#;
    let cfg: StreamTimeoutsConfig = serde_yaml::from_str(yaml).unwrap();
    assert_eq!(cfg.merge_line_read_secs, 900);
    assert_eq!(cfg.merge_parse_stall_secs, 180);
    assert_eq!(cfg.completion_grace_secs, 30);
    assert_eq!(cfg.launch_reservation_lease_secs, 30);
    assert_eq!(cfg.execution_attempt_start_tolerance_secs, 1);
}

#[test]
fn test_branch_freshness_timeout_env_override() {
    let mut cfg = AllRuntimeConfig {
        stream: StreamTimeoutsConfig::default(),
        reconciliation: ReconciliationConfig::default(),
        git: GitRuntimeConfig::default(),
        scheduler: SchedulerConfig::default(),
        supervisor: SupervisorRuntimeConfig::default(),
        limits: LimitsConfig::default(),
        verification: VerificationConfig::default(),
        external_mcp: ExternalMcpConfig::default(),
        child_session_activity_threshold_secs: None,
        ui_feature_flags: Default::default(),
        database_maintenance: DatabaseMaintenanceConfig::default(),
        delegation: DelegationConfig::default(),
    };

    apply_env_overrides_with(&mut cfg, &|name| match name {
        "RALPHX_RECONCILIATION_BRANCH_FRESHNESS_TIMEOUT_SECS" => Some("120".to_string()),
        _ => None,
    });

    assert_eq!(cfg.reconciliation.branch_freshness_timeout_secs, 120);
    // Other reconciliation fields should remain unchanged
    assert_eq!(cfg.reconciliation.attempt_merge_deadline_secs, 120);
}

// ── Execution recovery config defaults + validation (GAP M7) ──────────────────

#[test]
fn test_execution_failed_config_defaults_are_sensible() {
    let recon = ReconciliationConfig::default();

    assert_eq!(
        recon.execution_failed_max_retries, 3,
        "default max retries: 3"
    );
    assert_eq!(
        recon.execution_failed_retry_base_secs, 30,
        "default base: 30s"
    );
    assert_eq!(
        recon.execution_failed_retry_max_secs, 600,
        "default max: 600s"
    );
}

/// GAP M7: base_secs must be ≤ max_secs in default config.
#[test]
fn test_execution_failed_retry_base_le_max_in_defaults() {
    let recon = ReconciliationConfig::default();
    assert!(
        recon.execution_failed_retry_base_secs <= recon.execution_failed_retry_max_secs,
        "base ({}) must be ≤ max ({})",
        recon.execution_failed_retry_base_secs,
        recon.execution_failed_retry_max_secs
    );
}

#[test]
fn test_execution_failed_max_retries_is_positive() {
    let recon = ReconciliationConfig::default();
    assert!(
        recon.execution_failed_max_retries > 0,
        "execution_failed_max_retries must be > 0"
    );
}

#[test]
fn test_execution_failed_max_retries_env_override() {
    let mut cfg = AllRuntimeConfig {
        stream: StreamTimeoutsConfig::default(),
        reconciliation: ReconciliationConfig::default(),
        git: GitRuntimeConfig::default(),
        scheduler: SchedulerConfig::default(),
        supervisor: SupervisorRuntimeConfig::default(),
        limits: LimitsConfig::default(),
        verification: VerificationConfig::default(),
        external_mcp: ExternalMcpConfig::default(),
        child_session_activity_threshold_secs: None,
        ui_feature_flags: Default::default(),
        database_maintenance: DatabaseMaintenanceConfig::default(),
        delegation: DelegationConfig::default(),
    };

    apply_env_overrides_with(&mut cfg, &|name| match name {
        "RALPHX_RECONCILIATION_EXECUTION_FAILED_MAX_RETRIES" => Some("5".to_string()),
        _ => None,
    });

    assert_eq!(cfg.reconciliation.execution_failed_max_retries, 5);
    // Other fields remain unchanged
    assert_eq!(cfg.reconciliation.execution_failed_retry_base_secs, 30);
}

#[test]
fn test_execution_failed_retry_base_secs_env_override() {
    let mut cfg = AllRuntimeConfig {
        stream: StreamTimeoutsConfig::default(),
        reconciliation: ReconciliationConfig::default(),
        git: GitRuntimeConfig::default(),
        scheduler: SchedulerConfig::default(),
        supervisor: SupervisorRuntimeConfig::default(),
        limits: LimitsConfig::default(),
        verification: VerificationConfig::default(),
        external_mcp: ExternalMcpConfig::default(),
        child_session_activity_threshold_secs: None,
        ui_feature_flags: Default::default(),
        database_maintenance: DatabaseMaintenanceConfig::default(),
        delegation: DelegationConfig::default(),
    };

    apply_env_overrides_with(&mut cfg, &|name| match name {
        "RALPHX_RECONCILIATION_EXECUTION_FAILED_RETRY_BASE_SECS" => Some("60".to_string()),
        _ => None,
    });

    assert_eq!(cfg.reconciliation.execution_failed_retry_base_secs, 60);
}

#[test]
fn test_execution_failed_retry_max_secs_env_override() {
    let mut cfg = AllRuntimeConfig {
        stream: StreamTimeoutsConfig::default(),
        reconciliation: ReconciliationConfig::default(),
        git: GitRuntimeConfig::default(),
        scheduler: SchedulerConfig::default(),
        supervisor: SupervisorRuntimeConfig::default(),
        limits: LimitsConfig::default(),
        verification: VerificationConfig::default(),
        external_mcp: ExternalMcpConfig::default(),
        child_session_activity_threshold_secs: None,
        ui_feature_flags: Default::default(),
        database_maintenance: DatabaseMaintenanceConfig::default(),
        delegation: DelegationConfig::default(),
    };

    apply_env_overrides_with(&mut cfg, &|name| match name {
        "RALPHX_RECONCILIATION_EXECUTION_FAILED_RETRY_MAX_SECS" => Some("1200".to_string()),
        _ => None,
    });

    assert_eq!(cfg.reconciliation.execution_failed_retry_max_secs, 1200);
    // Base unchanged
    assert_eq!(cfg.reconciliation.execution_failed_retry_base_secs, 30);
}

#[test]
fn test_execution_failed_all_three_env_overrides_applied_together() {
    let mut cfg = AllRuntimeConfig {
        stream: StreamTimeoutsConfig::default(),
        reconciliation: ReconciliationConfig::default(),
        git: GitRuntimeConfig::default(),
        scheduler: SchedulerConfig::default(),
        supervisor: SupervisorRuntimeConfig::default(),
        limits: LimitsConfig::default(),
        verification: VerificationConfig::default(),
        external_mcp: ExternalMcpConfig::default(),
        child_session_activity_threshold_secs: None,
        ui_feature_flags: Default::default(),
        database_maintenance: DatabaseMaintenanceConfig::default(),
        delegation: DelegationConfig::default(),
    };

    apply_env_overrides_with(&mut cfg, &|name| match name {
        "RALPHX_RECONCILIATION_EXECUTION_FAILED_MAX_RETRIES" => Some("5".to_string()),
        "RALPHX_RECONCILIATION_EXECUTION_FAILED_RETRY_BASE_SECS" => Some("45".to_string()),
        "RALPHX_RECONCILIATION_EXECUTION_FAILED_RETRY_MAX_SECS" => Some("900".to_string()),
        _ => None,
    });

    assert_eq!(cfg.reconciliation.execution_failed_max_retries, 5);
    assert_eq!(cfg.reconciliation.execution_failed_retry_base_secs, 45);
    assert_eq!(cfg.reconciliation.execution_failed_retry_max_secs, 900);

    // GAP M7 validation: base still ≤ max after overrides
    assert!(
        cfg.reconciliation.execution_failed_retry_base_secs
            <= cfg.reconciliation.execution_failed_retry_max_secs
    );
}

#[test]
fn test_circuit_breaker_config_defaults() {
    let config = ReconciliationConfig::default();
    assert_eq!(config.merge_circuit_breaker_threshold, 3);
    assert_eq!(config.merge_circuit_breaker_window, 5);
}

#[test]
fn test_circuit_breaker_env_overrides() {
    let mut cfg = AllRuntimeConfig {
        stream: StreamTimeoutsConfig::default(),
        reconciliation: ReconciliationConfig::default(),
        git: GitRuntimeConfig::default(),
        scheduler: SchedulerConfig::default(),
        supervisor: SupervisorRuntimeConfig::default(),
        limits: LimitsConfig::default(),
        verification: VerificationConfig::default(),
        external_mcp: ExternalMcpConfig::default(),
        child_session_activity_threshold_secs: None,
        ui_feature_flags: Default::default(),
        database_maintenance: DatabaseMaintenanceConfig::default(),
        delegation: DelegationConfig::default(),
    };

    apply_env_overrides_with(&mut cfg, &|name| match name {
        "RALPHX_MERGE_CIRCUIT_BREAKER_THRESHOLD" => Some("5".to_string()),
        "RALPHX_MERGE_CIRCUIT_BREAKER_WINDOW" => Some("10".to_string()),
        _ => None,
    });

    assert_eq!(cfg.reconciliation.merge_circuit_breaker_threshold, 5);
    assert_eq!(cfg.reconciliation.merge_circuit_breaker_window, 10);
}

#[test]
fn test_execution_failed_invalid_env_values_keep_defaults() {
    let mut cfg = AllRuntimeConfig {
        stream: StreamTimeoutsConfig::default(),
        reconciliation: ReconciliationConfig::default(),
        git: GitRuntimeConfig::default(),
        scheduler: SchedulerConfig::default(),
        supervisor: SupervisorRuntimeConfig::default(),
        limits: LimitsConfig::default(),
        verification: VerificationConfig::default(),
        external_mcp: ExternalMcpConfig::default(),
        child_session_activity_threshold_secs: None,
        ui_feature_flags: Default::default(),
        database_maintenance: DatabaseMaintenanceConfig::default(),
        delegation: DelegationConfig::default(),
    };

    apply_env_overrides_with(&mut cfg, &|name| match name {
        "RALPHX_RECONCILIATION_EXECUTION_FAILED_MAX_RETRIES" => Some("not_a_number".to_string()),
        "RALPHX_RECONCILIATION_EXECUTION_FAILED_RETRY_BASE_SECS" => Some("abc".to_string()),
        _ => None,
    });

    // Invalid values ignored — defaults preserved
    assert_eq!(cfg.reconciliation.execution_failed_max_retries, 3);
    assert_eq!(cfg.reconciliation.execution_failed_retry_base_secs, 30);
}

// ── ExternalMcpConfig tests ───────────────────────────────────────────────

#[test]
fn test_external_mcp_config_defaults() {
    let cfg = ExternalMcpConfig::default();
    assert!(!cfg.enabled);
    assert_eq!(cfg.port, 3848);
    assert_eq!(cfg.host, "127.0.0.1");
    assert_eq!(cfg.max_restart_attempts, 3);
    assert_eq!(cfg.restart_delay_ms, 2000);
    assert_eq!(cfg.shutdown_grace_ms, 2000);
    assert_eq!(cfg.startup_timeout_secs, 30);
    assert_eq!(cfg.human_wait_timeout_secs, 285);
    assert!(cfg.auth_token.is_none());
    assert!(cfg.node_path.is_none());
}

#[test]
fn test_external_mcp_env_override_shutdown_grace() {
    let mut cfg = AllRuntimeConfig {
        stream: StreamTimeoutsConfig::default(),
        reconciliation: ReconciliationConfig::default(),
        git: GitRuntimeConfig::default(),
        scheduler: SchedulerConfig::default(),
        supervisor: SupervisorRuntimeConfig::default(),
        limits: LimitsConfig::default(),
        verification: VerificationConfig::default(),
        external_mcp: ExternalMcpConfig::default(),
        child_session_activity_threshold_secs: None,
        ui_feature_flags: Default::default(),
        database_maintenance: DatabaseMaintenanceConfig::default(),
        delegation: DelegationConfig::default(),
    };

    apply_env_overrides_with(&mut cfg, &|name| match name {
        "RALPHX_EXTERNAL_MCP_SHUTDOWN_GRACE_MS" => Some("750".to_string()),
        _ => None,
    });

    assert_eq!(cfg.external_mcp.shutdown_grace_ms, 750);
}

#[test]
fn shutdown_watchdog_config_defaults_and_accepts_env_override() {
    let mut config = ShutdownConfig::default();
    assert_eq!(config.watchdog_deadline_secs, 20);

    apply_shutdown_env_overrides_with_lookup(&mut config, &|name| match name {
        "RALPHX_SHUTDOWN_WATCHDOG_DEADLINE_SECS" => Some("35".to_string()),
        _ => None,
    });

    assert_eq!(config.watchdog_deadline_secs, 35);
    assert_eq!(bounded_shutdown_watchdog_deadline_secs(0), 20);
    assert_eq!(bounded_shutdown_watchdog_deadline_secs(u64::MAX), 300);
    assert_eq!(bounded_external_mcp_shutdown_grace_ms(u64::MAX), 30_000);
}

#[test]
fn test_external_mcp_env_overrides_enabled_true() {
    let mut cfg = AllRuntimeConfig {
        stream: StreamTimeoutsConfig::default(),
        reconciliation: ReconciliationConfig::default(),
        git: GitRuntimeConfig::default(),
        scheduler: SchedulerConfig::default(),
        supervisor: SupervisorRuntimeConfig::default(),
        limits: LimitsConfig::default(),
        verification: VerificationConfig::default(),
        external_mcp: ExternalMcpConfig::default(),
        child_session_activity_threshold_secs: None,
        ui_feature_flags: Default::default(),
        database_maintenance: DatabaseMaintenanceConfig::default(),
        delegation: DelegationConfig::default(),
    };

    apply_env_overrides_with(&mut cfg, &|name| match name {
        "RALPHX_EXTERNAL_MCP_ENABLED" => Some("true".to_string()),
        _ => None,
    });
    assert!(cfg.external_mcp.enabled);
}

#[test]
fn test_external_mcp_env_overrides_enabled_one() {
    let mut cfg = AllRuntimeConfig {
        stream: StreamTimeoutsConfig::default(),
        reconciliation: ReconciliationConfig::default(),
        git: GitRuntimeConfig::default(),
        scheduler: SchedulerConfig::default(),
        supervisor: SupervisorRuntimeConfig::default(),
        limits: LimitsConfig::default(),
        verification: VerificationConfig::default(),
        external_mcp: ExternalMcpConfig::default(),
        child_session_activity_threshold_secs: None,
        ui_feature_flags: Default::default(),
        database_maintenance: DatabaseMaintenanceConfig::default(),
        delegation: DelegationConfig::default(),
    };

    apply_env_overrides_with(&mut cfg, &|name| match name {
        "RALPHX_EXTERNAL_MCP_ENABLED" => Some("1".to_string()),
        _ => None,
    });
    assert!(cfg.external_mcp.enabled);
}

#[test]
fn test_external_mcp_env_overrides_enabled_false() {
    let mut cfg = AllRuntimeConfig {
        stream: StreamTimeoutsConfig::default(),
        reconciliation: ReconciliationConfig::default(),
        git: GitRuntimeConfig::default(),
        scheduler: SchedulerConfig::default(),
        supervisor: SupervisorRuntimeConfig::default(),
        limits: LimitsConfig::default(),
        verification: VerificationConfig::default(),
        external_mcp: ExternalMcpConfig {
            enabled: true,
            ..ExternalMcpConfig::default()
        },
        child_session_activity_threshold_secs: None,
        ui_feature_flags: Default::default(),
        database_maintenance: DatabaseMaintenanceConfig::default(),
        delegation: DelegationConfig::default(),
    };

    apply_env_overrides_with(&mut cfg, &|name| match name {
        "RALPHX_EXTERNAL_MCP_ENABLED" => Some("false".to_string()),
        _ => None,
    });
    assert!(!cfg.external_mcp.enabled);
}

#[test]
fn test_external_mcp_env_overrides_port_and_host() {
    let mut cfg = AllRuntimeConfig {
        stream: StreamTimeoutsConfig::default(),
        reconciliation: ReconciliationConfig::default(),
        git: GitRuntimeConfig::default(),
        scheduler: SchedulerConfig::default(),
        supervisor: SupervisorRuntimeConfig::default(),
        limits: LimitsConfig::default(),
        verification: VerificationConfig::default(),
        external_mcp: ExternalMcpConfig::default(),
        child_session_activity_threshold_secs: None,
        ui_feature_flags: Default::default(),
        database_maintenance: DatabaseMaintenanceConfig::default(),
        delegation: DelegationConfig::default(),
    };

    apply_env_overrides_with(&mut cfg, &|name| match name {
        "RALPHX_EXTERNAL_MCP_PORT" => Some("9999".to_string()),
        "RALPHX_EXTERNAL_MCP_HOST" => Some("0.0.0.0".to_string()),
        _ => None,
    });
    assert_eq!(cfg.external_mcp.port, 9999);
    assert_eq!(cfg.external_mcp.host, "0.0.0.0");
}

#[test]
fn test_external_mcp_env_override_node_path() {
    let mut cfg = AllRuntimeConfig {
        stream: StreamTimeoutsConfig::default(),
        reconciliation: ReconciliationConfig::default(),
        git: GitRuntimeConfig::default(),
        scheduler: SchedulerConfig::default(),
        supervisor: SupervisorRuntimeConfig::default(),
        limits: LimitsConfig::default(),
        verification: VerificationConfig::default(),
        external_mcp: ExternalMcpConfig::default(),
        child_session_activity_threshold_secs: None,
        ui_feature_flags: Default::default(),
        database_maintenance: DatabaseMaintenanceConfig::default(),
        delegation: DelegationConfig::default(),
    };

    apply_env_overrides_with(&mut cfg, &|name| match name {
        "RALPHX_NODE_PATH" => Some("/usr/local/bin/node".to_string()),
        _ => None,
    });
    assert_eq!(
        cfg.external_mcp.node_path,
        Some("/usr/local/bin/node".to_string())
    );
}

#[test]
fn test_external_mcp_env_override_human_wait_timeout() {
    let mut cfg = AllRuntimeConfig {
        stream: StreamTimeoutsConfig::default(),
        reconciliation: ReconciliationConfig::default(),
        git: GitRuntimeConfig::default(),
        scheduler: SchedulerConfig::default(),
        supervisor: SupervisorRuntimeConfig::default(),
        limits: LimitsConfig::default(),
        verification: VerificationConfig::default(),
        external_mcp: ExternalMcpConfig::default(),
        child_session_activity_threshold_secs: None,
        ui_feature_flags: Default::default(),
        database_maintenance: DatabaseMaintenanceConfig::default(),
        delegation: DelegationConfig::default(),
    };

    apply_env_overrides_with(&mut cfg, &|name| match name {
        "RALPHX_EXTERNAL_MCP_HUMAN_WAIT_TIMEOUT_SECS" => Some("240".to_string()),
        _ => None,
    });
    assert_eq!(cfg.external_mcp.human_wait_timeout_secs, 240);
}

#[test]
fn test_external_mcp_env_override_startup_timeout() {
    let mut cfg = AllRuntimeConfig {
        stream: StreamTimeoutsConfig::default(),
        reconciliation: ReconciliationConfig::default(),
        git: GitRuntimeConfig::default(),
        scheduler: SchedulerConfig::default(),
        supervisor: SupervisorRuntimeConfig::default(),
        limits: LimitsConfig::default(),
        verification: VerificationConfig::default(),
        external_mcp: ExternalMcpConfig::default(),
        child_session_activity_threshold_secs: None,
        ui_feature_flags: Default::default(),
        database_maintenance: DatabaseMaintenanceConfig::default(),
        delegation: DelegationConfig::default(),
    };

    apply_env_overrides_with(&mut cfg, &|name| match name {
        "RALPHX_EXTERNAL_MCP_STARTUP_TIMEOUT_SECS" => Some("45".to_string()),
        _ => None,
    });
    assert_eq!(cfg.external_mcp.startup_timeout_secs, 45);
}

#[test]
fn external_mcp_rejects_zero_startup_timeout() {
    let cfg = ExternalMcpConfig {
        startup_timeout_secs: 0,
        ..ExternalMcpConfig::default()
    };

    assert!(validate_external_mcp_config(&cfg)
        .expect_err("zero startup timeout")
        .contains("startup_timeout_secs"));
}

#[test]
fn test_external_mcp_invalid_port_env_keeps_default() {
    let mut cfg = AllRuntimeConfig {
        stream: StreamTimeoutsConfig::default(),
        reconciliation: ReconciliationConfig::default(),
        git: GitRuntimeConfig::default(),
        scheduler: SchedulerConfig::default(),
        supervisor: SupervisorRuntimeConfig::default(),
        limits: LimitsConfig::default(),
        verification: VerificationConfig::default(),
        external_mcp: ExternalMcpConfig::default(),
        child_session_activity_threshold_secs: None,
        ui_feature_flags: Default::default(),
        database_maintenance: DatabaseMaintenanceConfig::default(),
        delegation: DelegationConfig::default(),
    };

    apply_env_overrides_with(&mut cfg, &|name| match name {
        "RALPHX_EXTERNAL_MCP_PORT" => Some("not_a_port".to_string()),
        _ => None,
    });
    assert_eq!(cfg.external_mcp.port, 3848);
}

#[test]
fn test_validate_external_mcp_config_valid_local() {
    let cfg = ExternalMcpConfig {
        enabled: true,
        ..ExternalMcpConfig::default()
    };
    assert!(validate_external_mcp_config(&cfg).is_ok());
}

#[test]
fn test_validate_external_mcp_config_port_zero() {
    let cfg = ExternalMcpConfig {
        port: 0,
        ..ExternalMcpConfig::default()
    };
    let result = validate_external_mcp_config(&cfg);
    assert!(result.is_err());
    assert!(result.unwrap_err().contains("port"));
}

#[test]
fn test_validate_external_mcp_config_empty_host() {
    let cfg = ExternalMcpConfig {
        host: String::new(),
        ..ExternalMcpConfig::default()
    };
    let result = validate_external_mcp_config(&cfg);
    assert!(result.is_err());
    assert!(result.unwrap_err().contains("host"));
}

#[test]
fn test_validate_external_mcp_config_zero_human_wait_timeout() {
    let cfg = ExternalMcpConfig {
        human_wait_timeout_secs: 0,
        ..ExternalMcpConfig::default()
    };
    let result = validate_external_mcp_config(&cfg);
    assert!(result.is_err());
    assert!(result.unwrap_err().contains("human_wait_timeout_secs"));
}

#[test]
fn test_validate_external_mcp_config_disabled_non_local_no_tls_ok() {
    // When disabled, non-local host without TLS should be fine
    let cfg = ExternalMcpConfig {
        enabled: false,
        host: "192.168.1.100".to_string(),
        ..ExternalMcpConfig::default()
    };
    assert!(validate_external_mcp_config(&cfg).is_ok());
}

// ── GitIsolation config tests ─────────────────────────────────────────────────

#[test]
fn test_git_isolation_config_defaults() {
    let cfg = ReconciliationConfig::default();
    assert_eq!(
        cfg.git_isolation_retry_base_secs, 5,
        "default base should be 5s (shorter than execution_failed_retry_base_secs=30)"
    );
    assert_eq!(
        cfg.git_isolation_max_retries, 3,
        "default max retries should be 3"
    );
}

#[test]
fn test_git_isolation_config_backward_compat_deserialization() {
    // YAML without git_isolation keys must still deserialize via serde defaults.
    let yaml_without_new_keys = r#"
merger_timeout_secs: 1200
merging_max_retries: 3
pending_merge_stale_minutes: 2
qa_stale_minutes: 5
merge_incomplete_retry_base_secs: 5
merge_incomplete_retry_max_secs: 1800
merge_incomplete_max_retries: 5
validation_revert_max_count: 2
merge_conflict_retry_base_secs: 60
merge_conflict_retry_max_secs: 600
merge_conflict_max_retries: 3
executing_max_retries: 5
reviewing_max_retries: 3
qa_max_retries: 3
executing_max_wall_clock_minutes: 60
reviewing_max_wall_clock_minutes: 30
qa_max_wall_clock_minutes: 15
pre_merge_cleanup_timeout_secs: 60
attempt_merge_deadline_secs: 60
validation_deadline_secs: 1200
merge_registry_grace_period_secs: 60
validation_retry_min_cooldown_secs: 120
validation_failure_circuit_breaker_count: 3
merge_starvation_guard_secs: 60
branch_freshness_timeout_secs: 60
merge_watcher_grace_secs: 30
merge_watcher_poll_secs: 15
execution_failed_max_retries: 3
execution_failed_retry_base_secs: 30
execution_failed_retry_max_secs: 600
"#;
    let cfg: ReconciliationConfig = serde_yaml::from_str(yaml_without_new_keys)
        .expect("deserialize without git_isolation keys");
    assert_eq!(
        cfg.git_isolation_retry_base_secs, 5,
        "serde default should apply when key is absent"
    );
    assert_eq!(
        cfg.git_isolation_max_retries, 3,
        "serde default should apply when key is absent"
    );
}

#[test]
fn test_git_isolation_env_overrides() {
    let mut cfg = AllRuntimeConfig {
        stream: StreamTimeoutsConfig::default(),
        reconciliation: ReconciliationConfig::default(),
        git: GitRuntimeConfig::default(),
        scheduler: SchedulerConfig::default(),
        supervisor: SupervisorRuntimeConfig::default(),
        limits: LimitsConfig::default(),
        verification: VerificationConfig::default(),
        external_mcp: ExternalMcpConfig::default(),
        child_session_activity_threshold_secs: None,
        ui_feature_flags: Default::default(),
        database_maintenance: DatabaseMaintenanceConfig::default(),
        delegation: DelegationConfig::default(),
    };

    apply_env_overrides_with(&mut cfg, &|name| match name {
        "RALPHX_RECONCILIATION_GIT_ISOLATION_RETRY_BASE_SECS" => Some("10".to_string()),
        "RALPHX_RECONCILIATION_GIT_ISOLATION_MAX_RETRIES" => Some("5".to_string()),
        _ => None,
    });

    assert_eq!(cfg.reconciliation.git_isolation_retry_base_secs, 10);
    assert_eq!(cfg.reconciliation.git_isolation_max_retries, 5);
}

#[test]
fn test_validate_git_isolation_max_retries_zero_clamped() {
    let mut cfg = ReconciliationConfig {
        git_isolation_max_retries: 0,
        ..ReconciliationConfig::default()
    };
    validate_reconciliation_config(&mut cfg);
    assert!(
        cfg.git_isolation_max_retries > 0,
        "zero git_isolation_max_retries should be clamped to default"
    );
}

#[test]
fn test_validate_git_isolation_retry_base_secs_zero_clamped() {
    let mut cfg = ReconciliationConfig {
        git_isolation_retry_base_secs: 0,
        ..ReconciliationConfig::default()
    };
    validate_reconciliation_config(&mut cfg);
    assert!(
        cfg.git_isolation_retry_base_secs > 0,
        "zero git_isolation_retry_base_secs should be clamped to default"
    );
}

/// Build a default `AllRuntimeConfig` for env-override tests, then apply the
/// supplied `RALPHX_UI_TICKETING_DASHBOARD` value (if any) via the injectable
/// lookup so we never touch real process env (deterministic + parallel-safe).
fn ticketing_dashboard_after_env(value: Option<&str>) -> bool {
    let mut cfg = AllRuntimeConfig {
        stream: StreamTimeoutsConfig::default(),
        reconciliation: ReconciliationConfig::default(),
        git: GitRuntimeConfig::default(),
        scheduler: SchedulerConfig::default(),
        supervisor: SupervisorRuntimeConfig::default(),
        limits: LimitsConfig::default(),
        verification: VerificationConfig::default(),
        external_mcp: ExternalMcpConfig::default(),
        child_session_activity_threshold_secs: None,
        ui_feature_flags: Default::default(),
        database_maintenance: DatabaseMaintenanceConfig::default(),
        delegation: DelegationConfig::default(),
    };
    apply_env_overrides_with(&mut cfg, &|name| match name {
        "RALPHX_UI_TICKETING_DASHBOARD" => value.map(str::to_string),
        _ => None,
    });
    cfg.ui_feature_flags.ticketing_dashboard
}

fn agent_personas_after_env(value: Option<&str>) -> bool {
    let mut cfg = AllRuntimeConfig {
        stream: StreamTimeoutsConfig::default(),
        reconciliation: ReconciliationConfig::default(),
        git: GitRuntimeConfig::default(),
        scheduler: SchedulerConfig::default(),
        supervisor: SupervisorRuntimeConfig::default(),
        limits: LimitsConfig::default(),
        verification: VerificationConfig::default(),
        external_mcp: ExternalMcpConfig::default(),
        child_session_activity_threshold_secs: None,
        ui_feature_flags: Default::default(),
        database_maintenance: DatabaseMaintenanceConfig::default(),
        delegation: DelegationConfig::default(),
    };
    apply_env_overrides_with(&mut cfg, &|name| match name {
        "RALPHX_UI_AGENT_PERSONAS" => value.map(str::to_string),
        _ => None,
    });
    cfg.ui_feature_flags.agent_personas
}

#[test]
fn runtime_config_env_override_agent_personas_true_and_false() {
    assert!(agent_personas_after_env(Some("true")));
    assert!(agent_personas_after_env(Some("1")));
    assert!(!agent_personas_after_env(Some("false")));
    assert!(!agent_personas_after_env(None));
}

#[test]
fn runtime_config_env_override_persona_switch_fresh_session_fallback() {
    let mut cfg = AllRuntimeConfig {
        stream: StreamTimeoutsConfig::default(),
        reconciliation: ReconciliationConfig::default(),
        git: GitRuntimeConfig::default(),
        scheduler: SchedulerConfig::default(),
        supervisor: SupervisorRuntimeConfig::default(),
        limits: LimitsConfig::default(),
        verification: VerificationConfig::default(),
        external_mcp: ExternalMcpConfig::default(),
        child_session_activity_threshold_secs: None,
        ui_feature_flags: Default::default(),
        database_maintenance: DatabaseMaintenanceConfig::default(),
        delegation: DelegationConfig::default(),
    };
    apply_env_overrides_with(&mut cfg, &|name| match name {
        "RALPHX_UI_PERSONA_SWITCH_FORCES_FRESH_PROVIDER_SESSION" => Some("true".to_string()),
        _ => None,
    });
    assert!(
        cfg.ui_feature_flags
            .persona_switch_forces_fresh_provider_session
    );

    apply_env_overrides_with(&mut cfg, &|name| match name {
        "RALPHX_UI_PERSONA_SWITCH_FORCES_FRESH_PROVIDER_SESSION" => Some("false".to_string()),
        _ => None,
    });
    assert!(
        !cfg.ui_feature_flags
            .persona_switch_forces_fresh_provider_session
    );
}

#[test]
fn test_ui_ticketing_dashboard_default_is_false() {
    use crate::infrastructure::agents::claude::UiFeatureFlagsConfig;
    assert!(!UiFeatureFlagsConfig::default().ticketing_dashboard);
}

#[test]
fn test_ui_ticketing_dashboard_env_true_enables() {
    assert!(ticketing_dashboard_after_env(Some("true")));
}

#[test]
fn test_ui_ticketing_dashboard_env_one_enables() {
    assert!(ticketing_dashboard_after_env(Some("1")));
}

#[test]
fn test_ui_ticketing_dashboard_env_false_disables() {
    assert!(!ticketing_dashboard_after_env(Some("false")));
}

#[test]
fn test_ui_ticketing_dashboard_env_zero_disables() {
    assert!(!ticketing_dashboard_after_env(Some("0")));
}

#[test]
fn test_ui_ticketing_dashboard_env_unrecognized_disables() {
    // Only "true"/"1" (case-insensitive) enable; anything else parses to false.
    assert!(!ticketing_dashboard_after_env(Some("yes")));
    // Case-insensitive: uppercase TRUE still enables.
    assert!(ticketing_dashboard_after_env(Some("TRUE")));
}

#[test]
fn test_ui_ticketing_dashboard_env_missing_keeps_default() {
    // No env var present → field stays at its default (false).
    assert!(!ticketing_dashboard_after_env(None));
}
