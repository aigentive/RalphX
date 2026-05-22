use std::path::{Component, Path, PathBuf};

use chrono::Utc;
use sha2::{Digest, Sha256};

use crate::application::git_service::GitService;
use crate::domain::entities::{
    AgentConversationWorkspace,
    IdeationAnalysisBaseRefKind, IdeationAnalysisState, IdeationAnalysisWorkspaceKind,
    IdeationSession, IdeationSessionId, Project,
};
use crate::error::{AppError, AppResult};

#[derive(Debug, Clone, Default)]
pub struct IdeationAnalysisBaseSelection {
    pub kind: Option<IdeationAnalysisBaseRefKind>,
    pub base_ref: Option<String>,
    pub display_name: Option<String>,
}

pub fn resolve_ideation_workspace_path(
    session: &IdeationSession,
    project: &Project,
) -> Result<PathBuf, String> {
    let project_root = PathBuf::from(&project.working_directory);

    if session.analysis.requires_dedicated_workspace() {
        let Some(workspace_path) = session.analysis.workspace_path.as_deref() else {
            return Err(format!(
                "Ideation session {} requires a dedicated workspace but has no analysis_workspace_path",
                session.id
            ));
        };
        let path = PathBuf::from(workspace_path);
        if !path.is_dir() {
            return Err(format!(
                "Ideation session {} analysis workspace is missing: {}",
                session.id,
                path.display()
            ));
        }
        if path == project_root {
            return Err(format!(
                "Ideation session {} dedicated workspace points to project root",
                session.id
            ));
        }
        return Ok(path);
    }

    Ok(session
        .analysis
        .workspace_path
        .as_deref()
        .map(PathBuf::from)
        .unwrap_or(project_root))
}

pub async fn prepare_ideation_analysis_state(
    project: &Project,
    session_id: &IdeationSessionId,
    selection: IdeationAnalysisBaseSelection,
) -> AppResult<IdeationAnalysisState> {
    let repo_path = PathBuf::from(&project.working_directory);
    let current_branch = GitService::get_current_branch(&repo_path)
        .await
        .ok()
        .filter(|branch| branch != "HEAD");
    let project_default =
        GitService::resolve_project_default_branch(&repo_path, project.base_branch.as_deref())
            .await;
    let explicit_kind = selection.kind.is_some();
    let kind = selection.kind.unwrap_or_else(|| {
        if current_branch
            .as_deref()
            .is_some_and(|branch| branch != project_default)
        {
            IdeationAnalysisBaseRefKind::CurrentBranch
        } else {
            IdeationAnalysisBaseRefKind::ProjectDefault
        }
    });

    if kind == IdeationAnalysisBaseRefKind::PullRequest {
        return Err(AppError::Validation(
            "PR-backed ideation base refs require PR workspace provisioning and are not enabled in this slice"
                .to_string(),
        ));
    }

    let base_ref = match kind {
        IdeationAnalysisBaseRefKind::ProjectDefault => selection
            .base_ref
            .filter(|value| !value.trim().is_empty())
            .unwrap_or(project_default),
        IdeationAnalysisBaseRefKind::CurrentBranch => selection
            .base_ref
            .filter(|value| !value.trim().is_empty())
            .or(current_branch.clone())
            .ok_or_else(|| AppError::Validation("Unable to resolve current branch".to_string()))?,
        IdeationAnalysisBaseRefKind::LocalBranch => selection
            .base_ref
            .filter(|value| !value.trim().is_empty())
            .ok_or_else(|| {
                AppError::Validation(
                    "Local branch ideation base requires analysis_base_ref".to_string(),
                )
            })?,
        IdeationAnalysisBaseRefKind::PullRequest => unreachable!("handled above"),
    };
    let base_ref = if kind == IdeationAnalysisBaseRefKind::LocalBranch {
        GitService::ensure_local_branch_from_origin_if_missing(&repo_path, &base_ref).await?
    } else {
        base_ref
    };

    let display_name = selection
        .display_name
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| match kind {
            IdeationAnalysisBaseRefKind::ProjectDefault => format!("Project default ({base_ref})"),
            IdeationAnalysisBaseRefKind::CurrentBranch => format!("Current branch ({base_ref})"),
            IdeationAnalysisBaseRefKind::LocalBranch => base_ref.clone(),
            IdeationAnalysisBaseRefKind::PullRequest => base_ref.clone(),
        });

    if !explicit_kind && current_branch.is_none() {
        return Ok(IdeationAnalysisState {
            base_ref_kind: Some(kind),
            base_ref: Some(base_ref),
            base_display_name: Some(display_name),
            workspace_kind: IdeationAnalysisWorkspaceKind::ProjectRoot,
            workspace_path: Some(repo_path.to_string_lossy().to_string()),
            base_commit: None,
            base_locked_at: Some(Utc::now()),
        });
    }

    let current_matches = current_branch.as_deref() == Some(base_ref.as_str());
    let (workspace_kind, workspace_path) = if current_matches {
        (
            IdeationAnalysisWorkspaceKind::ProjectRoot,
            repo_path.clone(),
        )
    } else {
        if !GitService::ref_exists(&repo_path, &base_ref).await? {
            return Err(AppError::Validation(format!(
                "Ideation analysis base ref '{}' does not exist in the project repository",
                base_ref
            )));
        }
        let workspace_path = ideation_worktree_path(project, session_id)?;
        ensure_ideation_worktree(&repo_path, &workspace_path, &base_ref).await?;
        (
            IdeationAnalysisWorkspaceKind::IdeationWorktree,
            workspace_path,
        )
    };

    let base_commit = GitService::get_head_sha(&workspace_path).await.ok();
    Ok(IdeationAnalysisState {
        base_ref_kind: Some(kind),
        base_ref: Some(base_ref),
        base_display_name: Some(display_name),
        workspace_kind,
        workspace_path: Some(workspace_path.to_string_lossy().to_string()),
        base_commit,
        base_locked_at: Some(Utc::now()),
    })
}

pub async fn prepare_ideation_analysis_state_from_agent_workspace(
    project: &Project,
    workspace: &AgentConversationWorkspace,
) -> AppResult<IdeationAnalysisState> {
    let workspace_path =
        crate::application::agent_conversation_workspace::resolve_valid_agent_conversation_workspace_path(
            project,
            workspace,
        )
        .await?;
    let base_commit = GitService::get_head_sha(&workspace_path)
        .await
        .ok()
        .or_else(|| workspace.base_commit.clone());

    Ok(IdeationAnalysisState {
        base_ref_kind: Some(workspace.base_ref_kind),
        base_ref: Some(workspace.base_ref.clone()),
        base_display_name: workspace.base_display_name.clone(),
        workspace_kind: IdeationAnalysisWorkspaceKind::IdeationWorktree,
        workspace_path: Some(workspace_path.to_string_lossy().to_string()),
        base_commit,
        base_locked_at: Some(Utc::now()),
    })
}

async fn ensure_ideation_worktree(
    repo_path: &Path,
    workspace_path: &Path,
    base_ref: &str,
) -> AppResult<()> {
    if workspace_path.exists() {
        if !workspace_path.is_dir() {
            return Err(AppError::Validation(format!(
                "Ideation workspace path exists but is not a directory: {}",
                workspace_path.display()
            )));
        }
        let checked_out = GitService::get_current_branch(workspace_path).await?;
        if checked_out != base_ref {
            return Err(AppError::Validation(format!(
                "Existing ideation workspace {} is checked out at '{}' instead of '{}'",
                workspace_path.display(),
                checked_out,
                base_ref
            )));
        }
        return Ok(());
    }

    GitService::checkout_existing_branch_worktree(repo_path, workspace_path, base_ref).await
}

fn ideation_worktree_path(project: &Project, session_id: &IdeationSessionId) -> AppResult<PathBuf> {
    let parent = expand_worktree_parent(project.worktree_parent_or_default())?;
    Ok(parent
        .join(hashed_path_component("project", project.id.as_str()))
        .join(hashed_path_component("ideation", session_id.as_str())))
}

fn expand_worktree_parent(parent: &str) -> AppResult<PathBuf> {
    let expanded = if let Some(rest) = parent.strip_prefix("~/") {
        let home = dirs::home_dir().ok_or_else(|| {
            AppError::Validation(
                "Cannot expand worktree parent because home directory is unavailable".to_string(),
            )
        })?;
        home.join(rest)
    } else {
        PathBuf::from(parent)
    };

    if !expanded.is_absolute()
        || expanded
            .components()
            .any(|component| matches!(component, Component::ParentDir))
    {
        return Err(AppError::Validation(format!(
            "Invalid ideation worktree parent path: {}",
            expanded.display()
        )));
    }

    Ok(expanded)
}

fn hashed_path_component(prefix: &str, value: &str) -> String {
    let digest = Sha256::digest(value.as_bytes());
    let hash = digest[..12]
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    format!("{prefix}-{hash}")
}
