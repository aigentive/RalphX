use super::super::*;
use super::init_test_repo;
use crate::error::AppError;
use std::process::Command;

#[test]
fn test_parse_shortstat_full() {
    let output = " 3 files changed, 50 insertions(+), 10 deletions(-)";
    let (files, insertions, deletions) = GitService::parse_shortstat(output);
    assert_eq!(files, 3);
    assert_eq!(insertions, 50);
    assert_eq!(deletions, 10);
}

#[test]
fn test_parse_shortstat_insertions_only() {
    let output = " 1 file changed, 25 insertions(+)";
    let (files, insertions, deletions) = GitService::parse_shortstat(output);
    assert_eq!(files, 1);
    assert_eq!(insertions, 25);
    assert_eq!(deletions, 0);
}

#[test]
fn test_parse_shortstat_deletions_only() {
    let output = " 2 files changed, 15 deletions(-)";
    let (files, insertions, deletions) = GitService::parse_shortstat(output);
    assert_eq!(files, 2);
    assert_eq!(insertions, 0);
    assert_eq!(deletions, 15);
}

#[test]
fn test_parse_shortstat_empty() {
    let output = "";
    let (files, insertions, deletions) = GitService::parse_shortstat(output);
    assert_eq!(files, 0);
    assert_eq!(insertions, 0);
    assert_eq!(deletions, 0);
}

#[tokio::test]
async fn get_merge_base_returns_common_ancestor_for_diverged_branches() {
    let temp_dir = tempfile::tempdir().unwrap();
    let repo = temp_dir.path();
    init_test_repo(repo);

    std::fs::write(repo.join("base.txt"), "base\n").unwrap();
    Command::new("git")
        .args(["add", "base.txt"])
        .current_dir(repo)
        .output()
        .unwrap();
    Command::new("git")
        .args(["commit", "-m", "base"])
        .current_dir(repo)
        .output()
        .unwrap();
    let base_sha = String::from_utf8_lossy(
        &Command::new("git")
            .args(["rev-parse", "HEAD"])
            .current_dir(repo)
            .output()
            .unwrap()
            .stdout,
    )
    .trim()
    .to_string();

    Command::new("git")
        .args(["checkout", "-b", "feature"])
        .current_dir(repo)
        .output()
        .unwrap();
    std::fs::write(repo.join("feature.txt"), "feature\n").unwrap();
    Command::new("git")
        .args(["add", "feature.txt"])
        .current_dir(repo)
        .output()
        .unwrap();
    Command::new("git")
        .args(["commit", "-m", "feature"])
        .current_dir(repo)
        .output()
        .unwrap();

    Command::new("git")
        .args(["checkout", "main"])
        .current_dir(repo)
        .output()
        .unwrap();
    std::fs::write(repo.join("main.txt"), "main\n").unwrap();
    Command::new("git")
        .args(["add", "main.txt"])
        .current_dir(repo)
        .output()
        .unwrap();
    Command::new("git")
        .args(["commit", "-m", "main"])
        .current_dir(repo)
        .output()
        .unwrap();

    let merge_base = GitService::get_merge_base(repo, "main", "feature")
        .await
        .expect("merge base should resolve");

    assert_eq!(merge_base, base_sha);
}

#[tokio::test]
async fn get_merge_base_reports_git_failure_for_unknown_refs() {
    let temp_dir = tempfile::tempdir().unwrap();
    let repo = temp_dir.path();
    init_test_repo(repo);

    let result = GitService::get_merge_base(repo, "main", "missing-branch").await;

    assert!(
        matches!(result, Err(AppError::GitOperation(message)) if message.contains("Failed to resolve merge base between 'main' and 'missing-branch'"))
    );
}

// =========================================================================
// is_commit_on_branch Tests (Phase 78)
// =========================================================================

#[tokio::test]
async fn test_is_commit_on_branch_with_valid_ancestor() {
    // Create a temp git repo with a commit on main
    let temp_dir = tempfile::tempdir().unwrap();
    let repo = temp_dir.path();

    // Initialize repo
    Command::new("git")
        .args(["init"])
        .current_dir(repo)
        .output()
        .unwrap();

    // Configure git user for commits
    Command::new("git")
        .args(["config", "user.email", "test@example.com"])
        .current_dir(repo)
        .output()
        .unwrap();
    Command::new("git")
        .args(["config", "user.name", "Test User"])
        .current_dir(repo)
        .output()
        .unwrap();

    // Create initial commit
    std::fs::write(repo.join("test.txt"), "initial").unwrap();
    Command::new("git")
        .args(["add", "."])
        .current_dir(repo)
        .output()
        .unwrap();
    Command::new("git")
        .args(["commit", "-m", "initial commit"])
        .current_dir(repo)
        .output()
        .unwrap();

    // Get the commit SHA
    let output = Command::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(repo)
        .output()
        .unwrap();
    let commit_sha = String::from_utf8_lossy(&output.stdout).trim().to_string();

    // Verify commit is on HEAD (main/master)
    let result = GitService::is_commit_on_branch(repo, &commit_sha, "HEAD").await;
    assert!(result.is_ok());
    assert!(result.unwrap());
}

#[tokio::test]
async fn test_is_commit_on_branch_with_non_ancestor() {
    // Create a temp git repo with divergent branches
    let temp_dir = tempfile::tempdir().unwrap();
    let repo = temp_dir.path();

    // Initialize repo
    Command::new("git")
        .args(["init"])
        .current_dir(repo)
        .output()
        .unwrap();

    // Configure git user
    Command::new("git")
        .args(["config", "user.email", "test@example.com"])
        .current_dir(repo)
        .output()
        .unwrap();
    Command::new("git")
        .args(["config", "user.name", "Test User"])
        .current_dir(repo)
        .output()
        .unwrap();

    // Create initial commit on main
    std::fs::write(repo.join("test.txt"), "initial").unwrap();
    Command::new("git")
        .args(["add", "."])
        .current_dir(repo)
        .output()
        .unwrap();
    Command::new("git")
        .args(["commit", "-m", "initial commit"])
        .current_dir(repo)
        .output()
        .unwrap();

    // Create a feature branch
    Command::new("git")
        .args(["checkout", "-b", "feature"])
        .current_dir(repo)
        .output()
        .unwrap();

    // Make commit on feature branch
    std::fs::write(repo.join("feature.txt"), "feature content").unwrap();
    Command::new("git")
        .args(["add", "."])
        .current_dir(repo)
        .output()
        .unwrap();
    Command::new("git")
        .args(["commit", "-m", "feature commit"])
        .current_dir(repo)
        .output()
        .unwrap();

    // Get feature commit SHA
    let output = Command::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(repo)
        .output()
        .unwrap();
    let feature_sha = String::from_utf8_lossy(&output.stdout).trim().to_string();

    // Go back to main
    Command::new("git")
        .args(["checkout", "master"])
        .current_dir(repo)
        .output()
        .ok(); // May be "main" instead of "master"
    Command::new("git")
        .args(["checkout", "main"])
        .current_dir(repo)
        .output()
        .ok();

    // Get main branch name
    let branch_output = Command::new("git")
        .args(["rev-parse", "--abbrev-ref", "HEAD"])
        .current_dir(repo)
        .output()
        .unwrap();
    let main_branch = String::from_utf8_lossy(&branch_output.stdout)
        .trim()
        .to_string();

    // Feature commit should NOT be on main (not merged yet)
    let result = GitService::is_commit_on_branch(repo, &feature_sha, &main_branch).await;
    assert!(result.is_ok());
    assert!(!result.unwrap());
}

// =========================================================================
// get_commit_count Tests (Phase 78)
// =========================================================================

#[tokio::test]
async fn test_get_commit_count_empty_repo() {
    // Create a temp git repo with only an initial commit
    let temp_dir = tempfile::tempdir().unwrap();
    let repo = temp_dir.path();

    // Initialize repo
    Command::new("git")
        .args(["init"])
        .current_dir(repo)
        .output()
        .unwrap();

    // Configure git user
    Command::new("git")
        .args(["config", "user.email", "test@example.com"])
        .current_dir(repo)
        .output()
        .unwrap();
    Command::new("git")
        .args(["config", "user.name", "Test User"])
        .current_dir(repo)
        .output()
        .unwrap();

    // Create initial commit
    std::fs::write(repo.join("test.txt"), "initial").unwrap();
    Command::new("git")
        .args(["add", "."])
        .current_dir(repo)
        .output()
        .unwrap();
    Command::new("git")
        .args(["commit", "-m", "initial commit"])
        .current_dir(repo)
        .output()
        .unwrap();

    // Should have exactly 1 commit
    let result = GitService::get_commit_count(repo, "HEAD").await;
    assert!(result.is_ok());
    assert_eq!(result.unwrap(), 1);
}

#[tokio::test]
async fn test_get_commit_count_multiple_commits() {
    // Create a temp git repo with multiple commits
    let temp_dir = tempfile::tempdir().unwrap();
    let repo = temp_dir.path();

    // Initialize repo
    Command::new("git")
        .args(["init"])
        .current_dir(repo)
        .output()
        .unwrap();

    // Configure git user
    Command::new("git")
        .args(["config", "user.email", "test@example.com"])
        .current_dir(repo)
        .output()
        .unwrap();
    Command::new("git")
        .args(["config", "user.name", "Test User"])
        .current_dir(repo)
        .output()
        .unwrap();

    // Create 3 commits
    for i in 1..=3 {
        std::fs::write(
            repo.join(format!("test{}.txt", i)),
            format!("content {}", i),
        )
        .unwrap();
        Command::new("git")
            .args(["add", "."])
            .current_dir(repo)
            .output()
            .unwrap();
        Command::new("git")
            .args(["commit", "-m", &format!("commit {}", i)])
            .current_dir(repo)
            .output()
            .unwrap();
    }

    // Should have exactly 3 commits
    let result = GitService::get_commit_count(repo, "HEAD").await;
    assert!(result.is_ok());
    assert_eq!(result.unwrap(), 3);
}

// =========================================================================
// get_branch_authored_commits Tests
// =========================================================================
//
// Regression for PR-title contamination: after a base-update merge with a
// stale local base ref, `base..HEAD` includes the base's own commits (e.g.
// squash merges carrying another project's ticket token in the subject).
// Those commits must never count as branch-authored evidence.

#[tokio::test]
async fn get_branch_authored_commits_excludes_commits_imported_by_base_update_merge() {
    let temp_dir = tempfile::tempdir().unwrap();
    let repo = temp_dir.path();
    super::init_test_repo(repo);

    let commit = |message: &str| {
        Command::new("git")
            .args(["commit", "--allow-empty", "-m", message])
            .current_dir(repo)
            .output()
            .unwrap();
    };
    let checkout = |args: &[&str]| {
        Command::new("git")
            .args(args)
            .current_dir(repo)
            .output()
            .unwrap();
    };

    commit("base initial");
    checkout(&["checkout", "-b", "feature"]);
    commit("feature work");
    // Simulate the base advancing elsewhere while the local `main` ref stays
    // stale: the advanced base is merged into the branch from another ref.
    checkout(&["checkout", "-b", "advanced-base", "main"]);
    commit("MBE-3250: poisoned squash from another project");
    checkout(&["checkout", "feature"]);
    checkout(&["merge", "--no-edit", "advanced-base"]);
    commit("more feature work");

    // The naive range walk imports the poisoned base commit.
    let between = GitService::get_commits_between(repo, "main", "HEAD")
        .await
        .unwrap();
    assert!(
        between
            .iter()
            .any(|commit| commit.message.contains("MBE-3250")),
        "precondition: base..HEAD imports the base commit after the merge"
    );

    // The scoped walk keeps only branch-authored commits (plus the merge
    // commit itself, whose machine-generated subject carries no ticket token).
    let authored = GitService::get_branch_authored_commits(repo, "main")
        .await
        .unwrap();
    let subjects = authored
        .iter()
        .map(|commit| commit.message.as_str())
        .collect::<Vec<_>>();
    assert!(
        !subjects.iter().any(|subject| subject.contains("MBE-3250")),
        "imported base commit leaked into branch-authored evidence: {subjects:?}"
    );
    assert!(subjects.contains(&"feature work"));
    assert!(subjects.contains(&"more feature work"));
}

#[tokio::test]
async fn get_branch_authored_commits_tolerates_missing_origin_base_ref() {
    let temp_dir = tempfile::tempdir().unwrap();
    let repo = temp_dir.path();
    super::init_test_repo(repo);

    Command::new("git")
        .args(["commit", "--allow-empty", "-m", "base initial"])
        .current_dir(repo)
        .output()
        .unwrap();
    Command::new("git")
        .args(["checkout", "-b", "feature"])
        .current_dir(repo)
        .output()
        .unwrap();
    Command::new("git")
        .args(["commit", "--allow-empty", "-m", "feature work"])
        .current_dir(repo)
        .output()
        .unwrap();

    // No `origin` remote exists in this repo; the exclusion of
    // `origin/main` must be ignored rather than failing the walk.
    let authored = GitService::get_branch_authored_commits(repo, "main")
        .await
        .unwrap();
    assert_eq!(authored.len(), 1);
    assert_eq!(authored[0].message, "feature work");
}

// =========================================================================
// find_commit_by_message_grep Tests
// =========================================================================

#[tokio::test]
async fn test_find_commit_by_message_grep_finds_matching_commit() {
    let temp_dir = tempfile::tempdir().unwrap();
    let repo = temp_dir.path();

    super::init_test_repo(repo);

    // Create a commit with a task ID in the message
    std::fs::write(repo.join("file.txt"), "initial").unwrap();
    Command::new("git")
        .args(["add", "."])
        .current_dir(repo)
        .output()
        .unwrap();
    Command::new("git")
        .args(["commit", "-m", "task abc-123: initial work"])
        .current_dir(repo)
        .output()
        .unwrap();

    // Get the SHA of that commit for verification
    let expected_sha = {
        let out = Command::new("git")
            .args(["rev-parse", "HEAD"])
            .current_dir(repo)
            .output()
            .unwrap();
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    };

    // Get the branch name
    let branch = {
        let out = Command::new("git")
            .args(["branch", "--show-current"])
            .current_dir(repo)
            .output()
            .unwrap();
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    };

    let result = GitService::find_commit_by_message_grep(repo, "abc-123", &branch)
        .await
        .unwrap();
    assert_eq!(result, Some(expected_sha));
}

#[tokio::test]
async fn test_find_commit_by_message_grep_returns_none_when_not_found() {
    let temp_dir = tempfile::tempdir().unwrap();
    let repo = temp_dir.path();

    super::init_test_repo(repo);

    std::fs::write(repo.join("file.txt"), "initial").unwrap();
    Command::new("git")
        .args(["add", "."])
        .current_dir(repo)
        .output()
        .unwrap();
    Command::new("git")
        .args(["commit", "-m", "unrelated commit message"])
        .current_dir(repo)
        .output()
        .unwrap();

    let branch = {
        let out = Command::new("git")
            .args(["branch", "--show-current"])
            .current_dir(repo)
            .output()
            .unwrap();
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    };

    let result = GitService::find_commit_by_message_grep(repo, "nonexistent-id", &branch)
        .await
        .unwrap();
    assert_eq!(result, None);
}

// =========================================================================
// find_commit_by_message_grep — loose-match false positive regression test
// =========================================================================
//
// Demonstrates that grepping for just a task UUID (e.g. "abc-123-xyz") is
// too loose: a newer commit that merely *mentions* the UUID in its body
// (e.g. a rollback note) wins the `git log -1` race and becomes the
// "found" SHA, despite being completely unrelated to the task's own
// squash commit.
//
// The fix is to pass `source_branch` (e.g. "ralphx/ralphx/task-abc-123-xyz")
// as the grep pattern instead of just the task ID. The squash commit message
// format is:
//   `feat: <source_branch> (<title>)`
// so the branch name is an exact verbatim substring — no false positives.

#[tokio::test]
async fn test_find_commit_by_message_grep_loose_task_id_matches_unrelated_commit() {
    // ----- ARRANGE: two commits — only one is the real task squash commit -----
    let temp_dir = tempfile::tempdir().unwrap();
    let repo = temp_dir.path();
    super::init_test_repo(repo);

    let task_id = "abc-123-xyz";
    let source_branch = "ralphx/ralphx/task-abc-123-xyz";

    // Commit 1 — the legitimate task squash commit.
    // Message format mirrors build_squash_commit_msg: `<type>: <branch> (<title>)`
    std::fs::write(repo.join("work.txt"), "task work").unwrap();
    Command::new("git")
        .args(["add", "."])
        .current_dir(repo)
        .output()
        .unwrap();
    Command::new("git")
        .args([
            "commit",
            "-m",
            &format!("feat: {} (Implement feature X)", source_branch),
        ])
        .current_dir(repo)
        .output()
        .unwrap();
    let real_task_sha = {
        let out = Command::new("git")
            .args(["rev-parse", "HEAD"])
            .current_dir(repo)
            .output()
            .unwrap();
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    };

    // Commit 2 — an unrelated commit that merely *mentions* the task ID.
    // Simulates a rollback note, conflict annotation, or revert message.
    // It is NEWER than commit 1, so `git log -1 --grep=<task_id>` returns it.
    std::fs::write(repo.join("work.txt"), "rolled back").unwrap();
    Command::new("git")
        .args(["add", "."])
        .current_dir(repo)
        .output()
        .unwrap();
    Command::new("git")
        .args([
            "commit",
            "-m",
            &format!(
                "fix: rolled back changes related to {} due to conflicts",
                task_id
            ),
        ])
        .current_dir(repo)
        .output()
        .unwrap();
    let rollback_sha = {
        let out = Command::new("git")
            .args(["rev-parse", "HEAD"])
            .current_dir(repo)
            .output()
            .unwrap();
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    };

    let branch = {
        let out = Command::new("git")
            .args(["branch", "--show-current"])
            .current_dir(repo)
            .output()
            .unwrap();
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    };

    // ----- BUG: loose grep (task_id only) returns the WRONG commit -----
    // The rollback commit is newer and its message contains `task_id`,
    // so git log -1 returns it instead of the real task commit.
    let loose_result = GitService::find_commit_by_message_grep(repo, task_id, &branch)
        .await
        .unwrap();
    assert_eq!(
        loose_result,
        Some(rollback_sha.clone()),
        "loose grep returns the newer, unrelated rollback commit — not the real task commit"
    );

    // ----- FIX: precise grep (source_branch) returns the CORRECT commit -----
    // The rollback commit does not contain the full branch path, so no false match.
    let precise_result = GitService::find_commit_by_message_grep(repo, source_branch, &branch)
        .await
        .unwrap();
    assert_eq!(
        precise_result,
        Some(real_task_sha),
        "precise grep (source_branch) returns the real task squash commit"
    );
    assert_ne!(
        precise_result.as_deref(),
        Some(rollback_sha.as_str()),
        "precise grep must not return the rollback commit"
    );
}
