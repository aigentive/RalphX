// Tests for update_plan_from_main — ensures plan branches are brought up-to-date
// from main before task→plan merges to prevent false validation failures.

use super::super::freshness::observe_plan_freshness;
use super::super::merge_coordination::{
    update_plan_from_main, update_plan_from_main_isolated, PlanUpdateResult,
};
use super::helpers::*;
use crate::domain::entities::Project;

fn resolve_test_ref(repo: &std::path::Path, reference: &str) -> String {
    let output = std::process::Command::new("git")
        .args(["rev-parse", "--verify", reference])
        .current_dir(repo)
        .output()
        .expect("run git rev-parse");
    assert!(output.status.success(), "git rev-parse should succeed");
    String::from_utf8(output.stdout)
        .expect("git ref sha should be utf-8")
        .trim()
        .to_string()
}

/// Helper: create a real git repo with main, a plan branch (from main), and then
/// add a commit to main (simulating fixes committed after plan branch was created).
fn setup_plan_behind_main() -> (RealGitRepo, String) {
    let repo = setup_real_git_repo();
    let path = repo.path();

    // Create plan branch from main (before the new fix)
    let _ = std::process::Command::new("git")
        .args(["branch", "plan/feature-1"])
        .current_dir(path)
        .output()
        .expect("git branch plan/feature-1");

    // Add a fix commit to main (plan branch is now behind)
    std::fs::write(path.join("fix.rs"), "// clippy fix").unwrap();
    let _ = std::process::Command::new("git")
        .args(["add", "fix.rs"])
        .current_dir(path)
        .output();
    let _ = std::process::Command::new("git")
        .args(["commit", "-m", "fix: clippy errors"])
        .current_dir(path)
        .output();

    (repo, "plan/feature-1".to_string())
}

/// Helper: create a project entity pointing at a repo path.
fn make_test_project(repo_path: &str) -> Project {
    make_real_git_project(repo_path)
}

#[tokio::test]
async fn update_plan_skips_when_target_is_main() {
    let repo = setup_real_git_repo();
    let project = make_test_project(&repo.path_string());

    let result = update_plan_from_main(
        repo.path(),
        "main", // target = main
        "main", // base = main
        &project,
        "task-1",
        None,
    )
    .await;

    assert!(
        matches!(result, PlanUpdateResult::NotPlanBranch),
        "Should skip when target == base. Got: {:?}",
        result
    );
}

#[tokio::test]
async fn update_plan_already_up_to_date() {
    let repo = setup_real_git_repo();
    let path = repo.path();

    // Create plan branch from current main (no divergence)
    let _ = std::process::Command::new("git")
        .args(["branch", "plan/feature-up-to-date"])
        .current_dir(path)
        .output()
        .expect("git branch");

    let project = make_test_project(&repo.path_string());

    let result = update_plan_from_main(
        path,
        "plan/feature-up-to-date",
        "main",
        &project,
        "task-2",
        None,
    )
    .await;

    assert!(
        matches!(result, PlanUpdateResult::AlreadyUpToDate),
        "Plan branch at same point as main should be AlreadyUpToDate. Got: {:?}",
        result
    );
}

#[tokio::test]
async fn non_pr_freshness_observation_reports_stale_without_mutating_plan() {
    let (repo, plan_branch) = setup_plan_behind_main();
    let mut project = make_test_project(&repo.path_string());
    project.github_pr_enabled = false;

    let before = resolve_test_ref(repo.path(), &plan_branch);
    let result =
        observe_plan_freshness(repo.path(), &plan_branch, "main", &project, "task-3", None).await;

    assert!(
        matches!(result, PlanUpdateResult::Conflicts { .. }),
        "Plan branch behind main should require a dedicated update. Got: {:?}",
        result
    );
    let after = resolve_test_ref(repo.path(), &plan_branch);
    assert_eq!(before, after, "freshness observation must be read-only");
}

#[tokio::test]
async fn isolated_update_plan_refuses_primary_checkout_mutation() {
    let (repo, plan_branch) = setup_plan_behind_main();
    let path = repo.path();
    let project = make_test_project(&repo.path_string());

    let checkout = std::process::Command::new("git")
        .args(["checkout", &plan_branch])
        .current_dir(path)
        .output()
        .expect("git checkout plan branch");
    assert!(
        checkout.status.success(),
        "precondition failed: {}",
        String::from_utf8_lossy(&checkout.stderr)
    );
    assert!(
        !path.join("fix.rs").exists(),
        "plan branch should not contain main's later fix before isolated update"
    );

    let result = update_plan_from_main_isolated(
        path,
        &plan_branch,
        "main",
        &project,
        "task-isolated-primary-checkout",
        None,
    )
    .await;

    assert!(
        matches!(result, PlanUpdateResult::Error(_)),
        "isolated PR freshness update must not merge into the primary checkout. Got: {:?}",
        result
    );
    assert!(
        !path.join("fix.rs").exists(),
        "isolated update must leave the user's currently checked-out working tree untouched"
    );
}

#[tokio::test]
async fn isolated_update_plan_accepts_remote_tracking_base_ref() {
    let (repo, plan_branch) = setup_plan_behind_main();
    let path = repo.path();
    let project = make_test_project(&repo.path_string());

    let update_ref = std::process::Command::new("git")
        .args(["update-ref", "refs/remotes/origin/main", "main"])
        .current_dir(path)
        .output()
        .expect("git update-ref origin/main");
    assert!(
        update_ref.status.success(),
        "precondition failed: {}",
        String::from_utf8_lossy(&update_ref.stderr)
    );

    let result = update_plan_from_main_isolated(
        path,
        &plan_branch,
        "origin/main",
        &project,
        "task-isolated-remote-base",
        None,
    )
    .await;

    assert!(
        matches!(result, PlanUpdateResult::Updated),
        "PR freshness must accept remote-tracking base refs. Got: {:?}",
        result
    );

    let contains_fix = std::process::Command::new("git")
        .args(["merge-base", "--is-ancestor", "origin/main", &plan_branch])
        .current_dir(path)
        .output()
        .expect("git merge-base");
    assert!(
        contains_fix.status.success(),
        "plan branch should contain origin/main after isolated update"
    );
}

#[tokio::test]
async fn update_plan_with_conflicts_returns_conflicts() {
    let repo = setup_real_git_repo();
    let path = repo.path();

    // Create plan branch from main
    let _ = std::process::Command::new("git")
        .args(["branch", "plan/conflict-test"])
        .current_dir(path)
        .output();

    // Add a file change on main
    std::fs::write(path.join("shared.rs"), "// main version\nfn main() {}").unwrap();
    let _ = std::process::Command::new("git")
        .args(["add", "shared.rs"])
        .current_dir(path)
        .output();
    let _ = std::process::Command::new("git")
        .args(["commit", "-m", "fix: main changes shared.rs"])
        .current_dir(path)
        .output();

    // Add a conflicting change on the plan branch
    let _ = std::process::Command::new("git")
        .args(["checkout", "plan/conflict-test"])
        .current_dir(path)
        .output();
    std::fs::write(path.join("shared.rs"), "// plan version\nfn plan() {}").unwrap();
    let _ = std::process::Command::new("git")
        .args(["add", "shared.rs"])
        .current_dir(path)
        .output();
    let _ = std::process::Command::new("git")
        .args(["commit", "-m", "feat: plan changes shared.rs"])
        .current_dir(path)
        .output();

    // Switch back to main
    let _ = std::process::Command::new("git")
        .args(["checkout", "main"])
        .current_dir(path)
        .output();

    let project = make_test_project(&repo.path_string());

    let result =
        update_plan_from_main(path, "plan/conflict-test", "main", &project, "task-4", None).await;

    assert!(
        matches!(result, PlanUpdateResult::Conflicts { .. }),
        "Conflicting changes should return Conflicts. Got: {:?}",
        result
    );
}

#[tokio::test]
async fn update_plan_nonexistent_base_branch_returns_error() {
    let repo = setup_real_git_repo();
    let path = repo.path();

    let _ = std::process::Command::new("git")
        .args(["branch", "plan/orphan-test"])
        .current_dir(path)
        .output();

    let project = make_test_project(&repo.path_string());

    let result = update_plan_from_main(
        path,
        "plan/orphan-test",
        "nonexistent-base", // base branch doesn't exist
        &project,
        "task-5",
        None,
    )
    .await;

    assert!(
        matches!(result, PlanUpdateResult::Error(_)),
        "Nonexistent base branch should return Error. Got: {:?}",
        result
    );
}

#[tokio::test]
async fn update_plan_uses_existing_worktree_when_branch_checked_out() {
    // Simulate the real bug scenario: plan branch is already checked out in
    // a merge worktree from a prior attempt. update_plan_from_main should
    // fall back to merging main directly in that existing worktree.
    let (repo, plan_branch) = setup_plan_behind_main();
    let project = make_test_project(&repo.path_string());

    // Create a worktree that has the plan branch checked out (simulating
    // a stale merge worktree from a prior merge attempt)
    let worktree_dir = tempfile::tempdir().unwrap();
    let wt_path = worktree_dir.path().join("merge-stale");
    let _ = std::process::Command::new("git")
        .args(["worktree", "add", &wt_path.to_string_lossy(), &plan_branch])
        .current_dir(repo.path())
        .output()
        .expect("git worktree add");

    // Now update_plan_from_main should detect the existing worktree and
    // merge main there instead of failing
    let result = update_plan_from_main(
        repo.path(),
        &plan_branch,
        "main",
        &project,
        "task-existing-wt",
        None,
    )
    .await;

    assert!(
        matches!(result, PlanUpdateResult::Updated),
        "Should use existing worktree to update plan branch. Got: {:?}",
        result
    );

    // Verify: main's fix commit should now be on the plan branch
    let log_output = std::process::Command::new("git")
        .args(["log", "--oneline", &plan_branch])
        .current_dir(repo.path())
        .output()
        .expect("git log");
    let log_str = String::from_utf8_lossy(&log_output.stdout);
    assert!(
        log_str.contains("clippy"),
        "Plan branch should contain the fix from main after existing-worktree merge. Log:\n{}",
        log_str,
    );

    // Clean up worktree
    let _ = std::process::Command::new("git")
        .args(["worktree", "remove", &wt_path.to_string_lossy()])
        .current_dir(repo.path())
        .output();
}

/// Regression test (Fix 0): existing-worktree path with merge-already-in-progress
/// returns PlanUpdateResult::Conflicts instead of Error.
///
/// Before the fix, when the plan branch was already in a conflicted merge state
/// (MERGE_HEAD exists from a prior attempt), calling merge_branch() would return
/// Err("You have not concluded your merge") — no "CONFLICT" in stderr — causing
/// the Err arm to call abort_merge and return PlanUpdateResult::Error.
///
/// After the fix, the Err arm checks is_merge_in_progress: if true, the conflict
/// markers from the prior attempt are still there — return PlanUpdateResult::Conflicts
/// so the agent can resolve them rather than treating it as a fatal error.
#[tokio::test]
async fn existing_worktree_with_prior_conflict_returns_conflicts_not_error() {
    // Setup: plan branch behind main WITH conflicting changes (so merge conflicts)
    let repo = setup_real_git_repo();
    let path = repo.path();

    // Create plan branch from main
    let _ = std::process::Command::new("git")
        .args(["branch", "plan/conflict-existing-wt"])
        .current_dir(path)
        .output();

    // Add a conflicting commit to main
    std::fs::write(path.join("shared.rs"), "// main version").unwrap();
    let _ = std::process::Command::new("git")
        .args(["add", "shared.rs"])
        .current_dir(path)
        .output();
    let _ = std::process::Command::new("git")
        .args(["commit", "-m", "fix: main changes shared.rs"])
        .current_dir(path)
        .output();

    // Add a conflicting commit on the plan branch
    let _ = std::process::Command::new("git")
        .args(["checkout", "plan/conflict-existing-wt"])
        .current_dir(path)
        .output();
    std::fs::write(path.join("shared.rs"), "// plan version").unwrap();
    let _ = std::process::Command::new("git")
        .args(["add", "shared.rs"])
        .current_dir(path)
        .output();
    let _ = std::process::Command::new("git")
        .args(["commit", "-m", "feat: plan changes shared.rs"])
        .current_dir(path)
        .output();

    // Back to main
    let _ = std::process::Command::new("git")
        .args(["checkout", "main"])
        .current_dir(path)
        .output();

    let project = make_test_project(&repo.path_string());

    // Simulate "prior attempt left worktree in conflicted state":
    // Create a worktree with the plan branch, then start a conflicting merge in it
    // (leaving MERGE_HEAD in place without committing or aborting).
    let worktree_dir = tempfile::tempdir().unwrap();
    let wt_path = worktree_dir.path().join("merge-prior-conflict");
    let _ = std::process::Command::new("git")
        .args([
            "worktree",
            "add",
            &wt_path.to_string_lossy(),
            "plan/conflict-existing-wt",
        ])
        .current_dir(path)
        .output()
        .expect("git worktree add");

    // Start the merge in the worktree — this will conflict (both sides modified shared.rs)
    // and leave MERGE_HEAD in place (no abort, simulating a prior attempt that was interrupted).
    let _ = std::process::Command::new("git")
        .args(["merge", "main", "--no-edit"])
        .current_dir(&wt_path)
        .output();
    // Do NOT abort — the worktree is now in a conflicted state with MERGE_HEAD

    // Verify precondition: merge is actually in progress in the worktree
    let merge_head_exists =
        wt_path.join(".git").join("MERGE_HEAD").exists() || wt_path.join("MERGE_HEAD").exists();
    // Also check via the git-state pattern (worktrees store MERGE_HEAD in .git/worktrees/<name>/)
    let merge_head_in_git = std::fs::read_dir(path.join(".git").join("worktrees"))
        .ok()
        .and_then(|mut entries| {
            entries.find_map(|e| {
                let entry = e.ok()?;
                let merge_head = entry.path().join("MERGE_HEAD");
                if merge_head.exists() {
                    Some(true)
                } else {
                    None
                }
            })
        })
        .unwrap_or(false);

    if !merge_head_exists && !merge_head_in_git {
        // If for some reason the merge didn't conflict (environment difference),
        // skip this test — it requires a real conflicted state.
        let _ = std::process::Command::new("git")
            .args(["worktree", "remove", "--force", &wt_path.to_string_lossy()])
            .current_dir(path)
            .output();
        return;
    }

    // NOW call update_plan_from_main — should detect the existing worktree,
    // call merge_branch which returns Err("You have not concluded your merge"),
    // then detect is_merge_in_progress=true and return Conflicts instead of Error.
    let result = update_plan_from_main(
        path,
        "plan/conflict-existing-wt",
        "main",
        &project,
        "task-existing-conflict-wt",
        None,
    )
    .await;

    assert!(
        matches!(result, PlanUpdateResult::Conflicts { .. }),
        "Existing worktree with prior merge-in-progress must return Conflicts, not Error. \
         Before Fix 0, this returned PlanUpdateResult::Error. Got: {:?}",
        result
    );

    // Clean up
    let _ = std::process::Command::new("git")
        .args(["worktree", "remove", "--force", &wt_path.to_string_lossy()])
        .current_dir(path)
        .output();
}
