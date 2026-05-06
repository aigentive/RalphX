use std::path::{Component, Path, PathBuf};

use chrono::Utc;
use sha2::{Digest, Sha256};

use crate::application::agent_conversation_workspace_base::{
    apply_workspace_base_resolution, is_commit_contained_in, resolve_workspace_base, BaseStatus,
};
use crate::application::git_service::GitService;
use crate::domain::entities::{
    AgentConversationWorkspace, AgentConversationWorkspaceMode, ChatConversationId,
    IdeationAnalysisBaseRefKind, Project, Task,
};
use crate::domain::state_machine::transition_handler::run_pre_execution_setup;
use crate::error::{AppError, AppResult};
use crate::infrastructure::agents::claude::agent_names::{
    AGENT_CHAT_PROJECT, AGENT_GENERAL_EXPLORER, AGENT_GENERAL_WORKER,
};

pub const AGENT_CONVERSATION_WORKSPACE_CONTINUATION_MESSAGE: &str =
    "A new workspace branch has been created automatically.";

#[derive(Debug, Clone, Default)]
pub struct AgentConversationWorkspaceBaseSelection {
    pub kind: Option<IdeationAnalysisBaseRefKind>,
    pub base_ref: Option<String>,
    pub display_name: Option<String>,
}

pub async fn prepare_agent_conversation_workspace(
    project: &Project,
    conversation_id: &ChatConversationId,
    mode: AgentConversationWorkspaceMode,
    selection: AgentConversationWorkspaceBaseSelection,
) -> AppResult<AgentConversationWorkspace> {
    let repo_path = PathBuf::from(&project.working_directory);
    let current_branch = GitService::get_current_branch(&repo_path)
        .await
        .ok()
        .filter(|branch| branch != "HEAD");
    let project_default =
        GitService::resolve_project_default_branch(&repo_path, project.base_branch.as_deref())
            .await;
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
            "PR-backed agent conversation base refs require PR workspace provisioning and are not enabled in this slice"
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
            .or(current_branch)
            .ok_or_else(|| AppError::Validation("Unable to resolve current branch".to_string()))?,
        IdeationAnalysisBaseRefKind::LocalBranch => selection
            .base_ref
            .filter(|value| !value.trim().is_empty())
            .ok_or_else(|| {
                AppError::Validation(
                    "Local branch agent conversation base requires base_ref".to_string(),
                )
            })?,
        IdeationAnalysisBaseRefKind::PullRequest => unreachable!("handled above"),
    };

    if !GitService::ref_exists(&repo_path, &base_ref).await? {
        return Err(AppError::Validation(format!(
            "Agent conversation base ref '{}' does not exist in the project repository",
            base_ref
        )));
    }

    let display_name = selection
        .display_name
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| match kind {
            IdeationAnalysisBaseRefKind::ProjectDefault => format!("Project default ({base_ref})"),
            IdeationAnalysisBaseRefKind::CurrentBranch => format!("Current branch ({base_ref})"),
            IdeationAnalysisBaseRefKind::LocalBranch => base_ref.clone(),
            IdeationAnalysisBaseRefKind::PullRequest => base_ref.clone(),
        });
    let branch_name = agent_conversation_branch_name(project, conversation_id);
    let worktree_path = resolve_agent_conversation_workspace_path(project, conversation_id)?;

    ensure_agent_conversation_worktree(&repo_path, &worktree_path, &branch_name, &base_ref).await?;
    run_agent_conversation_workspace_setup(project, conversation_id, &worktree_path, &branch_name)
        .await;
    let base_commit = GitService::get_head_sha(&worktree_path).await?;

    Ok(AgentConversationWorkspace {
        conversation_id: conversation_id.clone(),
        project_id: project.id.clone(),
        mode,
        base_ref_kind: kind,
        base_ref,
        base_display_name: Some(display_name),
        base_commit: Some(base_commit),
        branch_name,
        worktree_path: worktree_path.to_string_lossy().to_string(),
        linked_ideation_session_id: None,
        linked_plan_branch_id: None,
        publication_pr_number: None,
        publication_pr_url: None,
        publication_pr_status: None,
        publication_push_status: None,
        status: crate::domain::entities::AgentConversationWorkspaceStatus::Active,
        created_at: Utc::now(),
        updated_at: Utc::now(),
    })
}

pub async fn rollover_agent_conversation_workspace(
    project: &Project,
    workspace: &AgentConversationWorkspace,
) -> AppResult<AgentConversationWorkspace> {
    if !is_terminal_agent_conversation_publication_status(
        workspace.publication_pr_status.as_deref(),
    ) {
        return Ok(workspace.clone());
    }

    if workspace.project_id != project.id {
        return Err(AppError::Validation(format!(
            "Agent conversation workspace {} belongs to project {} instead of {}",
            workspace.conversation_id, workspace.project_id, project.id
        )));
    }

    let repo_path = PathBuf::from(&project.working_directory);
    let expected_path =
        resolve_agent_conversation_workspace_path(project, &workspace.conversation_id)?;
    let stored_path = PathBuf::from(&workspace.worktree_path);
    if stored_path != expected_path {
        return Err(AppError::Validation(format!(
            "Agent conversation workspace path mismatch for conversation {}",
            workspace.conversation_id
        )));
    }

    let project_root = PathBuf::from(&project.working_directory);
    if expected_path == project_root {
        return Err(AppError::Validation(format!(
            "Agent conversation workspace {} points to the project root",
            workspace.conversation_id
        )));
    }

    if expected_path.exists() {
        if !expected_path.is_dir() {
            return Err(AppError::Validation(format!(
                "Agent conversation workspace path exists but is not a directory: {}",
                expected_path.display()
            )));
        }

        if GitService::has_uncommitted_changes(&expected_path).await? {
            return Err(AppError::Validation(format!(
                "Cannot continue agent conversation {} on a new branch because the old workspace has uncommitted changes",
                workspace.conversation_id
            )));
        }
    }

    let base_resolution = resolve_workspace_base(project, workspace).await?;
    if base_resolution.status == BaseStatus::Blocked {
        return Err(AppError::Validation(
            base_resolution
                .block_reason
                .clone()
                .unwrap_or_else(|| "Agent workspace base is blocked".to_string()),
        ));
    }
    if base_resolution.status == BaseStatus::Retargeted {
        let old_branch_head = GitService::get_branch_sha(&repo_path, &workspace.branch_name)
            .await
            .map_err(|error| {
                AppError::Validation(format!(
                    "Cannot verify old workspace branch containment before rollover: {error}"
                ))
            })?;
        let target_ref = base_resolution.effective_checkout_ref()?;
        if !is_commit_contained_in(&repo_path, &old_branch_head, target_ref).await? {
            return Err(AppError::Validation(format!(
                "Cannot continue agent conversation {} on a new branch because old workspace branch HEAD is not contained in the default branch",
                workspace.conversation_id
            )));
        }
    }

    if expected_path.exists() {
        GitService::delete_worktree(&repo_path, &expected_path).await?;
    }

    let base_checkout_ref = base_resolution.effective_checkout_ref()?.to_string();
    let branch_name =
        agent_conversation_continuation_branch_name(project, &workspace.conversation_id);

    ensure_agent_conversation_worktree(
        &repo_path,
        &expected_path,
        &branch_name,
        &base_checkout_ref,
    )
    .await?;
    run_agent_conversation_workspace_setup(
        project,
        &workspace.conversation_id,
        &expected_path,
        &branch_name,
    )
    .await;
    let base_commit = GitService::get_head_sha(&expected_path).await?;
    let mut updated = workspace.clone();
    apply_workspace_base_resolution(&mut updated, &base_resolution)?;
    updated.base_commit = Some(base_commit);
    updated.branch_name = branch_name;
    updated.worktree_path = expected_path.to_string_lossy().to_string();
    updated.publication_pr_number = None;
    updated.publication_pr_url = None;
    updated.publication_pr_status = None;
    updated.publication_push_status = None;
    updated.status = crate::domain::entities::AgentConversationWorkspaceStatus::Active;
    updated.updated_at = Utc::now();
    Ok(updated)
}

pub fn is_terminal_agent_conversation_publication_status(status: Option<&str>) -> bool {
    matches!(status, Some("merged" | "closed"))
}

async fn run_agent_conversation_workspace_setup(
    project: &Project,
    conversation_id: &ChatConversationId,
    worktree_path: &Path,
    branch_name: &str,
) {
    let conversation_id_str = conversation_id.as_str();
    let mut setup_task = Task::new(
        project.id.clone(),
        format!("Agent conversation {conversation_id_str}"),
    );
    setup_task.task_branch = Some(branch_name.to_string());
    setup_task.worktree_path = Some(worktree_path.to_string_lossy().to_string());

    let Some(result) = run_pre_execution_setup(
        project,
        &setup_task,
        worktree_path,
        &conversation_id_str,
        None,
        "agent_conversation_setup",
        &tokio_util::sync::CancellationToken::new(),
    )
    .await
    else {
        return;
    };

    if result.success {
        tracing::info!(
            conversation_id = %conversation_id,
            worktree_path = %worktree_path.display(),
            command_count = result.log.len(),
            "Agent conversation worktree setup completed"
        );
    } else {
        tracing::warn!(
            conversation_id = %conversation_id,
            worktree_path = %worktree_path.display(),
            command_count = result.log.len(),
            "Agent conversation worktree setup had failures; continuing with workspace launch"
        );
    }
}

pub fn agent_conversation_branch_name(
    project: &Project,
    conversation_id: &ChatConversationId,
) -> String {
    let project_slug = slug_branch_component(&project.name);
    let short_id = conversation_id
        .as_str()
        .chars()
        .filter(|ch| ch.is_ascii_alphanumeric())
        .take(8)
        .collect::<String>();
    let short_id = if short_id.is_empty() {
        "conversation".to_string()
    } else {
        short_id
    };
    format!("ralphx/{project_slug}/agent-{short_id}")
}

fn agent_conversation_continuation_branch_name(
    project: &Project,
    conversation_id: &ChatConversationId,
) -> String {
    let base = agent_conversation_branch_name(project, conversation_id);
    format!("{}-{}", base, Utc::now().timestamp_millis())
}

pub fn agent_name_for_workspace_mode(mode: AgentConversationWorkspaceMode) -> &'static str {
    match mode {
        AgentConversationWorkspaceMode::Chat => AGENT_GENERAL_EXPLORER,
        AgentConversationWorkspaceMode::Edit => AGENT_GENERAL_WORKER,
        AgentConversationWorkspaceMode::Ideation => AGENT_CHAT_PROJECT,
    }
}

async fn ensure_agent_conversation_worktree(
    repo_path: &Path,
    workspace_path: &Path,
    branch_name: &str,
    base_ref: &str,
) -> AppResult<()> {
    if workspace_path.exists() {
        if !workspace_path.is_dir() {
            return Err(AppError::Validation(format!(
                "Agent conversation workspace path exists but is not a directory: {}",
                workspace_path.display()
            )));
        }
        let checked_out = GitService::get_current_branch(workspace_path).await?;
        if checked_out != branch_name {
            return Err(AppError::Validation(format!(
                "Existing agent conversation workspace {} is checked out at '{}' instead of '{}'",
                workspace_path.display(),
                checked_out,
                branch_name
            )));
        }
        return Ok(());
    }

    if GitService::branch_exists(repo_path, branch_name).await? {
        GitService::checkout_existing_branch_worktree(repo_path, workspace_path, branch_name).await
    } else {
        GitService::create_worktree(repo_path, workspace_path, branch_name, base_ref).await
    }
}

pub fn resolve_agent_conversation_workspace_path(
    project: &Project,
    conversation_id: &ChatConversationId,
) -> AppResult<PathBuf> {
    let parent = expand_worktree_parent(project.worktree_parent_or_default())?;
    Ok(parent
        .join(hashed_path_component("project", project.id.as_str()))
        .join(hashed_path_component(
            "agent-conversation",
            &conversation_id.as_str(),
        )))
}

pub async fn resolve_valid_agent_conversation_workspace_path(
    project: &Project,
    workspace: &AgentConversationWorkspace,
) -> AppResult<PathBuf> {
    if workspace.project_id != project.id {
        return Err(AppError::Validation(format!(
            "Agent conversation workspace {} belongs to project {} instead of {}",
            workspace.conversation_id, workspace.project_id, project.id
        )));
    }

    let expected_path =
        resolve_agent_conversation_workspace_path(project, &workspace.conversation_id)?;
    let stored_path = PathBuf::from(&workspace.worktree_path);
    if stored_path != expected_path {
        return Err(AppError::Validation(format!(
            "Agent conversation workspace path mismatch for conversation {}",
            workspace.conversation_id
        )));
    }

    let project_root = PathBuf::from(&project.working_directory);
    if expected_path == project_root {
        return Err(AppError::Validation(format!(
            "Agent conversation workspace {} points to the project root",
            workspace.conversation_id
        )));
    }

    if !expected_path.is_dir() {
        return Err(AppError::Validation(format!(
            "Agent conversation workspace is missing: {}",
            expected_path.display()
        )));
    }

    let checked_out = GitService::get_current_branch(&expected_path).await?;
    if checked_out != workspace.branch_name {
        return Err(AppError::Validation(format!(
            "Agent conversation workspace {} is checked out at '{}' instead of '{}'",
            workspace.conversation_id, checked_out, workspace.branch_name
        )));
    }

    Ok(expected_path)
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
            "Invalid agent conversation worktree parent path: {}",
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

fn slug_branch_component(value: &str) -> String {
    let mut slug = String::new();
    let mut last_dash = false;

    for ch in value.chars().flat_map(char::to_lowercase) {
        if ch.is_ascii_alphanumeric() {
            slug.push(ch);
            last_dash = false;
        } else if !last_dash && !slug.is_empty() {
            slug.push('-');
            last_dash = true;
        }
    }

    while slug.ends_with('-') {
        slug.pop();
    }

    if slug.is_empty() {
        "project".to_string()
    } else {
        slug
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::entities::{
        AgentConversationWorkspaceMode, ChatConversationId, IdeationAnalysisBaseRefKind, Project,
    };
    use std::process::Command;

    fn git(repo: &Path, args: &[&str]) -> String {
        let output = Command::new("git")
            .args(args)
            .current_dir(repo)
            .output()
            .expect("git command should spawn");
        assert!(
            output.status.success(),
            "git {:?} failed: {}{}",
            args,
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8_lossy(&output.stdout).trim().to_string()
    }

    fn setup_repo(root: &Path) {
        std::fs::create_dir_all(root).expect("repo root should be created");
        git(root, &["init", "-b", "main"]);
        git(root, &["config", "user.email", "test@example.com"]);
        git(root, &["config", "user.name", "Test User"]);
        std::fs::write(root.join("README.md"), "hello\n").expect("fixture file should be written");
        git(root, &["add", "README.md"]);
        git(root, &["commit", "-m", "initial"]);
    }

    #[tokio::test]
    async fn prepare_agent_conversation_workspace_runs_project_worktree_setup() {
        let temp = tempfile::tempdir().expect("tempdir should be created");
        let repo_path = temp.path().join("repo");
        let worktree_parent = temp.path().join("worktrees");
        setup_repo(&repo_path);

        let mut project = Project::new(
            "Agent Setup".to_string(),
            repo_path.to_string_lossy().to_string(),
        );
        project.worktree_parent_directory = Some(worktree_parent.to_string_lossy().to_string());
        project.custom_analysis = Some(
            r#"[{"path": ".", "label": "Agent setup", "worktree_setup": ["touch .agent_setup_marker"]}]"#
                .to_string(),
        );

        let conversation_id =
            ChatConversationId::from_string("conversation-setup-test".to_string());
        let workspace = prepare_agent_conversation_workspace(
            &project,
            &conversation_id,
            AgentConversationWorkspaceMode::Edit,
            AgentConversationWorkspaceBaseSelection {
                kind: Some(IdeationAnalysisBaseRefKind::ProjectDefault),
                base_ref: Some("main".to_string()),
                display_name: None,
            },
        )
        .await
        .expect("workspace should be prepared");

        assert!(
            Path::new(&workspace.worktree_path)
                .join(".agent_setup_marker")
                .exists(),
            "agent conversation worktree should run project worktree_setup commands"
        );
        let captured_head = GitService::get_head_sha(Path::new(&workspace.worktree_path))
            .await
            .expect("workspace HEAD should resolve");
        assert_eq!(
            workspace.base_commit.as_deref(),
            Some(captured_head.as_str()),
            "agent conversation workspace should always capture the immutable base commit"
        );
    }

    #[tokio::test]
    async fn rollover_agent_conversation_workspace_creates_new_branch_after_terminal_pr() {
        let temp = tempfile::tempdir().expect("tempdir should be created");
        let repo_path = temp.path().join("repo");
        let worktree_parent = temp.path().join("worktrees");
        setup_repo(&repo_path);

        let mut project = Project::new(
            "Agent Rollover".to_string(),
            repo_path.to_string_lossy().to_string(),
        );
        project.worktree_parent_directory = Some(worktree_parent.to_string_lossy().to_string());

        let conversation_id =
            ChatConversationId::from_string("conversation-rollover-test".to_string());
        let mut workspace = prepare_agent_conversation_workspace(
            &project,
            &conversation_id,
            AgentConversationWorkspaceMode::Edit,
            AgentConversationWorkspaceBaseSelection {
                kind: Some(IdeationAnalysisBaseRefKind::ProjectDefault),
                base_ref: Some("main".to_string()),
                display_name: None,
            },
        )
        .await
        .expect("workspace should be prepared");
        let old_branch = workspace.branch_name.clone();
        let old_worktree_path = workspace.worktree_path.clone();
        workspace.publication_pr_number = Some(91);
        workspace.publication_pr_url = Some("https://example.test/pr/91".to_string());
        workspace.publication_pr_status = Some("merged".to_string());
        workspace.publication_push_status = Some("pushed".to_string());
        workspace.status = crate::domain::entities::AgentConversationWorkspaceStatus::Missing;

        let updated = rollover_agent_conversation_workspace(&project, &workspace)
            .await
            .expect("terminal published workspace should roll over");

        assert_eq!(updated.worktree_path, old_worktree_path);
        assert!(
            updated.branch_name.starts_with(&format!("{old_branch}-")),
            "continuation branch should extend the canonical workspace branch"
        );
        assert_ne!(updated.branch_name, old_branch);
        assert_eq!(updated.publication_pr_number, None);
        assert_eq!(updated.publication_pr_url, None);
        assert_eq!(updated.publication_pr_status, None);
        assert_eq!(updated.publication_push_status, None);
        assert_eq!(
            updated.status,
            crate::domain::entities::AgentConversationWorkspaceStatus::Active
        );
        let checked_out = GitService::get_current_branch(Path::new(&updated.worktree_path))
            .await
            .expect("rolled workspace branch should resolve");
        assert_eq!(checked_out, updated.branch_name);
    }

    #[tokio::test]
    async fn rollover_agent_conversation_workspace_blocks_dirty_old_worktree() {
        let temp = tempfile::tempdir().expect("tempdir should be created");
        let repo_path = temp.path().join("repo");
        let worktree_parent = temp.path().join("worktrees");
        setup_repo(&repo_path);

        let mut project = Project::new(
            "Agent Dirty Rollover".to_string(),
            repo_path.to_string_lossy().to_string(),
        );
        project.worktree_parent_directory = Some(worktree_parent.to_string_lossy().to_string());

        let conversation_id =
            ChatConversationId::from_string("conversation-dirty-rollover-test".to_string());
        let mut workspace = prepare_agent_conversation_workspace(
            &project,
            &conversation_id,
            AgentConversationWorkspaceMode::Edit,
            AgentConversationWorkspaceBaseSelection {
                kind: Some(IdeationAnalysisBaseRefKind::ProjectDefault),
                base_ref: Some("main".to_string()),
                display_name: None,
            },
        )
        .await
        .expect("workspace should be prepared");
        workspace.publication_pr_status = Some("merged".to_string());
        std::fs::write(
            Path::new(&workspace.worktree_path).join("dirty.txt"),
            "uncommitted\n",
        )
        .expect("dirty file should be written");

        let error = rollover_agent_conversation_workspace(&project, &workspace)
            .await
            .expect_err("dirty rollover should be blocked");

        assert!(
            error.to_string().contains("uncommitted changes"),
            "dirty workspace should produce a clear validation error: {error}"
        );
        let checked_out = GitService::get_current_branch(Path::new(&workspace.worktree_path))
            .await
            .expect("old workspace should remain checked out");
        assert_eq!(checked_out, workspace.branch_name);
    }

    #[tokio::test]
    async fn rollover_agent_conversation_workspace_blocks_stale_base_before_deleting_worktree() {
        let temp = tempfile::tempdir().expect("tempdir should be created");
        let repo_path = temp.path().join("repo");
        let worktree_parent = temp.path().join("worktrees");
        setup_repo(&repo_path);

        let mut project = Project::new(
            "Agent Unsafe Rollover".to_string(),
            repo_path.to_string_lossy().to_string(),
        );
        project.worktree_parent_directory = Some(worktree_parent.to_string_lossy().to_string());

        git(&repo_path, &["checkout", "--orphan", "unmerged-base"]);
        std::fs::write(repo_path.join("README.md"), "unmerged\n")
            .expect("fixture file should be written");
        git(&repo_path, &["add", "README.md"]);
        git(&repo_path, &["commit", "-m", "unmerged"]);
        let unmerged_sha = git(&repo_path, &["rev-parse", "HEAD"]);
        git(&repo_path, &["checkout", "main"]);

        let conversation_id =
            ChatConversationId::from_string("conversation-stale-rollover-test".to_string());
        let mut workspace = prepare_agent_conversation_workspace(
            &project,
            &conversation_id,
            AgentConversationWorkspaceMode::Edit,
            AgentConversationWorkspaceBaseSelection {
                kind: Some(IdeationAnalysisBaseRefKind::ProjectDefault),
                base_ref: Some("main".to_string()),
                display_name: None,
            },
        )
        .await
        .expect("workspace should be prepared");
        workspace.base_ref = "feature/deleted-base".to_string();
        workspace.base_display_name = Some("Current branch (feature/deleted-base)".to_string());
        workspace.base_commit = Some(unmerged_sha);
        workspace.publication_pr_status = Some("merged".to_string());
        let old_worktree_path = PathBuf::from(&workspace.worktree_path);

        let error = rollover_agent_conversation_workspace(&project, &workspace)
            .await
            .expect_err("unsafe stale base should block rollover");

        assert!(
            error
                .to_string()
                .contains("not contained in the default branch"),
            "blocked stale base should explain containment failure: {error}"
        );
        assert!(
            old_worktree_path.exists(),
            "blocked rollover must leave the old worktree intact"
        );
        let checked_out = GitService::get_current_branch(&old_worktree_path)
            .await
            .expect("old workspace should remain checked out");
        assert_eq!(checked_out, workspace.branch_name);
    }

    #[tokio::test]
    async fn rollover_agent_conversation_workspace_blocks_retarget_when_old_head_not_contained() {
        let temp = tempfile::tempdir().expect("tempdir should be created");
        let repo_path = temp.path().join("repo");
        let worktree_parent = temp.path().join("worktrees");
        setup_repo(&repo_path);

        let mut project = Project::new(
            "Agent Retarget Rollover".to_string(),
            repo_path.to_string_lossy().to_string(),
        );
        project.worktree_parent_directory = Some(worktree_parent.to_string_lossy().to_string());
        let main_sha = git(&repo_path, &["rev-parse", "main"]);

        let conversation_id =
            ChatConversationId::from_string("conversation-retarget-rollover-test".to_string());
        let mut workspace = prepare_agent_conversation_workspace(
            &project,
            &conversation_id,
            AgentConversationWorkspaceMode::Edit,
            AgentConversationWorkspaceBaseSelection {
                kind: Some(IdeationAnalysisBaseRefKind::ProjectDefault),
                base_ref: Some("main".to_string()),
                display_name: None,
            },
        )
        .await
        .expect("workspace should be prepared");

        std::fs::write(
            Path::new(&workspace.worktree_path).join("unmerged.txt"),
            "not merged to main\n",
        )
        .expect("fixture file should be written");
        git(
            Path::new(&workspace.worktree_path),
            &["add", "unmerged.txt"],
        );
        git(
            Path::new(&workspace.worktree_path),
            &["commit", "-m", "workspace-only change"],
        );

        workspace.base_ref = "feature/deleted-base".to_string();
        workspace.base_display_name = Some("Current branch (feature/deleted-base)".to_string());
        workspace.base_commit = Some(main_sha);
        workspace.publication_pr_status = Some("merged".to_string());
        let old_worktree_path = PathBuf::from(&workspace.worktree_path);

        let error = rollover_agent_conversation_workspace(&project, &workspace)
            .await
            .expect_err("old branch head outside default should block rollover");

        assert!(
            error
                .to_string()
                .contains("old workspace branch HEAD is not contained"),
            "blocked rollover should explain old branch containment failure: {error}"
        );
        assert!(
            old_worktree_path.exists(),
            "blocked rollover must not delete the old worktree"
        );
        let checked_out = GitService::get_current_branch(&old_worktree_path)
            .await
            .expect("old workspace should remain checked out");
        assert_eq!(checked_out, workspace.branch_name);
    }
}
