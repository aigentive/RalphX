//! Preflight inspection for the project-creation wizard.
//!
//! The wizard asks "what is actually at this path?" before it offers to register
//! anything, so every verdict here is read-only. The single exception is
//! [`prepare_new_project_directory`], which exists because
//! `GitService::bootstrap_project_repository` refuses a directory that does not
//! exist yet and the frontend must never create directories itself.

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use tauri::State;

use crate::application::agent_conversation_workspace::expand_worktree_parent_public;
use crate::application::git_service::{repository_top_level, symbolic_branch_opt};
use crate::application::{AppState, GitService};
use crate::domain::entities::Project;
use crate::error::{AppError, AppResult};
use crate::infrastructure::git_auth::{inspect_repository_capability, RepositoryCapability};
use crate::utils::path_safety::{
    checked_read_dir, require_under_root, validate_absolute_non_root_path,
};

/// A project already registered at the inspected path.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct RegisteredProjectRef {
    pub id: String,
    pub name: String,
}

/// What RalphX found at a candidate project path.
///
/// Serialized with an external `kind` tag so the wizard can render one card per
/// verdict, mirroring [`RepositoryCapability`].
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ProjectCandidate {
    /// Nothing exists at the path.
    NotFound,
    /// The path exists but is a file, symlink to a file, or similar.
    NotADirectory,
    /// An empty directory that is not inside any repository.
    EmptyDirectory,
    /// A directory with contents that is not a repository and is not inside one.
    NonEmptyNonRepo { entry_count: usize },
    /// The path sits inside a repository whose root is elsewhere.
    NestedInRepository { repository_root: String },
    /// A repository root with no symbolic branch.
    DetachedHead { repository_root: String },
    /// A usable repository root.
    Repository {
        repository_root: String,
        current_branch: String,
        default_branch: Option<String>,
        branches: Vec<String>,
        has_commits: bool,
        is_dirty: bool,
        capability: RepositoryCapability,
        already_registered_as: Option<RegisteredProjectRef>,
    },
    /// Inspection itself failed. The wizard degrades to "you can still continue".
    InspectionFailed { message: String },
}

/// Whether a chosen worktree parent folder will actually work.
///
/// Checked in the dialog so a bad choice fails now, not at first task execution.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(tag = "verdict", rename_all = "snake_case")]
pub enum WorktreeParentVerdict {
    Ok {
        path: String,
    },
    NotFound {
        path: String,
    },
    NotADirectory {
        path: String,
    },
    /// The parent sits inside the repository itself, which would make task
    /// worktrees show up as untracked changes in the project.
    InsideRepository {
        path: String,
    },
    /// Best-effort permission read; advisory rather than authoritative, since
    /// only an actual write proves writability.
    NotWritable {
        path: String,
    },
    Invalid {
        message: String,
    },
}

/// Result of preparing the destination directory for the Create New intent.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PreparedProjectDirectory {
    pub path: String,
    /// `true` when RalphX created the directory, which is what authorizes
    /// [`discard_prepared_project_directory`] to remove it again on rollback.
    pub created: bool,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PrepareNewProjectDirectoryInput {
    pub parent_directory: String,
    pub folder_name: String,
}

/// Inspect a candidate project path without modifying anything.
///
/// # Errors
/// Never returns `Err`; inspection problems become
/// [`ProjectCandidate::InspectionFailed`] so the wizard can keep going.
#[tauri::command]
pub async fn inspect_project_candidate(
    path: String,
    state: State<'_, AppState>,
) -> Result<ProjectCandidate, String> {
    let projects = state.project_repo.get_all().await.unwrap_or_default();
    Ok(inspect_candidate_path(Path::new(&path), &projects).await)
}

/// Create (or accept) the destination directory for a brand-new project.
///
/// # Errors
/// Returns `Err` when the folder name is unsafe, the parent is missing, or an
/// existing directory at the destination already has contents.
#[tauri::command]
pub async fn prepare_new_project_directory(
    input: PrepareNewProjectDirectoryInput,
) -> Result<PreparedProjectDirectory, String> {
    prepare_project_directory(&input.parent_directory, &input.folder_name)
        .await
        .map_err(|error| error.to_string())
}

/// Roll back a directory created by [`prepare_new_project_directory`] when the
/// subsequent `create_project` call failed.
///
/// Refuses anything the user could have put there: only an empty directory, or
/// one holding nothing but the `.git` that bootstrap just initialized, is removed.
///
/// # Errors
/// Returns `Err` when the path is unsafe, has unexpected contents, or removal fails.
#[tauri::command]
pub async fn discard_prepared_project_directory(path: String) -> Result<(), String> {
    discard_project_directory(Path::new(&path))
        .await
        .map_err(|error| error.to_string())
}

/// Check a worktree parent folder without creating anything.
///
/// # Errors
/// Never returns `Err`; problems come back as a verdict.
#[tauri::command]
pub async fn validate_worktree_parent(
    path: String,
    repository_root: Option<String>,
) -> Result<WorktreeParentVerdict, String> {
    Ok(worktree_parent_verdict(&path, repository_root.as_deref()))
}

pub(crate) fn worktree_parent_verdict(
    path: &str,
    repository_root: Option<&str>,
) -> WorktreeParentVerdict {
    // Reuses the same tilde expansion the workspace layer applies, so the
    // dialog validates exactly what execution will later resolve.
    let expanded = match expand_worktree_parent_public(path.trim()) {
        Ok(expanded) => expanded,
        Err(error) => {
            return WorktreeParentVerdict::Invalid {
                message: error.to_string(),
            }
        }
    };
    let expanded = match validate_absolute_non_root_path(&expanded, "worktree parent") {
        Ok(expanded) => expanded,
        Err(error) => {
            return WorktreeParentVerdict::Invalid {
                message: error.to_string(),
            }
        }
    };
    let display = expanded.display().to_string();

    if !expanded.exists() {
        return WorktreeParentVerdict::NotFound { path: display };
    }
    if !expanded.is_dir() {
        return WorktreeParentVerdict::NotADirectory { path: display };
    }
    if let Some(root) = repository_root
        .map(str::trim)
        .filter(|root| !root.is_empty())
    {
        if is_inside(&expanded, Path::new(root)) {
            return WorktreeParentVerdict::InsideRepository { path: display };
        }
    }
    if std::fs::metadata(&expanded).is_ok_and(|metadata| metadata.permissions().readonly()) {
        return WorktreeParentVerdict::NotWritable { path: display };
    }

    WorktreeParentVerdict::Ok { path: display }
}

/// Canonicalized containment check, so a symlinked parent cannot hide the fact
/// that it really lives inside the repository.
fn is_inside(candidate: &Path, root: &Path) -> bool {
    match (candidate.canonicalize(), root.canonicalize()) {
        (Ok(candidate), Ok(root)) => candidate.starts_with(root),
        _ => candidate.starts_with(root),
    }
}

pub(crate) async fn inspect_candidate_path(path: &Path, projects: &[Project]) -> ProjectCandidate {
    match inspect_candidate_path_inner(path, projects).await {
        Ok(candidate) => candidate,
        Err(error) => ProjectCandidate::InspectionFailed {
            message: error.to_string(),
        },
    }
}

async fn inspect_candidate_path_inner(
    path: &Path,
    projects: &[Project],
) -> AppResult<ProjectCandidate> {
    let safe_path = validate_absolute_non_root_path(path, "project candidate")?;
    if !safe_path.exists() {
        return Ok(ProjectCandidate::NotFound);
    }
    if !safe_path.is_dir() {
        return Ok(ProjectCandidate::NotADirectory);
    }

    let Some(repository_root) = repository_top_level(&safe_path).await? else {
        return non_repository_verdict(&safe_path).await;
    };

    let canonical_path = safe_path.canonicalize().map_err(|error| {
        AppError::Infrastructure(format!(
            "Failed to resolve {}: {error}",
            safe_path.display()
        ))
    })?;
    if canonical_path != repository_root {
        return Ok(ProjectCandidate::NestedInRepository {
            repository_root: repository_root.display().to_string(),
        });
    }

    let root_display = repository_root.display().to_string();
    let Some(current_branch) = symbolic_branch_opt(&safe_path).await? else {
        return Ok(ProjectCandidate::DetachedHead {
            repository_root: root_display,
        });
    };

    Ok(ProjectCandidate::Repository {
        repository_root: root_display,
        current_branch,
        // A brand-new repository has no branches to detect a default from; that
        // is a normal state, not an inspection failure.
        default_branch: GitService::detect_default_branch(&safe_path).await.ok(),
        branches: GitService::list_branches(&safe_path)
            .await
            .unwrap_or_default(),
        has_commits: GitService::get_head_sha(&safe_path).await.is_ok(),
        is_dirty: GitService::has_uncommitted_changes(&safe_path)
            .await
            .unwrap_or(false),
        capability: inspect_repository_capability(&safe_path).await,
        already_registered_as: registered_project_at(&repository_root, projects),
    })
}

async fn non_repository_verdict(path: &Path) -> AppResult<ProjectCandidate> {
    let mut entries = checked_read_dir(path, "project candidate").await?;
    let mut entry_count = 0usize;
    while entries
        .next_entry()
        .await
        .map_err(|error| {
            AppError::Infrastructure(format!("Failed to read {}: {error}", path.display()))
        })?
        .is_some()
    {
        entry_count += 1;
    }

    Ok(if entry_count == 0 {
        ProjectCandidate::EmptyDirectory
    } else {
        ProjectCandidate::NonEmptyNonRepo { entry_count }
    })
}

/// Match a repository root against registered projects.
///
/// Compares canonicalized paths because macOS filesystems are case-insensitive,
/// so a raw string compare would let the same repository be registered twice.
fn registered_project_at(
    repository_root: &Path,
    projects: &[Project],
) -> Option<RegisteredProjectRef> {
    projects
        .iter()
        .find(|project| {
            Path::new(&project.working_directory)
                .canonicalize()
                .is_ok_and(|canonical| canonical == repository_root)
        })
        .map(|project| RegisteredProjectRef {
            id: project.id.as_str().to_string(),
            name: project.name.clone(),
        })
}

pub(crate) async fn prepare_project_directory(
    parent_directory: &str,
    folder_name: &str,
) -> AppResult<PreparedProjectDirectory> {
    let destination = resolve_new_project_directory(parent_directory, folder_name)?;

    if !destination.exists() {
        // codeql[rust/path-injection]
        std::fs::create_dir(&destination).map_err(|error| {
            AppError::Infrastructure(format!(
                "Failed to create {}: {error}",
                destination.display()
            ))
        })?;
        return Ok(PreparedProjectDirectory {
            path: destination.display().to_string(),
            created: true,
        });
    }

    if !destination.is_dir() {
        return Err(AppError::Validation(format!(
            "A file already exists at {}",
            destination.display()
        )));
    }
    if !is_directory_empty(&destination).await? {
        return Err(AppError::Validation(format!(
            "{} already has contents. Use Add Existing Repository instead.",
            destination.display()
        )));
    }

    Ok(PreparedProjectDirectory {
        path: destination.display().to_string(),
        created: false,
    })
}

/// Join a user-supplied folder name onto a user-supplied parent, rejecting every
/// shape that could escape the parent instead of sanitizing it.
pub(crate) fn resolve_new_project_directory(
    parent_directory: &str,
    folder_name: &str,
) -> AppResult<PathBuf> {
    let folder_name = folder_name.trim();
    if folder_name.is_empty() {
        return Err(AppError::Validation(
            "Folder name cannot be empty".to_string(),
        ));
    }
    if folder_name.starts_with('~')
        || folder_name == "."
        || folder_name == ".."
        || folder_name.contains('/')
        || folder_name.contains('\\')
        || folder_name.contains('\0')
    {
        return Err(AppError::Validation(format!(
            "'{folder_name}' is not a valid folder name"
        )));
    }

    let parent =
        validate_absolute_non_root_path(Path::new(parent_directory.trim()), "parent directory")?;
    let canonical_parent = parent.canonicalize().map_err(|error| {
        AppError::Validation(format!(
            "Parent folder is not available: {} ({error})",
            parent.display()
        ))
    })?;
    if !canonical_parent.is_dir() {
        return Err(AppError::Validation(format!(
            "Parent folder is not a directory: {}",
            canonical_parent.display()
        )));
    }

    let destination = canonical_parent.join(folder_name);
    let destination = validate_absolute_non_root_path(&destination, "new project directory")?;
    require_under_root(&destination, &canonical_parent, "new project directory")?;

    Ok(destination)
}

pub(crate) async fn discard_project_directory(path: &Path) -> AppResult<()> {
    let safe_path = validate_absolute_non_root_path(path, "prepared project directory")?;
    if !safe_path.exists() {
        return Ok(());
    }
    if !safe_path.is_dir() {
        return Err(AppError::Validation(format!(
            "{} is not a directory",
            safe_path.display()
        )));
    }
    if !holds_only_fresh_git_metadata(&safe_path).await? {
        return Err(AppError::Validation(format!(
            "Refusing to remove {}: it has contents RalphX did not create",
            safe_path.display()
        )));
    }

    crate::utils::path_safety::checked_remove_dir_all(&safe_path, "prepared project directory")
        .await
}

async fn is_directory_empty(path: &Path) -> AppResult<bool> {
    let mut entries = checked_read_dir(path, "project directory").await?;
    let first = entries.next_entry().await.map_err(|error| {
        AppError::Infrastructure(format!("Failed to read {}: {error}", path.display()))
    })?;
    Ok(first.is_none())
}

async fn holds_only_fresh_git_metadata(path: &Path) -> AppResult<bool> {
    let mut entries = checked_read_dir(path, "prepared project directory").await?;
    while let Some(entry) = entries.next_entry().await.map_err(|error| {
        AppError::Infrastructure(format!("Failed to read {}: {error}", path.display()))
    })? {
        if entry.file_name() != ".git" {
            return Ok(false);
        }
    }
    Ok(true)
}
