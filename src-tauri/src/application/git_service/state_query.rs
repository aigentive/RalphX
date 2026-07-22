use super::git_cmd;
use super::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UnfinishedGitOperationState {
    pub merge_in_progress: bool,
    pub rebase_in_progress: bool,
}

impl UnfinishedGitOperationState {
    pub fn is_unfinished(self) -> bool {
        self.merge_in_progress || self.rebase_in_progress
    }
}

impl GitService {
    /// Get list of files with conflicts
    ///
    /// # Arguments
    /// * `repo` - Path to the git repository
    pub async fn get_conflict_files(repo: &Path) -> AppResult<Vec<PathBuf>> {
        let output = git_cmd::run(&["diff", "--name-only", "--diff-filter=U"], repo).await?;

        let stdout = String::from_utf8_lossy(&output.stdout);
        let files: Vec<PathBuf> = stdout
            .lines()
            .filter(|line| !line.is_empty())
            .map(PathBuf::from)
            .collect();

        Ok(files)
    }

    // =========================================================================
    // Merge State Detection (Phase 76 - Auto-completion)
    // =========================================================================

    /// Resolves the actual git directory for a worktree or repository.
    ///
    /// For regular repos, returns `worktree/.git`. For worktrees where `.git`
    /// is a file containing `gitdir: <path>`, follows the indirection.
    fn resolve_git_dir_checked(worktree: &Path) -> AppResult<PathBuf> {
        crate::utils::path_safety::validate_absolute_non_root_path(worktree, "git worktree")?;
        let git_path = worktree.join(".git");

        if crate::utils::path_safety::checked_is_file(&git_path, "git metadata")? {
            let content =
                crate::utils::path_safety::checked_read_to_string(&git_path, "git metadata")?;
            let path = content.strip_prefix("gitdir: ").ok_or_else(|| {
                AppError::Validation(format!(
                    "git metadata file has invalid gitdir syntax: {}",
                    git_path.display()
                ))
            })?;
            let git_dir = PathBuf::from(path.trim());
            if git_dir.as_os_str().is_empty() {
                return Err(AppError::Validation(format!(
                    "git metadata file has an empty gitdir: {}",
                    git_path.display()
                )));
            }
            let git_dir = if git_dir.is_absolute() {
                git_dir
            } else {
                worktree.join(git_dir)
            };
            return crate::utils::path_safety::validate_absolute_non_root_path(
                &git_dir,
                "resolved git directory",
            );
        }

        crate::utils::path_safety::validate_absolute_non_root_path(&git_path, "git metadata")
    }

    pub(super) fn resolve_git_dir(worktree: &Path) -> PathBuf {
        Self::resolve_git_dir_checked(worktree).unwrap_or_else(|_| worktree.join(".git"))
    }

    pub fn unfinished_operation_state(worktree: &Path) -> AppResult<UnfinishedGitOperationState> {
        let git_dir = Self::resolve_git_dir_checked(worktree)?;
        let merge_in_progress = crate::utils::path_safety::checked_exists(
            &git_dir.join("MERGE_HEAD"),
            "git merge state",
        )?;
        let rebase_merge = crate::utils::path_safety::checked_exists(
            &git_dir.join("rebase-merge"),
            "git rebase state",
        )?;
        let rebase_apply = crate::utils::path_safety::checked_exists(
            &git_dir.join("rebase-apply"),
            "git rebase state",
        )?;
        Ok(UnfinishedGitOperationState {
            merge_in_progress,
            rebase_in_progress: rebase_merge || rebase_apply,
        })
    }

    /// Check if a rebase is currently in progress
    ///
    /// Detects incomplete rebase by checking for `.git/rebase-merge` or `.git/rebase-apply`
    /// directories which exist while a rebase is paused (e.g., due to conflicts).
    ///
    /// # Arguments
    /// * `worktree` - Path to the git worktree or repository
    pub fn is_rebase_in_progress(worktree: &Path) -> bool {
        Self::unfinished_operation_state(worktree)
            .map(|state| state.rebase_in_progress)
            .unwrap_or(false)
    }

    /// Detects an incomplete `git merge` by checking for the MERGE_HEAD file.
    ///
    /// MERGE_HEAD exists when a merge has been started but not yet committed
    /// (e.g., the agent resolved conflicts but forgot `git merge --continue`).
    ///
    /// # Arguments
    /// * `worktree` - Path to the git worktree or repository
    pub fn is_merge_in_progress(worktree: &Path) -> bool {
        Self::unfinished_operation_state(worktree)
            .map(|state| state.merge_in_progress)
            .unwrap_or(false)
    }

    /// Collect changed file paths that are relevant for conflict-marker checks.
    ///
    /// We intentionally scope marker scanning to files involved in current index/worktree
    /// changes instead of all tracked files. This avoids false positives from committed
    /// docs/tests that intentionally contain marker-like strings.
    async fn collect_conflict_scan_candidates(worktree: &Path) -> AppResult<Vec<String>> {
        let mut seen = HashSet::new();
        let mut files = Vec::new();

        let commands: [&[&str]; 3] = [
            &["diff", "--name-only"],
            &["diff", "--cached", "--name-only"],
            &["diff", "--name-only", "--diff-filter=U"],
        ];

        for args in commands {
            let output = git_cmd::run(args, worktree).await?;

            if !output.status.success() {
                let stderr = String::from_utf8_lossy(&output.stderr);
                return Err(AppError::GitOperation(format!(
                    "Failed to list changed files via git {}: {}",
                    args.join(" "),
                    stderr
                )));
            }

            for line in String::from_utf8_lossy(&output.stdout).lines() {
                let file = line.trim();
                if file.is_empty() {
                    continue;
                }
                if seen.insert(file.to_string()) {
                    files.push(file.to_string());
                }
            }
        }

        Ok(files)
    }

    /// Check if a line is a git conflict-marker line.
    fn is_conflict_marker_line(line: &str) -> bool {
        line.starts_with("<<<<<<<")
            || line.starts_with(">>>>>>>")
            || line.starts_with("|||||||")
            || line == "======="
    }

    /// Check for conflict markers in changed files.
    ///
    /// Scans only changed files (unstaged/staged/unmerged) for git conflict markers.
    /// Returns true if any conflict markers are found.
    ///
    /// # Arguments
    /// * `worktree` - Path to the git worktree or repository
    pub async fn has_conflict_markers(worktree: &Path) -> AppResult<bool> {
        let candidate_files = Self::collect_conflict_scan_candidates(worktree).await?;
        for file in candidate_files {
            let file_path = worktree.join(&file);

            // Skip if file doesn't exist (could be deleted in working tree)
            if !crate::utils::path_safety::checked_exists(&file_path, "conflict marker scan")
                .unwrap_or(false)
            {
                continue;
            }

            // Skip binary files - only check text files
            if let Ok(content) = crate::utils::path_safety::checked_read_to_string(
                &file_path,
                "conflict marker scan",
            ) {
                if content.lines().any(Self::is_conflict_marker_line) {
                    debug!("Found conflict marker in file: {}", file);
                    return Ok(true);
                }
            }
        }

        Ok(false)
    }

    /// Classify a git merge error to determine if it's deferrable (branch/worktree contention)
    /// or terminal (true merge failure).
    ///
    /// Branch lock errors are deferrable and should keep the task in `pending_merge` for retry.
    /// Other errors are terminal and should transition to `merge_incomplete`.
    ///
    /// # Arguments
    /// * `error` - The error to classify
    ///
    /// # Returns
    /// `true` if the error is a deferrable branch lock error, `false` otherwise
    pub fn is_branch_lock_error(error: &AppError) -> bool {
        let error_msg = error.to_string().to_lowercase();

        // Match git error signatures for branch lock patterns
        error_msg.contains("already used by worktree")
            || error_msg.contains("already checked out")
            || error_msg.contains("is already checked out at")
            || error_msg.contains("branch is checked out")
    }

    /// Reset the current branch to a specific commit (hard reset)
    ///
    /// Used to revert a merge commit when post-merge validation fails.
    /// This discards the merge commit and restores the branch to its pre-merge state.
    ///
    /// # Arguments
    /// * `path` - Path to the git repository or worktree
    /// * `target` - The commit ref to reset to (e.g., "HEAD~1", a SHA)
    pub async fn reset_hard(path: &Path, target: &str) -> AppResult<()> {
        debug!("Hard resetting to '{}' in {:?}", target, path);

        let output = git_cmd::run(&["reset", "--hard", target], path).await?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(AppError::GitOperation(format!(
                "git reset --hard '{}' failed: {}",
                target, stderr
            )));
        }

        debug!("Hard reset to '{}' succeeded in {:?}", target, path);
        Ok(())
    }
}
