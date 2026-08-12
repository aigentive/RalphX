use super::publish_resilience::*;
use crate::domain::entities::Project;
use crate::domain::state_machine::transition_handler::SourceUpdateResult;
use std::path::{Path, PathBuf};
use std::process::Command;

fn git(repo: &Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .args(args)
        .current_dir(repo)
        .output()
        .expect("git command should spawn");
    assert!(
        output.status.success(),
        "git {:?} failed\nstdout:\n{}\nstderr:\n{}",
        args,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).trim().to_string()
}

fn setup_publish_freshness_repo(repo: &Path) -> String {
    std::fs::create_dir_all(repo).expect("repo root should be created");
    git(repo, &["init", "-b", "main"]);
    git(repo, &["config", "user.email", "test@example.com"]);
    git(repo, &["config", "user.name", "RalphX Test"]);
    std::fs::write(repo.join("README.md"), "base\n").expect("base fixture should be written");
    git(repo, &["add", "README.md"]);
    git(repo, &["commit", "-m", "base"]);
    git(repo, &["rev-parse", "HEAD"])
}

#[test]
fn classifies_commit_hook_policy_failures_as_agent_fixable() {
    let error = "Failed to commit changes: pre-commit hook failed: npm run typecheck failed";

    assert_eq!(
        classify_publish_failure(error),
        PublishFailureClass::AgentFixable
    );
}

#[test]
fn classifies_unknown_pre_commit_failures_as_agent_fixable() {
    let error = "Failed to commit changes: pre-commit hook exited with status 1";

    assert_eq!(
        classify_publish_failure(error),
        PublishFailureClass::AgentFixable
    );
}

#[test]
fn classifies_branch_conflicts_as_agent_fixable() {
    let error = "failed to update branch: merge conflict in frontend/src/App.tsx";

    assert_eq!(
        classify_publish_failure(error),
        PublishFailureClass::AgentFixable
    );
}

#[test]
fn classifies_dirty_worktree_merge_aborts_as_agent_fixable() {
    let error = concat!(
        "merge in existing worktree failed: Git operation error: Merge failed: ",
        "error: Your local changes to the following files would be overwritten by merge:\n",
        "\tsrc-tauri/src/application/agent_task_service.rs\n",
        "Please commit your changes or stash them before you merge.\n",
        "Aborting"
    );

    assert_eq!(
        classify_publish_failure(error),
        PublishFailureClass::AgentFixable
    );
    assert_eq!(publish_push_status_for_failure(error), "needs_agent");
}

#[test]
fn classifies_non_fast_forward_push_rejections_as_agent_fixable() {
    let error = "failed to push some refs: updates were rejected because the tip of your current branch is behind its remote counterpart (non-fast-forward)";

    assert_eq!(
        classify_publish_failure(error),
        PublishFailureClass::AgentFixable
    );
}

#[test]
fn classifies_github_availability_as_operational() {
    let error = "GitHub integration is not available";

    assert_eq!(
        classify_publish_failure(error),
        PublishFailureClass::Operational
    );
}

#[test]
fn classifies_git_authentication_failures_as_operational() {
    let error = "Git authentication error: Git could not authenticate while trying to fetch from `origin`. The fetch remote uses HTTPS.";

    assert_eq!(
        classify_publish_failure(error),
        PublishFailureClass::Operational
    );
}

#[test]
fn classifies_git_command_timeouts_as_operational() {
    let error = "Git operation error: git command timed out after 60s";

    assert_eq!(
        classify_publish_failure(error),
        PublishFailureClass::Operational
    );
    assert_eq!(publish_push_status_for_failure(error), "failed");
}

#[test]
fn classifies_commit_hook_environment_failures_as_operational() {
    let error = "Failed to commit changes: pre-commit failed: Cannot find package 'vitest'";

    assert_eq!(
        classify_publish_failure(error),
        PublishFailureClass::Operational
    );
}

#[test]
fn classifies_commit_hook_module_resolution_failures_as_operational() {
    let error = "Failed to commit changes: pre-commit failed: Cannot find module 'zod'";

    assert_eq!(
        classify_publish_failure(error),
        PublishFailureClass::Operational
    );
}

#[test]
fn requires_captured_base_commit_for_publish_review_base() {
    assert_eq!(
        review_base_for_publish(Some("abc123"), "main").expect("captured commit"),
        "abc123"
    );

    let error = review_base_for_publish(None, "main").expect_err("missing base commit");
    assert!(error.contains("captured base commit"));
}

#[test]
fn maps_source_update_conflicts_to_agent_fixable_publish_outcome() {
    let outcome = publish_branch_freshness_outcome_from_source_update(
        SourceUpdateResult::Conflicts {
            conflict_files: vec![PathBuf::from("frontend/src/App.tsx")],
        },
        "origin/main",
        "target-sha",
    );

    let PublishBranchFreshnessOutcome::NeedsAgent {
        message,
        conflict_files,
        base_commit,
        target_ref,
    } = outcome
    else {
        panic!("expected conflict to route to agent");
    };

    assert_eq!(conflict_files, vec!["frontend/src/App.tsx"]);
    assert_eq!(base_commit, "target-sha");
    assert_eq!(target_ref, "origin/main");
    assert_eq!(
        classify_publish_failure(&message),
        PublishFailureClass::AgentFixable
    );
}

#[test]
fn maps_source_update_branch_missing_to_operational_publish_outcome() {
    let outcome = publish_branch_freshness_outcome_from_source_update(
        SourceUpdateResult::BranchMissing {
            branch: "feature/missing".to_string(),
        },
        "origin/main",
        "target-sha",
    );

    assert_eq!(
        outcome,
        PublishBranchFreshnessOutcome::OperationalError {
            message: "branch missing before freshness update: feature/missing".to_string(),
        }
    );
}

#[test]
fn maps_successful_source_update_to_updated_publish_base() {
    let outcome = publish_branch_freshness_outcome_from_source_update(
        SourceUpdateResult::Updated,
        "origin/main",
        "target-sha",
    );

    assert_eq!(
        outcome,
        PublishBranchFreshnessOutcome::Updated {
            base_commit: "target-sha".to_string(),
            target_ref: "origin/main".to_string(),
        }
    );
}

#[test]
fn derives_remote_tracking_ref_for_publish_base() {
    assert_eq!(remote_tracking_ref_for_publish("main"), "origin/main");
    assert_eq!(
        remote_tracking_ref_for_publish("origin/main"),
        "origin/main"
    );
}

#[tokio::test]
async fn ensure_plan_publish_branch_fresh_updates_isolated_linked_worktree() {
    let temp = tempfile::TempDir::new().expect("tempdir should be created");
    let repo = temp.path().join("repo");
    let worktrees = temp.path().join("worktrees");
    setup_publish_freshness_repo(&repo);
    let plan_branch = "feature/plan-linked-worktree";
    git(&repo, &["branch", plan_branch]);
    std::fs::create_dir_all(&worktrees).expect("worktree parent should be created");
    let plan_worktree = worktrees.join("linked-plan");
    git(
        &repo,
        &[
            "worktree",
            "add",
            plan_worktree.to_str().expect("worktree path"),
            plan_branch,
        ],
    );

    std::fs::write(repo.join("base-fix.txt"), "base fix\n").expect("base fix should be written");
    git(&repo, &["add", "base-fix.txt"]);
    git(&repo, &["commit", "-m", "base fix"]);
    let main_sha = git(&repo, &["rev-parse", "HEAD"]);

    let mut project = Project::new(
        "Plan linked worktree freshness".to_string(),
        repo.to_string_lossy().to_string(),
    );
    project.base_branch = Some("main".to_string());
    project.worktree_parent_directory = Some(worktrees.to_string_lossy().to_string());

    let outcome = ensure_plan_publish_branch_fresh(
        &plan_worktree,
        &project,
        plan_branch,
        "main",
        "conversation-plan-linked-worktree",
        None,
    )
    .await;

    assert_eq!(
        outcome,
        PublishBranchFreshnessOutcome::Updated {
            base_commit: main_sha.clone(),
            target_ref: "main".to_string(),
        }
    );
    assert_eq!(git(&repo, &["branch", "--show-current"]), "main");
    assert_eq!(git(&repo, &["status", "--short"]), "");
    assert_eq!(
        git(&plan_worktree, &["branch", "--show-current"]),
        plan_branch
    );
    assert_eq!(git(&plan_worktree, &["rev-parse", "HEAD"]), main_sha);
}

#[tokio::test]
async fn counts_unpublished_commits_against_remote_workspace_branch() {
    let repo = tempfile::TempDir::new().expect("repo tempdir");
    git(repo.path(), &["init", "-b", "main"]);
    git(repo.path(), &["config", "user.email", "test@example.com"]);
    git(repo.path(), &["config", "user.name", "RalphX Test"]);
    std::fs::write(repo.path().join("README.md"), "base\n").expect("write base");
    git(repo.path(), &["add", "README.md"]);
    git(repo.path(), &["commit", "-m", "base"]);
    git(repo.path(), &["checkout", "-b", "ralphx/test/workspace"]);

    assert_eq!(
        count_unpublished_publish_commits(repo.path(), "ralphx/test/missing")
            .await
            .expect("count missing remote"),
        None
    );

    git(
        repo.path(),
        &[
            "update-ref",
            "refs/remotes/origin/ralphx/test/workspace",
            "HEAD",
        ],
    );
    assert_eq!(
        count_unpublished_publish_commits(repo.path(), "ralphx/test/workspace")
            .await
            .expect("count published branch"),
        Some(0)
    );

    std::fs::write(repo.path().join("agent.txt"), "local\n").expect("write local");
    git(repo.path(), &["add", "agent.txt"]);
    git(repo.path(), &["commit", "-m", "local update"]);

    assert_eq!(
        count_unpublished_publish_commits(repo.path(), "ralphx/test/workspace")
            .await
            .expect("count unpublished branch"),
        Some(1)
    );
}

#[tokio::test]
async fn counts_local_only_publish_commits_against_base_when_remote_branch_is_missing() {
    let repo = tempfile::TempDir::new().expect("repo tempdir");
    git(repo.path(), &["init", "-b", "main"]);
    git(repo.path(), &["config", "user.email", "test@example.com"]);
    git(repo.path(), &["config", "user.name", "RalphX Test"]);
    std::fs::write(repo.path().join("README.md"), "base\n").expect("write base");
    git(repo.path(), &["add", "README.md"]);
    git(repo.path(), &["commit", "-m", "base"]);
    git(repo.path(), &["checkout", "-b", "ralphx/test/workspace"]);
    std::fs::write(repo.path().join("agent.txt"), "local\n").expect("write local");
    git(repo.path(), &["add", "agent.txt"]);
    git(repo.path(), &["commit", "-m", "local update"]);

    assert_eq!(
        count_unpublished_publish_commits(repo.path(), "ralphx/test/workspace")
            .await
            .expect("count missing remote"),
        None
    );
    assert_eq!(
        count_publishable_commits_with_base_fallback(repo.path(), "ralphx/test/workspace", "main",)
            .await
            .expect("count local-only branch"),
        1
    );
}

#[test]
fn reports_publish_base_as_current_when_captured_commit_matches_target() {
    let status =
        publish_branch_freshness_status_from_commits(Some("base-sha"), "origin/main", "base-sha");

    assert_eq!(status.target_ref, "origin/main");
    assert_eq!(status.captured_base_commit.as_deref(), Some("base-sha"));
    assert_eq!(status.target_base_commit, "base-sha");
    assert!(!status.is_base_ahead);
}

#[test]
fn reports_publish_base_as_ahead_when_target_commit_changed() {
    let status =
        publish_branch_freshness_status_from_commits(Some("old-base"), "origin/main", "new-base");

    assert_eq!(status.captured_base_commit.as_deref(), Some("old-base"));
    assert_eq!(status.target_base_commit, "new-base");
    assert!(status.is_base_ahead);
}

#[test]
fn reports_publish_base_as_current_when_source_branch_contains_target_commit() {
    let status = publish_branch_freshness_status_from_commits_and_branch(
        Some("old-base"),
        "origin/main",
        "new-base",
        true,
    );

    assert_eq!(status.captured_base_commit.as_deref(), Some("new-base"));
    assert_eq!(status.target_base_commit, "new-base");
    assert!(!status.is_base_ahead);
}

#[test]
fn keeps_publish_base_ahead_when_source_branch_does_not_contain_target_commit() {
    let status = publish_branch_freshness_status_from_commits_and_branch(
        Some("old-base"),
        "origin/main",
        "new-base",
        false,
    );

    assert_eq!(status.captured_base_commit.as_deref(), Some("old-base"));
    assert_eq!(status.target_base_commit, "new-base");
    assert!(status.is_base_ahead);
}

fn repaired_workspace_check() -> AgentWorkspaceRepairCompletionCheck<'static> {
    let status = Box::leak(Box::new(
        publish_branch_freshness_status_from_commits_and_branch(
            Some("old-base"),
            "origin/main",
            "new-base",
            true,
        ),
    ));

    AgentWorkspaceRepairCompletionCheck {
        freshness_status: status,
        workspace_base_ref: "main",
        resolved_base_ref: "origin/main",
        resolved_base_commit: "new-base",
        repair_commit_sha: "repair-head",
        workspace_head_sha: "repair-head",
        has_uncommitted_changes: false,
        is_merge_in_progress: false,
        is_rebase_in_progress: false,
        has_conflict_files: false,
        has_conflict_markers: false,
    }
}

#[test]
fn verifies_clean_agent_workspace_repair_completion() {
    assert!(verify_agent_workspace_repair_completion(repaired_workspace_check()).is_ok());
}

#[test]
fn rejects_agent_workspace_repair_when_base_still_ahead() {
    let stale_status =
        publish_branch_freshness_status_from_commits(Some("old-base"), "origin/main", "new-base");
    let mut check = repaired_workspace_check();
    check.freshness_status = &stale_status;

    let error = verify_agent_workspace_repair_completion(check)
        .expect_err("stale base must reject repair completion");
    assert!(error.contains("still behind"));
}

#[test]
fn rejects_agent_workspace_repair_when_branch_does_not_contain_captured_target_base() {
    // A conflict-routed attempt records the freshly observed origin tip as its target base, so
    // `captured == target` and `is_base_ahead` is false even though the branch never merged it.
    // Only the ancestry proof can reject this.
    let unintegrated_status = publish_branch_freshness_status_from_commits_and_branch(
        Some("new-base"),
        "origin/main",
        "new-base",
        false,
    );
    let mut check = repaired_workspace_check();
    check.freshness_status = &unintegrated_status;

    let error = verify_agent_workspace_repair_completion(check)
        .expect_err("unintegrated base must reject repair completion");
    assert!(
        error.contains("does not contain base"),
        "unexpected error: {error}"
    );
}

#[test]
fn rejects_agent_workspace_repair_when_reported_base_commit_mismatches_current_target() {
    let mut check = repaired_workspace_check();
    check.resolved_base_commit = "other-base";

    let error = verify_agent_workspace_repair_completion(check)
        .expect_err("mismatched base commit must reject repair completion");
    assert!(error.contains("resolved_base_commit"));
}

#[test]
fn rejects_agent_workspace_repair_when_head_does_not_match_reported_repair_commit() {
    let mut check = repaired_workspace_check();
    check.workspace_head_sha = "different-head";

    let error = verify_agent_workspace_repair_completion(check)
        .expect_err("reported repair commit must be current HEAD");
    assert!(error.contains("reported fix commit"));
}

#[test]
fn rejects_agent_workspace_repair_when_worktree_is_dirty() {
    let mut check = repaired_workspace_check();
    check.has_uncommitted_changes = true;

    let error = verify_agent_workspace_repair_completion(check)
        .expect_err("dirty worktree must reject repair completion");
    assert!(error.contains("uncommitted"));
}

#[test]
fn rejects_agent_workspace_repair_when_merge_is_still_in_progress() {
    let mut check = repaired_workspace_check();
    check.is_merge_in_progress = true;

    let error = verify_agent_workspace_repair_completion(check)
        .expect_err("in-progress merge must reject repair completion");
    assert!(error.contains("merge is still in progress"));
}

#[test]
fn rejects_agent_workspace_repair_when_rebase_is_still_in_progress() {
    let mut check = repaired_workspace_check();
    check.is_rebase_in_progress = true;

    let error = verify_agent_workspace_repair_completion(check)
        .expect_err("in-progress rebase must reject repair completion");
    assert!(error.contains("rebase is still in progress"));
}

#[test]
fn rejects_agent_workspace_repair_when_conflict_markers_remain() {
    let mut check = repaired_workspace_check();
    check.has_conflict_markers = true;

    let error = verify_agent_workspace_repair_completion(check)
        .expect_err("conflict markers must reject repair completion");
    assert!(error.contains("conflict markers"));
}

#[test]
fn settled_head_verifier_rejects_unresolved_conflict_files() {
    let error = verify_agent_workspace_settled_current_head(AgentWorkspaceSettledHeadCheck {
        reported_head_sha: "head",
        workspace_head_sha: "head",
        has_uncommitted_changes: false,
        is_merge_in_progress: false,
        is_rebase_in_progress: false,
        has_conflict_files: true,
        has_conflict_markers: false,
    })
    .expect_err("unresolved index conflicts must reject completion");

    assert!(error.contains("unresolved conflict files"));
}
