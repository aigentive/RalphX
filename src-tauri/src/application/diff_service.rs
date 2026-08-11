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
use std::collections::{HashMap, HashSet};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

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
    /// Whether the file is considered auto-generated (source maps, lockfiles,
    /// minified bundles, build outputs). Set by `DiffService::compute_generated_flags`.
    #[serde(default)]
    pub is_generated: bool,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FileChangeStatus {
    Added,
    Modified,
    Deleted,
}

/// Diff data for a single file — hunk-based format
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileDiff {
    /// File path
    pub file_path: String,
    /// Programming language for syntax highlighting
    pub language: String,
    /// Parsed diff hunks
    pub hunks: Vec<DiffHunk>,
    /// Total line count of the old (before) version; 0 for new files
    pub old_total_lines: u32,
    /// Total line count of the new (after) version; 0 for deleted files
    pub new_total_lines: u32,
    /// True when git reports binary content and hunks are unavailable
    pub is_binary: bool,
}

pub const MAX_DIFF_PAGE_LIMIT: usize = 400;

/// Windowed diff data for one file. Rows are flattened so large files can be
/// rendered progressively without sending the full hunk payload to the UI.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileDiffPage {
    pub file_path: String,
    pub language: String,
    pub rows: Vec<DiffPageRow>,
    pub offset: usize,
    pub limit: usize,
    pub next_offset: Option<usize>,
    pub total_rows: usize,
    pub old_total_lines: u32,
    pub new_total_lines: u32,
    pub is_binary: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum DiffPageRow {
    HunkHeader {
        header: String,
        old_start: u32,
        old_lines: u32,
        new_start: u32,
        new_lines: u32,
    },
    Line {
        line: DiffLine,
    },
}

// =========================================================================
// Hunk-based diff types
// =========================================================================

/// Classification of a single diff line
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DiffLineKind {
    Context,
    Addition,
    Deletion,
}

/// A single line in a diff hunk with source-position metadata
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiffLine {
    pub kind: DiffLineKind,
    pub content: String,
    /// 1-indexed line number in the old file (None for additions)
    pub old_line_num: Option<u32>,
    /// 1-indexed line number in the new file (None for deletions)
    pub new_line_num: Option<u32>,
}

/// A contiguous changed region in a unified diff
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiffHunk {
    pub old_start: u32,
    pub old_lines: u32,
    pub new_start: u32,
    pub new_lines: u32,
    /// Raw `@@ ... @@` header line, including optional trailing function context
    pub header: String,
    pub lines: Vec<DiffLine>,
}

/// Which side of a diff to view (for range fetching)
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DiffSide {
    Old,
    New,
}

/// Identifies a git ref in the context of an agent workspace diff
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum DiffRefKind {
    Head,
    Staged,
    Unstaged,
    Commit {
        sha: String,
    },
    /// Workspace cumulative base ref — caller must resolve before passing to DiffService
    CumulativeBase,
    /// Workspace cumulative head ref — caller must resolve before passing to DiffService
    CumulativeHead,
}

/// A single line returned by the range-fetch endpoint
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RangeLine {
    /// 1-indexed line number
    pub line_num: u32,
    pub content: String,
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

    pub fn page_file_diff(diff: FileDiff, offset: usize, limit: usize) -> AppResult<FileDiffPage> {
        if limit == 0 {
            return Err(AppError::Validation(
                "Diff page limit must be greater than zero".to_string(),
            ));
        }
        if limit > MAX_DIFF_PAGE_LIMIT {
            return Err(AppError::Validation(format!(
                "Diff page limit too large: {limit} rows requested (max {MAX_DIFF_PAGE_LIMIT})"
            )));
        }

        let mut rows = Vec::new();
        for hunk in diff.hunks {
            rows.push(DiffPageRow::HunkHeader {
                header: hunk.header,
                old_start: hunk.old_start,
                old_lines: hunk.old_lines,
                new_start: hunk.new_start,
                new_lines: hunk.new_lines,
            });
            rows.extend(
                hunk.lines
                    .into_iter()
                    .map(|line| DiffPageRow::Line { line }),
            );
        }

        let total_rows = rows.len();
        let page_rows: Vec<DiffPageRow> = rows.into_iter().skip(offset).take(limit).collect();
        let consumed_until = offset.saturating_add(page_rows.len());
        let next_offset = (consumed_until < total_rows).then_some(consumed_until);

        Ok(FileDiffPage {
            file_path: diff.file_path,
            language: diff.language,
            rows: page_rows,
            offset,
            limit,
            next_offset,
            total_rows,
            old_total_lines: diff.old_total_lines,
            new_total_lines: diff.new_total_lines,
            is_binary: diff.is_binary,
        })
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
        let name_status = run_git_text(project_path, &["diff", "--name-status", base_ref])?;
        let line_counts = run_git_numstat_lossy(project_path, &["diff", "--numstat", base_ref]);
        let mut changes = file_changes_from_name_status(&name_status, &line_counts);
        self.extend_untracked_file_changes(project_path, &mut changes)?;
        Ok(changes)
    }

    /// Get the diff for a specific file between a base ref and the current worktree.
    /// Includes unstaged working-tree content and untracked files.
    pub fn get_worktree_file_diff_from_ref(
        &self,
        file_path: &str,
        project_path: &str,
        base_ref: &str,
    ) -> AppResult<FileDiff> {
        validate_diff_file_path(file_path)?;
        let raw_diff =
            run_git_text(project_path, &["diff", base_ref, "--", file_path]).unwrap_or_default();
        if raw_diff.trim().is_empty() && self.is_untracked_file(project_path, file_path)? {
            return self.get_untracked_file_diff(file_path, project_path);
        }
        let is_binary = raw_diff.contains("Binary files");
        let hunks = if is_binary {
            vec![]
        } else {
            parse_unified_diff(&raw_diff)
        };
        let old_total_lines = self.count_lines_at_ref(project_path, base_ref, file_path);
        let new_total_lines = Self::count_lines_on_disk(project_path, file_path);
        Ok(FileDiff {
            file_path: file_path.to_string(),
            language: get_language_from_path(file_path),
            hunks,
            old_total_lines,
            new_total_lines,
            is_binary,
        })
    }

    /// Get files staged in the index (git diff --cached).
    /// Only shows changes between HEAD and the index — excludes unstaged working-tree edits.
    pub fn get_staged_file_changes(&self, project_path: &str) -> AppResult<Vec<FileChange>> {
        let name_status = run_git_text(project_path, &["diff", "--cached", "--name-status"])?;
        let line_counts = run_git_numstat_lossy(project_path, &["diff", "--cached", "--numstat"]);
        Ok(file_changes_from_name_status(&name_status, &line_counts))
    }

    /// Get files with unstaged working-tree changes (git diff — index vs disk).
    /// Only shows changes that are not yet staged — excludes staged-only changes.
    pub fn get_unstaged_file_changes(&self, project_path: &str) -> AppResult<Vec<FileChange>> {
        let name_status = run_git_text(project_path, &["diff", "--name-status"])?;
        let line_counts = run_git_numstat_lossy(project_path, &["diff", "--numstat"]);
        let mut changes = file_changes_from_name_status(&name_status, &line_counts);
        self.extend_untracked_file_changes(project_path, &mut changes)?;
        Ok(changes)
    }

    /// Get the diff for a specific file between HEAD and the staging area.
    ///
    /// Old = committed HEAD version; New = staged (index) version.
    pub fn get_staged_file_diff(&self, file_path: &str, project_path: &str) -> AppResult<FileDiff> {
        validate_diff_file_path(file_path)?;
        let raw_diff = run_git_text(
            project_path,
            &["diff", "--no-ext-diff", "--cached", "HEAD", "--", file_path],
        )?;
        let is_binary = raw_diff.contains("Binary files");
        let hunks = if is_binary {
            vec![]
        } else {
            parse_unified_diff(&raw_diff)
        };
        let old_total_lines = self.count_lines_at_ref(project_path, "HEAD", file_path);
        let new_total_lines = self.count_lines_at_index(project_path, file_path);
        Ok(FileDiff {
            file_path: file_path.to_string(),
            language: get_language_from_path(file_path),
            hunks,
            old_total_lines,
            new_total_lines,
            is_binary,
        })
    }

    /// Get the diff for a specific file between the staging area and the working tree.
    ///
    /// Old = staged (index) version; New = current disk content.
    pub fn get_unstaged_file_diff(
        &self,
        file_path: &str,
        project_path: &str,
    ) -> AppResult<FileDiff> {
        validate_diff_file_path(file_path)?;
        let raw_diff = run_git_text(project_path, &["diff", "--no-ext-diff", "--", file_path])?;
        if raw_diff.trim().is_empty() && self.is_untracked_file(project_path, file_path)? {
            return self.get_untracked_file_diff(file_path, project_path);
        }
        let is_binary = raw_diff.contains("Binary files");
        let hunks = if is_binary {
            vec![]
        } else {
            parse_unified_diff(&raw_diff)
        };
        let old_total_lines = self.count_lines_at_index(project_path, file_path);
        let new_total_lines = Self::count_lines_on_disk(project_path, file_path);
        Ok(FileDiff {
            file_path: file_path.to_string(),
            language: get_language_from_path(file_path),
            hunks,
            old_total_lines,
            new_total_lines,
            is_binary,
        })
    }

    /// Get 3-way conflict data from Git's unmerged index stages.
    ///
    /// Stage 1 = merge base, stage 2 = ours, stage 3 = theirs. This is the
    /// most reliable source while a worktree is paused mid-merge or mid-rebase,
    /// because branch names may be detached or no longer point at the exact
    /// commits involved in the conflict.
    pub fn get_index_conflict_diff(
        &self,
        file_path: &str,
        project_path: &str,
    ) -> AppResult<ConflictDiff> {
        validate_diff_file_path(file_path)?;

        let base_content = self
            .get_file_content_at_index_stage(project_path, file_path, 1)
            .unwrap_or_default();
        let ours_content = self
            .get_file_content_at_index_stage(project_path, file_path, 2)
            .unwrap_or_default();
        let theirs_content = self
            .get_file_content_at_index_stage(project_path, file_path, 3)
            .unwrap_or_default();
        let full_path = Path::new(project_path).join(file_path);
        let merged_with_markers =
            crate::utils::path_safety::checked_read_to_string(&full_path, "conflict file")
                .unwrap_or_default();

        Ok(ConflictDiff {
            file_path: file_path.to_string(),
            base_content,
            ours_content,
            theirs_content,
            merged_with_markers,
            language: get_language_from_path(file_path),
        })
    }

    /// Read a file's content from the git index (staging area).
    ///
    /// Uses `git show :<file>` where the leading `:` refers to the index.
    /// Returns `None` if the file is not staged (e.g. untracked or only on disk).
    fn get_file_content_at_index(&self, project_path: &str, file_path: &str) -> Option<String> {
        let output = Command::new(resolve_git_cli_path())
            .args(["show", &format!(":{}", file_path)])
            .current_dir(project_path)
            .output()
            .ok()?;
        if output.status.success() {
            Some(String::from_utf8_lossy(&output.stdout).to_string())
        } else {
            None
        }
    }

    /// Read a file's content from a specific git index stage.
    fn get_file_content_at_index_stage(
        &self,
        project_path: &str,
        file_path: &str,
        stage: u8,
    ) -> Option<String> {
        let output = Command::new(resolve_git_cli_path())
            .args(["show", &format!(":{stage}:{file_path}")])
            .current_dir(project_path)
            .output()
            .ok()?;
        if output.status.success() {
            Some(String::from_utf8_lossy(&output.stdout).to_string())
        } else {
            None
        }
    }

    /// Get the diff content for a specific file: base_branch vs working tree
    pub fn get_file_diff(
        &self,
        file_path: &str,
        project_path: &str,
        base_branch: &str,
    ) -> AppResult<FileDiff> {
        validate_diff_file_path(file_path)?;
        let raw_diff =
            run_git_text(project_path, &["diff", base_branch, "--", file_path]).unwrap_or_default();
        if raw_diff.trim().is_empty() && self.is_untracked_file(project_path, file_path)? {
            return self.get_untracked_file_diff(file_path, project_path);
        }
        let is_binary = raw_diff.contains("Binary files");
        let hunks = if is_binary {
            vec![]
        } else {
            parse_unified_diff(&raw_diff)
        };
        let old_total_lines = self.count_lines_at_ref(project_path, base_branch, file_path);
        let new_total_lines = Self::count_lines_on_disk(project_path, file_path);
        Ok(FileDiff {
            file_path: file_path.to_string(),
            language: get_language_from_path(file_path),
            hunks,
            old_total_lines,
            new_total_lines,
            is_binary,
        })
    }

    fn extend_untracked_file_changes(
        &self,
        project_path: &str,
        changes: &mut Vec<FileChange>,
    ) -> AppResult<()> {
        let mut tracked_paths = changes
            .iter()
            .map(|change| change.path.clone())
            .collect::<HashSet<_>>();
        for change in self.get_untracked_file_changes(project_path)? {
            if tracked_paths.insert(change.path.clone()) {
                changes.push(change);
            }
        }
        changes.sort_by(|a, b| a.path.cmp(&b.path));
        Ok(())
    }

    fn get_untracked_file_changes(&self, project_path: &str) -> AppResult<Vec<FileChange>> {
        let status = run_git_text(project_path, &["status", "--porcelain=v1", "-z", "-uall"])?;
        let mut changes = Vec::new();
        for entry in status.split('\0').filter(|entry| !entry.is_empty()) {
            let Some(path) = entry.strip_prefix("?? ") else {
                continue;
            };
            validate_diff_file_path(path)?;
            let additions = self.count_untracked_file_lines(project_path, path)?;
            changes.push(FileChange {
                path: path.to_string(),
                status: FileChangeStatus::Added,
                additions,
                deletions: 0,
                is_generated: false,
            });
        }
        changes.sort_by(|a, b| a.path.cmp(&b.path));
        Ok(changes)
    }

    fn is_untracked_file(&self, project_path: &str, file_path: &str) -> AppResult<bool> {
        validate_diff_file_path(file_path)?;
        let status = run_git_text(
            project_path,
            &["status", "--porcelain=v1", "-z", "-uall", "--", file_path],
        )?;
        Ok(status
            .split('\0')
            .any(|entry| entry.strip_prefix("?? ") == Some(file_path)))
    }

    fn get_untracked_file_diff(&self, file_path: &str, project_path: &str) -> AppResult<FileDiff> {
        let Some(bytes) = read_validated_worktree_file_bytes(project_path, file_path)? else {
            return Ok(FileDiff {
                file_path: file_path.to_string(),
                language: get_language_from_path(file_path),
                hunks: Vec::new(),
                old_total_lines: 0,
                new_total_lines: 0,
                is_binary: true,
            });
        };
        let Ok(content) = String::from_utf8(bytes) else {
            return Ok(FileDiff {
                file_path: file_path.to_string(),
                language: get_language_from_path(file_path),
                hunks: Vec::new(),
                old_total_lines: 0,
                new_total_lines: 0,
                is_binary: true,
            });
        };
        let new_total_lines = content.lines().count() as u32;
        Ok(FileDiff {
            file_path: file_path.to_string(),
            language: get_language_from_path(file_path),
            hunks: added_file_hunks(&content),
            old_total_lines: 0,
            new_total_lines,
            is_binary: false,
        })
    }

    fn count_untracked_file_lines(&self, project_path: &str, file_path: &str) -> AppResult<u32> {
        let Some(bytes) = read_validated_worktree_file_bytes(project_path, file_path)? else {
            return Ok(0);
        };
        Ok(String::from_utf8(bytes)
            .map(|content| content.lines().count() as u32)
            .unwrap_or(0))
    }

    /// Get files changed in a specific commit
    pub fn get_commit_file_changes(
        &self,
        commit_sha: &str,
        project_path: &str,
    ) -> AppResult<Vec<FileChange>> {
        let name_status = run_git_text(
            project_path,
            &[
                "diff-tree",
                "--no-commit-id",
                "--name-status",
                "-r",
                commit_sha,
            ],
        )?;
        let parent_ref = format!("{}^", commit_sha);
        let line_counts = run_git_numstat_lossy(
            project_path,
            &["diff", "--numstat", &parent_ref, commit_sha],
        );
        Ok(file_changes_from_name_status(&name_status, &line_counts))
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
        let name_status = run_git_text(project_path, &["diff", "--name-status", from_ref, to_ref])?;
        let line_counts =
            run_git_numstat_lossy(project_path, &["diff", "--numstat", from_ref, to_ref]);
        Ok(file_changes_from_name_status(&name_status, &line_counts))
    }

    /// Get diff for a file between two git refs
    pub fn get_file_diff_between_refs(
        &self,
        file_path: &str,
        project_path: &str,
        from_ref: &str,
        to_ref: &str,
    ) -> AppResult<FileDiff> {
        validate_diff_file_path(file_path)?;
        let raw_diff = run_git_text(
            project_path,
            &["diff", "--no-ext-diff", from_ref, to_ref, "--", file_path],
        )?;
        let is_binary = raw_diff.contains("Binary files");
        let hunks = if is_binary {
            vec![]
        } else {
            parse_unified_diff(&raw_diff)
        };
        let old_total_lines = self.count_lines_at_ref(project_path, from_ref, file_path);
        let new_total_lines = self.count_lines_at_ref(project_path, to_ref, file_path);
        Ok(FileDiff {
            file_path: file_path.to_string(),
            language: get_language_from_path(file_path),
            hunks,
            old_total_lines,
            new_total_lines,
            is_binary,
        })
    }

    /// Get file changes from a unified multi-file patch.
    pub fn get_file_changes_from_unified_diff(&self, raw_patch: &str) -> Vec<FileChange> {
        let mut changes: Vec<FileChange> = file_patches_from_unified_diff(raw_patch)
            .into_iter()
            .map(|patch| FileChange {
                path: patch.path,
                status: patch.status,
                additions: patch.additions,
                deletions: patch.deletions,
                is_generated: false,
            })
            .collect();
        changes.sort_by(|a, b| a.path.cmp(&b.path));
        changes
    }

    /// Get a single file diff from a unified multi-file patch.
    pub fn get_file_diff_from_unified_diff(
        &self,
        raw_patch: &str,
        file_path: &str,
    ) -> AppResult<FileDiff> {
        validate_diff_file_path(file_path)?;
        let Some(patch) = file_patches_from_unified_diff(raw_patch)
            .into_iter()
            .find(|patch| patch.path == file_path)
        else {
            return Ok(FileDiff {
                file_path: file_path.to_string(),
                language: get_language_from_path(file_path),
                hunks: Vec::new(),
                old_total_lines: 0,
                new_total_lines: 0,
                is_binary: false,
            });
        };

        let hunks = if patch.is_binary {
            Vec::new()
        } else {
            parse_unified_diff(&patch.raw)
        };
        let old_total_lines = if matches!(patch.status, FileChangeStatus::Added) {
            0
        } else {
            max_old_line_from_hunks(&hunks)
        };
        let new_total_lines = if matches!(patch.status, FileChangeStatus::Deleted) {
            0
        } else {
            max_new_line_from_hunks(&hunks)
        };

        Ok(FileDiff {
            file_path: file_path.to_string(),
            language: get_language_from_path(file_path),
            hunks,
            old_total_lines,
            new_total_lines,
            is_binary: patch.is_binary,
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
    // Line-count helpers (used for old_total_lines / new_total_lines)
    // =========================================================================

    fn count_lines_at_ref(&self, project_path: &str, git_ref: &str, file_path: &str) -> u32 {
        self.get_file_content_at_ref(project_path, git_ref, file_path)
            .map(|c| c.lines().count() as u32)
            .unwrap_or(0)
    }

    fn count_lines_at_index(&self, project_path: &str, file_path: &str) -> u32 {
        self.get_file_content_at_index(project_path, file_path)
            .map(|c| c.lines().count() as u32)
            .unwrap_or(0)
    }

    fn count_lines_on_disk(project_path: &str, file_path: &str) -> u32 {
        read_validated_worktree_file_bytes(project_path, file_path)
            .ok()
            .flatten()
            .and_then(|bytes| String::from_utf8(bytes).ok())
            .map(|content| content.lines().count() as u32)
            .unwrap_or(0)
    }

    // =========================================================================
    // Range fetch endpoint
    // =========================================================================

    /// Fetch a range of lines [from, to] (1-indexed, inclusive) from a specific
    /// version of a file.  Maximum range size is 5 000 lines.
    ///
    /// `DiffRefKind::CumulativeBase` and `DiffRefKind::CumulativeHead` are NOT
    /// handled here — the caller must resolve them to `DiffRefKind::Commit { sha }`
    /// using workspace context before calling this method.
    ///
    /// # Errors
    /// * `Validation` — range too large, `from > to`, or unsafe path components.
    /// * `GitOperation` — file not found at the requested ref / index.
    pub fn get_file_content_range(
        &self,
        workspace_path: &str,
        side: &DiffSide,
        path: &str,
        ref_kind: &DiffRefKind,
        from: u32,
        to: u32,
    ) -> AppResult<Vec<RangeLine>> {
        const MAX_RANGE: u32 = 5_000;

        if from < 1 {
            return Err(AppError::Validation(
                "'from' must be >= 1 (1-indexed)".to_string(),
            ));
        }
        if to < from {
            return Err(AppError::Validation(format!(
                "'from' ({from}) must be <= 'to' ({to})"
            )));
        }
        if to - from + 1 > MAX_RANGE {
            return Err(AppError::Validation(format!(
                "Range too large: {} lines requested (max {MAX_RANGE})",
                to - from + 1
            )));
        }

        // Validate path: must be relative with no unsafe components
        validate_diff_file_path(path)?;

        let content: String = match ref_kind {
            DiffRefKind::Unstaged => {
                if matches!(side, DiffSide::New) {
                    // Working-tree file — use safe read helper (CodeQL path containment)
                    let full_path = std::path::PathBuf::from(workspace_path).join(path);
                    crate::utils::path_safety::checked_read_to_string(
                        &full_path,
                        "content range file",
                    )?
                } else {
                    // Old side of unstaged diff = index
                    self.get_file_content_at_index(workspace_path, path)
                        .ok_or_else(|| {
                            AppError::GitOperation(format!(
                                "File '{path}' not found in the git index"
                            ))
                        })?
                }
            }
            DiffRefKind::Staged => self
                .get_file_content_at_index(workspace_path, path)
                .ok_or_else(|| {
                    AppError::GitOperation(format!("File '{path}' not found in the git index"))
                })?,
            DiffRefKind::Head => self
                .get_file_content_at_ref(workspace_path, "HEAD", path)
                .ok_or_else(|| {
                    AppError::GitOperation(format!("File '{path}' not found at HEAD"))
                })?,
            DiffRefKind::Commit { sha } => self
                .get_file_content_at_ref(workspace_path, sha, path)
                .ok_or_else(|| {
                    AppError::GitOperation(format!("File '{path}' not found at commit {sha}"))
                })?,
            DiffRefKind::CumulativeBase | DiffRefKind::CumulativeHead => {
                return Err(AppError::Validation(
                    "CumulativeBase/CumulativeHead must be resolved to Commit by the caller"
                        .to_string(),
                ));
            }
        };

        let lines: Vec<RangeLine> = content
            .lines()
            .enumerate()
            .filter_map(|(i, line)| {
                let line_num = (i + 1) as u32;
                if line_num >= from && line_num <= to {
                    Some(RangeLine {
                        line_num,
                        content: line.to_string(),
                    })
                } else {
                    None
                }
            })
            .collect();

        Ok(lines)
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
            let output = Command::new(resolve_git_cli_path())
                .args(["--version"])
                .output();

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

    /// Determine whether each path in `paths` is auto-generated, using a single
    /// batched `git check-attr --stdin linguist-generated` call followed by a
    /// hardcoded heuristic for paths with no `.gitattributes` opinion.
    ///
    /// Returns a map from path → `is_generated`. Every path in `paths` is
    /// guaranteed to have an entry in the returned map.
    ///
    /// # Errors
    /// Always returns `Ok` — if the git call fails the function logs a warning
    /// and falls back to the heuristic for all paths.
    pub fn compute_generated_flags(
        &self,
        workspace_path: &Path,
        paths: &[&str],
    ) -> AppResult<HashMap<String, bool>> {
        if paths.is_empty() {
            return Ok(HashMap::new());
        }

        let attr_map = self.run_git_check_attr_map(workspace_path, paths);

        match attr_map {
            Ok(ref map) => {
                let mut flags = HashMap::with_capacity(paths.len());
                for &path in paths {
                    let value = map.get(path).map(String::as_str).unwrap_or("unspecified");
                    let is_gen = match value {
                        "unspecified" => is_generated_by_heuristic(path),
                        // Explicit opt-out via `.gitattributes`
                        "unset" | "false" => false,
                        // "set", "true", or any other value → generated
                        _ => true,
                    };
                    flags.insert(path.to_string(), is_gen);
                }
                Ok(flags)
            }
            Err(e) => {
                tracing::warn!("git check-attr failed, falling back to heuristic: {e}");
                let flags = paths
                    .iter()
                    .map(|&p| (p.to_string(), is_generated_by_heuristic(p)))
                    .collect();
                Ok(flags)
            }
        }
    }

    /// Run `git check-attr --stdin linguist-generated` and return a map of
    /// `path → attribute value` for each path.  Paths with no matching rule
    /// appear with value `"unspecified"`.
    fn run_git_check_attr_map(
        &self,
        workspace_path: &Path,
        paths: &[&str],
    ) -> AppResult<HashMap<String, String>> {
        let mut child = Command::new(resolve_git_cli_path())
            .args(["check-attr", "--stdin", "linguist-generated"])
            .current_dir(workspace_path)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| AppError::GitOperation(format!("Failed to spawn git check-attr: {e}")))?;

        // Write all paths to stdin (drop closes the pipe, signalling EOF to git)
        if let Some(mut stdin) = child.stdin.take() {
            for path in paths {
                // Paths from git diff --name-status are controlled git output, safe to forward
                writeln!(stdin, "{path}").ok();
            }
        }

        let output = child.wait_with_output().map_err(|e| {
            AppError::GitOperation(format!("Failed to wait for git check-attr: {e}"))
        })?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(AppError::GitOperation(format!(
                "git check-attr failed: {stderr}"
            )));
        }

        // Output format: "<path>: linguist-generated: <value>\n"
        let stdout = String::from_utf8_lossy(&output.stdout);
        let mut map = HashMap::with_capacity(paths.len());
        const ATTR_INFIX: &str = ": linguist-generated: ";
        for line in stdout.lines() {
            if let Some(attr_pos) = line.find(ATTR_INFIX) {
                let path = line[..attr_pos].to_string();
                let value = line[attr_pos + ATTR_INFIX.len()..].to_string();
                map.insert(path, value);
            }
        }
        Ok(map)
    }
}

#[derive(Debug)]
struct UnifiedFilePatch {
    path: String,
    status: FileChangeStatus,
    additions: u32,
    deletions: u32,
    raw: String,
    is_binary: bool,
}

#[derive(Debug)]
struct UnifiedFilePatchBuilder {
    old_path: Option<String>,
    new_path: Option<String>,
    status: FileChangeStatus,
    additions: u32,
    deletions: u32,
    raw: String,
    in_hunk: bool,
    is_binary: bool,
}

impl UnifiedFilePatchBuilder {
    fn new(first_line: &str) -> Self {
        let mut builder = Self {
            old_path: None,
            new_path: None,
            status: FileChangeStatus::Modified,
            additions: 0,
            deletions: 0,
            raw: String::new(),
            in_hunk: false,
            is_binary: false,
        };
        builder.record_line(first_line);
        builder
    }

    fn record_line(&mut self, line: &str) {
        self.raw.push_str(line);
        self.raw.push('\n');

        if line.starts_with("new file mode") {
            self.status = FileChangeStatus::Added;
            return;
        }
        if line.starts_with("deleted file mode") {
            self.status = FileChangeStatus::Deleted;
            return;
        }
        if let Some(path) = line.strip_prefix("--- ") {
            self.old_path = parse_unified_diff_path(path);
            if self.old_path.is_none() {
                self.status = FileChangeStatus::Added;
            }
            return;
        }
        if let Some(path) = line.strip_prefix("+++ ") {
            self.new_path = parse_unified_diff_path(path);
            if self.new_path.is_none() {
                self.status = FileChangeStatus::Deleted;
            }
            return;
        }
        if line.starts_with("Binary files ") || line.starts_with("GIT binary patch") {
            self.is_binary = true;
            return;
        }
        if line.starts_with("@@ ") {
            self.in_hunk = true;
            return;
        }
        if !self.in_hunk {
            return;
        }
        if line.starts_with('+') {
            self.additions += 1;
        } else if line.starts_with('-') {
            self.deletions += 1;
        }
    }

    fn finish(self) -> Option<UnifiedFilePatch> {
        let path = match self.status {
            FileChangeStatus::Deleted => self.old_path.or(self.new_path)?,
            _ => self.new_path.or(self.old_path)?,
        };
        if validate_diff_file_path(&path).is_err() {
            return None;
        }
        Some(UnifiedFilePatch {
            path,
            status: self.status,
            additions: self.additions,
            deletions: self.deletions,
            raw: self.raw,
            is_binary: self.is_binary,
        })
    }
}

fn file_patches_from_unified_diff(raw_patch: &str) -> Vec<UnifiedFilePatch> {
    let mut patches = Vec::new();
    let mut current: Option<UnifiedFilePatchBuilder> = None;

    for line in raw_patch.lines() {
        if line.starts_with("diff --git ") {
            if let Some(builder) = current.take().and_then(UnifiedFilePatchBuilder::finish) {
                patches.push(builder);
            }
            current = Some(UnifiedFilePatchBuilder::new(line));
            continue;
        }
        if let Some(builder) = current.as_mut() {
            builder.record_line(line);
        }
    }

    if let Some(builder) = current.and_then(UnifiedFilePatchBuilder::finish) {
        patches.push(builder);
    }

    patches
}

fn parse_unified_diff_path(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    if trimmed == "/dev/null" || trimmed.is_empty() {
        return None;
    }
    let unquoted = trimmed.trim_matches('"');
    unquoted
        .strip_prefix("a/")
        .or_else(|| unquoted.strip_prefix("b/"))
        .or(Some(unquoted))
        .map(str::to_string)
}

fn max_old_line_from_hunks(hunks: &[DiffHunk]) -> u32 {
    hunks
        .iter()
        .filter(|hunk| hunk.old_lines > 0)
        .map(|hunk| hunk.old_start + hunk.old_lines - 1)
        .max()
        .unwrap_or(0)
}

fn max_new_line_from_hunks(hunks: &[DiffHunk]) -> u32 {
    hunks
        .iter()
        .filter(|hunk| hunk.new_lines > 0)
        .map(|hunk| hunk.new_start + hunk.new_lines - 1)
        .max()
        .unwrap_or(0)
}

fn added_file_hunks(content: &str) -> Vec<DiffHunk> {
    let lines = content
        .lines()
        .enumerate()
        .map(|(index, line)| DiffLine {
            kind: DiffLineKind::Addition,
            content: line.to_string(),
            old_line_num: None,
            new_line_num: Some((index + 1) as u32),
        })
        .collect::<Vec<_>>();
    let new_lines = lines.len() as u32;
    if new_lines == 0 {
        return Vec::new();
    }

    vec![DiffHunk {
        old_start: 0,
        old_lines: 0,
        new_start: 1,
        new_lines,
        header: format!("@@ -0,0 +1,{new_lines} @@"),
        lines,
    }]
}

fn validated_worktree_file_path(project_path: &str, file_path: &str) -> AppResult<PathBuf> {
    validate_diff_file_path(file_path)?;
    let root = crate::utils::path_safety::validate_absolute_non_root_path(
        Path::new(project_path),
        "workspace root",
    )?;
    let canonical_root = root.canonicalize().map_err(|error| {
        AppError::Infrastructure(format!(
            "Failed to canonicalize workspace root {}: {error}",
            root.display()
        ))
    })?;
    let candidate = crate::utils::path_safety::validate_absolute_non_root_path(
        &root.join(file_path),
        "diff file",
    )?;
    let parent = candidate.parent().ok_or_else(|| {
        AppError::Validation(format!(
            "Diff file path has no parent: {}",
            candidate.display()
        ))
    })?;
    let canonical_parent = parent.canonicalize().map_err(|error| {
        AppError::Infrastructure(format!(
            "Failed to canonicalize diff file parent {}: {error}",
            parent.display()
        ))
    })?;
    if !canonical_parent.starts_with(&canonical_root) {
        return Err(AppError::Validation(format!(
            "File path escapes workspace root: {}",
            candidate.display()
        )));
    }
    let file_name = candidate.file_name().ok_or_else(|| {
        AppError::Validation(format!(
            "Diff file path has no filename: {}",
            candidate.display()
        ))
    })?;
    Ok(canonical_parent.join(file_name))
}

pub(crate) fn validate_worktree_diff_file_containment(
    project_path: &str,
    file_path: &str,
) -> AppResult<()> {
    let file_path = validated_worktree_file_path(project_path, file_path)?;
    let metadata = match std::fs::symlink_metadata(&file_path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => {
            return Err(AppError::Infrastructure(format!(
                "Failed to inspect diff file {}: {error}",
                file_path.display()
            )));
        }
    };
    if !metadata.file_type().is_symlink() {
        return Ok(());
    }
    let canonical_root = Path::new(project_path).canonicalize().map_err(|error| {
        AppError::Infrastructure(format!(
            "Failed to canonicalize workspace root {project_path}: {error}"
        ))
    })?;
    let canonical_target = file_path.canonicalize().map_err(|error| {
        AppError::Validation(format!(
            "Diff symlink target is unavailable for {}: {error}",
            file_path.display()
        ))
    })?;
    if !canonical_target.starts_with(canonical_root) {
        return Err(AppError::Validation(format!(
            "Diff symlink target escapes workspace root: {}",
            file_path.display()
        )));
    }
    Ok(())
}

fn read_validated_worktree_file_bytes(
    project_path: &str,
    file_path: &str,
) -> AppResult<Option<Vec<u8>>> {
    let file_path = validated_worktree_file_path(project_path, file_path)?;
    let metadata = match std::fs::symlink_metadata(&file_path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => {
            return Err(AppError::Infrastructure(format!(
                "Failed to inspect diff file {}: {error}",
                file_path.display()
            )));
        }
    };
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Ok(None);
    }

    std::fs::read(&file_path).map(Some).map_err(|error| {
        AppError::Infrastructure(format!(
            "Failed to read diff file {}: {error}",
            file_path.display()
        ))
    })
}

fn run_git_text(project_path: &str, args: &[&str]) -> AppResult<String> {
    let output = Command::new(resolve_git_cli_path())
        .args(args)
        .current_dir(project_path)
        .output()
        .map_err(|e| AppError::GitOperation(format!("Failed to run git {}: {}", args[0], e)))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(AppError::GitOperation(format!(
            "git {} failed: {}",
            args.join(" "),
            stderr.trim()
        )));
    }

    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

// =========================================================================
// Generated-file heuristic
// =========================================================================

lazy_static::lazy_static! {
    /// Patterns that identify auto-generated files when `.gitattributes` has no opinion.
    static ref GENERATED_PATTERNS: Vec<regex::Regex> = vec![
        // Source maps
        regex::Regex::new(r"\.map$").unwrap(),
        // Minified JS / CSS
        regex::Regex::new(r"\.min\.(js|css)$").unwrap(),
        // Common lockfiles
        regex::Regex::new(
            r"(?:^|/)(?:package-lock\.json|yarn\.lock|pnpm-lock\.yaml|Cargo\.lock|Gemfile\.lock|composer\.lock|poetry\.lock|uv\.lock)$"
        ).unwrap(),
        // Jest / Vitest snapshots
        regex::Regex::new(r"\.snap$").unwrap(),
        // Build / dist output directories
        regex::Regex::new(r"^(?:dist|build|out|target)/").unwrap(),
    ];
}

/// Return `true` when a file path matches one of the hardcoded generated-file
/// heuristics (used when `.gitattributes` does not specify `linguist-generated`).
fn is_generated_by_heuristic(path: &str) -> bool {
    GENERATED_PATTERNS.iter().any(|re| re.is_match(path))
}

fn run_git_numstat_lossy(project_path: &str, args: &[&str]) -> HashMap<String, (u32, u32)> {
    run_git_text(project_path, args)
        .map(|stdout| numstat_map_from_stdout(&stdout))
        .unwrap_or_default()
}

fn numstat_map_from_stdout(stdout: &str) -> HashMap<String, (u32, u32)> {
    let mut counts = HashMap::new();
    for line in stdout.lines() {
        let mut parts = line.split('\t');
        let Some(additions) = parts.next().and_then(parse_numstat_count) else {
            continue;
        };
        let Some(deletions) = parts.next().and_then(parse_numstat_count) else {
            continue;
        };
        let path_parts: Vec<&str> = parts.collect();
        let Some(path) = path_parts.last().map(|value| value.trim()) else {
            continue;
        };
        if path.is_empty() {
            continue;
        }
        counts.insert(path.to_string(), (additions, deletions));
    }
    counts
}

fn parse_numstat_count(value: &str) -> Option<u32> {
    if value == "-" {
        return Some(0);
    }
    value.parse().ok()
}

fn file_changes_from_name_status(
    name_status: &str,
    line_counts: &HashMap<String, (u32, u32)>,
) -> Vec<FileChange> {
    let mut changes = Vec::new();
    for line in name_status.lines() {
        let Some((status, path)) = parse_name_status_line(line) else {
            continue;
        };
        let (additions, deletions) = line_counts.get(&path).copied().unwrap_or((0, 0));
        changes.push(FileChange {
            path,
            status,
            additions,
            deletions,
            is_generated: false,
        });
    }
    changes.sort_by(|a, b| a.path.cmp(&b.path));
    changes
}

fn parse_name_status_line(line: &str) -> Option<(FileChangeStatus, String)> {
    let mut parts = line.split('\t');
    let status_token = parts.next()?;
    let path = parts.next_back()?.trim();
    if path.is_empty() {
        return None;
    }

    let status = match status_token.chars().next().unwrap_or('M') {
        'A' => FileChangeStatus::Added,
        'D' => FileChangeStatus::Deleted,
        _ => FileChangeStatus::Modified,
    };

    Some((status, path.to_string()))
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

// =========================================================================
// Unified diff parser
// =========================================================================

/// Parse a unified diff (e.g. from `git diff`) into a list of hunks.
///
/// Handles:
/// * Multi-hunk diffs with mixed additions / deletions / context lines.
/// * New-file hunks (`@@ -0,0 +1,N @@`).
/// * Deleted-file hunks.
/// * `\ No newline at end of file` markers — silently skipped.
/// * Binary-file output — caller detects `"Binary files"` and passes `vec![]`
///   directly; this function returns `vec![]` for truly empty raw strings.
pub fn parse_unified_diff(raw: &str) -> Vec<DiffHunk> {
    let mut hunks: Vec<DiffHunk> = Vec::new();
    let mut current_hunk: Option<DiffHunk> = None;
    let mut old_line: u32 = 0;
    let mut new_line: u32 = 0;

    for line in raw.lines() {
        if line.starts_with("@@ ") {
            if let Some(h) = current_hunk.take() {
                hunks.push(h);
            }
            if let Some(hunk) = parse_hunk_header(line) {
                old_line = hunk.old_start;
                new_line = hunk.new_start;
                current_hunk = Some(hunk);
            }
        } else if let Some(ref mut hunk) = current_hunk {
            if let Some(content) = line.strip_prefix('+') {
                hunk.lines.push(DiffLine {
                    kind: DiffLineKind::Addition,
                    content: content.to_string(),
                    old_line_num: None,
                    new_line_num: Some(new_line),
                });
                new_line += 1;
            } else if let Some(content) = line.strip_prefix('-') {
                hunk.lines.push(DiffLine {
                    kind: DiffLineKind::Deletion,
                    content: content.to_string(),
                    old_line_num: Some(old_line),
                    new_line_num: None,
                });
                old_line += 1;
            } else if let Some(content) = line.strip_prefix(' ') {
                hunk.lines.push(DiffLine {
                    kind: DiffLineKind::Context,
                    content: content.to_string(),
                    old_line_num: Some(old_line),
                    new_line_num: Some(new_line),
                });
                old_line += 1;
                new_line += 1;
            }
            // '\ No newline at end of file' — skip (don't emit, don't fail)
            // Other preamble / header lines (diff --git, index, ---, +++) — skip
        }
    }

    if let Some(h) = current_hunk.take() {
        hunks.push(h);
    }

    hunks
}

/// Parse a single `@@ -A,B +C,D @@ optional text` hunk header line.
fn parse_hunk_header(line: &str) -> Option<DiffHunk> {
    // Strip leading "@@ "
    let after_prefix = line.strip_prefix("@@ ")?;
    // Find the closing " @@"
    let close_pos = after_prefix.find(" @@")?;
    let ranges = &after_prefix[..close_pos];

    let mut parts = ranges.split(' ');
    let old_range = parts.next()?.strip_prefix('-')?;
    let new_range = parts.next()?.strip_prefix('+')?;

    let (old_start, old_lines) = parse_range_pair(old_range)?;
    let (new_start, new_lines) = parse_range_pair(new_range)?;

    Some(DiffHunk {
        old_start,
        old_lines,
        new_start,
        new_lines,
        header: line.to_string(),
        lines: Vec::new(),
    })
}

/// Parse `"A,B"` → `(A, B)` or `"A"` → `(A, 1)`.
fn parse_range_pair(s: &str) -> Option<(u32, u32)> {
    if let Some(comma_pos) = s.find(',') {
        let start: u32 = s[..comma_pos].parse().ok()?;
        let count: u32 = s[comma_pos + 1..].parse().ok()?;
        Some((start, count))
    } else {
        let start: u32 = s.parse().ok()?;
        Some((start, 1))
    }
}

/// Validate a file path received from external input (Tauri command / HTTP query).
///
/// Accepts only relative paths whose every component is a normal filename —
/// no `..`, `.`, absolute roots, or Windows drive prefixes.
fn validate_diff_file_path(path: &str) -> AppResult<()> {
    let p = std::path::Path::new(path);
    if p.is_absolute() {
        return Err(AppError::Validation(format!(
            "File path must be relative: {path}"
        )));
    }
    for component in p.components() {
        if !matches!(component, std::path::Component::Normal(_)) {
            return Err(AppError::Validation(format!(
                "File path contains unsafe components: {path}"
            )));
        }
    }
    if path.is_empty() {
        return Err(AppError::Validation(
            "File path must not be empty".to_string(),
        ));
    }
    Ok(())
}

#[cfg(test)]
#[path = "diff_service_tests.rs"]
mod tests;
