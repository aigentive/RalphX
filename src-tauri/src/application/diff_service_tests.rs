use super::*;
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use tempfile::TempDir;

#[test]
fn test_get_language_from_path() {
    assert_eq!(get_language_from_path("src/app.ts"), "typescript");
    assert_eq!(get_language_from_path("src/app.tsx"), "typescript");
    assert_eq!(get_language_from_path("main.rs"), "rust");
    assert_eq!(get_language_from_path("app.py"), "python");
    assert_eq!(get_language_from_path("config.json"), "json");
    assert_eq!(get_language_from_path("README.md"), "markdown");
    assert_eq!(get_language_from_path("unknown"), "plaintext");
}

#[test]
fn file_changes_parse_counts_from_single_numstat_output() {
    let mut counts = HashMap::new();
    counts.insert("src/a.rs".to_string(), (2, 1));
    counts.insert("src/b.rs".to_string(), (0, 4));

    let changes = file_changes_from_name_status("M\tsrc/b.rs\nA\tsrc/a.rs\n", &counts);

    assert_eq!(changes.len(), 2);
    assert_eq!(changes[0].path, "src/a.rs");
    assert!(matches!(changes[0].status, FileChangeStatus::Added));
    assert_eq!(changes[0].additions, 2);
    assert_eq!(changes[0].deletions, 1);
    assert_eq!(changes[1].path, "src/b.rs");
    assert!(matches!(changes[1].status, FileChangeStatus::Modified));
    assert_eq!(changes[1].additions, 0);
    assert_eq!(changes[1].deletions, 4);
}

// =========================================================================
// Conflict Detection Tests
// =========================================================================

/// Helper to create a git repo with initial commit
fn create_git_repo() -> (TempDir, PathBuf) {
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let repo_path = temp_dir.path().to_path_buf();

    // Initialize git repo
    std::process::Command::new("git")
        .args(["init"])
        .current_dir(&repo_path)
        .output()
        .expect("Failed to init git repo");

    // Configure git user
    std::process::Command::new("git")
        .args(["config", "user.email", "test@test.com"])
        .current_dir(&repo_path)
        .output()
        .expect("Failed to config git email");

    std::process::Command::new("git")
        .args(["config", "user.name", "Test User"])
        .current_dir(&repo_path)
        .output()
        .expect("Failed to config git name");

    // Create initial commit
    fs::write(repo_path.join("README.md"), "# Test Repo\n").expect("Failed to write README");
    std::process::Command::new("git")
        .args(["add", "."])
        .current_dir(&repo_path)
        .output()
        .expect("Failed to git add");

    std::process::Command::new("git")
        .args(["commit", "-m", "Initial commit"])
        .current_dir(&repo_path)
        .output()
        .expect("Failed to commit");

    (temp_dir, repo_path)
}

/// Helper to create a branch with a file change
fn create_branch_with_change(repo_path: &Path, branch_name: &str, file_name: &str, content: &str) {
    std::process::Command::new("git")
        .args(["checkout", "-b", branch_name])
        .current_dir(repo_path)
        .output()
        .expect("Failed to create branch");

    fs::write(repo_path.join(file_name), content).expect("Failed to write file");

    std::process::Command::new("git")
        .args(["add", "."])
        .current_dir(repo_path)
        .output()
        .expect("Failed to git add");

    std::process::Command::new("git")
        .args(["commit", "-m", &format!("Add {}", file_name)])
        .current_dir(repo_path)
        .output()
        .expect("Failed to commit");

    // Switch back to main
    std::process::Command::new("git")
        .args(["checkout", "master"])
        .current_dir(repo_path)
        .output()
        .expect("Failed to checkout master");
}

fn git_stdout(repo_path: &Path, args: &[&str]) -> String {
    let output = std::process::Command::new("git")
        .args(args)
        .current_dir(repo_path)
        .output()
        .expect("Failed to run git command");
    assert!(
        output.status.success(),
        "git {:?} failed: {}",
        args,
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).trim().to_string()
}

#[test]
fn test_get_worktree_file_changes_uses_combined_numstat_counts() {
    let (_temp_dir, repo_path) = create_git_repo();
    let base = git_stdout(&repo_path, &["rev-parse", "HEAD"]);
    fs::write(repo_path.join("README.md"), "# Test Repo\n\nMore detail\n")
        .expect("Failed to update README");
    fs::write(
        repo_path.join("src.rs"),
        "fn main() {}\nprintln!(\"hi\");\n",
    )
    .expect("Failed to write added file");
    std::process::Command::new("git")
        .args(["add", "."])
        .current_dir(&repo_path)
        .output()
        .expect("Failed to git add");

    let diff_service = DiffService::new();
    let changes = diff_service
        .get_worktree_file_changes_from_ref(&repo_path.to_string_lossy(), &base)
        .unwrap();

    assert_eq!(changes.len(), 2);
    let readme = changes
        .iter()
        .find(|change| change.path == "README.md")
        .unwrap();
    assert!(matches!(readme.status, FileChangeStatus::Modified));
    assert_eq!(readme.additions, 2);
    assert_eq!(readme.deletions, 0);
    let added = changes
        .iter()
        .find(|change| change.path == "src.rs")
        .unwrap();
    assert!(matches!(added.status, FileChangeStatus::Added));
    assert_eq!(added.additions, 2);
    assert_eq!(added.deletions, 0);
}

#[test]
fn test_get_file_changes_between_refs_uses_combined_numstat_counts() {
    let (_temp_dir, repo_path) = create_git_repo();
    let base = git_stdout(&repo_path, &["rev-parse", "HEAD"]);
    fs::write(
        repo_path.join("src.rs"),
        "fn main() {}\nprintln!(\"hi\");\n",
    )
    .expect("Failed to write added file");
    std::process::Command::new("git")
        .args(["add", "."])
        .current_dir(&repo_path)
        .output()
        .expect("Failed to git add");
    std::process::Command::new("git")
        .args(["commit", "-m", "Add source"])
        .current_dir(&repo_path)
        .output()
        .expect("Failed to commit");

    let diff_service = DiffService::new();
    let changes = diff_service
        .get_file_changes_between_refs(&repo_path.to_string_lossy(), &base, "HEAD")
        .unwrap();

    assert_eq!(changes.len(), 1);
    assert_eq!(changes[0].path, "src.rs");
    assert!(matches!(changes[0].status, FileChangeStatus::Added));
    assert_eq!(changes[0].additions, 2);
    assert_eq!(changes[0].deletions, 0);
}

#[tokio::test]
async fn test_detect_conflicts_clean_merge() {
    let (_temp_dir, repo_path) = create_git_repo();
    let repo_path_str = repo_path.to_string_lossy().to_string();

    // Create a branch with non-conflicting changes
    create_branch_with_change(&repo_path, "feature-a", "file_a.txt", "Content A\n");

    let diff_service = DiffService::new();
    let result = diff_service
        .detect_conflicts(&repo_path_str, "feature-a", "master")
        .await;

    // Should succeed with no conflicts
    assert!(result.is_ok());
    let conflicts = result.unwrap();
    assert!(
        conflicts.is_empty(),
        "Expected no conflicts, got: {:?}",
        conflicts
    );
}

#[test]
fn test_is_merge_in_progress_no_merge() {
    let (_temp_dir, repo_path) = create_git_repo();

    // No merge in progress initially
    assert!(!DiffService::is_merge_in_progress(&repo_path));
}

#[test]
fn test_get_conflict_files_empty() {
    let (_temp_dir, repo_path) = create_git_repo();

    // No conflicts initially
    let result = DiffService::get_conflict_files(&repo_path);
    assert!(result.is_ok());
    let files = result.unwrap();
    assert!(
        files.is_empty(),
        "Expected no conflict files, got: {:?}",
        files
    );
}

#[test]
fn test_resolve_git_dir_regular_repo() {
    let (_temp_dir, repo_path) = create_git_repo();

    let git_dir = DiffService::resolve_git_dir(&repo_path);
    assert!(
        git_dir.ends_with(".git"),
        "Expected .git dir, got: {:?}",
        git_dir
    );
}

#[test]
fn test_is_git_238_or_newer() {
    // This test just verifies the function runs without error
    // The actual result depends on the installed Git version
    let _result = DiffService::is_git_238_or_newer();
    // Should not panic
}
