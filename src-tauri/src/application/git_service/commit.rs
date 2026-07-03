use super::git_cmd;
use super::*;

#[derive(Debug, Default)]
struct StageSelection {
    files_to_stage: Vec<String>,
    skipped_deletions: Vec<String>,
    skipped_generated_artifacts: Vec<String>,
}

fn normalize_git_status_path(path: &str) -> String {
    path.trim_start_matches("./").replace('\\', "/")
}

fn is_ralphx_generated_artifact_path(path: &str) -> bool {
    let normalized = normalize_git_status_path(path);
    let generated_prefixes = [
        ".claude/mcp-proxy/",
        ".claude/worktrees/",
        ".claude/memory-archive/",
        ".artifacts/screenshots/",
        ".artifacts/logs/mcp-proxy/",
    ];
    let generated_exact = [
        ".claude/mcp-proxy",
        ".claude/worktrees",
        ".claude/memory-archive",
        ".artifacts/screenshots",
        ".artifacts/logs/mcp-proxy",
    ];

    generated_exact.contains(&normalized.as_str())
        || generated_prefixes
            .iter()
            .any(|prefix| normalized.starts_with(prefix))
}

fn collect_stage_selection(status_stdout: &str, include_deletions: bool) -> StageSelection {
    let mut selection = StageSelection::default();

    // Format: "XY filename\0" — renames have two entries: "R  newname\0oldname\0"
    let entries: Vec<&str> = status_stdout.split('\0').collect();
    let mut i = 0;
    while i < entries.len() {
        let entry = entries[i];
        if entry.is_empty() {
            i += 1;
            continue;
        }

        let status_code = entry.get(..2).unwrap_or("");
        let filename = entry.get(3..).unwrap_or("");
        let is_deletion = status_code.contains('D');

        if is_deletion && !include_deletions {
            selection.skipped_deletions.push(filename.to_string());
            i += 1;
            continue;
        }

        if is_ralphx_generated_artifact_path(filename) && !is_deletion {
            selection
                .skipped_generated_artifacts
                .push(filename.to_string());
            i += if status_code.starts_with('R') || status_code.starts_with('C') {
                2
            } else {
                1
            };
            continue;
        }

        // Renames (R) and copies (C) have a second NUL-separated entry (old name)
        if status_code.starts_with('R') || status_code.starts_with('C') {
            selection.files_to_stage.push(filename.to_string());
            i += 2; // skip the old-name entry
            continue;
        }

        selection.files_to_stage.push(filename.to_string());
        i += 1;
    }

    selection
}

fn log_skipped_generated_artifacts(files: &[String]) {
    if !files.is_empty() {
        tracing::warn!(
            count = files.len(),
            files = ?files,
            "Skipped staging RalphX-generated artifact path(s)"
        );
    }
}

impl GitService {
    // =========================================================================
    // Commit Operations
    // =========================================================================

    /// Stage modified/new files (excluding deletions) and create a commit.
    ///
    /// SAFETY: This intentionally does NOT stage file deletions. Using `git add -A`
    /// in worktrees would stage deletions for every file absent from the worktree
    /// but present in the repo, causing catastrophic data loss on auto-commit.
    ///
    /// For merge conflict resolution where deletions are intentional, use
    /// `commit_all_including_deletions` instead.
    ///
    /// # Errors
    /// Returns `AppError::GitOperation` if git commands fail.
    pub async fn commit_all(path: &Path, message: &str) -> AppResult<Option<String>> {
        debug!(
            "Committing all changes in {:?} with message: {}",
            path, message
        );

        Self::stage_non_deletion_changes(path).await?;

        Self::commit_staged(path, message).await
    }

    /// Stage ALL changes including deletions and create a commit.
    ///
    /// Only use this for merge conflict resolution where the user has intentionally
    /// resolved conflicts (which may include file deletions).
    ///
    /// # Errors
    /// Returns `AppError::GitOperation` if git commands fail.
    pub async fn commit_all_including_deletions(
        path: &Path,
        message: &str,
    ) -> AppResult<Option<String>> {
        debug!(
            "Committing all changes (including deletions) in {:?} with message: {}",
            path, message
        );

        // Use git status --porcelain -z -uall for safe, .gitignore-respecting staging
        // (instead of `git add -A` which can stage build artifacts)
        let status_output = git_cmd::run(&["status", "--porcelain", "-z", "-uall"], path).await?;
        if !status_output.status.success() {
            let stderr = String::from_utf8_lossy(&status_output.stderr);
            return Err(AppError::GitOperation(format!(
                "Failed to get git status: {}",
                stderr
            )));
        }

        let stdout = String::from_utf8_lossy(&status_output.stdout);
        let selection = collect_stage_selection(&stdout, true);
        log_skipped_generated_artifacts(&selection.skipped_generated_artifacts);

        // Batch git add in chunks of 100
        for chunk in selection.files_to_stage.chunks(100) {
            let mut args: Vec<&str> = vec!["add", "--"];
            args.extend(chunk.iter().map(|s| s.as_str()));
            let add_output = git_cmd::run(&args, path).await?;
            if !add_output.status.success() {
                let stderr = String::from_utf8_lossy(&add_output.stderr);
                return Err(AppError::GitOperation(format!(
                    "Failed to stage batch: {}",
                    stderr
                )));
            }
        }

        Self::commit_staged(path, message).await
    }

    /// Stage modified and new files, skipping deletions.
    ///
    /// Uses `git status --porcelain -z -uall` for NUL-separated output that handles
    /// filenames with spaces, quotes, and special characters without quoting.
    async fn stage_non_deletion_changes(path: &Path) -> AppResult<()> {
        // -z: NUL-separated, no quoting — safe for all filenames
        let status_output = git_cmd::run(&["status", "--porcelain", "-z", "-uall"], path).await?;
        if !status_output.status.success() {
            let stderr = String::from_utf8_lossy(&status_output.stderr);
            return Err(AppError::GitOperation(format!(
                "Failed to get git status: {}",
                stderr
            )));
        }

        let stdout = String::from_utf8_lossy(&status_output.stdout);
        let selection = collect_stage_selection(&stdout, false);
        log_skipped_generated_artifacts(&selection.skipped_generated_artifacts);

        if !selection.skipped_deletions.is_empty() {
            tracing::warn!(
                count = selection.skipped_deletions.len(),
                files = ?selection.skipped_deletions,
                "Skipped staging {} deleted file(s) in auto-commit (safety: prevents worktree deletion propagation)",
                selection.skipped_deletions.len()
            );
        }

        for file in &selection.files_to_stage {
            let add_output = git_cmd::run(&["add", "--", file], path).await?;
            if !add_output.status.success() {
                let stderr = String::from_utf8_lossy(&add_output.stderr);
                tracing::warn!("Failed to stage file {}: {}", file, stderr);
            }
        }

        Ok(())
    }

    /// Commit whatever is currently staged, returning the SHA or None if nothing staged.
    async fn commit_staged(path: &Path, message: &str) -> AppResult<Option<String>> {
        if !Self::has_staged_changes(path).await? {
            debug!("No changes to commit");
            return Ok(None);
        }

        let commit_output = git_cmd::run(&["commit", "-m", message], path).await?;

        if !commit_output.status.success() {
            let stderr = String::from_utf8_lossy(&commit_output.stderr);
            return Err(AppError::GitOperation(format!(
                "Failed to commit: {}",
                stderr
            )));
        }

        let sha = Self::get_head_sha(path).await?;
        Ok(Some(sha))
    }

    /// Check if there are uncommitted changes in the working directory
    ///
    /// # Arguments
    /// * `path` - Path to the git repository or worktree
    pub async fn has_uncommitted_changes(path: &Path) -> AppResult<bool> {
        Ok(!Self::uncommitted_change_summary(path).await?.is_empty())
    }

    /// Return a short, human-readable summary of staged/unstaged/untracked changes.
    ///
    /// Uses `git status --porcelain=v1 -uall`, which respects `.gitignore` and
    /// includes untracked source files that would otherwise be lost if a task
    /// worktree were cleaned up before committing.
    pub async fn uncommitted_change_summary(path: &Path) -> AppResult<Vec<String>> {
        let output = git_cmd::run(&["status", "--porcelain=v1", "-uall"], path).await?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(AppError::GitOperation(format!(
                "Failed to check status: {}",
                stderr
            )));
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        Ok(stdout
            .lines()
            .filter_map(|line| {
                let trimmed = line.trim();
                if trimmed.is_empty() {
                    None
                } else {
                    Some(trimmed.to_string())
                }
            })
            .take(25)
            .collect())
    }

    /// Check if there are staged changes ready to commit
    async fn has_staged_changes(path: &Path) -> AppResult<bool> {
        let output = git_cmd::run(&["diff", "--cached", "--quiet"], path).await?;

        // Exit code 1 means there are differences (staged changes)
        Ok(!output.status.success())
    }

    /// Get the SHA of HEAD
    pub async fn get_head_sha(path: &Path) -> AppResult<String> {
        let output = git_cmd::run(&["rev-parse", "HEAD"], path).await?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(AppError::GitOperation(format!(
                "Failed to get HEAD SHA: {}",
                stderr
            )));
        }

        let sha = String::from_utf8_lossy(&output.stdout).trim().to_string();
        Ok(sha)
    }

    /// Get the SHA of a specific branch tip (without checking it out).
    pub async fn get_branch_sha(repo: &Path, branch: &str) -> AppResult<String> {
        let output = git_cmd::run(&["rev-parse", branch], repo).await?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(AppError::GitOperation(format!(
                "Failed to get SHA for branch {}: {}",
                branch, stderr
            )));
        }

        let sha = String::from_utf8_lossy(&output.stdout).trim().to_string();
        Ok(sha)
    }
}
