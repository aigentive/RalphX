use super::*;

fn init_empty_git_repo(path: &std::path::Path) {
    let output = std::process::Command::new("git")
        .arg("init")
        .arg(path)
        .output()
        .expect("git init should run");
    assert!(
        output.status.success(),
        "git init failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

fn freshness_test_config() -> ReconciliationConfig {
    ReconciliationConfig {
        branch_freshness_timeout_secs: 5,
        freshness_skip_window_secs: 0,
        ..Default::default()
    }
}

fn freshness_test_project(path: &std::path::Path, base_branch: &str) -> Project {
    let mut project = Project::new(
        "freshness coverage project".to_string(),
        path.to_string_lossy().to_string(),
    );
    project.base_branch = Some(base_branch.to_string());
    project
}

fn freshness_test_task(project: &Project) -> Task {
    Task::new(project.id.clone(), "freshness coverage task".to_string())
}

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
        last_plan_freshness_check_at: Some("2026-01-01T00:00:00Z".to_string()),
        last_task_freshness_check_at: Some("2026-01-01T00:00:00Z".to_string()),
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
        14,
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
        last_plan_freshness_check_at: None,
        last_task_freshness_check_at: None,
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

#[tokio::test]
async fn ensure_branches_fresh_blocks_execution_when_worktree_status_unreadable() {
    let temp = tempfile::tempdir().expect("temp dir");
    let missing_repo = temp.path().join("missing-repo");
    let project = freshness_test_project(&missing_repo, "main");
    let task = freshness_test_task(&project);
    let config = freshness_test_config();

    let result = ensure_branches_fresh(
        &missing_repo,
        &task,
        &project,
        task.id.as_str(),
        None,
        None,
        None,
        None,
        "executing",
        &config,
    )
    .await;

    assert!(
        matches!(
            result,
            Err(FreshnessAction::ExecutionBlocked { ref reason, .. })
                if reason.contains("Failed to check worktree status before freshness check")
        ),
        "execution-origin unreadable worktree status must block: {result:?}"
    );
}

#[tokio::test]
async fn ensure_branches_fresh_retries_and_blocks_plan_update_errors() {
    let temp = tempfile::tempdir().expect("temp dir");
    init_empty_git_repo(temp.path());
    let project = freshness_test_project(temp.path(), "main");
    let task = freshness_test_task(&project);
    let config = freshness_test_config();

    let result = ensure_branches_fresh(
        temp.path(),
        &task,
        &project,
        task.id.as_str(),
        Some("plan/missing"),
        Some("missing-base"),
        None,
        None,
        "executing",
        &config,
    )
    .await;

    assert!(
        matches!(
            result,
            Err(FreshnessAction::ExecutionBlocked { ref reason, .. })
                if reason.contains("update_plan_from_main failed after retry")
                    && reason.contains("missing-base")
        ),
        "plan update errors must retry once then block: {result:?}"
    );
}

#[tokio::test]
async fn ensure_branches_fresh_blocks_source_update_branch_missing() {
    let temp = tempfile::tempdir().expect("temp dir");
    init_empty_git_repo(temp.path());
    let project = freshness_test_project(temp.path(), "missing-target");
    let mut task = freshness_test_task(&project);
    task.task_branch = Some("task/missing".to_string());
    let config = freshness_test_config();

    let result = ensure_branches_fresh(
        temp.path(),
        &task,
        &project,
        task.id.as_str(),
        None,
        None,
        None,
        None,
        "executing",
        &config,
    )
    .await;

    assert!(
            matches!(
                result,
                Err(FreshnessAction::ExecutionBlocked { ref reason, .. })
                    if reason.contains("branch missing before source update")
                        && reason.contains("missing-target")
            ),
            "source update branch misses must block without pretending the branch is retryable: {result:?}"
        );
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
        source_retry_decision_after_error(
            Some(SourceUpdateResult::BranchMissing {
                branch: "feature/missing".to_string(),
            }),
            11,
        ),
        FreshnessRetryDecision::Block {
            reason: "branch missing before source update retry: feature/missing".to_string()
        }
    );
    assert_eq!(
        source_retry_decision_after_error(None, 11),
        FreshnessRetryDecision::Block {
            reason: "update_source_from_target retry timed out after 11s".to_string()
        }
    );
}

#[test]
fn worktree_status_errors_fail_closed_only_for_execution_origins() {
    assert_eq!(
        worktree_status_error_decision("reviewing", "permission denied"),
        FreshnessWorktreeGuardDecision::Skip
    );
    assert_eq!(
        worktree_status_error_decision("executing", "permission denied"),
        FreshnessWorktreeGuardDecision::Block {
            reason_code: "worktree_status_unreadable",
            check: "worktree_status",
            reason: "Failed to check worktree status before freshness check: permission denied"
                .to_string()
        }
    );
    assert_eq!(
        worktree_status_error_decision("re_executing", "stale handle"),
        FreshnessWorktreeGuardDecision::Block {
            reason_code: "worktree_status_unreadable",
            check: "worktree_status",
            reason: "Failed to check worktree status before freshness check: stale handle"
                .to_string()
        }
    );
}

#[test]
fn dirty_worktree_autocommit_errors_fail_closed_only_for_execution_origins() {
    assert_eq!(
        dirty_worktree_autocommit_error_decision("reviewing", "commit failed"),
        FreshnessWorktreeGuardDecision::Skip
    );
    assert_eq!(
        dirty_worktree_autocommit_error_decision("executing", "commit failed"),
        FreshnessWorktreeGuardDecision::Block {
            reason_code: "dirty_worktree_autocommit_failed",
            check: "dirty_worktree_autocommit",
            reason: "Emergency auto-commit failed before freshness check: commit failed"
                .to_string()
        }
    );
    assert_eq!(
        dirty_worktree_autocommit_error_decision("re_executing", "index locked"),
        FreshnessWorktreeGuardDecision::Block {
            reason_code: "dirty_worktree_autocommit_failed",
            check: "dirty_worktree_autocommit",
            reason: "Emergency auto-commit failed before freshness check: index locked".to_string()
        }
    );
}

#[tokio::test]
async fn block_freshness_update_error_returns_execution_blocked_action() {
    let action = block_freshness_update_error(
        None,
        "task-1",
        "plan_update",
        "retry failed".to_string(),
        None,
    )
    .await;

    assert!(
        matches!(action, FreshnessAction::ExecutionBlocked { ref reason, .. } if reason == "retry failed"),
        "freshness update errors must block execution after retry: {action:?}"
    );
}
