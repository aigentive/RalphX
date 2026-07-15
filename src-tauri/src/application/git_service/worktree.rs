use super::git_cmd;
use super::*;
use crate::utils::path_safety::validate_absolute_non_root_path;

impl GitService {
    /// Check for a local branch without collapsing Git failures into absence.
    pub async fn branch_exists_strict(repo_path: &Path, branch: &str) -> AppResult<bool> {
        if !Self::check_ref_format(repo_path, branch).await? {
            return Err(AppError::Validation(format!(
                "Invalid local branch name: '{branch}'"
            )));
        }

        let branch_ref = format!("refs/heads/{branch}");
        let output = git_cmd::run(
            &["show-ref", "--verify", "--quiet", branch_ref.as_str()],
            repo_path,
        )
        .await?;
        match output.status.code() {
            Some(0) => Ok(true),
            Some(1) => Ok(false),
            _ => Err(AppError::GitOperation(format!(
                "Failed to determine whether local branch '{branch}' exists: {}",
                String::from_utf8_lossy(&output.stderr)
            ))),
        }
    }

    // =========================================================================
    // Worktree Operations (Worktree mode only)
    // =========================================================================

    /// Create a new worktree with a new branch
    ///
    /// NOTE: This method creates parent directories if they don't exist.
    ///
    /// # Arguments
    /// * `repo` - Path to the main git repository
    /// * `worktree` - Path where the worktree should be created
    /// * `branch` - Name of the new branch to create in the worktree
    /// * `base` - Name of the base branch to branch from
    pub async fn create_worktree(
        repo: &Path,
        worktree: &Path,
        branch: &str,
        base: &str,
    ) -> AppResult<()> {
        debug!(
            "Creating worktree at {:?} with branch '{}' from '{}' in {:?}",
            worktree, branch, base, repo
        );

        // Ensure parent directory exists (per plan: auto-create if needed)
        if let Some(parent) = worktree.parent() {
            crate::utils::path_safety::validate_absolute_non_root_path(parent, "worktree parent")?;
            // codeql[rust/path-injection]
            std::fs::create_dir_all(parent).map_err(|e| {
                AppError::GitOperation(format!(
                    "Failed to create worktree parent directory {:?}: {}",
                    parent, e
                ))
            })?;
        }

        let args = [
            "worktree",
            "add",
            "-b",
            branch,
            worktree.to_str().unwrap_or_default(),
            base,
        ];
        let output = git_cmd::run(&args, repo).await?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);

            // Guard: stale locked entry blocks `git worktree add` with "missing but locked"
            // error. Unlock + prune to clear stale metadata, then retry once.
            //
            // Note: git may create the branch before detecting the locked path, so we
            // also delete any partially-created branch to allow a clean retry with -b.
            if stderr.contains("locked") {
                debug!(
                    "create_worktree: locked stale entry detected at {:?}, attempting unlock + prune",
                    worktree
                );
                let _ = git_cmd::run(
                    &["worktree", "unlock", worktree.to_str().unwrap_or_default()],
                    repo,
                )
                .await;
                let _ = git_cmd::run(&["worktree", "prune"], repo).await;
                // Clean up any partially-created branch from the failed first attempt
                // (ignore errors — branch may not have been created yet).
                let _ = git_cmd::run(&["branch", "-D", branch], repo).await;

                let retry = git_cmd::run(&args, repo).await?;
                if !retry.status.success() {
                    let retry_stderr = String::from_utf8_lossy(&retry.stderr);
                    return Err(AppError::GitOperation(format!(
                        "Failed to create worktree at {:?} after unlock+prune retry: {}",
                        worktree, retry_stderr
                    )));
                }
                return Ok(());
            }

            // Guard: branch already exists (race between branch_exists() check and create)
            // Retry without -b to checkout the existing branch instead.
            if stderr.contains("already exists") {
                debug!(
                    "create_worktree: branch '{}' already exists (race), retrying as checkout of existing branch",
                    branch
                );
                let checkout_args = [
                    "worktree",
                    "add",
                    worktree.to_str().unwrap_or_default(),
                    branch,
                ];
                let retry = git_cmd::run(&checkout_args, repo).await?;
                if !retry.status.success() {
                    let retry_stderr = String::from_utf8_lossy(&retry.stderr);
                    return Err(AppError::GitOperation(format!(
                        "Failed to create worktree at {:?} for branch '{}' after reference-exists retry: {}",
                        worktree, branch, retry_stderr
                    )));
                }
                return Ok(());
            }

            return Err(AppError::GitOperation(format!(
                "Failed to create worktree at {:?}: {}",
                worktree, stderr
            )));
        }

        Ok(())
    }

    /// Create a new branch worktree without deleting, moving, or adopting any
    /// competing path when Git reports a race or ownership conflict.
    pub async fn create_worktree_strict(
        repo: &Path,
        worktree: &Path,
        branch: &str,
        base: &str,
    ) -> AppResult<()> {
        if !Self::check_ref_format(repo, branch).await? {
            return Err(AppError::Validation(format!(
                "Invalid worktree branch name: '{branch}'"
            )));
        }
        let parent = worktree.parent().ok_or_else(|| {
            AppError::Validation("Strict worktree destination has no parent".to_string())
        })?;
        validate_absolute_non_root_path(parent, "worktree parent")?;
        // codeql[rust/path-injection]
        std::fs::create_dir_all(parent).map_err(|error| {
            AppError::GitOperation(format!(
                "Failed to create strict worktree parent directory: {error}"
            ))
        })?;

        let output = git_cmd::run(
            &[
                "worktree",
                "add",
                "-b",
                branch,
                worktree.to_str().unwrap_or_default(),
                base,
            ],
            repo,
        )
        .await?;
        if !output.status.success() {
            return Err(AppError::GitOperation(format!(
                "Failed to create strict worktree for branch '{branch}': {}",
                String::from_utf8_lossy(&output.stderr)
            )));
        }
        Ok(())
    }

    /// Delete a worktree
    ///
    /// # Arguments
    /// * `repo` - Path to the main git repository
    /// * `worktree` - Path of the worktree to delete
    pub async fn delete_worktree(repo: &Path, worktree: &Path) -> AppResult<()> {
        debug!("Deleting worktree at {:?} from {:?}", worktree, repo);

        // Unlock first (ignore errors — worktree may not be locked, or path may be gone).
        // This allows `git worktree prune` to clean up stale locked metadata entries.
        let _ = git_cmd::run(
            &["worktree", "unlock", worktree.to_str().unwrap_or_default()],
            repo,
        )
        .await;

        // Double-force (-f -f) overrides locks atomically (git 2.17+).
        // Single --force only bypasses dirty-tree checks, NOT lock markers.
        let output = git_cmd::run(
            &[
                "worktree",
                "remove",
                "-f",
                "-f",
                worktree.to_str().unwrap_or_default(),
            ],
            repo,
        )
        .await?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            warn!(
                "git worktree remove failed at {:?}: {}, trying rm -rf fallback",
                worktree, stderr
            );
            if crate::utils::path_safety::checked_exists(worktree, "worktree removal target")
                .unwrap_or(false)
            {
                if let Err(e) =
                    crate::utils::path_safety::checked_remove_dir_all(worktree, "worktree").await
                {
                    return Err(AppError::GitOperation(format!(
                        "Failed to remove stale worktree directory at '{}': {}",
                        worktree.to_string_lossy(),
                        e
                    )));
                }
            }
            // Always prune stale git metadata regardless of path existence
            let _ = git_cmd::run(&["worktree", "prune"], repo).await;
        }

        Ok(())
    }

    /// Move an owned worktree to a new, validated location.
    pub async fn move_worktree(repo: &Path, worktree: &Path, destination: &Path) -> AppResult<()> {
        let repo = validate_absolute_non_root_path(repo, "project checkout")?;
        let worktree = validate_absolute_non_root_path(worktree, "worktree")?;
        let destination = validate_absolute_non_root_path(destination, "worktree destination")?;
        let parent = destination
            .parent()
            .expect("validated non-root worktree destination has a parent");
        validate_absolute_non_root_path(parent, "worktree parent")?;
        // codeql[rust/path-injection]
        std::fs::create_dir_all(parent)
            .map_err(|error| AppError::GitOperation(error.to_string()))?;

        let output = git_cmd::run(
            &[
                "worktree",
                "move",
                worktree.to_str().unwrap_or_default(),
                destination.to_str().unwrap_or_default(),
            ],
            &repo,
        )
        .await?;
        if !output.status.success() {
            return Err(AppError::GitOperation(format!(
                "Failed to move worktree from '{}' to '{}': {}",
                worktree.display(),
                destination.display(),
                String::from_utf8_lossy(&output.stderr)
            )));
        }

        Ok(())
    }

    /// Create a worktree that checks out an existing branch (no new branch creation)
    ///
    /// Unlike `create_worktree` which uses `git worktree add -b <new_branch>`,
    /// this method uses `git worktree add <path> <existing_branch>` to check out
    /// a branch that already exists. Used for merge worktrees where we need to
    /// check out the target branch in an isolated directory.
    ///
    /// # Arguments
    /// * `repo` - Path to the main git repository
    /// * `worktree` - Path where the worktree should be created
    /// * `branch` - Name of the existing branch to check out
    pub async fn checkout_existing_branch_worktree(
        repo: &Path,
        worktree: &Path,
        branch: &str,
    ) -> AppResult<()> {
        debug!(
            "Creating worktree at {:?} checking out existing branch '{}' in {:?}",
            worktree, branch, repo
        );

        // Ensure parent directory exists
        if let Some(parent) = worktree.parent() {
            crate::utils::path_safety::validate_absolute_non_root_path(parent, "worktree parent")?;
            // codeql[rust/path-injection]
            std::fs::create_dir_all(parent).map_err(|e| {
                AppError::GitOperation(format!(
                    "Failed to create worktree parent directory {:?}: {}",
                    parent, e
                ))
            })?;
        }

        let args = [
            "worktree",
            "add",
            worktree.to_str().unwrap_or_default(),
            branch,
        ];
        let output = git_cmd::run(&args, repo).await?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);

            // Guard: stale locked entry blocks `git worktree add` with "missing but locked"
            // error. Unlock + prune to clear stale metadata, then retry once.
            if stderr.contains("locked") {
                debug!(
                    "checkout_existing_branch_worktree: locked stale entry at {:?}, attempting unlock + prune",
                    worktree
                );
                let _ = git_cmd::run(
                    &["worktree", "unlock", worktree.to_str().unwrap_or_default()],
                    repo,
                )
                .await;
                let _ = git_cmd::run(&["worktree", "prune"], repo).await;

                let retry = git_cmd::run(&args, repo).await?;
                if !retry.status.success() {
                    let retry_stderr = String::from_utf8_lossy(&retry.stderr);
                    return Err(AppError::GitOperation(format!(
                        "Failed to create worktree at {:?} for branch '{}' after unlock+prune retry: {}",
                        worktree, branch, retry_stderr
                    )));
                }
                return Ok(());
            }

            // Guard: branch is already checked out in another worktree (stale/orphan from prior run).
            // Extract the other worktree path from the error, delete it, prune, and retry.
            //
            // Git error formats (varies by version):
            //   "fatal: '<branch>' is already checked out at '<path>'"   (older git)
            //   "fatal: '<branch>' is already used by worktree at '<path>'"  (newer git)
            if stderr.contains("already checked out at")
                || stderr.contains("already used by worktree at")
            {
                debug!(
                    "checkout_existing_branch_worktree: branch '{}' already checked out, cleaning stale worktree",
                    branch
                );
                // Extract path from either format:
                //   "is already checked out at '<path>'"
                //   "is already used by worktree at '<path>'"
                let other_path = [
                    "is already checked out at '",
                    "is already used by worktree at '",
                ]
                .iter()
                .find_map(|marker| {
                    stderr.find(marker).and_then(|i| {
                        let rest = &stderr[i + marker.len()..];
                        rest.find('\'').map(|end| &rest[..end])
                    })
                });
                if let Some(other) = other_path {
                    let other_path = std::path::PathBuf::from(other);
                    let same_as_repo = {
                        // Canonicalization here is a safety comparison before any removal.
                        // codeql[rust/path-injection]
                        let repo_norm = repo.canonicalize().unwrap_or_else(|_| repo.to_path_buf());
                        // Canonicalization here is a safety comparison before any removal.
                        // codeql[rust/path-injection]
                        let other_norm = other_path
                            .canonicalize()
                            .unwrap_or_else(|_| other_path.clone());
                        other_norm == repo_norm
                    };

                    if same_as_repo {
                        return Err(AppError::GitOperation(format!(
                            "Refusing to remove primary repo path '{}' while creating worktree for branch '{}'",
                            other_path.display(),
                            branch
                        )));
                    }

                    debug!("Removing stale worktree at {:?} to free branch", other_path);
                    let _ = git_cmd::run(&["worktree", "unlock", other], repo).await;
                    let _ = git_cmd::run(&["worktree", "remove", "-f", "-f", other], repo).await;
                    // Also try tokio::fs::remove_dir_all as fallback for unregistered dirs
                    if crate::utils::path_safety::checked_exists(&other_path, "stale git worktree")
                        .unwrap_or(false)
                    {
                        let _ = crate::utils::path_safety::checked_remove_dir_all(
                            &other_path,
                            "stale git worktree",
                        )
                        .await;
                    }
                }
                let _ = git_cmd::run(&["worktree", "prune"], repo).await;

                let retry = git_cmd::run(&args, repo).await?;
                if !retry.status.success() {
                    let retry_stderr = String::from_utf8_lossy(&retry.stderr);
                    return Err(AppError::GitOperation(format!(
                        "Failed to create worktree at {:?} for branch '{}' after already-checked-out recovery: {}",
                        worktree, branch, retry_stderr
                    )));
                }
                return Ok(());
            }

            // Guard: target directory physically exists (e.g., from failed cleanup of a prior
            // run) but is not registered as a worktree. Git fails with "already exists".
            // Force-remove the stale directory, prune, then retry once.
            if stderr.contains("already exists") {
                debug!(
                    "checkout_existing_branch_worktree: target path {:?} already exists on disk, force-removing and retrying",
                    worktree
                );
                let _ = crate::utils::path_safety::checked_remove_dir_all(
                    worktree,
                    "existing worktree path",
                )
                .await;
                let _ = git_cmd::run(&["worktree", "prune"], repo).await;

                let retry = git_cmd::run(&args, repo).await?;
                if !retry.status.success() {
                    let retry_stderr = String::from_utf8_lossy(&retry.stderr);
                    return Err(AppError::GitOperation(format!(
                        "Failed to create worktree at {:?} for branch '{}' after already-exists cleanup retry: {}",
                        worktree, branch, retry_stderr
                    )));
                }
                return Ok(());
            }

            return Err(AppError::GitOperation(format!(
                "Failed to create worktree at {:?} for branch '{}': {}",
                worktree, branch, stderr
            )));
        }

        Ok(())
    }

    /// Create a worktree that checks out an existing branch without recovery cleanup.
    ///
    /// This is for user-selected branches where another active checkout may own
    /// the branch. Callers must preflight known worktrees and should surface git
    /// conflicts instead of deleting or moving another worktree.
    pub async fn checkout_existing_branch_worktree_strict(
        repo: &Path,
        worktree: &Path,
        branch: &str,
    ) -> AppResult<()> {
        debug!(
            "Creating strict worktree at {:?} checking out existing branch '{}' in {:?}",
            worktree, branch, repo
        );

        if let Some(parent) = worktree.parent() {
            crate::utils::path_safety::validate_absolute_non_root_path(parent, "worktree parent")?;
            // codeql[rust/path-injection]
            std::fs::create_dir_all(parent).map_err(|e| {
                AppError::GitOperation(format!(
                    "Failed to create worktree parent directory {:?}: {}",
                    parent, e
                ))
            })?;
        }

        let args = [
            "worktree",
            "add",
            worktree.to_str().unwrap_or_default(),
            branch,
        ];
        let output = git_cmd::run(&args, repo).await?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(AppError::GitOperation(format!(
                "Failed to create worktree at {:?} for branch '{}' without recovery cleanup: {}",
                worktree, branch, stderr
            )));
        }

        Ok(())
    }

    // =========================================================================
    // Worktree Query Operations
    // =========================================================================

    /// List all worktrees in the repository
    ///
    /// Runs `git worktree list --porcelain` and parses the output into
    /// structured `WorktreeInfo` entries.
    ///
    /// # Arguments
    /// * `repo` - Path to the git repository
    pub async fn list_worktrees(repo: &Path) -> AppResult<Vec<WorktreeInfo>> {
        debug!("Listing worktrees in {:?}", repo);

        let output = git_cmd::run(&["worktree", "list", "--porcelain"], repo).await?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(AppError::GitOperation(format!(
                "Failed to list worktrees: {}",
                stderr
            )));
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        Ok(Self::parse_worktree_porcelain(&stdout))
    }

    /// Parse `git worktree list --porcelain` output into `WorktreeInfo` entries
    ///
    /// Porcelain format outputs blocks separated by blank lines. Each block has:
    /// - `worktree <path>` (always present)
    /// - `HEAD <sha>` (absent for bare repos)
    /// - `branch refs/heads/<name>` (absent for detached HEAD or bare)
    /// - Optional flags: `bare`, `detached`, `prunable`
    pub(super) fn parse_worktree_porcelain(output: &str) -> Vec<WorktreeInfo> {
        let mut worktrees = Vec::new();
        let mut path: Option<String> = None;
        let mut head: Option<String> = None;
        let mut branch: Option<String> = None;

        for line in output.lines() {
            if line.is_empty() {
                // End of a worktree block — flush
                if let Some(p) = path.take() {
                    worktrees.push(WorktreeInfo {
                        path: p,
                        branch: branch.take(),
                        head: head.take(),
                    });
                }
                head = None;
                branch = None;
            } else if let Some(rest) = line.strip_prefix("worktree ") {
                path = Some(rest.to_string());
            } else if let Some(rest) = line.strip_prefix("HEAD ") {
                head = Some(rest.to_string());
            } else if let Some(rest) = line.strip_prefix("branch ") {
                // Strip refs/heads/ prefix to get the short branch name
                let short = rest.strip_prefix("refs/heads/").unwrap_or(rest);
                branch = Some(short.to_string());
            }
            // Ignore `bare`, `detached`, `prunable` lines
        }

        // Flush last block (porcelain output may not end with a blank line)
        if let Some(p) = path.take() {
            worktrees.push(WorktreeInfo {
                path: p,
                branch: branch.take(),
                head: head.take(),
            });
        }

        worktrees
    }

    /// Prune stale worktree references
    ///
    /// Runs `git worktree prune` to clean up stale worktree metadata entries
    /// that reference directories that no longer exist on disk.
    ///
    /// # Arguments
    /// * `repo` - Path to the git repository
    pub async fn prune_worktrees(repo: &Path) -> AppResult<()> {
        debug!("Pruning stale worktrees in {:?}", repo);

        let output = git_cmd::run(&["worktree", "prune"], repo).await?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            warn!("Failed to prune worktrees in {:?}: {}", repo, stderr);
            return Err(AppError::GitOperation(format!(
                "Failed to prune worktrees in '{}': {}",
                repo.to_string_lossy(),
                stderr.trim()
            )));
        }

        Ok(())
    }

    /// Convenience wrapper that runs the three stale-artifact cleanup steps in order:
    ///
    /// 1. Remove stale `.git/index.lock` (using `index_lock_stale_secs` from config)
    /// 2. Prune stale worktree metadata (`git worktree prune`)
    /// 3. Delete the task worktree directory if it exists and is not in use
    ///
    /// # Arguments
    /// * `repo` - Path to the main git repository
    /// * `task_worktree_path` - Explicit worktree path, or `None` to compute from `project`
    /// * `project` - Project used to compute the path when `task_worktree_path` is `None`
    /// * `task_id` - Task ID used to compute the path when `task_worktree_path` is `None`
    ///
    /// # Errors
    ///
    /// Returns an error if index.lock removal fails, pruning fails, or directory removal fails.
    pub async fn cleanup_stale_worktree_artifacts(
        repo: &Path,
        task_worktree_path: Option<&Path>,
        project: &crate::domain::entities::project::Project,
        task_id: &str,
    ) -> AppResult<()> {
        // Resolve worktree path: use provided or compute from project
        let computed;
        let worktree_path: &Path = match task_worktree_path {
            Some(p) => p,
            None => {
                computed = project.task_worktree_path(task_id);
                computed.as_path()
            }
        };

        // Get index_lock_stale_secs from config
        let index_lock_stale_secs =
            crate::infrastructure::agents::claude::git_runtime_config().index_lock_stale_secs;

        // Remove stale index.lock (sync)
        Self::remove_stale_index_lock(repo, index_lock_stale_secs)?;

        // Prune stale worktree metadata
        Self::prune_worktrees(repo).await?;

        // Delete worktree dir if it exists and is not in use
        if crate::utils::path_safety::checked_exists(worktree_path, "task worktree cleanup")
            .unwrap_or(false)
        {
            if crate::domain::services::worktree_guard::is_worktree_in_use(worktree_path) {
                warn!(
                    "Worktree {} in use, skipping deletion during cleanup",
                    worktree_path.display()
                );
            } else {
                crate::utils::path_safety::checked_remove_dir_all(
                    worktree_path,
                    "task worktree cleanup",
                )
                .await
                .map_err(|e| {
                    AppError::GitOperation(format!(
                        "Failed to remove stale worktree directory at '{}': {}",
                        worktree_path.display(),
                        e
                    ))
                })?;
            }
        }

        Ok(())
    }

    /// Remove a stale `.git/index.lock` file if it is older than `min_age_secs`.
    ///
    /// `index.lock` is created by git when it starts an index-modifying operation and
    /// removed when the operation completes. A lock file that is older than `min_age_secs`
    /// was left behind by a crashed or killed process and is safe to remove.
    ///
    /// Returns `Ok(true)` if the lock was removed, `Ok(false)` if it did not exist or
    /// was too young to remove, and `Err` only if removal was attempted but failed.
    ///
    /// # Arguments
    /// * `repo` - Path to the git repository (the directory containing `.git`)
    /// * `min_age_secs` - Minimum age in seconds before the lock is considered stale
    pub fn remove_stale_index_lock(repo: &Path, min_age_secs: u64) -> AppResult<bool> {
        let git_dir = Self::resolve_git_dir(repo);
        let lock_path = git_dir.join("index.lock");

        if !crate::utils::path_safety::checked_exists(&lock_path, "git index lock").unwrap_or(false)
        {
            return Ok(false);
        }

        crate::utils::path_safety::validate_absolute_non_root_path(&lock_path, "git index lock")?;
        // codeql[rust/path-injection]
        let metadata = std::fs::metadata(&lock_path).map_err(|e| {
            AppError::GitOperation(format!(
                "Failed to read index.lock metadata at '{}': {}",
                lock_path.display(),
                e
            ))
        })?;

        let age_secs = metadata
            .modified()
            .ok()
            .and_then(|mtime| mtime.elapsed().ok())
            .map(|dur| dur.as_secs())
            .unwrap_or(0);

        if age_secs < min_age_secs {
            debug!(
                "index.lock at '{}' is only {}s old (threshold: {}s) — skipping removal",
                lock_path.display(),
                age_secs,
                min_age_secs
            );
            return Ok(false);
        }

        crate::utils::path_safety::checked_remove_file(&lock_path, "git index lock").map_err(
            |e| {
                AppError::GitOperation(format!(
                    "Failed to remove stale index.lock at '{}': {}",
                    lock_path.display(),
                    e
                ))
            },
        )?;

        Ok(true)
    }
}
