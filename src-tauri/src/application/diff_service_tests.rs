use super::*;
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use tempfile::TempDir;

#[test]
fn file_diff_paging_flattens_hunks_and_enforces_page_bounds() {
    let diff = FileDiff {
        file_path: "src/lib.rs".to_string(),
        language: "rust".to_string(),
        hunks: vec![DiffHunk {
            old_start: 1,
            old_lines: 1,
            new_start: 1,
            new_lines: 2,
            header: "@@ -1 +1,2 @@".to_string(),
            lines: vec![
                DiffLine {
                    kind: DiffLineKind::Deletion,
                    content: "old".to_string(),
                    old_line_num: Some(1),
                    new_line_num: None,
                },
                DiffLine {
                    kind: DiffLineKind::Addition,
                    content: "new".to_string(),
                    old_line_num: None,
                    new_line_num: Some(1),
                },
            ],
        }],
        old_total_lines: 1,
        new_total_lines: 2,
        is_binary: false,
    };

    let first = DiffService::page_file_diff(diff.clone(), 0, 2).expect("first page");
    assert_eq!(first.rows.len(), 2);
    assert_eq!(first.next_offset, Some(2));
    let final_page = DiffService::page_file_diff(diff, 2, 2).expect("final page");
    assert_eq!(final_page.rows.len(), 1);
    assert_eq!(final_page.next_offset, None);
    assert!(DiffService::page_file_diff(final_page_to_diff(), 0, 0).is_err());
    assert!(DiffService::page_file_diff(final_page_to_diff(), 0, MAX_DIFF_PAGE_LIMIT + 1).is_err());
}

fn final_page_to_diff() -> FileDiff {
    FileDiff {
        file_path: "empty.txt".to_string(),
        language: "plaintext".to_string(),
        hunks: Vec::new(),
        old_total_lines: 0,
        new_total_lines: 0,
        is_binary: false,
    }
}

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

#[test]
fn numstat_parser_handles_binary_files_renames_and_invalid_lines() {
    let counts = numstat_map_from_stdout(
        "-\t-\tassets/logo.png\n12\t3\told/path.rs\tnew/path.rs\ninvalid\n",
    );

    assert_eq!(counts.get("assets/logo.png"), Some(&(0, 0)));
    assert_eq!(counts.get("new/path.rs"), Some(&(12, 3)));
    assert!(!counts.contains_key("old/path.rs"));
    assert_eq!(counts.len(), 2);
}

#[test]
fn file_changes_parse_deleted_renamed_and_skip_empty_paths() {
    let mut counts = HashMap::new();
    counts.insert("old.rs".to_string(), (0, 7));
    counts.insert("new/name.rs".to_string(), (4, 1));

    let changes = file_changes_from_name_status(
        "D\told.rs\nR100\told/name.rs\tnew/name.rs\nA\t   \n",
        &counts,
    );

    assert_eq!(changes.len(), 2);
    let renamed = changes
        .iter()
        .find(|change| change.path == "new/name.rs")
        .expect("renamed destination should be tracked");
    assert!(matches!(renamed.status, FileChangeStatus::Modified));
    assert_eq!(renamed.additions, 4);
    assert_eq!(renamed.deletions, 1);

    let deleted = changes
        .iter()
        .find(|change| change.path == "old.rs")
        .expect("deleted file should be tracked");
    assert!(matches!(deleted.status, FileChangeStatus::Deleted));
    assert_eq!(deleted.additions, 0);
    assert_eq!(deleted.deletions, 7);
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
fn worktree_file_changes_include_untracked_files() {
    let (_temp_dir, repo_path) = create_git_repo();
    let base = git_stdout(&repo_path, &["rev-parse", "HEAD"]);
    fs::write(repo_path.join("notes.md"), "one\ntwo\n").expect("Failed to write untracked file");

    let diff_service = DiffService::new();
    let changes = diff_service
        .get_worktree_file_changes_from_ref(&repo_path.to_string_lossy(), &base)
        .unwrap();

    assert_eq!(changes.len(), 1);
    assert_eq!(changes[0].path, "notes.md");
    assert!(matches!(changes[0].status, FileChangeStatus::Added));
    assert_eq!(changes[0].additions, 2);
    assert_eq!(changes[0].deletions, 0);
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

#[test]
fn test_get_file_diff_for_worktree_uses_current_disk_content() {
    let (_temp_dir, repo_path) = create_git_repo();
    let base = git_stdout(&repo_path, &["rev-parse", "HEAD"]);
    fs::write(repo_path.join("README.md"), "# Test Repo\n\nUncommitted\n")
        .expect("Failed to update README");

    let diff_service = DiffService::new();
    let diff = diff_service
        .get_file_diff("README.md", &repo_path.to_string_lossy(), &base)
        .unwrap();

    assert_eq!(diff.file_path, "README.md");
    assert_eq!(diff.language, "markdown");
    // Hunk-based: the uncommitted addition appears as a hunk addition line
    assert!(
        !diff.hunks.is_empty(),
        "Diff should have at least one hunk for the uncommitted change"
    );
    assert!(
        diff.hunks
            .iter()
            .flat_map(|h| h.lines.iter())
            .any(|l| l.content.contains("Uncommitted")),
        "Diff hunks should contain the uncommitted addition"
    );
    // old_total_lines = HEAD (1 line), new_total_lines = disk (3 lines)
    assert_eq!(diff.old_total_lines, 1, "HEAD has 1 line");
    assert_eq!(diff.new_total_lines, 3, "Disk has 3 lines after edit");
}

#[test]
fn worktree_file_diff_from_ref_reads_tracked_disk_changes() {
    let (_temp_dir, repo_path) = create_git_repo();
    let base = git_stdout(&repo_path, &["rev-parse", "HEAD"]);
    fs::write(repo_path.join("README.md"), "# Test Repo\n\nChanged\n")
        .expect("Failed to update README");

    let diff_service = DiffService::new();
    let diff = diff_service
        .get_worktree_file_diff_from_ref("README.md", &repo_path.to_string_lossy(), &base)
        .expect("worktree diff should load");

    assert_eq!(diff.file_path, "README.md");
    assert_eq!(diff.language, "markdown");
    assert!(!diff.is_binary);
    assert_eq!(diff.old_total_lines, 1);
    assert_eq!(diff.new_total_lines, 3);
    assert!(diff
        .hunks
        .iter()
        .flat_map(|hunk| hunk.lines.iter())
        .any(|line| line.content.contains("Changed")));
}

#[test]
fn worktree_file_diff_from_ref_falls_back_to_untracked_file_diff() {
    let (_temp_dir, repo_path) = create_git_repo();
    let base = git_stdout(&repo_path, &["rev-parse", "HEAD"]);
    fs::write(repo_path.join("notes.md"), "one\ntwo\n").expect("Failed to write notes");

    let diff_service = DiffService::new();
    let diff = diff_service
        .get_worktree_file_diff_from_ref("notes.md", &repo_path.to_string_lossy(), &base)
        .expect("untracked file diff should load");

    assert_eq!(diff.file_path, "notes.md");
    assert_eq!(diff.language, "markdown");
    assert!(!diff.is_binary);
    assert_eq!(diff.old_total_lines, 0);
    assert_eq!(diff.new_total_lines, 2);
    assert!(diff
        .hunks
        .iter()
        .flat_map(|hunk| hunk.lines.iter())
        .any(|line| line.content == "two"));
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
fn index_conflict_diff_reads_unmerged_git_stages() {
    let (_tmp, repo) = create_staged_unstaged_repo();
    let repo_str = repo.to_string_lossy().to_string();

    fs::write(repo.join("base.txt"), "base\nours\n").unwrap();
    git_cmd(&repo, &["add", "base.txt"]);
    git_cmd(&repo, &["commit", "-m", "Update ours"]);

    git_cmd(&repo, &["checkout", "-b", "incoming", "HEAD~1"]);
    fs::write(repo.join("base.txt"), "base\ntheirs\n").unwrap();
    git_cmd(&repo, &["add", "base.txt"]);
    git_cmd(&repo, &["commit", "-m", "Update theirs"]);

    git_cmd(&repo, &["checkout", "main"]);
    let output = std::process::Command::new("git")
        .args(["merge", "incoming"])
        .current_dir(&repo)
        .output()
        .expect("git merge should run");
    assert!(
        !output.status.success(),
        "merge should leave base.txt conflicted"
    );

    let diff = DiffService::new()
        .get_index_conflict_diff("base.txt", &repo_str)
        .expect("conflict diff should read index stages");

    assert_eq!(diff.file_path, "base.txt");
    assert_eq!(diff.base_content, "base\n");
    assert_eq!(diff.ours_content, "base\nours\n");
    assert_eq!(diff.theirs_content, "base\ntheirs\n");
    assert!(diff.merged_with_markers.contains("<<<<<<<"));
    assert!(diff.merged_with_markers.contains("ours"));
    assert!(diff.merged_with_markers.contains("theirs"));
    assert_eq!(diff.language, "plaintext");
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

// =========================================================================
// Staged / Unstaged Change Tests (Extension A)
// =========================================================================

/// Helper: run git commands in a repo and assert success.
fn git_cmd(repo_path: &PathBuf, args: &[&str]) {
    let output = std::process::Command::new("git")
        .args(args)
        .current_dir(repo_path)
        .output()
        .expect("git command should run");
    assert!(
        output.status.success(),
        "git {:?} failed: {}",
        args,
        String::from_utf8_lossy(&output.stderr)
    );
}

/// Creates a git repo with a committed base file and returns (temp_dir, repo_path).
fn create_staged_unstaged_repo() -> (TempDir, PathBuf) {
    let temp_dir = TempDir::new().expect("temp dir");
    let repo = temp_dir.path().to_path_buf();
    git_cmd(&repo, &["init", "-b", "main"]);
    git_cmd(&repo, &["config", "user.email", "test@example.com"]);
    git_cmd(&repo, &["config", "user.name", "Test"]);
    fs::write(repo.join("base.txt"), "base\n").unwrap();
    git_cmd(&repo, &["add", "."]);
    git_cmd(&repo, &["commit", "-m", "Initial"]);
    (temp_dir, repo)
}

#[test]
fn staged_file_changes_returns_only_staged_files() {
    let (_tmp, repo) = create_staged_unstaged_repo();
    let repo_str = repo.to_string_lossy().to_string();

    // Stage a new file
    fs::write(repo.join("staged.txt"), "staged content\n").unwrap();
    git_cmd(&repo, &["add", "staged.txt"]);

    // Also write an unstaged file (NOT added to index)
    fs::write(repo.join("unstaged.txt"), "unstaged content\n").unwrap();

    let svc = DiffService::new();
    let changes = svc.get_staged_file_changes(&repo_str).unwrap();

    assert_eq!(changes.len(), 1, "Only staged file should be listed");
    assert_eq!(changes[0].path, "staged.txt");
    assert!(matches!(changes[0].status, FileChangeStatus::Added));
    assert_eq!(changes[0].additions, 1);
}

#[test]
fn unstaged_file_changes_returns_only_unstaged_files() {
    let (_tmp, repo) = create_staged_unstaged_repo();
    let repo_str = repo.to_string_lossy().to_string();

    // Stage a file but don't include it in working-tree view
    fs::write(repo.join("staged_only.txt"), "staged\n").unwrap();
    git_cmd(&repo, &["add", "staged_only.txt"]);

    // Modify the committed file WITHOUT staging
    fs::write(repo.join("base.txt"), "base\nmodified\n").unwrap();

    let svc = DiffService::new();
    let changes = svc.get_unstaged_file_changes(&repo_str).unwrap();

    // Only base.txt (unstaged modification) should appear
    assert!(
        changes.iter().any(|c| c.path == "base.txt"),
        "Unstaged modification should be listed"
    );
    assert!(
        !changes.iter().any(|c| c.path == "staged_only.txt"),
        "Staged-only file should not appear in unstaged changes"
    );
}

#[test]
fn unstaged_file_changes_include_untracked_files() {
    let (_tmp, repo) = create_staged_unstaged_repo();
    let repo_str = repo.to_string_lossy().to_string();

    fs::write(repo.join("untracked.txt"), "first\nsecond\n").unwrap();

    let svc = DiffService::new();
    let changes = svc.get_unstaged_file_changes(&repo_str).unwrap();

    assert_eq!(changes.len(), 1);
    assert_eq!(changes[0].path, "untracked.txt");
    assert!(matches!(changes[0].status, FileChangeStatus::Added));
    assert_eq!(changes[0].additions, 2);
    assert_eq!(changes[0].deletions, 0);
}

#[test]
fn unstaged_file_changes_exclude_ignored_untracked_files() {
    let (_tmp, repo) = create_staged_unstaged_repo();
    let repo_str = repo.to_string_lossy().to_string();

    fs::write(repo.join(".gitignore"), "*.log\n").unwrap();
    git_cmd(&repo, &["add", ".gitignore"]);
    git_cmd(&repo, &["commit", "-m", "Ignore logs"]);
    fs::write(repo.join("debug.log"), "ignored\n").unwrap();

    let svc = DiffService::new();
    let changes = svc.get_unstaged_file_changes(&repo_str).unwrap();

    assert!(
        changes.is_empty(),
        "Ignored untracked files should not appear in unstaged changes"
    );
}

#[test]
fn staged_file_diff_shows_head_vs_index_content() {
    let (_tmp, repo) = create_staged_unstaged_repo();
    let repo_str = repo.to_string_lossy().to_string();

    // Stage a modification to base.txt
    fs::write(repo.join("base.txt"), "base\nadded line\n").unwrap();
    git_cmd(&repo, &["add", "base.txt"]);

    // Write a further unstaged change (should NOT appear in staged diff)
    fs::write(
        repo.join("base.txt"),
        "base\nadded line\nfurther unstaged\n",
    )
    .unwrap();

    let svc = DiffService::new();
    let diff = svc.get_staged_file_diff("base.txt", &repo_str).unwrap();

    assert_eq!(diff.file_path, "base.txt");
    assert_eq!(diff.language, "plaintext");
    // Hunk-based: staged diff HEAD→index; "added line" appears as an addition
    assert!(
        diff.hunks
            .iter()
            .flat_map(|h| h.lines.iter())
            .any(|l| l.content.contains("added line")),
        "Staged diff hunks should contain the staged addition"
    );
    // The further unstaged change must NOT appear in the staged diff
    assert!(
        !diff
            .hunks
            .iter()
            .flat_map(|h| h.lines.iter())
            .any(|l| l.content.contains("further unstaged")),
        "Staged diff should not include disk-only change"
    );
    assert_eq!(diff.old_total_lines, 1, "HEAD has 1 line");
    assert_eq!(diff.new_total_lines, 2, "Index has 2 lines after staging");
}

#[test]
fn unstaged_file_diff_shows_index_vs_disk_content() {
    let (_tmp, repo) = create_staged_unstaged_repo();
    let repo_str = repo.to_string_lossy().to_string();

    // Stage a modification to base.txt first
    fs::write(repo.join("base.txt"), "base\nstaged line\n").unwrap();
    git_cmd(&repo, &["add", "base.txt"]);

    // Then make a further disk change (unstaged)
    fs::write(repo.join("base.txt"), "base\nstaged line\ndisk change\n").unwrap();

    let svc = DiffService::new();
    let diff = svc.get_unstaged_file_diff("base.txt", &repo_str).unwrap();

    assert_eq!(diff.file_path, "base.txt");
    // Hunk-based: unstaged diff index→disk; "disk change" appears as an addition
    assert!(
        diff.hunks
            .iter()
            .flat_map(|h| h.lines.iter())
            .any(|l| l.content.contains("disk change")),
        "Unstaged diff hunks should contain the disk-only addition"
    );
    assert_eq!(diff.old_total_lines, 2, "Index has 2 lines");
    assert_eq!(diff.new_total_lines, 3, "Disk has 3 lines");
}

#[test]
fn unstaged_file_diff_renders_untracked_file_as_added() {
    let (_tmp, repo) = create_staged_unstaged_repo();
    let repo_str = repo.to_string_lossy().to_string();

    fs::write(repo.join("untracked.md"), "alpha\nbeta\n").unwrap();

    let svc = DiffService::new();
    let diff = svc
        .get_unstaged_file_diff("untracked.md", &repo_str)
        .unwrap();

    assert_eq!(diff.file_path, "untracked.md");
    assert_eq!(diff.language, "markdown");
    assert_eq!(diff.old_total_lines, 0);
    assert_eq!(diff.new_total_lines, 2);
    assert!(
        diff.hunks
            .iter()
            .flat_map(|h| h.lines.iter())
            .any(|line| line.kind == DiffLineKind::Addition && line.content == "alpha"),
        "Untracked file diff should render disk content as additions"
    );
}

#[test]
fn unstaged_file_diff_renders_empty_untracked_file_without_hunks() {
    let (_tmp, repo) = create_staged_unstaged_repo();
    let repo_str = repo.to_string_lossy().to_string();

    fs::write(repo.join("empty.txt"), "").unwrap();

    let svc = DiffService::new();
    let diff = svc.get_unstaged_file_diff("empty.txt", &repo_str).unwrap();

    assert_eq!(diff.file_path, "empty.txt");
    assert!(!diff.is_binary);
    assert_eq!(diff.old_total_lines, 0);
    assert_eq!(diff.new_total_lines, 0);
    assert!(diff.hunks.is_empty());
}

#[test]
fn unstaged_file_diff_treats_invalid_utf8_untracked_file_as_binary() {
    let (_tmp, repo) = create_staged_unstaged_repo();
    let repo_str = repo.to_string_lossy().to_string();

    fs::write(repo.join("binary.bin"), [0xff, 0xfe, 0xfd]).unwrap();

    let svc = DiffService::new();
    let changes = svc.get_unstaged_file_changes(&repo_str).unwrap();
    let binary_change = changes
        .iter()
        .find(|change| change.path == "binary.bin")
        .unwrap();
    assert_eq!(binary_change.additions, 0);

    let diff = svc.get_unstaged_file_diff("binary.bin", &repo_str).unwrap();
    assert_eq!(diff.file_path, "binary.bin");
    assert!(diff.is_binary);
    assert!(diff.hunks.is_empty());
    assert_eq!(diff.old_total_lines, 0);
    assert_eq!(diff.new_total_lines, 0);
}

#[cfg(unix)]
#[test]
fn unstaged_file_diff_treats_untracked_symlink_as_binary_without_reading_target() {
    let (_tmp, repo) = create_staged_unstaged_repo();
    let repo_str = repo.to_string_lossy().to_string();

    std::os::unix::fs::symlink("README.md", repo.join("linked.md")).unwrap();

    let svc = DiffService::new();
    let changes = svc.get_unstaged_file_changes(&repo_str).unwrap();
    let linked_change = changes
        .iter()
        .find(|change| change.path == "linked.md")
        .unwrap();
    assert_eq!(linked_change.additions, 0);

    let diff = svc.get_unstaged_file_diff("linked.md", &repo_str).unwrap();
    assert_eq!(diff.file_path, "linked.md");
    assert!(diff.is_binary);
    assert!(diff.hunks.is_empty());
    assert_eq!(diff.old_total_lines, 0);
    assert_eq!(diff.new_total_lines, 0);
}

#[test]
fn staged_file_changes_empty_when_nothing_staged() {
    let (_tmp, repo) = create_staged_unstaged_repo();
    let repo_str = repo.to_string_lossy().to_string();

    // Only unstaged change
    fs::write(repo.join("base.txt"), "base\nmore\n").unwrap();

    let svc = DiffService::new();
    let changes = svc.get_staged_file_changes(&repo_str).unwrap();
    assert!(
        changes.is_empty(),
        "No staged changes, result should be empty"
    );
}

#[test]
fn unstaged_file_changes_empty_when_working_tree_clean() {
    let (_tmp, repo) = create_staged_unstaged_repo();
    let repo_str = repo.to_string_lossy().to_string();

    // Stage a file (no unstaged changes on committed files)
    fs::write(repo.join("new.txt"), "new\n").unwrap();
    git_cmd(&repo, &["add", "new.txt"]);

    let svc = DiffService::new();
    let changes = svc.get_unstaged_file_changes(&repo_str).unwrap();
    assert!(
        changes.is_empty(),
        "No unstaged changes on tracked files, result should be empty"
    );
}

// =============================================================================
// parse_unified_diff unit tests
// =============================================================================

#[test]
fn parse_unified_diff_empty_input_returns_no_hunks() {
    let hunks = parse_unified_diff("");
    assert!(hunks.is_empty());
}

#[test]
fn parse_unified_diff_unchanged_file_returns_no_hunks() {
    // git diff on unchanged file outputs nothing
    let raw = "diff --git a/foo.rs b/foo.rs\n";
    let hunks = parse_unified_diff(raw);
    assert!(hunks.is_empty());
}

#[test]
fn parse_unified_diff_single_hunk_mixed_lines() {
    let raw = "\
diff --git a/src/lib.rs b/src/lib.rs
index abc..def 100644
--- a/src/lib.rs
+++ b/src/lib.rs
@@ -1,4 +1,5 @@
 fn foo() {
-    let x = 1;
+    let x = 2;
+    let y = 3;
 }
";
    let hunks = parse_unified_diff(raw);
    assert_eq!(hunks.len(), 1);
    let hunk = &hunks[0];
    assert_eq!(hunk.old_start, 1);
    assert_eq!(hunk.old_lines, 4);
    assert_eq!(hunk.new_start, 1);
    assert_eq!(hunk.new_lines, 5);

    // Context: "fn foo() {"
    assert_eq!(hunk.lines[0].kind, DiffLineKind::Context);
    assert_eq!(hunk.lines[0].old_line_num, Some(1));
    assert_eq!(hunk.lines[0].new_line_num, Some(1));

    // Deletion: "    let x = 1;"
    assert_eq!(hunk.lines[1].kind, DiffLineKind::Deletion);
    assert_eq!(hunk.lines[1].content, "    let x = 1;");
    assert_eq!(hunk.lines[1].old_line_num, Some(2));
    assert_eq!(hunk.lines[1].new_line_num, None);

    // Addition: "    let x = 2;"
    assert_eq!(hunk.lines[2].kind, DiffLineKind::Addition);
    assert_eq!(hunk.lines[2].content, "    let x = 2;");
    assert_eq!(hunk.lines[2].old_line_num, None);
    assert_eq!(hunk.lines[2].new_line_num, Some(2));

    // Addition: "    let y = 3;"
    assert_eq!(hunk.lines[3].kind, DiffLineKind::Addition);
    assert_eq!(hunk.lines[3].new_line_num, Some(3));

    // Context: "}"
    assert_eq!(hunk.lines[4].kind, DiffLineKind::Context);
    assert_eq!(hunk.lines[4].old_line_num, Some(3));
    assert_eq!(hunk.lines[4].new_line_num, Some(4));
}

#[test]
fn parse_unified_diff_multi_hunk() {
    let raw = "\
@@ -1,3 +1,3 @@
 line1
-old2
+new2
 line3
@@ -10,3 +10,3 @@
 line10
-old11
+new11
 line12
";
    let hunks = parse_unified_diff(raw);
    assert_eq!(hunks.len(), 2);
    assert_eq!(hunks[0].old_start, 1);
    assert_eq!(hunks[1].old_start, 10);
    assert_eq!(hunks[1].lines[1].old_line_num, Some(11));
    assert_eq!(hunks[1].lines[2].new_line_num, Some(11));
}

#[test]
fn parse_unified_diff_new_file() {
    // New file: @@ -0,0 +1,2 @@
    let raw = "\
diff --git a/new.rs b/new.rs
new file mode 100644
--- /dev/null
+++ b/new.rs
@@ -0,0 +1,2 @@
+fn new() {}
+// end
";
    let hunks = parse_unified_diff(raw);
    assert_eq!(hunks.len(), 1);
    assert_eq!(hunks[0].old_start, 0);
    assert_eq!(hunks[0].old_lines, 0);
    assert_eq!(hunks[0].new_start, 1);
    let first = &hunks[0].lines[0];
    assert_eq!(first.kind, DiffLineKind::Addition);
    assert_eq!(first.old_line_num, None);
    assert_eq!(first.new_line_num, Some(1));
}

#[test]
fn parse_unified_diff_deleted_file() {
    let raw = "\
diff --git a/gone.rs b/gone.rs
deleted file mode 100644
--- a/gone.rs
+++ /dev/null
@@ -1,2 +0,0 @@
-fn old() {}
-// end
";
    let hunks = parse_unified_diff(raw);
    assert_eq!(hunks.len(), 1);
    assert_eq!(hunks[0].new_start, 0);
    assert_eq!(hunks[0].new_lines, 0);
    let first = &hunks[0].lines[0];
    assert_eq!(first.kind, DiffLineKind::Deletion);
    assert_eq!(first.new_line_num, None);
    assert_eq!(first.old_line_num, Some(1));
}

#[test]
fn parse_unified_diff_no_newline_marker_skipped() {
    let raw = "\
@@ -1,1 +1,1 @@
-old
\\ No newline at end of file
+new
\\ No newline at end of file
";
    let hunks = parse_unified_diff(raw);
    assert_eq!(hunks.len(), 1);
    // Only Deletion and Addition — the backslash lines are skipped
    assert_eq!(hunks[0].lines.len(), 2);
    assert_eq!(hunks[0].lines[0].kind, DiffLineKind::Deletion);
    assert_eq!(hunks[0].lines[1].kind, DiffLineKind::Addition);
}

#[test]
fn parse_unified_diff_hunk_with_optional_trailing_text() {
    // @@ -10,7 +10,7 @@ fn my_function() {
    let raw = "@@ -10,7 +10,7 @@ fn my_function() {\n-old\n+new\n";
    let hunks = parse_unified_diff(raw);
    assert_eq!(hunks.len(), 1);
    assert!(hunks[0].header.contains("fn my_function()"));
    assert_eq!(hunks[0].old_start, 10);
}

#[test]
fn parse_unified_diff_binary_file_returns_empty() {
    // Binary diff output — caller detects "Binary files" and skips parsing;
    // parse_unified_diff itself receives empty string in that branch.
    let raw = "Binary files a/img.png and b/img.png differ\n";
    // Demonstrate: if someone calls it directly, no crash, returns empty
    let hunks = parse_unified_diff(raw);
    assert!(hunks.is_empty());
}

#[test]
fn file_changes_from_unified_diff_tracks_counts_and_statuses() {
    let raw = "\
diff --git a/src/lib.rs b/src/lib.rs
index abc..def 100644
--- a/src/lib.rs
+++ b/src/lib.rs
@@ -1,3 +1,4 @@
 pub fn answer() -> u8 {
-    41
+    42
 }
+// done
diff --git a/old.txt b/old.txt
deleted file mode 100644
--- a/old.txt
+++ /dev/null
@@ -1,1 +0,0 @@
-old
";

    let changes = DiffService::new().get_file_changes_from_unified_diff(raw);

    assert_eq!(changes.len(), 2);
    let deleted = changes
        .iter()
        .find(|change| change.path == "old.txt")
        .expect("deleted file should be listed");
    assert!(matches!(deleted.status, FileChangeStatus::Deleted));
    assert_eq!(deleted.additions, 0);
    assert_eq!(deleted.deletions, 1);

    let modified = changes
        .iter()
        .find(|change| change.path == "src/lib.rs")
        .expect("modified file should be listed");
    assert!(matches!(modified.status, FileChangeStatus::Modified));
    assert_eq!(modified.additions, 2);
    assert_eq!(modified.deletions, 1);
}

#[test]
fn file_diff_from_unified_diff_returns_single_file_hunks() {
    let raw = "\
diff --git a/src/lib.rs b/src/lib.rs
index abc..def 100644
--- a/src/lib.rs
+++ b/src/lib.rs
@@ -1,3 +1,4 @@
 pub fn answer() -> u8 {
-    41
+    42
 }
+// done
";

    let diff = DiffService::new()
        .get_file_diff_from_unified_diff(raw, "src/lib.rs")
        .expect("patch-backed file diff should parse");

    assert_eq!(diff.file_path, "src/lib.rs");
    assert_eq!(diff.language, "rust");
    assert_eq!(diff.hunks.len(), 1);
    assert_eq!(diff.old_total_lines, 3);
    assert_eq!(diff.new_total_lines, 4);
    assert!(diff
        .hunks
        .iter()
        .flat_map(|hunk| hunk.lines.iter())
        .any(|line| line.content.contains("42")));
}

#[test]
fn file_diff_page_limits_rows_and_reports_next_offset() {
    let diff = FileDiff {
        file_path: "src/lib.rs".to_string(),
        language: "rust".to_string(),
        hunks: vec![
            DiffHunk {
                old_start: 1,
                old_lines: 2,
                new_start: 1,
                new_lines: 2,
                header: "@@ -1,2 +1,2 @@".to_string(),
                lines: vec![
                    DiffLine {
                        kind: DiffLineKind::Context,
                        content: "pub fn before() {}".to_string(),
                        old_line_num: Some(1),
                        new_line_num: Some(1),
                    },
                    DiffLine {
                        kind: DiffLineKind::Addition,
                        content: "pub fn after() {}".to_string(),
                        old_line_num: None,
                        new_line_num: Some(2),
                    },
                ],
            },
            DiffHunk {
                old_start: 20,
                old_lines: 2,
                new_start: 21,
                new_lines: 3,
                header: "@@ -20,2 +21,3 @@".to_string(),
                lines: vec![
                    DiffLine {
                        kind: DiffLineKind::Deletion,
                        content: "old();".to_string(),
                        old_line_num: Some(20),
                        new_line_num: None,
                    },
                    DiffLine {
                        kind: DiffLineKind::Addition,
                        content: "new();".to_string(),
                        old_line_num: None,
                        new_line_num: Some(21),
                    },
                    DiffLine {
                        kind: DiffLineKind::Context,
                        content: "done();".to_string(),
                        old_line_num: Some(21),
                        new_line_num: Some(22),
                    },
                ],
            },
        ],
        old_total_lines: 21,
        new_total_lines: 22,
        is_binary: false,
    };

    let page = DiffService::page_file_diff(diff.clone(), 0, 3)
        .expect("page should slice the flattened diff");

    assert_eq!(page.file_path, "src/lib.rs");
    assert_eq!(page.offset, 0);
    assert_eq!(page.limit, 3);
    assert_eq!(page.total_rows, 7);
    assert_eq!(page.next_offset, Some(3));
    assert_eq!(page.rows.len(), 3);
    assert!(matches!(
        page.rows[0],
        DiffPageRow::HunkHeader {
            old_start: 1,
            old_lines: 2,
            new_start: 1,
            new_lines: 2,
            ..
        }
    ));
    assert!(matches!(page.rows[1], DiffPageRow::Line { .. }));
    assert!(matches!(page.rows[2], DiffPageRow::Line { .. }));

    let tail =
        DiffService::page_file_diff(diff, 5, 3).expect("tail page should slice remaining rows");
    assert_eq!(tail.offset, 5);
    assert_eq!(tail.rows.len(), 2);
    assert_eq!(tail.next_offset, None);
}

#[test]
fn file_diff_page_rejects_zero_and_oversized_limits() {
    let diff = FileDiff {
        file_path: "src/lib.rs".to_string(),
        language: "rust".to_string(),
        hunks: Vec::new(),
        old_total_lines: 0,
        new_total_lines: 0,
        is_binary: false,
    };

    let zero = DiffService::page_file_diff(diff.clone(), 0, 0).unwrap_err();
    assert!(zero.to_string().contains("limit"));

    let oversized = DiffService::page_file_diff(diff, 0, MAX_DIFF_PAGE_LIMIT + 1).unwrap_err();
    assert!(oversized.to_string().contains("too large"));
}

// =============================================================================
// validate_diff_file_path unit tests
// =============================================================================

#[test]
fn validate_diff_file_path_rejects_absolute() {
    let err = validate_diff_file_path("/etc/passwd").unwrap_err();
    assert!(err.to_string().contains("relative"));
}

#[test]
fn validate_diff_file_path_rejects_parent_traversal() {
    let err = validate_diff_file_path("../secret").unwrap_err();
    assert!(err.to_string().contains("unsafe"));
}

#[test]
fn validate_diff_file_path_rejects_embedded_traversal() {
    let err = validate_diff_file_path("src/../../etc/passwd").unwrap_err();
    assert!(err.to_string().contains("unsafe"));
}

#[test]
fn validate_diff_file_path_accepts_normal_path() {
    validate_diff_file_path("src/lib.rs").unwrap();
    validate_diff_file_path("frontend/src/components/App.tsx").unwrap();
}

// =============================================================================
// get_file_content_range unit tests
// =============================================================================

#[test]
fn get_file_content_range_rejects_oversized_range() {
    let svc = DiffService::new();
    let err = svc
        .get_file_content_range(".", &DiffSide::New, "any.rs", &DiffRefKind::Head, 1, 5001)
        .unwrap_err();
    assert!(err.to_string().contains("too large") || err.to_string().contains("5000"));
}

#[test]
fn get_file_content_range_rejects_from_greater_than_to() {
    let svc = DiffService::new();
    let err = svc
        .get_file_content_range(".", &DiffSide::New, "any.rs", &DiffRefKind::Head, 10, 5)
        .unwrap_err();
    assert!(err.to_string().contains("from") || err.to_string().contains("to"));
}

#[test]
fn get_file_content_range_rejects_traversal_path() {
    let svc = DiffService::new();
    let err = svc
        .get_file_content_range(
            ".",
            &DiffSide::New,
            "../etc/passwd",
            &DiffRefKind::Head,
            1,
            10,
        )
        .unwrap_err();
    assert!(err.to_string().contains("unsafe") || err.to_string().contains("relative"));
}

#[test]
fn get_file_content_range_reports_missing_ref_content() {
    let (_tmp, repo) = create_staged_unstaged_repo();
    let repo_str = repo.to_string_lossy().to_string();
    let head = git_stdout(&repo, &["rev-parse", "HEAD"]);
    let svc = DiffService::new();

    let staged_err = svc
        .get_file_content_range(
            &repo_str,
            &DiffSide::New,
            "missing.txt",
            &DiffRefKind::Staged,
            1,
            1,
        )
        .unwrap_err();
    assert!(staged_err.to_string().contains("git index"));

    let head_err = svc
        .get_file_content_range(
            &repo_str,
            &DiffSide::New,
            "missing.txt",
            &DiffRefKind::Head,
            1,
            1,
        )
        .unwrap_err();
    assert!(head_err.to_string().contains("HEAD"));

    let commit_err = svc
        .get_file_content_range(
            &repo_str,
            &DiffSide::New,
            "missing.txt",
            &DiffRefKind::Commit { sha: head },
            1,
            1,
        )
        .unwrap_err();
    assert!(commit_err.to_string().contains("commit"));
}

#[test]
fn unstaged_file_diff_rejects_empty_file_path() {
    let (_tmp, repo) = create_staged_unstaged_repo();
    let repo_str = repo.to_string_lossy().to_string();
    let svc = DiffService::new();

    let err = svc.get_unstaged_file_diff("", &repo_str).unwrap_err();

    assert!(err.to_string().contains("empty"));
}

#[test]
fn externally_reachable_file_diff_sources_reject_unsafe_paths_before_git_reads() {
    let (_tmp, repo) = create_staged_unstaged_repo();
    let repo_str = repo.to_string_lossy().to_string();
    let head = git_stdout(&repo, &["rev-parse", "HEAD"]);
    let svc = DiffService::new();

    for unsafe_path in ["", "/etc/passwd", "../secret", "src/../../secret"] {
        assert!(svc.get_staged_file_diff(unsafe_path, &repo_str).is_err());
        assert!(svc.get_unstaged_file_diff(unsafe_path, &repo_str).is_err());
        assert!(svc
            .get_file_diff_between_refs(unsafe_path, &repo_str, &head, "HEAD")
            .is_err());
    }
}

#[test]
fn externally_reachable_file_diffs_ignore_failing_external_diff_driver() {
    let (_tmp, repo) = create_staged_unstaged_repo();
    let repo_str = repo.to_string_lossy().to_string();
    let initial_head = git_stdout(&repo, &["rev-parse", "HEAD"]);
    let svc = DiffService::new();

    fs::write(repo.join("base.txt"), "base\nstaged\n").unwrap();
    git_cmd(&repo, &["add", "base.txt"]);
    git_cmd(&repo, &["config", "diff.external", "false"]);
    let staged = svc
        .get_staged_file_diff("base.txt", &repo_str)
        .expect("staged diff should use the built-in diff engine");
    assert!(!staged.hunks.is_empty());

    fs::write(repo.join("base.txt"), "base\nstaged\nunstaged\n").unwrap();
    let unstaged = svc
        .get_unstaged_file_diff("base.txt", &repo_str)
        .expect("unstaged diff should use the built-in diff engine");
    assert!(!unstaged.hunks.is_empty());

    git_cmd(&repo, &["commit", "-m", "staged change"]);
    let committed = svc
        .get_file_diff_between_refs("base.txt", &repo_str, &initial_head, "HEAD")
        .expect("between-ref diff should use the built-in diff engine");
    assert!(!committed.hunks.is_empty());
}

#[test]
fn get_file_content_range_reads_working_tree_lines() {
    let (_tmp, repo) = create_staged_unstaged_repo();
    let repo_str = repo.to_string_lossy().to_string();

    // base.txt already exists with content "base\n" (1 line)
    let svc = DiffService::new();
    let lines = svc
        .get_file_content_range(
            &repo_str,
            &DiffSide::New,
            "base.txt",
            &DiffRefKind::Unstaged,
            1,
            1,
        )
        .unwrap();
    assert_eq!(lines.len(), 1);
    assert_eq!(lines[0].line_num, 1);
    assert_eq!(lines[0].content, "base");
}

#[test]
fn get_file_content_range_rejects_cumulative_base_and_head() {
    let svc = DiffService::new();
    let err_base = svc
        .get_file_content_range(
            ".",
            &DiffSide::New,
            "x.rs",
            &DiffRefKind::CumulativeBase,
            1,
            5,
        )
        .unwrap_err();
    assert!(
        err_base.to_string().contains("CumulativeBase")
            || err_base.to_string().contains("resolved")
    );

    let err_head = svc
        .get_file_content_range(
            ".",
            &DiffSide::New,
            "x.rs",
            &DiffRefKind::CumulativeHead,
            1,
            5,
        )
        .unwrap_err();
    assert!(
        err_head.to_string().contains("CumulativeHead")
            || err_head.to_string().contains("resolved")
    );
}

#[test]
fn get_file_content_range_rejects_from_zero() {
    // from must be >= 1 (1-indexed)
    let svc = DiffService::new();
    let err = svc
        .get_file_content_range(".", &DiffSide::New, "any.rs", &DiffRefKind::Head, 0, 5)
        .unwrap_err();
    assert!(err.to_string().contains("from") || err.to_string().contains("1-indexed"));
}

#[test]
fn get_file_content_range_reads_head_ref() {
    let (_tmp, repo) = create_staged_unstaged_repo();
    let repo_str = repo.to_string_lossy().to_string();
    // base.txt committed as "base\n" — HEAD has 1 line
    let svc = DiffService::new();
    let lines = svc
        .get_file_content_range(
            &repo_str,
            &DiffSide::Old,
            "base.txt",
            &DiffRefKind::Head,
            1,
            1,
        )
        .unwrap();
    assert_eq!(lines.len(), 1);
    assert_eq!(lines[0].line_num, 1);
    assert_eq!(lines[0].content, "base");
}

#[test]
fn get_file_content_range_reads_staged_ref() {
    let (_tmp, repo) = create_staged_unstaged_repo();
    let repo_str = repo.to_string_lossy().to_string();
    // Stage a new version of base.txt
    fs::write(repo.join("base.txt"), "base\nstaged\n").unwrap();
    git_cmd(&repo, &["add", "base.txt"]);
    // Staged ref reads from the index — should see "staged" line
    let svc = DiffService::new();
    let lines = svc
        .get_file_content_range(
            &repo_str,
            &DiffSide::New,
            "base.txt",
            &DiffRefKind::Staged,
            1,
            2,
        )
        .unwrap();
    assert_eq!(lines.len(), 2);
    assert_eq!(lines[0].content, "base");
    assert_eq!(lines[1].content, "staged");
}

#[test]
fn get_file_content_range_reads_unstaged_old_side_from_index() {
    let (_tmp, repo) = create_staged_unstaged_repo();
    let repo_str = repo.to_string_lossy().to_string();
    // Stage a modification so index differs from HEAD
    fs::write(repo.join("base.txt"), "base\nindex_line\n").unwrap();
    git_cmd(&repo, &["add", "base.txt"]);
    // Then make a further disk change (unstaged)
    fs::write(repo.join("base.txt"), "base\nindex_line\ndisk_line\n").unwrap();
    // Side::Old for Unstaged reads from the index — should see 2 lines
    let svc = DiffService::new();
    let lines = svc
        .get_file_content_range(
            &repo_str,
            &DiffSide::Old,
            "base.txt",
            &DiffRefKind::Unstaged,
            1,
            2,
        )
        .unwrap();
    assert_eq!(lines.len(), 2);
    assert_eq!(lines[1].content, "index_line");
}

#[test]
fn get_file_content_range_reads_commit_ref() {
    let (_tmp, repo) = create_staged_unstaged_repo();
    let repo_str = repo.to_string_lossy().to_string();
    // Capture the initial commit SHA (base.txt = "base\n")
    let sha = std::process::Command::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(&repo)
        .output()
        .unwrap();
    let sha = String::from_utf8_lossy(&sha.stdout).trim().to_string();

    // Make an additional commit so HEAD is now different
    fs::write(repo.join("base.txt"), "base\nafter\n").unwrap();
    git_cmd(&repo, &["add", "base.txt"]);
    git_cmd(&repo, &["commit", "-m", "second"]);

    let svc = DiffService::new();
    let lines = svc
        .get_file_content_range(
            &repo_str,
            &DiffSide::Old,
            "base.txt",
            &DiffRefKind::Commit { sha },
            1,
            1,
        )
        .unwrap();
    assert_eq!(lines.len(), 1);
    assert_eq!(lines[0].content, "base");
}

#[test]
fn get_file_content_range_returns_only_requested_window() {
    let (_tmp, repo) = create_staged_unstaged_repo();
    let repo_str = repo.to_string_lossy().to_string();
    // Write a 5-line file and commit it
    fs::write(repo.join("multi.txt"), "a\nb\nc\nd\ne\n").unwrap();
    git_cmd(&repo, &["add", "multi.txt"]);
    git_cmd(&repo, &["commit", "-m", "5 lines"]);

    let svc = DiffService::new();
    // Request lines 2–4 of the committed file
    let lines = svc
        .get_file_content_range(
            &repo_str,
            &DiffSide::Old,
            "multi.txt",
            &DiffRefKind::Head,
            2,
            4,
        )
        .unwrap();
    assert_eq!(lines.len(), 3);
    assert_eq!(lines[0].line_num, 2);
    assert_eq!(lines[0].content, "b");
    assert_eq!(lines[2].line_num, 4);
    assert_eq!(lines[2].content, "d");
}

// =============================================================================
// is_generated_by_heuristic unit tests
// =============================================================================

#[test]
fn heuristic_matches_source_maps() {
    assert!(is_generated_by_heuristic("bundle.js.map"));
    assert!(is_generated_by_heuristic("src/bundle.js.map"));
    assert!(is_generated_by_heuristic("foo.map"));
    // 'map' in a directory name or as a non-extension part is not a match
    assert!(!is_generated_by_heuristic("src/maputils.rs"));
    assert!(!is_generated_by_heuristic("sitemap.xml")); // .xml not .map
}

#[test]
fn heuristic_matches_minified_bundles() {
    assert!(is_generated_by_heuristic("app.min.js"));
    assert!(is_generated_by_heuristic("vendor.min.css"));
    assert!(!is_generated_by_heuristic("app.min.ts")); // not js or css
    assert!(!is_generated_by_heuristic("minimal.js")); // does not match .min.js pattern
    assert!(!is_generated_by_heuristic("app.js")); // plain JS, not minified
}

#[test]
fn heuristic_matches_all_known_lockfiles() {
    assert!(is_generated_by_heuristic("package-lock.json"));
    assert!(is_generated_by_heuristic("yarn.lock"));
    assert!(is_generated_by_heuristic("pnpm-lock.yaml"));
    assert!(is_generated_by_heuristic("Cargo.lock"));
    assert!(is_generated_by_heuristic("Gemfile.lock"));
    assert!(is_generated_by_heuristic("composer.lock"));
    assert!(is_generated_by_heuristic("poetry.lock"));
    assert!(is_generated_by_heuristic("uv.lock"));
    // Nested inside a workspace package
    assert!(is_generated_by_heuristic("packages/app/package-lock.json"));
    assert!(is_generated_by_heuristic("subdir/yarn.lock"));
}

#[test]
fn heuristic_matches_snapshots() {
    assert!(is_generated_by_heuristic("__snapshots__/App.test.snap"));
    assert!(is_generated_by_heuristic("tests/ui/button.snap"));
    assert!(is_generated_by_heuristic("foo.snap"));
    // 'snap' in a filename but not as the final extension is not a match
    assert!(!is_generated_by_heuristic("snapshot_utils.rs"));
    assert!(!is_generated_by_heuristic("snapshots/config.yaml")); // .yaml not .snap
}

#[test]
fn heuristic_matches_build_output_directories() {
    assert!(is_generated_by_heuristic("dist/app.js"));
    assert!(is_generated_by_heuristic("build/index.html"));
    assert!(is_generated_by_heuristic("out/bundle.css"));
    assert!(is_generated_by_heuristic("target/debug/ralphx"));
    assert!(is_generated_by_heuristic("target/release/libfoo.rlib"));
    // Must be the leading directory component, not an infix
    assert!(!is_generated_by_heuristic("src/dist.rs"));
    assert!(!is_generated_by_heuristic("distribution/app.js"));
    assert!(!is_generated_by_heuristic("src/build_utils.rs"));
}

#[test]
fn heuristic_returns_false_for_normal_source_files() {
    assert!(!is_generated_by_heuristic("src/app.ts"));
    assert!(!is_generated_by_heuristic("src/lib.rs"));
    assert!(!is_generated_by_heuristic("README.md"));
    assert!(!is_generated_by_heuristic("app.js"));
    assert!(!is_generated_by_heuristic("config.yaml"));
    assert!(!is_generated_by_heuristic("frontend/src/index.tsx"));
    assert!(!is_generated_by_heuristic("Cargo.toml"));
    assert!(!is_generated_by_heuristic("package.json")); // not a lockfile
}

// =============================================================================
// compute_generated_flags integration tests
// =============================================================================

#[test]
fn compute_generated_flags_empty_paths_returns_empty_map() {
    let svc = DiffService::new();
    let result = svc.compute_generated_flags(Path::new("."), &[]).unwrap();
    assert!(result.is_empty(), "Empty input must return an empty map");
}

#[test]
fn compute_generated_flags_applies_heuristic_when_no_gitattributes_opinion() {
    let (_tmp, repo) = create_staged_unstaged_repo();
    let svc = DiffService::new();
    let paths = ["bundle.js.map", "src/app.ts", "Cargo.lock"];
    let flags = svc.compute_generated_flags(&repo, &paths).unwrap();
    assert_eq!(
        flags.get("bundle.js.map"),
        Some(&true),
        ".map files should be flagged as generated"
    );
    assert_eq!(
        flags.get("src/app.ts"),
        Some(&false),
        "Normal source files should not be flagged"
    );
    assert_eq!(
        flags.get("Cargo.lock"),
        Some(&true),
        "Lockfiles should be flagged as generated"
    );
}

#[test]
fn compute_generated_flags_gitattributes_true_overrides_non_generated_heuristic() {
    // A .md file is not generated by heuristic, but if .gitattributes says
    // linguist-generated=true, the flag must be true.
    let (_tmp, repo) = create_staged_unstaged_repo();
    fs::write(repo.join(".gitattributes"), "*.md linguist-generated\n").unwrap();
    git_cmd(&repo, &["add", ".gitattributes"]);
    git_cmd(&repo, &["commit", "-m", "Mark .md as generated"]);

    let svc = DiffService::new();
    let flags = svc.compute_generated_flags(&repo, &["README.md"]).unwrap();
    assert_eq!(
        flags.get("README.md"),
        Some(&true),
        "linguist-generated attribute must override heuristic to true"
    );
}

#[test]
fn compute_generated_flags_gitattributes_false_overrides_generated_heuristic() {
    // A .map file is generated by heuristic, but if .gitattributes explicitly
    // unsets linguist-generated, the flag must be false.
    let (_tmp, repo) = create_staged_unstaged_repo();
    fs::write(repo.join(".gitattributes"), "*.map -linguist-generated\n").unwrap();
    git_cmd(&repo, &["add", ".gitattributes"]);
    git_cmd(&repo, &["commit", "-m", "Mark .map as not generated"]);

    let svc = DiffService::new();
    let flags = svc
        .compute_generated_flags(&repo, &["bundle.js.map"])
        .unwrap();
    assert_eq!(
        flags.get("bundle.js.map"),
        Some(&false),
        "-linguist-generated must override heuristic to false"
    );
}

#[test]
fn compute_generated_flags_falls_back_to_heuristic_when_git_unavailable() {
    // A non-git directory causes git check-attr to fail; must not error — must
    // fall back to heuristic for every requested path.
    let tmp = TempDir::new().unwrap();
    let svc = DiffService::new();
    let paths = ["bundle.js.map", "src/app.ts"];
    let flags = svc.compute_generated_flags(tmp.path(), &paths).unwrap();
    assert_eq!(
        flags.get("bundle.js.map"),
        Some(&true),
        "Fallback heuristic must still flag .map files"
    );
    assert_eq!(
        flags.get("src/app.ts"),
        Some(&false),
        "Fallback heuristic must leave normal source files unflagged"
    );
    // Every requested path must have an entry in the returned map
    assert_eq!(
        flags.len(),
        2,
        "All requested paths must appear in the result"
    );
}
