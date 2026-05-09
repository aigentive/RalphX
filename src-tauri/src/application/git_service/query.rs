use super::git_cmd;
use super::*;

impl GitService {
    // =========================================================================
    // Query Operations
    // =========================================================================

    /// Get the number of commits on a branch
    ///
    /// # Arguments
    /// * `repo` - Path to the git repository
    /// * `branch` - Name of the branch to count commits on
    ///
    /// # Returns
    /// The number of commits on the branch
    pub async fn get_commit_count(repo: &Path, branch: &str) -> AppResult<u32> {
        debug!("Getting commit count for branch '{}' in {:?}", branch, repo);

        let output = git_cmd::run(&["rev-list", "--count", branch], repo).await?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(AppError::GitOperation(format!(
                "Failed to count commits on '{}': {}",
                branch, stderr
            )));
        }

        let count_str = String::from_utf8_lossy(&output.stdout).trim().to_string();
        let count = count_str.parse::<u32>().map_err(|e| {
            AppError::GitOperation(format!(
                "Failed to parse commit count '{}': {}",
                count_str, e
            ))
        })?;

        Ok(count)
    }

    /// Check if a commit is an ancestor of (or equal to) a branch head
    ///
    /// Uses `git merge-base --is-ancestor` to verify that commit_sha is reachable
    /// from the specified branch. This is useful for verifying that a merge
    /// actually happened on a target branch.
    ///
    /// # Arguments
    /// * `repo` - Path to the git repository
    /// * `commit_sha` - SHA of the commit to check
    /// * `branch` - Name of the branch to check against
    ///
    /// # Returns
    /// * `Ok(true)` if commit is on the branch (is an ancestor or equal)
    /// * `Ok(false)` if commit is not on the branch
    /// * `Err` if git command fails
    pub async fn is_commit_on_branch(
        repo: &Path,
        commit_sha: &str,
        branch: &str,
    ) -> AppResult<bool> {
        debug!(
            "Checking if commit {} is on branch '{}' in {:?}",
            commit_sha, branch, repo
        );

        let output =
            git_cmd::run(&["merge-base", "--is-ancestor", commit_sha, branch], repo).await?;

        // Exit code 0 = commit is ancestor (on branch), 1 = not ancestor (not on branch)
        // Other exit codes indicate errors
        match output.status.code() {
            Some(0) => Ok(true),
            Some(1) => Ok(false),
            Some(code) => {
                let stderr = String::from_utf8_lossy(&output.stderr);
                Err(AppError::GitOperation(format!(
                    "git merge-base failed with exit code {}: {}",
                    code, stderr
                )))
            }
            None => Err(AppError::GitOperation(
                "git merge-base was terminated by signal".to_string(),
            )),
        }
    }

    /// Get commits on the current branch since it diverged from base
    ///
    /// # Arguments
    /// * `path` - Path to the git repository or worktree
    /// * `base` - Name of the base branch
    pub async fn get_commits_since(path: &Path, base: &str) -> AppResult<Vec<CommitInfo>> {
        Self::get_commits_between(path, base, "HEAD").await
    }

    /// Get commits reachable from `head` that are not reachable from `base`.
    pub async fn get_commits_between(
        path: &Path,
        base: &str,
        head: &str,
    ) -> AppResult<Vec<CommitInfo>> {
        let range = format!("{}..{}", base, head);
        let output =
            git_cmd::run(&["log", &range, "--pretty=format:%H|%h|%s|%an|%aI"], path).await?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(AppError::GitOperation(format!(
                "Failed to get commits: {}",
                stderr
            )));
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        let commits: Vec<CommitInfo> = stdout
            .lines()
            .filter_map(|line| {
                let parts: Vec<&str> = line.split('|').collect();
                if parts.len() >= 5 {
                    Some(CommitInfo {
                        sha: parts[0].to_string(),
                        short_sha: parts[1].to_string(),
                        message: parts[2].to_string(),
                        author: parts[3].to_string(),
                        timestamp: parts[4].to_string(),
                    })
                } else {
                    None
                }
            })
            .collect();

        Ok(commits)
    }

    /// Get commits that were part of a merged task
    ///
    /// After a task is merged, the task branch/worktree is deleted, so we can't use
    /// `base..HEAD` from the working path. Instead, we query commits between the
    /// merge-base and the merge commit SHA.
    ///
    /// # Arguments
    /// * `repo` - Path to the git repository
    /// * `base_branch` - Name of the base branch
    /// * `merge_commit_sha` - SHA of the merge commit
    pub async fn get_merged_task_commits(
        repo: &Path,
        base_branch: &str,
        merge_commit_sha: &str,
    ) -> AppResult<Vec<CommitInfo>> {
        // For a merge commit:
        // - First parent (^1) is the base branch commit
        // - Second parent (^2) is the task branch tip
        //
        // For a non-merge recorded merge SHA, RalphX usually records the single
        // squash/rebase commit that represented the task. Use its direct parent
        // to avoid pulling in unrelated earlier plan/agent-branch commits when
        // the project base has moved independently.
        let check_output = git_cmd::run(
            &["rev-parse", "--verify", &format!("{}^2", merge_commit_sha)],
            repo,
        )
        .await?;

        let range = if check_output.status.success() {
            // True merge - get commits from first parent to second parent
            format!("{}^1..{}^2", merge_commit_sha, merge_commit_sha)
        } else {
            let parent_ref = format!("{}^", merge_commit_sha);
            let parent_output = git_cmd::run(&["rev-parse", "--verify", &parent_ref], repo).await?;
            if parent_output.status.success() {
                format!("{}..{}", parent_ref, merge_commit_sha)
            } else {
                // Root commit or unusual manual state: fall back to the older
                // merge-base behavior so the UI still has something to show.
                let merge_base_output =
                    git_cmd::run(&["merge-base", base_branch, merge_commit_sha], repo).await?;

                if !merge_base_output.status.success() {
                    format!("{}..{}", base_branch, merge_commit_sha)
                } else {
                    let merge_base = String::from_utf8_lossy(&merge_base_output.stdout)
                        .trim()
                        .to_string();
                    format!("{}..{}", merge_base, merge_commit_sha)
                }
            }
        };

        let mut output =
            git_cmd::run(&["log", &range, "--pretty=format:%H|%h|%s|%an|%aI"], repo).await?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(AppError::GitOperation(format!(
                "Failed to get merged commits: {}",
                stderr
            )));
        }

        if output.stdout.is_empty() {
            // Fallback: include the merge commit itself (e.g., manual resolution commit on base)
            let single_range = format!("{}^..{}", merge_commit_sha, merge_commit_sha);
            output = git_cmd::run(
                &["log", &single_range, "--pretty=format:%H|%h|%s|%an|%aI"],
                repo,
            )
            .await?;

            if !output.status.success() {
                let stderr = String::from_utf8_lossy(&output.stderr);
                return Err(AppError::GitOperation(format!(
                    "Failed to get merged commits: {}",
                    stderr
                )));
            }
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        let commits: Vec<CommitInfo> = stdout
            .lines()
            .filter_map(|line| {
                let parts: Vec<&str> = line.split('|').collect();
                if parts.len() >= 5 {
                    Some(CommitInfo {
                        sha: parts[0].to_string(),
                        short_sha: parts[1].to_string(),
                        message: parts[2].to_string(),
                        author: parts[3].to_string(),
                        timestamp: parts[4].to_string(),
                    })
                } else {
                    None
                }
            })
            .collect();

        Ok(commits)
    }

    /// Get diff statistics between current branch and base
    ///
    /// # Arguments
    /// * `path` - Path to the git repository or worktree
    /// * `base` - Name of the base branch
    pub async fn get_diff_stats(path: &Path, base: &str) -> AppResult<DiffStats> {
        // Get shortstat for summary
        let stat_output = git_cmd::run(&["diff", "--shortstat", base], path).await?;

        let stat_stdout = String::from_utf8_lossy(&stat_output.stdout);

        // Parse "N files changed, M insertions(+), K deletions(-)"
        let (files_changed, insertions, deletions) = Self::parse_shortstat(&stat_stdout);

        // Get list of changed files
        let name_output = git_cmd::run(&["diff", "--name-only", base], path).await?;

        let name_stdout = String::from_utf8_lossy(&name_output.stdout);
        let changed_files: Vec<String> = name_stdout
            .lines()
            .filter(|line| !line.is_empty())
            .map(String::from)
            .collect();

        Ok(DiffStats {
            files_changed,
            insertions,
            deletions,
            changed_files,
        })
    }

    pub async fn get_diff_stats_between(
        path: &Path,
        from_ref: &str,
        to_ref: &str,
    ) -> AppResult<DiffStats> {
        let range = format!("{from_ref}..{to_ref}");

        let stat_output = git_cmd::run(&["diff", "--shortstat", &range], path).await?;
        let stat_stdout = String::from_utf8_lossy(&stat_output.stdout);
        let (files_changed, insertions, deletions) = Self::parse_shortstat(&stat_stdout);

        let name_output = git_cmd::run(&["diff", "--name-only", &range], path).await?;
        let name_stdout = String::from_utf8_lossy(&name_output.stdout);
        let changed_files: Vec<String> = name_stdout
            .lines()
            .filter(|line| !line.is_empty())
            .map(String::from)
            .collect();

        Ok(DiffStats {
            files_changed,
            insertions,
            deletions,
            changed_files,
        })
    }

    /// Parse git shortstat output
    pub(super) fn parse_shortstat(output: &str) -> (u32, u32, u32) {
        let mut files = 0u32;
        let mut insertions = 0u32;
        let mut deletions = 0u32;

        // Format: " N files changed, M insertions(+), K deletions(-)"
        for part in output.split(',') {
            let part = part.trim();
            if part.contains("file") {
                if let Some(num_str) = part.split_whitespace().next() {
                    files = num_str.parse().unwrap_or(0);
                }
            } else if part.contains("insertion") {
                if let Some(num_str) = part.split_whitespace().next() {
                    insertions = num_str.parse().unwrap_or(0);
                }
            } else if part.contains("deletion") {
                if let Some(num_str) = part.split_whitespace().next() {
                    deletions = num_str.parse().unwrap_or(0);
                }
            }
        }

        (files, insertions, deletions)
    }

    /// Find a commit on `branch` whose message contains `grep_pattern`.
    ///
    /// Uses `git log --grep` (fixed-string match) and returns the SHA of the
    /// most recent matching commit, or `None` if no match is found.
    ///
    /// # Arguments
    /// * `repo`         - Path to the git repository
    /// * `grep_pattern` - Fixed string to search for in commit messages
    /// * `branch`       - Branch to search on
    pub async fn find_commit_by_message_grep(
        repo: &Path,
        grep_pattern: &str,
        branch: &str,
    ) -> AppResult<Option<String>> {
        let output = git_cmd::run(
            &[
                "log",
                "--fixed-strings",
                &format!("--grep={}", grep_pattern),
                "--format=%H",
                branch,
                "-1",
            ],
            repo,
        )
        .await?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(AppError::GitOperation(format!(
                "git log --grep failed: {}",
                stderr
            )));
        }

        let sha = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if sha.is_empty() {
            Ok(None)
        } else {
            Ok(Some(sha))
        }
    }

    /// Count commits reachable from `source` that are NOT reachable from `base`.
    ///
    /// Runs `git rev-list --count base..source`. A result of 0 means `source` has no
    /// unique commits beyond `base` — the branch never diverged or all its work is
    /// already incorporated in `base`.
    ///
    /// Used by `check_already_merged` to prevent a false positive where `source_sha`
    /// happens to be an ancestor of `target` (e.g. plan branch was never diverged from
    /// main) but no real merge ever took place.
    pub async fn count_commits_not_on_branch(
        repo: &Path,
        source: &str,
        base: &str,
    ) -> AppResult<u32> {
        let range = format!("{}..{}", base, source);
        let output = git_cmd::run(&["rev-list", "--count", &range], repo).await?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(AppError::GitOperation(format!(
                "Failed to count commits not on branch ({}): {}",
                range, stderr
            )));
        }

        let count_str = String::from_utf8_lossy(&output.stdout).trim().to_string();
        count_str.parse::<u32>().map_err(|e| {
            AppError::GitOperation(format!(
                "Failed to parse commit count '{}': {}",
                count_str, e
            ))
        })
    }
}
