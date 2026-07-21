use super::super::*;

// =========================================================================
// Merge State Detection Tests (Phase 76)
// =========================================================================

#[test]
fn test_is_rebase_in_progress_no_rebase() {
    // Use a temp directory without rebase state
    let temp_dir = tempfile::tempdir().unwrap();
    let git_dir = temp_dir.path().join(".git");
    std::fs::create_dir(&git_dir).unwrap();

    assert!(!GitService::is_rebase_in_progress(temp_dir.path()));
}

#[test]
fn test_is_rebase_in_progress_with_rebase_merge() {
    let temp_dir = tempfile::tempdir().unwrap();
    let git_dir = temp_dir.path().join(".git");
    std::fs::create_dir(&git_dir).unwrap();

    // Simulate rebase-merge directory (interactive rebase in progress)
    std::fs::create_dir(git_dir.join("rebase-merge")).unwrap();

    assert!(GitService::is_rebase_in_progress(temp_dir.path()));
}

#[test]
fn test_is_rebase_in_progress_with_rebase_apply() {
    let temp_dir = tempfile::tempdir().unwrap();
    let git_dir = temp_dir.path().join(".git");
    std::fs::create_dir(&git_dir).unwrap();

    // Simulate rebase-apply directory (git am or older rebase in progress)
    std::fs::create_dir(git_dir.join("rebase-apply")).unwrap();

    assert!(GitService::is_rebase_in_progress(temp_dir.path()));
}

#[test]
fn test_is_rebase_in_progress_worktree_style() {
    // Test worktree-style .git file pointing to gitdir
    let temp_dir = tempfile::tempdir().unwrap();
    let git_path = temp_dir.path().join(".git");

    // Create the actual git directory somewhere else
    let actual_git_dir = temp_dir.path().join("actual_git_dir");
    std::fs::create_dir(&actual_git_dir).unwrap();

    // Create .git file pointing to actual git dir
    std::fs::write(&git_path, format!("gitdir: {}", actual_git_dir.display())).unwrap();

    // No rebase in progress
    assert!(!GitService::is_rebase_in_progress(temp_dir.path()));

    // Add rebase-merge to actual git dir
    std::fs::create_dir(actual_git_dir.join("rebase-merge")).unwrap();

    assert!(GitService::is_rebase_in_progress(temp_dir.path()));
}

// =========================================================================
// resolve_git_dir Tests
// =========================================================================

#[test]
fn test_resolve_git_dir_regular_repo() {
    let temp_dir = tempfile::tempdir().unwrap();
    let git_dir = temp_dir.path().join(".git");
    std::fs::create_dir(&git_dir).unwrap();

    assert_eq!(GitService::resolve_git_dir(temp_dir.path()), git_dir);
}

#[test]
fn test_resolve_git_dir_worktree_style() {
    let temp_dir = tempfile::tempdir().unwrap();
    let git_path = temp_dir.path().join(".git");

    let actual_git_dir = temp_dir.path().join("actual_git_dir");
    std::fs::create_dir(&actual_git_dir).unwrap();

    std::fs::write(&git_path, format!("gitdir: {}", actual_git_dir.display())).unwrap();

    assert_eq!(GitService::resolve_git_dir(temp_dir.path()), actual_git_dir);
}

// =========================================================================
// is_merge_in_progress Tests
// =========================================================================

#[test]
fn test_is_merge_in_progress_no_merge() {
    let temp_dir = tempfile::tempdir().unwrap();
    let git_dir = temp_dir.path().join(".git");
    std::fs::create_dir(&git_dir).unwrap();

    assert!(!GitService::is_merge_in_progress(temp_dir.path()));
}

#[test]
fn test_is_merge_in_progress_with_merge_head() {
    let temp_dir = tempfile::tempdir().unwrap();
    let git_dir = temp_dir.path().join(".git");
    std::fs::create_dir(&git_dir).unwrap();

    // Simulate MERGE_HEAD file (merge started but not committed)
    std::fs::write(git_dir.join("MERGE_HEAD"), "abc123\n").unwrap();

    assert!(GitService::is_merge_in_progress(temp_dir.path()));
}

#[test]
fn test_is_merge_in_progress_worktree_style() {
    // Test worktree-style .git file pointing to gitdir
    let temp_dir = tempfile::tempdir().unwrap();
    let git_path = temp_dir.path().join(".git");

    // Create the actual git directory somewhere else
    let actual_git_dir = temp_dir.path().join("actual_git_dir");
    std::fs::create_dir(&actual_git_dir).unwrap();

    // Create .git file pointing to actual git dir
    std::fs::write(&git_path, format!("gitdir: {}", actual_git_dir.display())).unwrap();

    // No merge in progress
    assert!(!GitService::is_merge_in_progress(temp_dir.path()));

    // Add MERGE_HEAD to actual git dir
    std::fs::write(actual_git_dir.join("MERGE_HEAD"), "abc123\n").unwrap();

    assert!(GitService::is_merge_in_progress(temp_dir.path()));
}

#[test]
fn state_query_unfinished_operation_reports_checked_regular_repo_state() {
    let temp_dir = tempfile::tempdir().unwrap();
    let git_dir = temp_dir.path().join(".git");
    std::fs::create_dir(&git_dir).unwrap();

    let settled = GitService::unfinished_operation_state(temp_dir.path()).unwrap();
    assert!(!settled.is_unfinished());

    std::fs::write(git_dir.join("MERGE_HEAD"), "abc123\n").unwrap();
    std::fs::create_dir(git_dir.join("rebase-apply")).unwrap();

    let unfinished = GitService::unfinished_operation_state(temp_dir.path()).unwrap();
    assert!(unfinished.is_unfinished());
    assert!(unfinished.merge_in_progress);
    assert!(unfinished.rebase_in_progress);
}

#[test]
fn state_query_unfinished_operation_follows_relative_linked_worktree_git_dir() {
    let temp_dir = tempfile::tempdir().unwrap();
    let worktree = temp_dir.path().join("worktree");
    let git_dir = worktree.join("metadata").join("worktrees").join("linked");
    std::fs::create_dir_all(&worktree).unwrap();
    std::fs::create_dir_all(&git_dir).unwrap();
    std::fs::write(worktree.join(".git"), "gitdir: metadata/worktrees/linked\n").unwrap();
    std::fs::create_dir(git_dir.join("rebase-merge")).unwrap();

    let state = GitService::unfinished_operation_state(&worktree).unwrap();

    assert!(!state.merge_in_progress);
    assert!(state.rebase_in_progress);
}

#[test]
fn state_query_unfinished_operation_rejects_malformed_git_indirection() {
    let temp_dir = tempfile::tempdir().unwrap();
    std::fs::write(temp_dir.path().join(".git"), "not-a-gitdir\n").unwrap();

    let error = GitService::unfinished_operation_state(temp_dir.path())
        .expect_err("malformed linked-worktree metadata must fail closed");

    assert!(matches!(error, AppError::Validation(_)));
}

#[test]
fn state_query_unfinished_operation_rejects_empty_git_indirection() {
    let temp_dir = tempfile::tempdir().unwrap();
    std::fs::write(temp_dir.path().join(".git"), "gitdir: \n").unwrap();

    let error = GitService::unfinished_operation_state(temp_dir.path())
        .expect_err("empty linked-worktree metadata must fail closed");

    assert!(matches!(error, AppError::Validation(_)));
}

#[test]
fn state_query_unfinished_operation_rejects_unsafe_git_indirection() {
    let temp_dir = tempfile::tempdir().unwrap();
    std::fs::write(temp_dir.path().join(".git"), "gitdir: ../../escaped\n").unwrap();

    let error = GitService::unfinished_operation_state(temp_dir.path())
        .expect_err("unsafe linked-worktree metadata must fail closed");

    assert!(matches!(error, AppError::Validation(_)));
}
