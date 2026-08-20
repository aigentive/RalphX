use super::{git_cmd, GitService};
use crate::error::{AppError, AppResult};
use crate::utils::path_safety::validate_absolute_non_root_path;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};

const DEFAULT_BASE_BRANCH: &str = "main";
const BOOTSTRAP_COMMIT_MESSAGE: &str = "Initial commit (auto-created by RalphX)";
const BOOTSTRAP_AUTHOR_NAME: &str = "RalphX";
const BOOTSTRAP_AUTHOR_EMAIL: &str = "ralphx@localhost";

/// The optional base selected by a project-registration adapter.
#[derive(Debug, Clone, Default)]
pub struct GitBootstrapRequest {
    selected_base_branch: Option<String>,
}

impl GitBootstrapRequest {
    pub fn new(selected_base_branch: Option<String>) -> Self {
        Self {
            selected_base_branch,
        }
    }
}

/// Verified local Git state that is safe to persist on a project.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectGitBootstrap {
    pub base_branch: String,
}

impl GitService {
    /// Make a project repository usable for worktrees without rewriting user history.
    ///
    /// New repositories are initialized on the validated requested base. Existing
    /// unborn repositories retain their symbolic branch and receive only a
    /// command-local, empty root commit. Existing repositories with history are
    /// validated but never initialized, reset, rebased, or committed.
    pub async fn bootstrap_project_repository(
        repository: &Path,
        request: GitBootstrapRequest,
    ) -> AppResult<ProjectGitBootstrap> {
        let repository = validate_absolute_non_root_path(repository, "project working directory")?;
        if !repository.is_dir() {
            return Err(AppError::Validation(format!(
                "Project working directory does not exist: {}",
                repository.display()
            )));
        }

        let selected_base = request
            .selected_base_branch
            .as_deref()
            .map(str::trim)
            .map(|branch| {
                (!branch.is_empty())
                    .then(|| branch.to_string())
                    .ok_or_else(|| {
                        AppError::Validation("Selected base branch cannot be empty".to_string())
                    })
            })
            .transpose()?;

        let git_metadata_path = repository.join(".git");
        let git_metadata_exists = match std::fs::symlink_metadata(&git_metadata_path) {
            Ok(metadata) if metadata.is_dir() || metadata.is_file() => true,
            Ok(_) => {
                return Err(AppError::Validation(format!(
                    "Git metadata is invalid at {}: expected a directory or file",
                    git_metadata_path.display()
                )));
            }
            Err(error) if error.kind() == ErrorKind::NotFound => false,
            Err(error) => {
                return Err(AppError::GitOperation(format!(
                    "Failed to inspect project Git metadata at {}: {error}",
                    git_metadata_path.display()
                )));
            }
        };
        if git_metadata_exists {
            ensure_repository_root(&repository, &git_metadata_path).await?;
        } else {
            let requested_base = selected_base
                .as_deref()
                .unwrap_or(DEFAULT_BASE_BRANCH)
                .to_string();
            validate_branch_name(&repository, &requested_base).await?;
            ensure_success(
                git_cmd::run(&["init", "--initial-branch", &requested_base], &repository).await?,
                "initialize project repository",
            )?;
        }

        let current_symbolic_branch = symbolic_branch(&repository).await?;
        let requested_base = selected_base.unwrap_or_else(|| current_symbolic_branch.clone());
        validate_branch_name(&repository, &requested_base).await?;
        let head_exists = git_cmd::run(
            &["rev-parse", "--verify", "--quiet", "HEAD^{commit}"],
            &repository,
        )
        .await?
        .status
        .success();

        let base_branch = if head_exists {
            ensure_branch_exists(&repository, &requested_base).await?;
            requested_base
        } else {
            create_empty_root_commit(&repository).await?;
            current_symbolic_branch
        };

        // Persistence is allowed only after both the symbolic branch and HEAD
        // have been verified after every bootstrap path.
        let verified_branch = symbolic_branch(&repository).await?;
        ensure_head_exists(&repository).await?;
        if !head_exists && verified_branch != base_branch {
            return Err(AppError::GitOperation(format!(
                "Git symbolic branch changed during bootstrap: expected {base_branch}, found {verified_branch}"
            )));
        }

        Ok(ProjectGitBootstrap { base_branch })
    }
}

async fn ensure_repository_root(repository: &Path, git_metadata_path: &Path) -> AppResult<()> {
    let canonical_repository = repository.canonicalize().map_err(|error| {
        AppError::GitOperation(format!(
            "Failed to canonicalize project working directory {}: {error}",
            repository.display()
        ))
    })?;

    let canonical_top_level = repository_top_level(repository).await?.ok_or_else(|| {
        AppError::Validation(format!(
            "Git metadata exists but is invalid at {}",
            git_metadata_path.display()
        ))
    })?;
    if canonical_top_level != canonical_repository {
        return Err(AppError::Validation(format!(
            "Git metadata at {} belongs to a different repository root",
            git_metadata_path.display()
        )));
    }

    Ok(())
}

/// Resolve the canonical repository root that owns `directory`, or `None` when
/// the directory is not inside a Git work tree.
///
/// Shared by bootstrap's own root check and the project-candidate probe, which
/// needs the root itself to tell "this *is* a repository" apart from "this sits
/// inside someone else's repository".
///
/// # Errors
/// Returns `Err` when Git reports a root that cannot be decoded or canonicalized.
pub(crate) async fn repository_top_level(directory: &Path) -> AppResult<Option<PathBuf>> {
    let output = git_cmd::run(&["rev-parse", "--show-toplevel"], directory).await?;
    if !output.status.success() {
        return Ok(None);
    }

    let top_level = String::from_utf8(output.stdout).map_err(|error| {
        AppError::GitOperation(format!(
            "Git reported a non-UTF-8 repository root for {}: {error}",
            directory.display()
        ))
    })?;
    let top_level = top_level.trim_end_matches(['\r', '\n']);
    let top_level = validate_absolute_non_root_path(Path::new(top_level), "Git repository root")?;
    let canonical_top_level = top_level.canonicalize().map_err(|error| {
        AppError::GitOperation(format!(
            "Failed to canonicalize Git repository root {}: {error}",
            top_level.display()
        ))
    })?;

    Ok(Some(canonical_top_level))
}

async fn validate_branch_name(repository: &Path, branch: &str) -> AppResult<()> {
    let output = git_cmd::run(&["check-ref-format", "--branch", branch], repository).await?;
    if output.status.success() {
        Ok(())
    } else {
        Err(AppError::Validation(format!(
            "Selected base branch '{branch}' is not a valid local branch name"
        )))
    }
}

/// Read the repository's current symbolic branch, or `None` on a detached HEAD.
///
/// The probe surfaces detached HEAD as its own verdict rather than an error, so
/// it needs the unopinionated read; bootstrap keeps the strict wrapper below.
///
/// # Errors
/// Returns `Err` when the underlying Git command cannot run.
pub(crate) async fn symbolic_branch_opt(repository: &Path) -> AppResult<Option<String>> {
    let output = git_cmd::run(&["symbolic-ref", "--quiet", "--short", "HEAD"], repository).await?;
    if !output.status.success() {
        return Ok(None);
    }
    let branch = String::from_utf8_lossy(&output.stdout).trim().to_string();
    Ok((!branch.is_empty()).then_some(branch))
}

async fn symbolic_branch(repository: &Path) -> AppResult<String> {
    symbolic_branch_opt(repository).await?.ok_or_else(|| {
        AppError::Validation(
            "Project repository must have a symbolic branch; detached HEAD is unsupported"
                .to_string(),
        )
    })
}

async fn ensure_branch_exists(repository: &Path, branch: &str) -> AppResult<()> {
    let reference = format!("refs/heads/{branch}^{{commit}}");
    let output = git_cmd::run(
        &[
            "rev-parse",
            "--verify",
            "--quiet",
            "--end-of-options",
            &reference,
        ],
        repository,
    )
    .await?;
    if output.status.success() {
        Ok(())
    } else {
        Err(AppError::Validation(format!(
            "Selected base branch '{branch}' does not exist in the repository"
        )))
    }
}

async fn create_empty_root_commit(repository: &Path) -> AppResult<()> {
    let identity = [
        ("GIT_AUTHOR_NAME", BOOTSTRAP_AUTHOR_NAME),
        ("GIT_AUTHOR_EMAIL", BOOTSTRAP_AUTHOR_EMAIL),
        ("GIT_COMMITTER_NAME", BOOTSTRAP_AUTHOR_NAME),
        ("GIT_COMMITTER_EMAIL", BOOTSTRAP_AUTHOR_EMAIL),
    ];
    ensure_success(
        git_cmd::run_with_env(
            &[
                "commit",
                "--allow-empty",
                "--only",
                "--no-verify",
                "--no-gpg-sign",
                "-m",
                BOOTSTRAP_COMMIT_MESSAGE,
            ],
            repository,
            &identity,
        )
        .await?,
        "create RalphX bootstrap commit",
    )
}

async fn ensure_head_exists(repository: &Path) -> AppResult<()> {
    let output = git_cmd::run(
        &["rev-parse", "--verify", "--quiet", "HEAD^{commit}"],
        repository,
    )
    .await?;
    if output.status.success() {
        Ok(())
    } else {
        Err(AppError::GitOperation(
            "Project repository has no verified HEAD after bootstrap".to_string(),
        ))
    }
}

fn ensure_success(output: std::process::Output, operation: &str) -> AppResult<()> {
    if output.status.success() {
        Ok(())
    } else {
        Err(AppError::GitOperation(format!(
            "Failed to {operation}: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        )))
    }
}
