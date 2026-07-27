use super::{git_cmd, GitService};
use crate::domain::entities::GitTargetIdentity;
use crate::error::{AppError, AppResult};
use std::path::Path;

impl GitService {
    /// Verify that Git can resolve an author identity before any staging mutation.
    pub async fn ensure_commit_identity(repo: &Path) -> AppResult<()> {
        let output = git_cmd::run(&["var", "GIT_AUTHOR_IDENT"], repo).await?;
        if output.status.success() {
            return Ok(());
        }

        Err(AppError::GitOperation(format!(
            "Git commit identity is not configured: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        )))
    }

    pub async fn resolve_ref_sha(repo: &Path, reference: &str) -> AppResult<String> {
        let output = git_cmd::run(&["rev-parse", "--verify", reference], repo).await?;
        if !output.status.success() {
            return Err(AppError::GitOperation(format!(
                "failed to resolve Git ref {reference}: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            )));
        }
        let sha = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if sha.len() != 40 || !sha.chars().all(|ch| ch.is_ascii_hexdigit()) {
            return Err(AppError::GitOperation(format!(
                "Git ref {reference} did not resolve to a full commit SHA"
            )));
        }
        Ok(sha)
    }

    /// Resolve the process-independent authority key for a local branch target.
    ///
    /// Linked worktrees have distinct administrative directories but share one
    /// common Git directory and ref namespace. Canonicalizing Git's reported
    /// common directory makes every worktree converge on the same lease key.
    pub async fn canonical_target_identity(
        repo: &Path,
        branch: &str,
    ) -> AppResult<GitTargetIdentity> {
        let output = git_cmd::run(&["rev-parse", "--git-common-dir"], repo).await?;
        if !output.status.success() {
            return Err(AppError::GitOperation(format!(
                "failed to resolve Git common directory for {}: {}",
                repo.display(),
                String::from_utf8_lossy(&output.stderr).trim()
            )));
        }
        let reported = String::from_utf8(output.stdout)
            .map_err(|error| AppError::GitOperation(format!("invalid Git path output: {error}")))?;
        let reported = Path::new(reported.trim());
        let candidate = if reported.is_absolute() {
            reported.to_path_buf()
        } else {
            repo.join(reported)
        };
        let common_dir = tokio::fs::canonicalize(&candidate).await.map_err(|error| {
            AppError::GitOperation(format!(
                "failed to canonicalize Git common directory {}: {error}",
                candidate.display()
            ))
        })?;
        let full_ref = if branch.starts_with("refs/") {
            branch.to_string()
        } else {
            format!("refs/heads/{branch}")
        };
        GitTargetIdentity::new(common_dir, full_ref)
            .map_err(|error| AppError::Validation(error.to_string()))
    }
}
