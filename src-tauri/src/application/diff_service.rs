//! Diff Service - Extracts file changes from agent activity and git
//!
//! Provides file change information for the DiffViewer by:
//! 1. Querying activity events to find Write/Edit tool calls
//! 2. Using git to get actual diff content
//! 3. Detecting merge conflicts (live and pre-merge preview)

use crate::application::git_service::checkout_free;
use crate::domain::entities::TaskId;
use crate::error::{AppError, AppResult};
use crate::infrastructure::tool_paths::resolve_git_cli_path;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::process::Command;

/// A file that was changed by the agent
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileChange {
    /// File path relative to project root
    pub path: String,
    /// Change status
    pub status: FileChangeStatus,
    /// Number of lines added
    pub additions: u32,
    /// Number of lines deleted
    pub deletions: u32,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FileChangeStatus {
    Added,
    Modified,
    Deleted,
}

/// Diff data for a single file
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileDiff {
    /// File path
    pub file_path: String,
    /// Content before changes (empty for new files)
    pub old_content: String,
    /// Content after changes (empty for deleted files)
    pub new_content: String,
    /// Programming language for syntax highlighting
    pub language: String,
}

/// 3-way diff data for a file with merge conflicts
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConflictDiff {
    /// File path relative to project root
    pub file_path: String,
    /// Content from merge-base (common ancestor)
    pub base_content: String,
    /// Content from target branch (base_branch, "ours" in merge)
    pub ours_content: String,
    /// Content from source branch (task_branch, "theirs" in merge)
    pub theirs_content: String,
    /// Current file content with conflict markers from failed merge
    pub merged_with_markers: String,
    /// Programming language for syntax highlighting
    pub language: String,
}

/// Service for extracting diff information
#[derive(Default)]
pub struct DiffService;

impl DiffService {
    pub fn new() -> Self {
        Self
    }

    /// Get all files changed by the agent for a task
    /// Compares against base_branch to show all changes since branching
    /// Uses git diff directly instead of activity events to capture all changes
    pub async fn get_task_file_changes(
        &self,
        _task_id: &TaskId,
        project_path: &str,
        base_branch: &str,
    ) -> AppResult<Vec<FileChange>> {
        self.get_file_changes_between_refs(project_path, base_branch, "HEAD")
    }

    /// Get all files changed in the worktree compared to a base ref.
    /// Includes committed, staged, and unstaged changes so review surfaces match what will publish.
    pub fn get_worktree_file_changes_from_ref(
        &self,
        project_path: &str,
        base_ref: &str,
    ) -> AppResult<Vec<FileChange>> {
        let output = Command::new("git")
            .args(["diff", "--name-status", base_ref])
            .current_dir(project_path)
            .output()
            .map_err(|e| AppError::GitOperation(format!("Failed to run git diff: {}", e)))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(AppError::GitOperation(format!(
                "git diff failed: {}",
                stderr
            )));
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        let mut changes = Vec::new();

        for line in stdout.lines() {
            let parts: Vec<&str> = line.split('\t').collect();
            if parts.len() >= 2 {
                let status_char = parts[0].chars().next().unwrap_or('M');
                let file_path = parts[1];

                let status = match status_char {
                    'A' => FileChangeStatus::Added,
                    'D' => FileChangeStatus::Deleted,
                    _ => FileChangeStatus::Modified,
                };

                let (additions, deletions) =
                    self.get_worktree_file_line_counts_from_ref(file_path, project_path, base_ref);

                changes.push(FileChange {
                    path: file_path.to_string(),
                    status,
                    additions,
                    deletions,
                });
            }
        }

        changes.sort_by(|a, b| a.path.cmp(&b.path));
        Ok(changes)
    }

    /// Get line additions/deletions for a file compared to base branch
    #[allow(dead_code)]
    fn get_file_line_counts(
        &self,
        file_path: &str,
        project_path: &str,
        base_branch: &str,
    ) -> (u32, u32) {
        let output = Command::new(resolve_git_cli_path())
            .args(["diff", "--numstat", base_branch, "--", file_path])
            .current_dir(project_path)
            .output();

        if let Ok(output) = output {
            let stdout = String::from_utf8_lossy(&output.stdout);
            if let Some(line) = stdout.lines().next() {
                let parts: Vec<&str> = line.split_whitespace().collect();
                if parts.len() >= 2 {
                    let additions: u32 = parts[0].parse().unwrap_or(0);
                    let deletions: u32 = parts[1].parse().unwrap_or(0);
                    return (additions, deletions);
                }
            }
        }

        (0, 0)
    }

    fn get_worktree_file_line_counts_from_ref(
        &self,
        file_path: &str,
        project_path: &str,
        base_ref: &str,
    ) -> (u32, u32) {
        let output = Command::new("git")
            .args(["diff", "--numstat", base_ref, "--", file_path])
            .current_dir(project_path)
            .output();

        if let Ok(output) = output {
            let stdout = String::from_utf8_lossy(&output.stdout);
            if let Some(line) = stdout.lines().next() {
                let parts: Vec<&str> = line.split_whitespace().collect();
                if parts.len() >= 2 {
                    let additions: u32 = parts[0].parse().unwrap_or(0);
                    let deletions: u32 = parts[1].parse().unwrap_or(0);
                    return (additions, deletions);
                }
            }
        }

        (0, 0)
    }

    /// Get the diff content for a specific file
    /// Shows old content from base_branch for accurate comparison
    pub fn get_file_diff(
        &self,
        file_path: &str,
        project_path: &str,
        base_branch: &str,
    ) -> AppResult<FileDiff> {
        let full_path = std::path::Path::new(project_path).join(file_path);
        let new_content = std::fs::read_to_string(&full_path).unwrap_or_default();
        self.get_file_diff_between_refs_with_new(file_path, project_path, base_branch, &new_content)
    }

    /// Get files changed in a specific commit
    pub fn get_commit_file_changes(
        &self,
        commit_sha: &str,
        project_path: &str,
    ) -> AppResult<Vec<FileChange>> {
        // Use git diff-tree to get files changed in this commit
        // Format: status\tpath (e.g., "A\tfile.rs" for added, "M\tfile.rs" for modified)
        let output = Command::new(resolve_git_cli_path())
            .args([
                "diff-tree",
                "--no-commit-id",
                "--name-status",
                "-r",
                commit_sha,
            ])
            .current_dir(project_path)
            .output()
            .map_err(|e| AppError::GitOperation(format!("Failed to run git diff-tree: {}", e)))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(AppError::GitOperation(format!(
                "git diff-tree failed: {}",
                stderr
            )));
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        let mut changes = Vec::new();

        for line in stdout.lines() {
            let parts: Vec<&str> = line.split('\t').collect();
            if parts.len() >= 2 {
                let status_char = parts[0].chars().next().unwrap_or('M');
                let file_path = parts[1];

                let status = match status_char {
                    'A' => FileChangeStatus::Added,
                    'D' => FileChangeStatus::Deleted,
                    _ => FileChangeStatus::Modified, // M, R, C, etc.
                };

                // Get line counts using git diff for this specific commit
                let (additions, deletions) =
                    self.get_commit_file_line_counts(commit_sha, file_path, project_path);

                changes.push(FileChange {
                    path: file_path.to_string(),
                    status,
                    additions,
                    deletions,
                });
            }
        }

        // Sort by path for consistent ordering
        changes.sort_by(|a, b| a.path.cmp(&b.path));

        Ok(changes)
    }

    /// Get line additions/deletions for a file in a specific commit
    fn get_commit_file_line_counts(
        &self,
        commit_sha: &str,
        file_path: &str,
        project_path: &str,
    ) -> (u32, u32) {
        // git diff commit^..commit --numstat -- file_path
        let output = Command::new(resolve_git_cli_path())
            .args([
                "diff",
                "--numstat",
                &format!("{}^", commit_sha),
                commit_sha,
                "--",
                file_path,
            ])
            .current_dir(project_path)
            .output();

        if let Ok(output) = output {
            let stdout = String::from_utf8_lossy(&output.stdout);
            if let Some(line) = stdout.lines().next() {
                let parts: Vec<&str> = line.split_whitespace().collect();
                if parts.len() >= 2 {
                    let additions: u32 = parts[0].parse().unwrap_or(0);
                    let deletions: u32 = parts[1].parse().unwrap_or(0);
                    return (additions, deletions);
                }
            }
        }

        (0, 0)
    }

    /// Get diff for a file in a specific commit (comparing to its parent)
    pub fn get_commit_file_diff(
        &self,
        commit_sha: &str,
        file_path: &str,
        project_path: &str,
    ) -> AppResult<FileDiff> {
        self.get_file_diff_between_refs(
            file_path,
            project_path,
            &format!("{}^", commit_sha),
            commit_sha,
        )
    }

    /// Get file changes between two refs (used for merged tasks and range diffs)
    pub fn get_file_changes_between_refs(
        &self,
        project_path: &str,
        from_ref: &str,
        to_ref: &str,
    ) -> AppResult<Vec<FileChange>> {
        let output = Command::new(resolve_git_cli_path())
            .args(["diff", "--name-status", from_ref, to_ref])
            .current_dir(project_path)
            .output()
            .map_err(|e| AppError::GitOperation(format!("Failed to run git diff: {}", e)))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(AppError::GitOperation(format!(
                "git diff failed: {}",
                stderr
            )));
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        let mut changes = Vec::new();

        for line in stdout.lines() {
            let parts: Vec<&str> = line.split('\t').collect();
            if parts.len() >= 2 {
                let status_char = parts[0].chars().next().unwrap_or('M');
                let file_path = parts[1];

                let status = match status_char {
                    'A' => FileChangeStatus::Added,
                    'D' => FileChangeStatus::Deleted,
                    _ => FileChangeStatus::Modified, // M, R, C, etc.
                };

                let (additions, deletions) = self.get_file_line_counts_between_refs(
                    file_path,
                    project_path,
                    from_ref,
                    to_ref,
                );

                changes.push(FileChange {
                    path: file_path.to_string(),
                    status,
                    additions,
                    deletions,
                });
            }
        }

        changes.sort_by(|a, b| a.path.cmp(&b.path));
        Ok(changes)
    }

    /// Get line additions/deletions for a file between two refs
    fn get_file_line_counts_between_refs(
        &self,
        file_path: &str,
        project_path: &str,
        from_ref: &str,
        to_ref: &str,
    ) -> (u32, u32) {
        let output = Command::new(resolve_git_cli_path())
            .args(["diff", "--numstat", from_ref, to_ref, "--", file_path])
            .current_dir(project_path)
            .output();

        if let Ok(output) = output {
            let stdout = String::from_utf8_lossy(&output.stdout);
            if let Some(line) = stdout.lines().next() {
                let parts: Vec<&str> = line.split_whitespace().collect();
                if parts.len() >= 2 {
                    let additions: u32 = parts[0].parse().unwrap_or(0);
                    let deletions: u32 = parts[1].parse().unwrap_or(0);
                    return (additions, deletions);
                }
            }
        }

        (0, 0)
    }

    /// Get diff for a file between two refs
    pub fn get_file_diff_between_refs(
        &self,
        file_path: &str,
        project_path: &str,
        from_ref: &str,
        to_ref: &str,
    ) -> AppResult<FileDiff> {
        let old_content = self
            .get_file_content_at_ref(project_path, from_ref, file_path)
            .unwrap_or_default();
        let new_content = self
            .get_file_content_at_ref(project_path, to_ref, file_path)
            .unwrap_or_default();

        let language = get_language_from_path(file_path);
        Ok(FileDiff {
            file_path: file_path.to_string(),
            old_content,
            new_content,
            language,
        })
    }

    /// Same as get_file_diff_between_refs, but with new content already read from disk.
    fn get_file_diff_between_refs_with_new(
        &self,
        file_path: &str,
        project_path: &str,
        from_ref: &str,
        new_content: &str,
    ) -> AppResult<FileDiff> {
        let old_content = self
            .get_file_content_at_ref(project_path, from_ref, file_path)
            .unwrap_or_default();
        let language = get_language_from_path(file_path);
        Ok(FileDiff {
            file_path: file_path.to_string(),
            old_content,
            new_content: new_content.to_string(),
            language,
        })
    }

    /// Determine if a commit has a second parent (true merge commit)
    pub fn is_merge_commit(&self, project_path: &str, commit_sha: &str) -> bool {
        let output = Command::new(resolve_git_cli_path())
            .args(["rev-parse", "--verify", &format!("{}^2", commit_sha)])
            .current_dir(project_path)
            .output();
        output.map(|o| o.status.success()).unwrap_or(false)
    }

    /// Compute base ref for a merged task range.
    /// True merge commits compare against their first parent. Non-merge recorded
    /// task merge SHAs are usually squash/rebase commits, so compare against the
    /// direct parent instead of a current-base merge-base that may include older
    /// plan/agent branch work.
    pub fn get_merged_base_ref(
        &self,
        project_path: &str,
        base_branch: &str,
        merge_commit_sha: &str,
    ) -> String {
        if self.is_merge_commit(project_path, merge_commit_sha) {
            return format!("{}^1", merge_commit_sha);
        }

        let parent_ref = format!("{}^", merge_commit_sha);
        let parent_output = Command::new(resolve_git_cli_path())
            .args(["rev-parse", "--verify", &parent_ref])
            .current_dir(project_path)
            .output();
        if parent_output
            .map(|output| output.status.success())
            .unwrap_or(false)
        {
            return parent_ref;
        }

        let output = Command::new(resolve_git_cli_path())
            .args(["merge-base", base_branch, merge_commit_sha])
            .current_dir(project_path)
            .output();

        if let Ok(output) = output {
            if output.status.success() {
                let base = String::from_utf8_lossy(&output.stdout).trim().to_string();
                if !base.is_empty() {
                    let resolved_merge = Command::new(resolve_git_cli_path())
                        .args(["rev-parse", merge_commit_sha])
                        .current_dir(project_path)
                        .output()
                        .ok()
                        .filter(|output| output.status.success())
                        .map(|output| String::from_utf8_lossy(&output.stdout).trim().to_string());
                    if resolved_merge.as_deref() == Some(base.as_str()) {
                        let parent_ref = format!("{}^", merge_commit_sha);
                        let parent_output = Command::new(resolve_git_cli_path())
                            .args(["rev-parse", "--verify", &parent_ref])
                            .current_dir(project_path)
                            .output();
                        if parent_output
                            .map(|output| output.status.success())
                            .unwrap_or(false)
                        {
                            return parent_ref;
                        }
                    }
                    return base;
                }
            }
        }

        base_branch.to_string()
    }

    /// Get file changes for a merged task using merge commit range.
    pub fn get_merged_task_file_changes(
        &self,
        project_path: &str,
        base_branch: &str,
        merge_commit_sha: &str,
    ) -> AppResult<Vec<FileChange>> {
        let from_ref = self.get_merged_base_ref(project_path, base_branch, merge_commit_sha);
        self.get_file_changes_between_refs(project_path, &from_ref, merge_commit_sha)
    }

    /// Get file diff for a merged task using merge commit range.
    pub fn get_merged_task_file_diff(
        &self,
        file_path: &str,
        project_path: &str,
        base_branch: &str,
        merge_commit_sha: &str,
    ) -> AppResult<FileDiff> {
        let from_ref = self.get_merged_base_ref(project_path, base_branch, merge_commit_sha);
        self.get_file_diff_between_refs(file_path, project_path, &from_ref, merge_commit_sha)
    }

    // =========================================================================
    // Conflict Detection (Phase - Live Merge Conflict Detection)
    // =========================================================================

    /// Detect merge conflicts for a task.
    ///
    /// Uses two strategies based on the current git state:
    /// 1. **Active merge (MERGE_HEAD exists)**: Uses `git diff --name-only --diff-filter=U`
    ///    to find files with conflict markers in the index.
    /// 2. **Pre-merge preview (no active merge)**: Uses `git merge-tree --write-tree`
    ///    to simulate the merge and detect conflicts before actually merging.
    ///
    /// # Arguments
    /// * `project_path` - Path to the git repository or worktree
    /// * `task_branch` - The task branch to merge (source)
    /// * `base_branch` - The target branch to merge into (target)
    ///
    /// # Returns
    /// * `Vec<String>` - List of file paths with conflicts
    ///
    /// # Git Version Requirements
    /// * `merge-tree --write-tree` requires Git 2.38+
    /// * Falls back to `get_conflict_files` only if Git < 2.38
    pub async fn detect_conflicts(
        &self,
        project_path: &str,
        task_branch: &str,
        base_branch: &str,
    ) -> AppResult<Vec<String>> {
        let repo = Path::new(project_path);

        // Check for active merge first (MERGE_HEAD exists)
        if Self::is_merge_in_progress(repo) {
            // Active merge: use git diff to find conflict files
            return Self::get_conflict_files(repo).map(|paths| {
                paths
                    .into_iter()
                    .map(|p| p.to_string_lossy().to_string())
                    .collect()
            });
        }

        // Pre-merge preview: use merge-tree --write-tree if Git 2.38+
        if Self::is_git_238_or_newer() {
            match checkout_free::merge_tree_write(repo, base_branch, task_branch).await? {
                Ok(_tree_sha) => {
                    // Clean merge - no conflicts
                    Ok(Vec::new())
                }
                Err(conflict_files) => {
                    // Conflicts detected - return file paths
                    Ok(conflict_files
                        .into_iter()
                        .map(|p| p.to_string_lossy().to_string())
                        .collect())
                }
            }
        } else {
            // Git < 2.38: can't do pre-merge preview without --write-tree
            // Return empty list (no conflicts detectable without active merge)
            Ok(Vec::new())
        }
    }

    /// Check if a merge is currently in progress (MERGE_HEAD exists).
    fn is_merge_in_progress(repo: &Path) -> bool {
        let git_dir = Self::resolve_git_dir(repo);
        git_dir.join("MERGE_HEAD").exists()
    }

    /// Resolve the git directory for a worktree or repository.
    ///
    /// For regular repos, returns `worktree/.git`.
    /// For worktrees where `.git` is a file containing `gitdir: <path>`,
    /// follows the indirection.
    fn resolve_git_dir(worktree: &Path) -> PathBuf {
        let git_path = worktree.join(".git");

        if git_path.is_file() {
            if let Ok(content) = std::fs::read_to_string(&git_path) {
                if let Some(path) = content.strip_prefix("gitdir: ") {
                    return PathBuf::from(path.trim());
                }
            }
        }

        git_path
    }

    /// Get list of files with conflicts in the index.
    ///
    /// Uses `git diff --name-only --diff-filter=U` to find unmerged files.
    fn get_conflict_files(repo: &Path) -> AppResult<Vec<PathBuf>> {
        let output = Command::new(resolve_git_cli_path())
            .args(["diff", "--name-only", "--diff-filter=U"])
            .current_dir(repo)
            .output()
            .map_err(|e| AppError::GitOperation(format!("Failed to run git diff: {}", e)))?;

        let stdout = String::from_utf8_lossy(&output.stdout);
        let files: Vec<PathBuf> = stdout
            .lines()
            .filter(|line| !line.is_empty())
            .map(PathBuf::from)
            .collect();

        Ok(files)
    }

    /// Check if Git version is 2.38 or newer.
    ///
    /// Git 2.38 introduced `merge-tree --write-tree` which is needed for
    /// pre-merge conflict detection.
    fn is_git_238_or_newer() -> bool {
        static CACHE: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
        *CACHE.get_or_init(|| {
            let output = Command::new(resolve_git_cli_path()).args(["--version"]).output();

            if let Ok(output) = output {
                let version_str = String::from_utf8_lossy(&output.stdout);
                // Parse "git version 2.38.0" or similar
                if let Some(version_part) = version_str.to_lowercase().strip_prefix("git version ")
                {
                    let parts: Vec<&str> = version_part.split('.').collect();
                    if parts.len() >= 2 {
                        if let (Ok(major), Ok(minor)) =
                            (parts[0].parse::<u32>(), parts[1].parse::<u32>())
                        {
                            return major > 2 || (major == 2 && minor >= 38);
                        }
                    }
                }
            }
            false
        })
    }

    // =========================================================================
    // 3-Way Conflict Diff (Phase 2 - Live Merge Conflict Detection)
    // =========================================================================

    /// Get 3-way diff data for a file with merge conflicts.
    ///
    /// Returns the content from all three sides of the merge plus the current
    /// file with conflict markers for inline conflict rendering.
    ///
    /// # Arguments
    /// * `file_path` - Path to the file with conflicts (relative to project root)
    /// * `project_path` - Path to the git repository or worktree
    /// * `task_branch` - The source branch (task branch, "theirs" in merge)
    /// * `base_branch` - The target branch ("ours" in merge, e.g., "main")
    ///
    /// # Returns
    /// * `ConflictDiff` - All three versions plus merged content with markers
    pub fn get_conflict_diff(
        &self,
        file_path: &str,
        project_path: &str,
        task_branch: &str,
        base_branch: &str,
    ) -> AppResult<ConflictDiff> {
        let repo = Path::new(project_path);

        // 1. Get merge-base (common ancestor)
        let merge_base = self.get_merge_base(repo, base_branch, task_branch)?;

        // 2. Get base_content from merge-base (may be empty if file is new)
        let base_content = self
            .get_file_content_at_ref(project_path, &merge_base, file_path)
            .unwrap_or_default();

        // 3. Get ours_content from base_branch (target branch)
        let ours_content = self
            .get_file_content_at_ref(project_path, base_branch, file_path)
            .unwrap_or_default();

        // 4. Get theirs_content from task_branch (source branch)
        let theirs_content = self
            .get_file_content_at_ref(project_path, task_branch, file_path)
            .unwrap_or_default();

        // 5. Get merged_with_markers by reading the file directly from disk
        // (it already has conflict markers from the failed merge)
        let full_path = repo.join(file_path);
        let merged_with_markers = std::fs::read_to_string(&full_path).unwrap_or_default();

        // 6. Get language from file extension
        let language = get_language_from_path(file_path);

        Ok(ConflictDiff {
            file_path: file_path.to_string(),
            base_content,
            ours_content,
            theirs_content,
            merged_with_markers,
            language,
        })
    }

    /// Get the merge-base commit SHA between two branches.
    fn get_merge_base(
        &self,
        repo: &Path,
        base_branch: &str,
        task_branch: &str,
    ) -> AppResult<String> {
        let output = Command::new(resolve_git_cli_path())
            .args(["merge-base", base_branch, task_branch])
            .current_dir(repo)
            .output()
            .map_err(|e| AppError::GitOperation(format!("Failed to run git merge-base: {}", e)))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(AppError::GitOperation(format!(
                "git merge-base failed: {}",
                stderr
            )));
        }

        let merge_base = String::from_utf8_lossy(&output.stdout).trim().to_string();
        Ok(merge_base)
    }

    fn get_file_content_at_ref(
        &self,
        project_path: &str,
        git_ref: &str,
        file_path: &str,
    ) -> Option<String> {
        let output = Command::new(resolve_git_cli_path())
            .args(["show", &format!("{}:{}", git_ref, file_path)])
            .current_dir(project_path)
            .output()
            .ok()?;

        if output.status.success() {
            Some(String::from_utf8_lossy(&output.stdout).to_string())
        } else {
            None
        }
    }
}

/// Get programming language from file path
fn get_language_from_path(path: &str) -> String {
    let ext = path.rsplit('.').next().unwrap_or("").to_lowercase();
    match ext.as_str() {
        "ts" | "tsx" => "typescript".to_string(),
        "js" | "jsx" => "javascript".to_string(),
        "rs" => "rust".to_string(),
        "py" => "python".to_string(),
        "go" => "go".to_string(),
        "java" => "java".to_string(),
        "c" | "h" => "c".to_string(),
        "cpp" | "hpp" | "cc" => "cpp".to_string(),
        "rb" => "ruby".to_string(),
        "php" => "php".to_string(),
        "swift" => "swift".to_string(),
        "kt" => "kotlin".to_string(),
        "md" => "markdown".to_string(),
        "json" => "json".to_string(),
        "yaml" | "yml" => "yaml".to_string(),
        "toml" => "toml".to_string(),
        "html" => "html".to_string(),
        "css" => "css".to_string(),
        "scss" | "sass" => "scss".to_string(),
        "sql" => "sql".to_string(),
        "sh" | "bash" | "zsh" => "bash".to_string(),
        _ => "plaintext".to_string(),
    }
}

#[cfg(test)]
#[path = "diff_service_tests.rs"]
mod tests;
