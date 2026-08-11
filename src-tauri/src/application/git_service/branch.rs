use super::git_cmd;
use super::*;

impl GitService {
    // =========================================================================
    // Branch Operations (both modes)
    // =========================================================================

    /// Create a new branch from a base branch
    ///
    /// # Arguments
    /// * `repo` - Path to the git repository
    /// * `branch` - Name of the new branch to create
    /// * `base` - Name of the base branch to branch from
    pub async fn create_branch(repo: &Path, branch: &str, base: &str) -> AppResult<()> {
        debug!("Creating branch '{}' from '{}' in {:?}", branch, base, repo);

        let output = git_cmd::run(&["branch", branch, base], repo).await?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(AppError::GitOperation(format!(
                "Failed to create branch '{}': {}",
                branch, stderr
            )));
        }

        Ok(())
    }

    /// Ensure a selected branch name exists locally, creating it from `origin/<branch>` if needed.
    pub async fn ensure_local_branch_from_origin_if_missing(
        repo: &Path,
        branch: &str,
    ) -> AppResult<String> {
        let trimmed = branch.trim();
        if trimmed.is_empty() {
            return Err(AppError::Validation(
                "Selected branch name cannot be empty".to_string(),
            ));
        }

        let local_branch = trimmed
            .strip_prefix("refs/remotes/origin/")
            .or_else(|| trimmed.strip_prefix("origin/"))
            .unwrap_or(trimmed);

        if Self::branch_exists(repo, local_branch).await? {
            return Ok(local_branch.to_string());
        }

        let remote_ref = format!("origin/{local_branch}");
        if !Self::ref_exists(repo, &remote_ref).await? {
            Self::fetch_origin(repo).await?;
        }

        if !Self::ref_exists(repo, &remote_ref).await? {
            return Err(AppError::Validation(format!(
                "Selected branch '{}' does not exist locally or on origin",
                local_branch
            )));
        }

        match Self::create_branch(repo, local_branch, &remote_ref).await {
            Ok(()) => Ok(local_branch.to_string()),
            Err(error) => {
                if Self::branch_exists(repo, local_branch).await? {
                    Ok(local_branch.to_string())
                } else {
                    Err(error)
                }
            }
        }
    }

    /// Detect a repository's default branch.
    ///
    /// Fallback chain matches the project settings UI:
    /// `origin/HEAD` -> `main` -> `master` -> first local branch.
    pub async fn detect_default_branch(repo: &Path) -> AppResult<String> {
        let origin_head = git_cmd::run(&["symbolic-ref", "refs/remotes/origin/HEAD"], repo).await?;
        if origin_head.status.success() {
            let stdout = String::from_utf8_lossy(&origin_head.stdout);
            if let Some(branch) = stdout.trim().strip_prefix("refs/remotes/origin/") {
                if !branch.is_empty() {
                    return Ok(branch.to_string());
                }
            }
        }

        for branch in ["main", "master"] {
            let ref_name = format!("refs/heads/{branch}");
            let output = git_cmd::run(&["rev-parse", "--verify", &ref_name], repo).await?;
            if output.status.success() {
                return Ok(branch.to_string());
            }
        }

        let output = git_cmd::run(&["branch", "--format=%(refname:short)"], repo).await?;
        if output.status.success() {
            let stdout = String::from_utf8_lossy(&output.stdout);
            if let Some(branch) = stdout
                .lines()
                .map(str::trim)
                .find(|branch| !branch.is_empty())
            {
                return Ok(branch.to_string());
            }
        }

        Err(AppError::GitOperation(
            "No branches found in repository".to_string(),
        ))
    }

    /// List local and remote branches, normalized to branch names with `main`/`master` first.
    pub async fn list_branches(repo: &Path) -> AppResult<Vec<String>> {
        let output = git_cmd::run(&["branch", "-a"], repo).await?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(AppError::GitOperation(format!(
                "git branch failed: {}",
                stderr
            )));
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        let branches: Vec<String> = stdout
            .lines()
            .filter_map(|line| {
                let trimmed = line.trim().trim_start_matches(&['*', '+'][..]).trim_start();
                if trimmed.is_empty() {
                    return None;
                }
                if let Some(remote_branch) = trimmed.strip_prefix("remotes/origin/") {
                    if remote_branch.starts_with("HEAD") {
                        return None;
                    }
                    Some(remote_branch.to_string())
                } else {
                    Some(trimmed.to_string())
                }
            })
            .collect::<HashSet<_>>()
            .into_iter()
            .collect();

        let mut sorted = branches;
        sorted.sort_by(|a, b| {
            let a_priority = if a == "main" || a == "master" { 0 } else { 1 };
            let b_priority = if b == "main" || b == "master" { 0 } else { 1 };
            a_priority.cmp(&b_priority).then(a.cmp(b))
        });

        Ok(sorted)
    }

    /// List local branch names without probing each branch individually.
    pub(crate) async fn list_local_branch_names(repo: &Path) -> AppResult<HashSet<String>> {
        let output = git_cmd::run(
            &["for-each-ref", "--format=%(refname:short)", "refs/heads"],
            repo,
        )
        .await?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(AppError::GitOperation(format!(
                "git for-each-ref failed: {}",
                stderr
            )));
        }

        Ok(String::from_utf8_lossy(&output.stdout)
            .lines()
            .map(str::trim)
            .filter(|branch| !branch.is_empty())
            .map(ToOwned::to_owned)
            .collect())
    }

    /// Resolve the default branch for project workflows.
    ///
    /// Explicit project settings win; otherwise this uses the same automatic
    /// detection chain as the Settings UI before falling back to `main`.
    pub async fn resolve_project_default_branch(
        repo: &Path,
        configured_base_branch: Option<&str>,
    ) -> String {
        if let Some(branch) = configured_base_branch
            .map(str::trim)
            .filter(|branch| !branch.is_empty())
        {
            return branch.to_string();
        }

        Self::detect_default_branch(repo)
            .await
            .unwrap_or_else(|_| "main".to_string())
    }

    /// Validate that a branch name is well-formed per `git check-ref-format`.
    ///
    /// Returns `Ok(true)` only when git accepts the name as a valid ref under
    /// `refs/heads/`. Rejects (returns `Ok(false)`) traversal-prone or malformed
    /// names such as `../escape`, names ending in `.lock`, names containing spaces,
    /// `..`, control chars, or a trailing `/`. This MUST be called before any branch
    /// name derived from external input (e.g. an issue key) is used as a git argument
    /// or filesystem ref component.
    ///
    /// A leading `-` is rejected up front so the candidate can never be parsed as a
    /// CLI flag, and the candidate is validated as a full `refs/heads/<branch>` ref
    /// to avoid the shorthand-resolution ambiguity of `--branch`.
    pub async fn check_ref_format(repo: &Path, branch: &str) -> AppResult<bool> {
        if branch.is_empty() || branch.starts_with('-') {
            return Ok(false);
        }
        let full_ref = format!("refs/heads/{branch}");
        Ok(git_cmd::run_status(&["check-ref-format", &full_ref], repo)
            .await
            .unwrap_or(false))
    }

    /// Create a new branch pointing at a specific commit SHA
    ///
    /// Unlike `create_branch` which branches from another branch name,
    /// this creates a branch at an exact commit. Used for conflict resolution
    /// worktrees after checkout-free merge detects conflicts.
    pub async fn create_branch_at(repo: &Path, branch: &str, sha: &str) -> AppResult<()> {
        debug!("Creating branch '{}' at {} in {:?}", branch, sha, repo);

        let output = git_cmd::run(&["branch", branch, sha], repo).await?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(AppError::GitOperation(format!(
                "Failed to create branch '{}' at {}: {}",
                branch, sha, stderr
            )));
        }

        Ok(())
    }

    /// Sync working tree to match the current branch HEAD
    ///
    /// Runs `git reset --hard HEAD` to atomically update all files.
    /// Used after checkout-free merge operations that advance the branch ref
    /// without touching the working tree.
    pub async fn hard_reset_to_head(repo: &Path) -> AppResult<()> {
        debug!("Resetting working tree to HEAD in {:?}", repo);

        let output = git_cmd::run(&["reset", "--hard", "HEAD"], repo).await?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(AppError::GitOperation(format!(
                "git reset --hard HEAD failed: {}",
                stderr
            )));
        }

        debug!("Working tree synced to HEAD");
        Ok(())
    }

    /// Ensure a clean working tree by resetting tracked files and removing untracked files/dirs.
    ///
    /// This method:
    /// 1. Checks `git status --porcelain` — early return if already clean
    /// 2. Runs `hard_reset_to_head()` to reset tracked files to HEAD state
    /// 3. Runs `git clean -fd` to remove untracked files and directories
    ///    (not `-fdx` — preserves .gitignore'd files like node_modules and src-tauri/target)
    /// 4. Logs dirty entries before cleaning (capped at 20 lines)
    ///
    /// # Arguments
    /// * `repo` - Path to the git repository
    pub async fn clean_working_tree(repo: &Path) -> AppResult<()> {
        debug!("Cleaning working tree in {:?}", repo);

        // Check if already clean (early return optimization)
        let status_output = git_cmd::run(&["status", "--porcelain"], repo).await?;

        if !status_output.status.success() {
            let stderr = String::from_utf8_lossy(&status_output.stderr);
            return Err(AppError::GitOperation(format!(
                "git status --porcelain failed: {}",
                stderr
            )));
        }

        let status_output_str = String::from_utf8_lossy(&status_output.stdout);

        // Early return if working tree is already clean
        if status_output_str.trim().is_empty() {
            debug!("Working tree is already clean");
            return Ok(());
        }

        // Log dirty entries (capped at 20 lines)
        let dirty_entries: Vec<&str> = status_output_str.lines().collect();
        let to_log: Vec<&&str> = if dirty_entries.len() > 20 {
            dirty_entries.iter().take(20).collect()
        } else {
            dirty_entries.iter().collect()
        };

        for entry in to_log {
            warn!("Dirty entry: {}", entry);
        }

        if dirty_entries.len() > 20 {
            warn!(
                "... and {} more dirty entries (total: {})",
                dirty_entries.len() - 20,
                dirty_entries.len()
            );
        }

        // Reset tracked files to HEAD state
        Self::hard_reset_to_head(repo).await?;

        // Remove untracked files and directories
        // Using -fd (not -fdx) to preserve .gitignore'd files
        let clean_output = git_cmd::run(&["clean", "-fd"], repo).await?;

        if !clean_output.status.success() {
            let stderr = String::from_utf8_lossy(&clean_output.stderr);
            return Err(AppError::GitOperation(format!(
                "git clean -fd failed: {}",
                stderr
            )));
        }

        debug!("Working tree cleaned successfully");
        Ok(())
    }

    /// Checkout an existing branch
    ///
    /// # Arguments
    /// * `repo` - Path to the git repository
    /// * `branch` - Name of the branch to checkout
    pub async fn checkout_branch(repo: &Path, branch: &str) -> AppResult<()> {
        debug!("Checking out branch '{}' in {:?}", branch, repo);

        let output = git_cmd::run(&["checkout", branch], repo).await?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(AppError::GitOperation(format!(
                "Failed to checkout branch '{}': {}",
                branch, stderr
            )));
        }

        Ok(())
    }

    /// Delete a branch
    ///
    /// # Arguments
    /// * `repo` - Path to the git repository
    /// * `branch` - Name of the branch to delete
    /// * `force` - If true, use -D (force delete), otherwise -d (safe delete)
    pub async fn delete_branch(repo: &Path, branch: &str, force: bool) -> AppResult<()> {
        debug!(
            "Deleting branch '{}' (force={}) in {:?}",
            branch, force, repo
        );

        let flag = if force { "-D" } else { "-d" };
        let output = git_cmd::run(&["branch", flag, branch], repo).await?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(AppError::GitOperation(format!(
                "Failed to delete branch '{}': {}",
                branch, stderr
            )));
        }

        Ok(())
    }

    /// Create a feature branch without checking it out.
    /// Used for plan group feature branches that isolate plan work.
    ///
    /// # Arguments
    /// * `repo_path` - Path to the git repository
    /// * `branch_name` - Name of the feature branch (e.g., "ralphx/my-app/plan-abc123")
    /// * `source_branch` - Branch to create from (e.g., "main")
    pub async fn create_feature_branch(
        repo_path: &Path,
        branch_name: &str,
        source_branch: &str,
    ) -> AppResult<()> {
        debug!(
            "Creating feature branch '{}' from '{}' in {:?}",
            branch_name, source_branch, repo_path
        );

        let output = git_cmd::run(&["branch", branch_name, source_branch], repo_path).await?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(AppError::GitOperation(format!(
                "Failed to create feature branch '{}': {}",
                branch_name, stderr
            )));
        }

        debug!("Feature branch '{}' created successfully", branch_name);
        Ok(())
    }

    /// Delete a feature branch after it has been merged.
    /// Uses force delete (-D) because feature branches may have been squash-merged,
    /// creating new commits that aren't merge ancestors (so -d would fail).
    ///
    /// # Arguments
    /// * `repo_path` - Path to the git repository
    /// * `branch_name` - Name of the feature branch to delete
    pub async fn delete_feature_branch(repo_path: &Path, branch_name: &str) -> AppResult<()> {
        debug!(
            "Deleting feature branch '{}' in {:?}",
            branch_name, repo_path
        );

        let output = git_cmd::run(&["branch", "-D", branch_name], repo_path).await?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(AppError::GitOperation(format!(
                "Failed to delete feature branch '{}': {}",
                branch_name, stderr
            )));
        }

        debug!("Feature branch '{}' deleted successfully", branch_name);
        Ok(())
    }

    /// Get the current branch name
    ///
    /// # Arguments
    /// * `repo` - Path to the git repository
    pub async fn get_current_branch(repo: &Path) -> AppResult<String> {
        let output = git_cmd::run(&["rev-parse", "--abbrev-ref", "HEAD"], repo).await?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(AppError::GitOperation(format!(
                "Failed to get current branch: {}",
                stderr
            )));
        }

        let branch = String::from_utf8_lossy(&output.stdout).trim().to_string();
        Ok(branch)
    }

    /// Check if a local branch exists in the repo.
    ///
    /// Uses `git rev-parse --verify refs/heads/{branch}` to check only local branches.
    /// Conservative failure mode: git errors (timeout, IO) return `Ok(false)` — callers
    /// use `.unwrap_or(false)` so failures safely skip operations that require the branch.
    pub async fn branch_exists(repo_path: &Path, branch: &str) -> AppResult<bool> {
        Ok(git_cmd::run_status(
            &["rev-parse", "--verify", &format!("refs/heads/{branch}")],
            repo_path,
        )
        .await
        .unwrap_or(false))
    }

    /// Check if any commit-ish git ref exists in the repo.
    ///
    /// Unlike `branch_exists`, this accepts remote-tracking refs such as
    /// `origin/main`, which are valid merge sources but not local branches.
    pub async fn ref_exists(repo_path: &Path, ref_name: &str) -> AppResult<bool> {
        let commit_ref = format!("{ref_name}^{{commit}}");
        Ok(git_cmd::run_status(
            &[
                "rev-parse",
                "--verify",
                "--quiet",
                "--end-of-options",
                &commit_ref,
            ],
            repo_path,
        )
        .await
        .unwrap_or(false))
    }

    /// Check if `commit` is an ancestor of `target` in the given repo.
    ///
    /// Uses `git merge-base --is-ancestor commit target`. Conservative failure mode:
    /// git errors (corrupt repo, invalid ref, timeout) collapse to `Ok(false)` — callers
    /// use `.unwrap_or(false)` so failures safely skip branch deletion.
    pub async fn is_ancestor(repo_path: &Path, commit: &str, target: &str) -> AppResult<bool> {
        let output =
            git_cmd::run(&["merge-base", "--is-ancestor", commit, target], repo_path).await;
        Ok(output.map_or(false, |o| o.status.success()))
    }

    /// Validate that both source and target branches exist before merge.
    /// Returns `Some(BranchNotFound)` if either is missing, `None` if both exist.
    pub(super) async fn validate_merge_branches(
        repo: &Path,
        source_branch: &str,
        target_branch: &str,
    ) -> Option<MergeAttemptResult> {
        if !Self::ref_exists(repo, source_branch).await.unwrap_or(false) {
            warn!("Source ref '{}' does not exist", source_branch);
            return Some(MergeAttemptResult::BranchNotFound {
                branch: source_branch.to_string(),
            });
        }
        if !Self::branch_exists(repo, target_branch)
            .await
            .unwrap_or(false)
        {
            warn!("Target branch '{}' does not exist", target_branch);
            return Some(MergeAttemptResult::BranchNotFound {
                branch: target_branch.to_string(),
            });
        }
        None
    }

    /// Check if two branches have identical content (tree-level diff).
    ///
    /// Uses `git diff --quiet` which exits 0 if identical, 1 if different.
    pub async fn branches_have_same_content(
        repo: &Path,
        branch_a: &str,
        branch_b: &str,
    ) -> AppResult<bool> {
        let output = git_cmd::run(&["diff", "--quiet", branch_a, branch_b], repo).await?;
        Ok(output.status.success())
    }

    /// Check if a source branch is safe to delete after merging into a target branch.
    ///
    /// Returns `(safe: bool, reason: &'static str)` where reason is one of:
    /// - `"ancestor"` — source is a git ancestor of target (normal merge)
    /// - `"content_match"` — not an ancestor but tree content is identical (clean squash merge)
    /// - `"content_differs"` — neither condition holds; deletion skipped (conflict-resolved squash
    ///   or branches have truly diverged)
    ///
    /// Conservative failure mode: git errors collapse to `false` via `unwrap_or(false)`.
    /// Callers add context-appropriate log messages using the returned reason string.
    pub async fn is_branch_merged_or_content_equivalent(
        repo: &Path,
        source_branch: &str,
        target_branch: &str,
    ) -> (bool, &'static str) {
        let is_anc = Self::is_ancestor(repo, source_branch, target_branch)
            .await
            .unwrap_or(false);
        if is_anc {
            return (true, "ancestor");
        }
        let same_content = Self::branches_have_same_content(repo, source_branch, target_branch)
            .await
            .unwrap_or(false);
        if same_content {
            (true, "content_match")
        } else {
            (false, "content_differs")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::GitService;
    use std::path::Path;
    use std::process::Command;

    fn git(repo: &Path, args: &[&str]) {
        let output = Command::new("git")
            .args(args)
            .current_dir(repo)
            .output()
            .expect("git should run");
        assert!(
            output.status.success(),
            "git {:?} failed: {}",
            args,
            String::from_utf8_lossy(&output.stderr)
        );
    }

    fn init_repo_with_origin() -> (tempfile::TempDir, std::path::PathBuf) {
        let temp = tempfile::tempdir().expect("tempdir");
        let origin = temp.path().join("origin.git");
        let repo = temp.path().join("repo");

        let output = Command::new("git")
            .args(["init", "--bare", origin.to_str().expect("origin path")])
            .output()
            .expect("git init bare should run");
        assert!(output.status.success());
        let output = Command::new("git")
            .args(["init", "-b", "main", repo.to_str().expect("repo path")])
            .output()
            .expect("git init should run");
        assert!(output.status.success());

        git(&repo, &["config", "user.email", "test@example.com"]);
        git(&repo, &["config", "user.name", "Test User"]);
        std::fs::write(repo.join("README.md"), "main\n").expect("write readme");
        git(&repo, &["add", "."]);
        git(&repo, &["commit", "-m", "initial"]);
        git(
            &repo,
            &["remote", "add", "origin", origin.to_str().unwrap()],
        );
        git(&repo, &["push", "-u", "origin", "main"]);

        (temp, repo)
    }

    #[tokio::test]
    async fn ensure_local_branch_returns_existing_branch_with_normalized_origin_prefix() {
        let (_temp, repo) = init_repo_with_origin();

        let branch = GitService::ensure_local_branch_from_origin_if_missing(
            &repo,
            "refs/remotes/origin/main",
        )
        .await
        .expect("existing branch should normalize");

        assert_eq!(branch, "main");
    }

    #[tokio::test]
    async fn ensure_local_branch_creates_missing_branch_from_origin_ref() {
        let (_temp, repo) = init_repo_with_origin();
        git(&repo, &["checkout", "-b", "feature/pr-head"]);
        std::fs::write(repo.join("feature.txt"), "feature\n").expect("write feature");
        git(&repo, &["add", "."]);
        git(&repo, &["commit", "-m", "feature"]);
        git(&repo, &["push", "origin", "feature/pr-head"]);
        git(&repo, &["checkout", "main"]);
        git(&repo, &["branch", "-D", "feature/pr-head"]);

        let branch =
            GitService::ensure_local_branch_from_origin_if_missing(&repo, "origin/feature/pr-head")
                .await
                .expect("missing branch should be created from origin");

        assert_eq!(branch, "feature/pr-head");
        assert!(GitService::branch_exists(&repo, "feature/pr-head")
            .await
            .expect("branch lookup should work"));
    }

    #[tokio::test]
    async fn ensure_local_branch_rejects_empty_or_unknown_branch() {
        let (_temp, repo) = init_repo_with_origin();

        let empty_error = GitService::ensure_local_branch_from_origin_if_missing(&repo, "   ")
            .await
            .expect_err("empty branch should fail");
        assert!(empty_error.to_string().contains("cannot be empty"));

        let missing_error =
            GitService::ensure_local_branch_from_origin_if_missing(&repo, "missing/pr")
                .await
                .expect_err("missing branch should fail");
        assert!(missing_error
            .to_string()
            .contains("does not exist locally or on origin"));
    }

    #[tokio::test]
    async fn check_ref_format_rejects_empty_branch() {
        let (_temp, repo) = init_repo_with_origin();
        assert!(!GitService::check_ref_format(&repo, "")
            .await
            .expect("check should run"));
    }

    #[tokio::test]
    async fn check_ref_format_rejects_leading_dash() {
        // A leading '-' must be rejected so it can never be parsed as a CLI flag.
        let (_temp, repo) = init_repo_with_origin();
        assert!(!GitService::check_ref_format(&repo, "-rf")
            .await
            .expect("check should run"));
    }

    #[tokio::test]
    async fn check_ref_format_rejects_spaces() {
        let (_temp, repo) = init_repo_with_origin();
        assert!(!GitService::check_ref_format(&repo, "has space")
            .await
            .expect("check should run"));
    }

    #[tokio::test]
    async fn check_ref_format_rejects_lock_suffix() {
        let (_temp, repo) = init_repo_with_origin();
        assert!(!GitService::check_ref_format(&repo, "feature.lock")
            .await
            .expect("check should run"));
    }

    #[tokio::test]
    async fn check_ref_format_rejects_path_traversal() {
        // "../escape" contains ".." which git rejects in a ref component, and the
        // full-ref validation prevents using it as a filesystem-traversal vector.
        let (_temp, repo) = init_repo_with_origin();
        assert!(!GitService::check_ref_format(&repo, "../escape")
            .await
            .expect("check should run"));
    }

    #[tokio::test]
    async fn check_ref_format_accepts_valid_branch() {
        let (_temp, repo) = init_repo_with_origin();
        assert!(GitService::check_ref_format(&repo, "feature/my-branch")
            .await
            .expect("check should run"));
    }
}
