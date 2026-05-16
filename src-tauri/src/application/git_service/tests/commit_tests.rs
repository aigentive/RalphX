use super::super::*;
use super::init_test_repo;
use std::process::Command;

// =========================================================================
// commit_all Tests
// =========================================================================

#[tokio::test]
async fn test_commit_all_excludes_environment_artifacts() {
    let temp_dir = tempfile::tempdir().unwrap();
    let repo = temp_dir.path();

    Command::new("git")
        .args(["init"])
        .current_dir(repo)
        .output()
        .unwrap();
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

    // Create .gitignore with environment artifact patterns
    std::fs::write(repo.join(".gitignore"), "node_modules\nsrc-tauri/target\n").unwrap();

    std::fs::write(repo.join("tracked.txt"), "initial").unwrap();
    Command::new("git")
        .args(["add", "."])
        .current_dir(repo)
        .output()
        .unwrap();
    Command::new("git")
        .args(["commit", "-m", "initial"])
        .current_dir(repo)
        .output()
        .unwrap();

    // Create .gitignore to exclude environment artifacts
    std::fs::write(repo.join(".gitignore"), "node_modules\nsrc-tauri/target\n").unwrap();

    std::fs::write(repo.join("tracked.txt"), "updated").unwrap();
    std::fs::write(repo.join("node_modules"), "placeholder").unwrap();
    std::fs::create_dir_all(repo.join("src-tauri")).unwrap();
    std::fs::write(repo.join("src-tauri/target"), "placeholder").unwrap();

    let commit_sha = GitService::commit_all(repo, "test commit")
        .await
        .unwrap()
        .expect("commit should be created");
    assert!(!commit_sha.is_empty());

    let tracked_files = Command::new("git")
        .args(["ls-tree", "-r", "--name-only", "HEAD"])
        .current_dir(repo)
        .output()
        .unwrap();
    let tracked_files = String::from_utf8_lossy(&tracked_files.stdout);

    assert!(tracked_files.contains("tracked.txt"));
    assert!(!tracked_files.contains("node_modules"));
    assert!(!tracked_files.contains("src-tauri/target"));
}

// =========================================================================
// commit_all: Safe Staging (No Deletions)
// =========================================================================

#[tokio::test]
async fn test_commit_all_does_not_stage_deleted_files() {
    let temp_dir = tempfile::tempdir().unwrap();
    let repo = temp_dir.path();
    init_test_repo(repo);

    // Create two files and commit them
    std::fs::write(repo.join("keep.txt"), "keep me").unwrap();
    std::fs::write(repo.join("delete_me.txt"), "will be deleted").unwrap();
    Command::new("git")
        .args(["add", "."])
        .current_dir(repo)
        .output()
        .unwrap();
    Command::new("git")
        .args(["commit", "-m", "initial"])
        .current_dir(repo)
        .output()
        .unwrap();

    // Delete one file from disk, modify the other
    std::fs::remove_file(repo.join("delete_me.txt")).unwrap();
    std::fs::write(repo.join("keep.txt"), "modified").unwrap();

    let sha = GitService::commit_all(repo, "safe commit")
        .await
        .unwrap()
        .expect("commit should be created");
    assert!(!sha.is_empty());

    // Verify delete_me.txt is still in HEAD (deletion was NOT staged)
    let tree = Command::new("git")
        .args(["ls-tree", "-r", "--name-only", "HEAD"])
        .current_dir(repo)
        .output()
        .unwrap();
    let tree_output = String::from_utf8_lossy(&tree.stdout);
    assert!(
        tree_output.contains("delete_me.txt"),
        "Deleted file must NOT be staged by commit_all"
    );
    assert!(tree_output.contains("keep.txt"));

    // Verify keep.txt was updated
    let show = Command::new("git")
        .args(["show", "HEAD:keep.txt"])
        .current_dir(repo)
        .output()
        .unwrap();
    assert_eq!(String::from_utf8_lossy(&show.stdout).trim(), "modified");
}

#[tokio::test]
async fn test_commit_all_stages_new_untracked_files() {
    let temp_dir = tempfile::tempdir().unwrap();
    let repo = temp_dir.path();
    init_test_repo(repo);

    std::fs::write(repo.join("initial.txt"), "initial").unwrap();
    Command::new("git")
        .args(["add", "."])
        .current_dir(repo)
        .output()
        .unwrap();
    Command::new("git")
        .args(["commit", "-m", "initial"])
        .current_dir(repo)
        .output()
        .unwrap();

    // Create new untracked file
    std::fs::write(repo.join("new_file.txt"), "brand new").unwrap();

    let sha = GitService::commit_all(repo, "add new file")
        .await
        .unwrap()
        .expect("commit should be created");
    assert!(!sha.is_empty());

    let tree = Command::new("git")
        .args(["ls-tree", "-r", "--name-only", "HEAD"])
        .current_dir(repo)
        .output()
        .unwrap();
    let tree_output = String::from_utf8_lossy(&tree.stdout);
    assert!(
        tree_output.contains("new_file.txt"),
        "New untracked files must be staged by commit_all"
    );
}

#[tokio::test]
async fn test_commit_all_stages_modified_files() {
    let temp_dir = tempfile::tempdir().unwrap();
    let repo = temp_dir.path();
    init_test_repo(repo);

    std::fs::write(repo.join("file.txt"), "original").unwrap();
    Command::new("git")
        .args(["add", "."])
        .current_dir(repo)
        .output()
        .unwrap();
    Command::new("git")
        .args(["commit", "-m", "initial"])
        .current_dir(repo)
        .output()
        .unwrap();

    std::fs::write(repo.join("file.txt"), "modified content").unwrap();

    let sha = GitService::commit_all(repo, "modify file")
        .await
        .unwrap()
        .expect("commit should be created");
    assert!(!sha.is_empty());

    let show = Command::new("git")
        .args(["show", "HEAD:file.txt"])
        .current_dir(repo)
        .output()
        .unwrap();
    assert_eq!(
        String::from_utf8_lossy(&show.stdout).trim(),
        "modified content"
    );
}

#[tokio::test]
async fn test_commit_all_including_deletions_stages_deleted_files() {
    let temp_dir = tempfile::tempdir().unwrap();
    let repo = temp_dir.path();
    init_test_repo(repo);

    std::fs::write(repo.join("keep.txt"), "keep").unwrap();
    std::fs::write(repo.join("remove.txt"), "remove me").unwrap();
    Command::new("git")
        .args(["add", "."])
        .current_dir(repo)
        .output()
        .unwrap();
    Command::new("git")
        .args(["commit", "-m", "initial"])
        .current_dir(repo)
        .output()
        .unwrap();

    std::fs::remove_file(repo.join("remove.txt")).unwrap();
    std::fs::write(repo.join("keep.txt"), "updated").unwrap();

    let sha = GitService::commit_all_including_deletions(repo, "merge commit")
        .await
        .unwrap()
        .expect("commit should be created");
    assert!(!sha.is_empty());

    let tree = Command::new("git")
        .args(["ls-tree", "-r", "--name-only", "HEAD"])
        .current_dir(repo)
        .output()
        .unwrap();
    let tree_output = String::from_utf8_lossy(&tree.stdout);
    assert!(
        !tree_output.contains("remove.txt"),
        "commit_all_including_deletions must stage deletions"
    );
    assert!(tree_output.contains("keep.txt"));
}

#[tokio::test]
async fn test_commit_all_handles_files_with_spaces() {
    let temp_dir = tempfile::tempdir().unwrap();
    let repo = temp_dir.path();
    init_test_repo(repo);

    std::fs::write(repo.join("normal.txt"), "init").unwrap();
    Command::new("git")
        .args(["add", "."])
        .current_dir(repo)
        .output()
        .unwrap();
    Command::new("git")
        .args(["commit", "-m", "initial"])
        .current_dir(repo)
        .output()
        .unwrap();

    std::fs::write(repo.join("file with spaces.txt"), "content").unwrap();

    let sha = GitService::commit_all(repo, "spaces in name")
        .await
        .unwrap()
        .expect("commit should be created");
    assert!(!sha.is_empty());

    let tree = Command::new("git")
        .args(["ls-tree", "-r", "--name-only", "HEAD"])
        .current_dir(repo)
        .output()
        .unwrap();
    let tree_output = String::from_utf8_lossy(&tree.stdout);
    assert!(
        tree_output.contains("file with spaces.txt"),
        "Files with spaces must be staged correctly"
    );
}

#[tokio::test]
async fn test_commit_all_handles_renamed_files() {
    let temp_dir = tempfile::tempdir().unwrap();
    let repo = temp_dir.path();
    init_test_repo(repo);

    std::fs::write(repo.join("old_name.txt"), "content").unwrap();
    Command::new("git")
        .args(["add", "."])
        .current_dir(repo)
        .output()
        .unwrap();
    Command::new("git")
        .args(["commit", "-m", "initial"])
        .current_dir(repo)
        .output()
        .unwrap();

    // Rename via git mv so git detects it as a rename
    Command::new("git")
        .args(["mv", "old_name.txt", "new name.txt"])
        .current_dir(repo)
        .output()
        .unwrap();

    let sha = GitService::commit_all(repo, "rename file")
        .await
        .unwrap()
        .expect("commit should be created");
    assert!(!sha.is_empty());

    let tree = Command::new("git")
        .args(["ls-tree", "-r", "--name-only", "HEAD"])
        .current_dir(repo)
        .output()
        .unwrap();
    let tree_output = String::from_utf8_lossy(&tree.stdout);
    assert!(
        tree_output.contains("new name.txt"),
        "Renamed file must appear with new name"
    );
}

#[tokio::test]
async fn test_commit_all_handles_utf8_filenames() {
    let temp_dir = tempfile::tempdir().unwrap();
    let repo = temp_dir.path();
    init_test_repo(repo);

    std::fs::write(repo.join("base.txt"), "init").unwrap();
    Command::new("git")
        .args(["add", "."])
        .current_dir(repo)
        .output()
        .unwrap();
    Command::new("git")
        .args(["commit", "-m", "initial"])
        .current_dir(repo)
        .output()
        .unwrap();

    std::fs::write(repo.join("café.txt"), "utf8 content").unwrap();

    let sha = GitService::commit_all(repo, "utf8 filename")
        .await
        .unwrap()
        .expect("commit should be created");
    assert!(!sha.is_empty());

    // Use -z to get unquoted filenames (git ls-tree quotes UTF-8 by default)
    let tree = Command::new("git")
        .args(["ls-tree", "-r", "--name-only", "-z", "HEAD"])
        .current_dir(repo)
        .output()
        .unwrap();
    let tree_output = String::from_utf8_lossy(&tree.stdout);
    assert!(
        tree_output.contains("café.txt"),
        "UTF-8 filenames must be staged correctly"
    );
}

#[tokio::test]
async fn test_commit_all_returns_none_when_only_deletions() {
    let temp_dir = tempfile::tempdir().unwrap();
    let repo = temp_dir.path();
    init_test_repo(repo);

    std::fs::write(repo.join("only_file.txt"), "content").unwrap();
    Command::new("git")
        .args(["add", "."])
        .current_dir(repo)
        .output()
        .unwrap();
    Command::new("git")
        .args(["commit", "-m", "initial"])
        .current_dir(repo)
        .output()
        .unwrap();

    // Only change is a deletion — commit_all should skip it and return None
    std::fs::remove_file(repo.join("only_file.txt")).unwrap();

    let result = GitService::commit_all(repo, "should be empty")
        .await
        .unwrap();
    assert!(
        result.is_none(),
        "commit_all should return None when the only changes are deletions"
    );

    // Verify file still exists in HEAD
    let tree = Command::new("git")
        .args(["ls-tree", "-r", "--name-only", "HEAD"])
        .current_dir(repo)
        .output()
        .unwrap();
    let tree_output = String::from_utf8_lossy(&tree.stdout);
    assert!(tree_output.contains("only_file.txt"));
}

// =========================================================================
// Conflict Marker Detection Tests
// =========================================================================

#[tokio::test]
async fn test_has_conflict_markers_ignores_committed_marker_literals() {
    let temp_dir = tempfile::tempdir().unwrap();
    let repo = temp_dir.path();
    init_test_repo(repo);

    // Commit a file that intentionally contains marker-like content.
    std::fs::write(
        repo.join("fixture.txt"),
        "this literal is intentional: <<<<<<< HEAD\n",
    )
    .unwrap();
    Command::new("git")
        .args(["add", "."])
        .current_dir(repo)
        .output()
        .unwrap();
    Command::new("git")
        .args(["commit", "-m", "add fixture"])
        .current_dir(repo)
        .output()
        .unwrap();

    let has_markers = GitService::has_conflict_markers(repo).await.unwrap();
    assert!(
        !has_markers,
        "Committed marker literals in unchanged files should not block merge completion"
    );
}

#[tokio::test]
async fn test_has_conflict_markers_detects_unstaged_markers() {
    let temp_dir = tempfile::tempdir().unwrap();
    let repo = temp_dir.path();
    init_test_repo(repo);

    std::fs::write(repo.join("file.txt"), "line one\nline two\n").unwrap();
    Command::new("git")
        .args(["add", "."])
        .current_dir(repo)
        .output()
        .unwrap();
    Command::new("git")
        .args(["commit", "-m", "initial"])
        .current_dir(repo)
        .output()
        .unwrap();

    std::fs::write(
        repo.join("file.txt"),
        "<<<<<<< ours\nline\n=======\nline\n>>>>>>> theirs\n",
    )
    .unwrap();

    let has_markers = GitService::has_conflict_markers(repo).await.unwrap();
    assert!(
        has_markers,
        "Unstaged conflict markers in changed files should be detected"
    );
}

// =========================================================================
// branches_have_same_content Tests
// =========================================================================

#[tokio::test]
async fn test_branches_have_same_content_identical() {
    let temp_dir = tempfile::tempdir().unwrap();
    let repo = temp_dir.path();
    init_test_repo(repo);

    // Create initial commit on main
    std::fs::write(repo.join("file.txt"), "hello\n").unwrap();
    Command::new("git")
        .args(["add", "."])
        .current_dir(repo)
        .output()
        .unwrap();
    Command::new("git")
        .args(["commit", "-m", "initial"])
        .current_dir(repo)
        .output()
        .unwrap();

    // Create a branch pointing at the same commit (identical content)
    Command::new("git")
        .args(["branch", "feature"])
        .current_dir(repo)
        .output()
        .unwrap();

    let result = GitService::branches_have_same_content(repo, "main", "feature")
        .await
        .unwrap();
    assert!(result, "Branches at same commit should be identical");
}

#[tokio::test]
async fn test_branches_have_same_content_diverged() {
    let temp_dir = tempfile::tempdir().unwrap();
    let repo = temp_dir.path();
    init_test_repo(repo);

    // Create initial commit on main
    std::fs::write(repo.join("file.txt"), "hello\n").unwrap();
    Command::new("git")
        .args(["add", "."])
        .current_dir(repo)
        .output()
        .unwrap();
    Command::new("git")
        .args(["commit", "-m", "initial"])
        .current_dir(repo)
        .output()
        .unwrap();

    // Create feature branch and add a commit
    Command::new("git")
        .args(["checkout", "-b", "feature"])
        .current_dir(repo)
        .output()
        .unwrap();
    std::fs::write(repo.join("file.txt"), "changed\n").unwrap();
    Command::new("git")
        .args(["add", "."])
        .current_dir(repo)
        .output()
        .unwrap();
    Command::new("git")
        .args(["commit", "-m", "feature change"])
        .current_dir(repo)
        .output()
        .unwrap();

    let result = GitService::branches_have_same_content(repo, "main", "feature")
        .await
        .unwrap();
    assert!(
        !result,
        "Branches with different content should not be identical"
    );
}

// =========================================================================
// commit_all_including_deletions: Safe Batched Staging (replaces git add -A)
// =========================================================================

#[tokio::test]
async fn test_commit_all_including_deletions_respects_gitignore() {
    let temp_dir = tempfile::tempdir().unwrap();
    let repo = temp_dir.path();
    init_test_repo(repo);

    // Create .gitignore FIRST, then initial commit
    std::fs::write(repo.join(".gitignore"), "build/\n*.log\n").unwrap();
    std::fs::write(repo.join("tracked.txt"), "initial").unwrap();
    Command::new("git")
        .args(["add", "."])
        .current_dir(repo)
        .output()
        .unwrap();
    Command::new("git")
        .args(["commit", "-m", "initial"])
        .current_dir(repo)
        .output()
        .unwrap();

    // Create ignored files
    std::fs::create_dir_all(repo.join("build")).unwrap();
    std::fs::write(repo.join("build/output.js"), "compiled").unwrap();
    std::fs::write(repo.join("build/output.d.ts"), "types").unwrap();
    std::fs::write(repo.join("app.log"), "log data").unwrap();

    // Also modify tracked file
    std::fs::write(repo.join("tracked.txt"), "modified").unwrap();

    let sha = GitService::commit_all_including_deletions(repo, "should not include ignored")
        .await
        .unwrap()
        .expect("commit should be created");
    assert!(!sha.is_empty());

    // Verify ignored files are NOT in the commit
    let tree = Command::new("git")
        .args(["ls-tree", "-r", "--name-only", "HEAD"])
        .current_dir(repo)
        .output()
        .unwrap();
    let tree_output = String::from_utf8_lossy(&tree.stdout);
    assert!(
        !tree_output.contains("build/output.js"),
        "commit_all_including_deletions must respect .gitignore (build/output.js)"
    );
    assert!(
        !tree_output.contains("build/output.d.ts"),
        "commit_all_including_deletions must respect .gitignore (build/output.d.ts)"
    );
    assert!(
        !tree_output.contains("app.log"),
        "commit_all_including_deletions must respect .gitignore (*.log)"
    );
    assert!(
        tree_output.contains("tracked.txt"),
        "Tracked file should still be committed"
    );
}

#[tokio::test]
async fn test_commit_all_including_deletions_skips_ralphx_generated_artifacts_only() {
    let temp_dir = tempfile::tempdir().unwrap();
    let repo = temp_dir.path();
    init_test_repo(repo);

    std::fs::write(repo.join("tracked.txt"), "initial").unwrap();
    Command::new("git")
        .args(["add", "."])
        .current_dir(repo)
        .output()
        .unwrap();
    Command::new("git")
        .args(["commit", "-m", "initial"])
        .current_dir(repo)
        .output()
        .unwrap();

    std::fs::write(repo.join("tracked.txt"), "modified").unwrap();

    std::fs::create_dir_all(repo.join(".claude/mcp-proxy")).unwrap();
    std::fs::write(repo.join(".claude/mcp-proxy/trace.jsonl"), "{}\n").unwrap();
    std::fs::create_dir_all(repo.join(".claude/worktrees/session-a")).unwrap();
    std::fs::write(repo.join(".claude/worktrees/session-a/file.txt"), "scratch").unwrap();
    std::fs::create_dir_all(repo.join(".claude/memory-archive/memories")).unwrap();
    std::fs::write(
        repo.join(".claude/memory-archive/memories/mem.md"),
        "snapshot",
    )
    .unwrap();
    std::fs::create_dir_all(repo.join(".artifacts/screenshots")).unwrap();
    std::fs::write(repo.join(".artifacts/screenshots/qa.png"), "png").unwrap();

    std::fs::create_dir_all(repo.join(".claude/agents")).unwrap();
    std::fs::write(repo.join(".claude/agents/reviewer.md"), "project subagent").unwrap();

    let sha = GitService::commit_all_including_deletions(repo, "skip generated artifacts")
        .await
        .unwrap()
        .expect("commit should be created");
    assert!(!sha.is_empty());

    let tree = Command::new("git")
        .args(["ls-tree", "-r", "--name-only", "HEAD"])
        .current_dir(repo)
        .output()
        .unwrap();
    let tree_output = String::from_utf8_lossy(&tree.stdout);

    assert!(tree_output.contains("tracked.txt"));
    assert!(
        tree_output.contains(".claude/agents/reviewer.md"),
        "Claude-native project subagents should remain stageable"
    );
    assert!(!tree_output.contains(".claude/mcp-proxy/trace.jsonl"));
    assert!(!tree_output.contains(".claude/worktrees/session-a/file.txt"));
    assert!(!tree_output.contains(".claude/memory-archive/memories/mem.md"));
    assert!(!tree_output.contains(".artifacts/screenshots/qa.png"));
}

#[tokio::test]
async fn test_commit_all_including_deletions_handles_renames() {
    let temp_dir = tempfile::tempdir().unwrap();
    let repo = temp_dir.path();
    init_test_repo(repo);

    std::fs::write(repo.join("original.txt"), "content for rename test").unwrap();
    std::fs::write(repo.join("other.txt"), "other content").unwrap();
    Command::new("git")
        .args(["add", "."])
        .current_dir(repo)
        .output()
        .unwrap();
    Command::new("git")
        .args(["commit", "-m", "initial"])
        .current_dir(repo)
        .output()
        .unwrap();

    // Rename via git mv so git detects it as a rename (R status)
    Command::new("git")
        .args(["mv", "original.txt", "renamed.txt"])
        .current_dir(repo)
        .output()
        .unwrap();

    // Also modify other.txt to have mixed changes
    std::fs::write(repo.join("other.txt"), "modified other").unwrap();

    let sha = GitService::commit_all_including_deletions(repo, "rename + modify")
        .await
        .unwrap()
        .expect("commit should be created");
    assert!(!sha.is_empty());

    let tree = Command::new("git")
        .args(["ls-tree", "-r", "--name-only", "HEAD"])
        .current_dir(repo)
        .output()
        .unwrap();
    let tree_output = String::from_utf8_lossy(&tree.stdout);
    assert!(
        tree_output.contains("renamed.txt"),
        "Renamed file must appear with new name"
    );
    assert!(
        !tree_output.contains("original.txt"),
        "Original name should no longer exist after rename"
    );
    assert!(
        tree_output.contains("other.txt"),
        "Other modified files should still be present"
    );
}

#[tokio::test]
async fn test_commit_all_including_deletions_batches_large_file_count() {
    let temp_dir = tempfile::tempdir().unwrap();
    let repo = temp_dir.path();
    init_test_repo(repo);

    // Create initial commit
    std::fs::write(repo.join("base.txt"), "base").unwrap();
    Command::new("git")
        .args(["add", "."])
        .current_dir(repo)
        .output()
        .unwrap();
    Command::new("git")
        .args(["commit", "-m", "initial"])
        .current_dir(repo)
        .output()
        .unwrap();

    // Create 150 files (exceeds batch size of 100) to verify batching works
    for i in 0..150 {
        std::fs::write(
            repo.join(format!("file_{:03}.txt", i)),
            format!("content {}", i),
        )
        .unwrap();
    }

    let sha = GitService::commit_all_including_deletions(repo, "batch test")
        .await
        .unwrap()
        .expect("commit should be created");
    assert!(!sha.is_empty());

    // Verify all 150 files are committed
    let tree = Command::new("git")
        .args(["ls-tree", "-r", "--name-only", "HEAD"])
        .current_dir(repo)
        .output()
        .unwrap();
    let tree_output = String::from_utf8_lossy(&tree.stdout);
    for i in 0..150 {
        assert!(
            tree_output.contains(&format!("file_{:03}.txt", i)),
            "File file_{:03}.txt should be committed (batch staging)",
            i
        );
    }
}
