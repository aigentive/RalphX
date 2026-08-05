use std::collections::HashMap;
use std::path::PathBuf;

use crate::application::agent_conversation_workspace::{
    prepare_agent_conversation_workspace_with_setup_mode_and_defaults,
    AgentConversationWorkspaceBaseSelection, AgentConversationWorkspacePrAutomationDefaults,
    AgentConversationWorkspaceSetupMode,
};
use crate::application::interactive_process_registry::InteractiveProcessKey;
use crate::application::provider_session_fork::{
    fork_provider_session_from_state_home_for_target, ProviderSessionForkResult,
    ProviderSessionForkTarget,
};
use crate::application::AppState;
use crate::domain::agents::ProviderSessionRef;
use crate::domain::entities::{
    AgentConversationWorkspace, AgentConversationWorkspaceMode, ChatContextType, ChatConversation,
    ChatConversationId, ChatMessage, ChatMessageId, ChatTimelineItem, ProjectId,
};
use crate::domain::services::RunningAgentKey;
use crate::error::{AppError, AppResult};

const DEFAULT_AGENT_TITLE: &str = "Untitled agent";
const FORK_TITLE_PREFIX: &str = "[Fork] ";
const MAX_FORK_TITLE_CHARS: usize = 72;

#[derive(Debug, Clone)]
pub struct AgentConversationForkResult {
    pub parent_conversation: ChatConversation,
    pub conversation: ChatConversation,
    pub workspace: Option<AgentConversationWorkspace>,
    pub provider_session: Option<ProviderSessionForkResult>,
    pub copied_message_count: usize,
    pub copied_timeline_item_count: usize,
}

pub async fn fork_agent_conversation(
    state: &AppState,
    parent_conversation_id: &ChatConversationId,
) -> AppResult<AgentConversationForkResult> {
    fork_agent_conversation_with_id(state, parent_conversation_id, ChatConversationId::new()).await
}

pub async fn fork_agent_conversation_with_id(
    state: &AppState,
    parent_conversation_id: &ChatConversationId,
    child_conversation_id: ChatConversationId,
) -> AppResult<AgentConversationForkResult> {
    let parent_conversation = state
        .chat_conversation_repo
        .get_by_id(parent_conversation_id)
        .await?
        .ok_or_else(|| {
            AppError::NotFound(format!(
                "Parent agent conversation not found: {}",
                parent_conversation_id
            ))
        })?;
    validate_forkable_parent(state, &parent_conversation).await?;

    let project_id = ProjectId::from_string(parent_conversation.context_id.clone());
    let project = state
        .project_repo
        .get_by_id(&project_id)
        .await?
        .ok_or_else(|| AppError::ProjectNotFound(parent_conversation.context_id.clone()))?;

    let parent_workspace = state
        .agent_conversation_workspace_repo
        .get_by_conversation_id(parent_conversation_id)
        .await?;
    let mode = parent_workspace
        .as_ref()
        .map(|workspace| workspace.mode)
        .or(parent_conversation.agent_mode)
        .unwrap_or(AgentConversationWorkspaceMode::Edit);
    if mode == AgentConversationWorkspaceMode::Tasks {
        return Err(AppError::Validation(
            "Tasks conversations cannot be forked because their pipeline attachment belongs to the owning conversation"
                .to_string(),
        ));
    }

    let mut child_conversation = ChatConversation::new_project(project_id.clone());
    child_conversation.id = child_conversation_id;
    child_conversation.parent_conversation_id = Some(parent_conversation.id.as_str().to_string());
    child_conversation.set_title(forked_conversation_title(
        parent_conversation.title.as_deref(),
    ));
    child_conversation.set_agent_mode(Some(mode));
    child_conversation.upstream_provider = parent_conversation.upstream_provider.clone();
    child_conversation.provider_profile = parent_conversation.provider_profile.clone();

    let workspace = if agent_mode_requires_workspace(mode) {
        let selection = workspace_base_selection(parent_workspace.as_ref());
        let settings = state
            .execution_settings_repo
            .get_settings(Some(&project.id))
            .await
            .map_err(|error| AppError::Infrastructure(error.to_string()))?;
        Some(
            prepare_agent_conversation_workspace_with_setup_mode_and_defaults(
                &project,
                &child_conversation.id,
                mode,
                selection,
                AgentConversationWorkspaceSetupMode::Deferred,
                AgentConversationWorkspacePrAutomationDefaults::from(&settings),
                false,
            )
            .await?,
        )
    } else {
        None
    };

    let provider_session = fork_parent_provider_session(&parent_conversation, workspace.as_ref())?;
    if let Some(provider_session) = provider_session.as_ref() {
        child_conversation.set_provider_session_ref(provider_session.session_ref.clone());
    }

    let child_conversation = state
        .chat_conversation_repo
        .create(child_conversation)
        .await?;
    if let Some(workspace) = workspace.clone() {
        state
            .agent_conversation_workspace_repo
            .create_or_update(workspace)
            .await?;
    }

    let message_id_map = copy_conversation_messages(
        state,
        &parent_conversation,
        &child_conversation,
        provider_session.as_ref().map(|result| &result.session_ref),
    )
    .await?;
    let copied_timeline_item_count = copy_conversation_timeline(
        state,
        &parent_conversation,
        &child_conversation,
        &message_id_map,
        provider_session.as_ref().map(|result| &result.session_ref),
    )
    .await?;

    let conversation = state
        .chat_conversation_repo
        .get_by_id(&child_conversation.id)
        .await?
        .unwrap_or(child_conversation);

    Ok(AgentConversationForkResult {
        parent_conversation,
        conversation,
        workspace,
        provider_session,
        copied_message_count: message_id_map.len(),
        copied_timeline_item_count,
    })
}

pub(crate) async fn validate_forkable_parent(
    state: &AppState,
    parent_conversation: &ChatConversation,
) -> AppResult<()> {
    if parent_conversation.context_type != ChatContextType::Project {
        return Err(AppError::Validation(
            "Only project agent conversations can be forked".to_string(),
        ));
    }

    let runtime_key = RunningAgentKey::new("project", parent_conversation.id.as_str());
    if state.running_agent_registry.is_running(&runtime_key).await {
        return Err(AppError::Conflict(
            "Cannot fork while the parent agent conversation is running".to_string(),
        ));
    }

    let interactive_key = InteractiveProcessKey::new("project", parent_conversation.id.as_str());
    if state
        .interactive_process_registry
        .has_process(&interactive_key)
        .await
    {
        return Err(AppError::Conflict(
            "Cannot fork while the parent agent conversation has an active provider process"
                .to_string(),
        ));
    }

    Ok(())
}

fn agent_mode_requires_workspace(mode: AgentConversationWorkspaceMode) -> bool {
    matches!(
        mode,
        AgentConversationWorkspaceMode::Edit
            | AgentConversationWorkspaceMode::Autopilot
            | AgentConversationWorkspaceMode::Ideation
    )
}

fn workspace_base_selection(
    workspace: Option<&AgentConversationWorkspace>,
) -> AgentConversationWorkspaceBaseSelection {
    workspace
        .map(AgentConversationWorkspaceBaseSelection::for_workspace_reuse)
        .unwrap_or_default()
}

fn forked_conversation_title(parent_title: Option<&str>) -> String {
    let source_title = parent_title
        .map(str::trim)
        .filter(|title| !title.is_empty())
        .unwrap_or(DEFAULT_AGENT_TITLE);
    let unprefixed_title = source_title
        .strip_prefix(FORK_TITLE_PREFIX)
        .unwrap_or(source_title)
        .trim();
    truncate_fork_title(&format!("{FORK_TITLE_PREFIX}{unprefixed_title}"))
}

fn truncate_fork_title(title: &str) -> String {
    let char_count = title.chars().count();
    if char_count <= MAX_FORK_TITLE_CHARS {
        return title.to_string();
    }

    let mut truncated = title.chars().take(MAX_FORK_TITLE_CHARS).collect::<String>();
    if let Some((prefix, _)) = truncated.rsplit_once(char::is_whitespace) {
        if prefix.len() >= FORK_TITLE_PREFIX.len() {
            truncated = prefix.trim_end().to_string();
        }
    }
    truncated
}

fn fork_parent_provider_session(
    parent_conversation: &ChatConversation,
    child_workspace: Option<&AgentConversationWorkspace>,
) -> AppResult<Option<ProviderSessionForkResult>> {
    let target = child_workspace.map(|workspace| ProviderSessionForkTarget {
        working_directory: PathBuf::from(&workspace.worktree_path),
        git_branch: Some(workspace.branch_name.clone()),
    });
    parent_conversation
        .provider_session_ref()
        .as_ref()
        .map(|parent_ref| {
            fork_provider_session_from_state_home_for_target(parent_ref, target.as_ref())
        })
        .transpose()
}

async fn copy_conversation_messages(
    state: &AppState,
    parent_conversation: &ChatConversation,
    child_conversation: &ChatConversation,
    child_provider_ref: Option<&ProviderSessionRef>,
) -> AppResult<HashMap<ChatMessageId, ChatMessageId>> {
    let parent_messages = state
        .chat_message_repo
        .get_by_conversation(&parent_conversation.id)
        .await?;
    let message_id_map = parent_messages
        .iter()
        .map(|message| (message.id.clone(), ChatMessageId::new()))
        .collect::<HashMap<_, _>>();

    for parent_message in parent_messages {
        let old_message_id = parent_message.id.clone();
        let mut child_message = clone_message_for_child(
            parent_message,
            child_conversation,
            &message_id_map,
            parent_conversation.provider_session_ref().as_ref(),
            child_provider_ref,
        );
        child_message.id = message_id_map
            .get(&old_message_id)
            .cloned()
            .unwrap_or_else(ChatMessageId::new);
        state.chat_message_repo.create(child_message).await?;
    }

    Ok(message_id_map)
}

fn clone_message_for_child(
    mut message: ChatMessage,
    child_conversation: &ChatConversation,
    message_id_map: &HashMap<ChatMessageId, ChatMessageId>,
    parent_provider_ref: Option<&ProviderSessionRef>,
    child_provider_ref: Option<&ProviderSessionRef>,
) -> ChatMessage {
    message.session_id = None;
    message.project_id = Some(ProjectId::from_string(
        child_conversation.context_id.clone(),
    ));
    message.task_id = None;
    message.conversation_id = Some(child_conversation.id.clone());
    message.parent_message_id = message
        .parent_message_id
        .as_ref()
        .and_then(|id| message_id_map.get(id).cloned());
    rewrite_provider_session_metadata(
        &mut message.provider_session_id,
        message.provider_harness,
        parent_provider_ref,
        child_provider_ref,
    );
    message
}

async fn copy_conversation_timeline(
    state: &AppState,
    parent_conversation: &ChatConversation,
    child_conversation: &ChatConversation,
    message_id_map: &HashMap<ChatMessageId, ChatMessageId>,
    child_provider_ref: Option<&ProviderSessionRef>,
) -> AppResult<usize> {
    let timeline_items = state
        .chat_timeline_repo
        .get_by_conversation(&parent_conversation.id)
        .await?;
    let copied_count = timeline_items.len();
    let parent_provider_ref = parent_conversation.provider_session_ref();

    for item in timeline_items {
        let child_item = clone_timeline_item_for_child(
            item,
            child_conversation,
            message_id_map,
            parent_provider_ref.as_ref(),
            child_provider_ref,
        );
        state.chat_timeline_repo.upsert_item(child_item).await?;
    }

    Ok(copied_count)
}

fn clone_timeline_item_for_child(
    mut item: ChatTimelineItem,
    child_conversation: &ChatConversation,
    message_id_map: &HashMap<ChatMessageId, ChatMessageId>,
    parent_provider_ref: Option<&ProviderSessionRef>,
    child_provider_ref: Option<&ProviderSessionRef>,
) -> ChatTimelineItem {
    let mapped_message_id = item
        .message_id
        .as_ref()
        .and_then(|id| message_id_map.get(id).cloned());
    item.id = mapped_message_id
        .as_ref()
        .map(|message_id| ChatTimelineItem::stable_message_block_id(message_id, item.block_index))
        .unwrap_or_default();
    item.conversation_id = child_conversation.id.clone();
    item.message_id = mapped_message_id;
    item.run_id = None;
    rewrite_provider_session_metadata(
        &mut item.provider_session_id,
        item.provider_harness,
        parent_provider_ref,
        child_provider_ref,
    );
    item
}

fn rewrite_provider_session_metadata(
    provider_session_id: &mut Option<String>,
    provider_harness: Option<crate::domain::agents::AgentHarnessKind>,
    parent_provider_ref: Option<&ProviderSessionRef>,
    child_provider_ref: Option<&ProviderSessionRef>,
) {
    let (Some(parent_provider_ref), Some(child_provider_ref), Some(session_id)) =
        (parent_provider_ref, child_provider_ref, provider_session_id)
    else {
        return;
    };
    if provider_harness == Some(parent_provider_ref.harness)
        && *session_id == parent_provider_ref.provider_session_id
    {
        *session_id = child_provider_ref.provider_session_id.clone();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::agents::AgentHarnessKind;
    use crate::domain::entities::{
        AgentConversationWorkspaceBranchMode, AgentWorkspaceSourcePullRequest,
        ChatTimelineItemKind, ChatTimelineItemStatus, IdeationAnalysisBaseRefKind, MessageRole,
        Project,
    };

    fn run_git(repo: &std::path::Path, args: &[&str]) {
        let output = std::process::Command::new("git")
            .args(args)
            .current_dir(repo)
            .output()
            .expect("git command should run");
        assert!(
            output.status.success(),
            "git {:?} failed\nstdout:\n{}\nstderr:\n{}",
            args,
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }

    fn init_repo(repo_path: &std::path::Path) {
        std::fs::create_dir_all(repo_path).expect("create repo dir");
        run_git(repo_path, &["init"]);
        run_git(repo_path, &["config", "user.email", "test@example.com"]);
        run_git(repo_path, &["config", "user.name", "Test User"]);
        run_git(repo_path, &["checkout", "-b", "main"]);
        std::fs::write(repo_path.join("README.md"), "base\n").expect("write readme");
        run_git(repo_path, &["add", "."]);
        run_git(repo_path, &["commit", "-m", "initial"]);
    }

    async fn create_project(state: &AppState, working_directory: &str) -> Project {
        let mut project = Project::new("Project".to_string(), working_directory.to_string());
        project.base_branch = Some("main".to_string());
        state
            .project_repo
            .create(project)
            .await
            .expect("create project")
    }

    async fn create_parent_conversation(
        state: &AppState,
        project: &Project,
        mode: AgentConversationWorkspaceMode,
    ) -> ChatConversation {
        let mut parent = ChatConversation::new_project(project.id.clone());
        parent.set_title("Build fork flow");
        parent.set_agent_mode(Some(mode));
        state
            .chat_conversation_repo
            .create(parent)
            .await
            .expect("create parent conversation")
    }

    #[test]
    fn forked_conversation_title_prefixes_parent_title() {
        assert_eq!(
            forked_conversation_title(Some("Build checkout flow")),
            "[Fork] Build checkout flow"
        );
    }

    #[test]
    fn forked_conversation_title_uses_safe_default_for_blank_parent_title() {
        assert_eq!(
            forked_conversation_title(Some("  ")),
            "[Fork] Untitled agent"
        );
        assert_eq!(forked_conversation_title(None), "[Fork] Untitled agent");
    }

    #[test]
    fn forked_conversation_title_does_not_duplicate_prefix() {
        assert_eq!(
            forked_conversation_title(Some("[Fork] Build checkout flow")),
            "[Fork] Build checkout flow"
        );
    }

    #[test]
    fn forked_conversation_title_truncates_long_titles() {
        let title = forked_conversation_title(Some(
            "Investigate provider session continuity and linked workspace publication state",
        ));

        assert!(title.starts_with("[Fork] "));
        assert!(title.chars().count() <= MAX_FORK_TITLE_CHARS);
    }

    #[test]
    fn workspace_base_selection_preserves_branch_mode_without_pr_metadata() {
        let mut workspace = AgentConversationWorkspace::new(
            ChatConversationId::from_string("conversation-1"),
            ProjectId::from_string("project-1".to_string()),
            AgentConversationWorkspaceMode::Edit,
            IdeationAnalysisBaseRefKind::LocalBranch,
            "feature/shared".to_string(),
            Some("feature/shared".to_string()),
            Some("abc123".to_string()),
            "feature/shared".to_string(),
            "/tmp/worktree".to_string(),
        );
        workspace.branch_mode = AgentConversationWorkspaceBranchMode::Linked;
        workspace.source_pull_request = None;

        let selection = workspace_base_selection(Some(&workspace));

        assert_eq!(
            selection.branch_mode,
            Some(AgentConversationWorkspaceBranchMode::Linked)
        );
        assert_eq!(
            selection.kind,
            Some(IdeationAnalysisBaseRefKind::LocalBranch)
        );
        assert_eq!(selection.base_ref.as_deref(), Some("feature/shared"));
        assert!(selection.source_pull_request.is_none());
    }

    #[test]
    fn workspace_base_selection_uses_pr_head_for_pr_backed_linked_workspace() {
        let mut workspace = AgentConversationWorkspace::new(
            ChatConversationId::from_string("conversation-1"),
            ProjectId::from_string("project-1".to_string()),
            AgentConversationWorkspaceMode::Edit,
            IdeationAnalysisBaseRefKind::ProjectDefault,
            "main".to_string(),
            Some("PR #42: Add linked PR".to_string()),
            Some("base123".to_string()),
            "feature/linked-pr".to_string(),
            "/tmp/worktree".to_string(),
        );
        workspace.branch_mode = AgentConversationWorkspaceBranchMode::Linked;
        workspace.source_pull_request = Some(AgentWorkspaceSourcePullRequest {
            number: 42,
            url: Some("https://github.test/pull/42".to_string()),
            title: Some("Add linked PR".to_string()),
            head_ref_name: "feature/linked-pr".to_string(),
            base_ref_name: Some("main".to_string()),
            head_ref_oid: Some("head123".to_string()),
        });

        let selection = workspace_base_selection(Some(&workspace));

        assert_eq!(
            selection.kind,
            Some(IdeationAnalysisBaseRefKind::LocalBranch)
        );
        assert_eq!(
            selection.branch_mode,
            Some(AgentConversationWorkspaceBranchMode::Linked)
        );
        assert_eq!(selection.base_ref.as_deref(), Some("feature/linked-pr"));
        assert_eq!(
            selection
                .source_pull_request
                .as_ref()
                .and_then(|pull_request| pull_request.base_ref_name.as_deref()),
            Some("main")
        );
    }

    #[test]
    fn cloned_message_rewrites_parent_link_and_provider_session() {
        let parent_provider_ref = ProviderSessionRef {
            harness: crate::domain::agents::AgentHarnessKind::Codex,
            provider_session_id: "parent-session".to_string(),
        };
        let child_provider_ref = ProviderSessionRef {
            harness: crate::domain::agents::AgentHarnessKind::Codex,
            provider_session_id: "child-session".to_string(),
        };
        let child_conversation =
            ChatConversation::new_project(ProjectId::from_string("project-1".to_string()));
        let old_parent_id = ChatMessageId::from_string("old-parent");
        let new_parent_id = ChatMessageId::from_string("new-parent");
        let mut message =
            ChatMessage::user_in_project(ProjectId::from_string("project-1".to_string()), "hello");
        message.role = MessageRole::Orchestrator;
        message.parent_message_id = Some(old_parent_id.clone());
        message.provider_harness = Some(parent_provider_ref.harness);
        message.provider_session_id = Some(parent_provider_ref.provider_session_id.clone());
        let message_id_map = HashMap::from([(old_parent_id, new_parent_id.clone())]);

        let cloned = clone_message_for_child(
            message,
            &child_conversation,
            &message_id_map,
            Some(&parent_provider_ref),
            Some(&child_provider_ref),
        );

        assert_eq!(cloned.conversation_id, Some(child_conversation.id.clone()));
        assert_eq!(cloned.parent_message_id, Some(new_parent_id));
        assert_eq!(cloned.provider_session_id.as_deref(), Some("child-session"));
    }

    #[tokio::test]
    async fn forks_chat_conversation_and_copies_message_timeline_attribution() {
        let state = AppState::new_test();
        let project = create_project(&state, "/tmp/project").await;
        let parent =
            create_parent_conversation(&state, &project, AgentConversationWorkspaceMode::Chat)
                .await;
        let provider_ref = ProviderSessionRef {
            harness: AgentHarnessKind::Codex,
            provider_session_id: "parent-session".to_string(),
        };

        let mut user = ChatMessage::user_in_project(project.id.clone(), "hello");
        user.conversation_id = Some(parent.id.clone());
        user.update_provider_session_ref(&provider_ref);
        let user = state
            .chat_message_repo
            .create(user)
            .await
            .expect("create user message");

        let mut assistant = ChatMessage::user_in_project(project.id.clone(), "answer");
        assistant.role = MessageRole::Orchestrator;
        assistant.conversation_id = Some(parent.id.clone());
        assistant.parent_message_id = Some(user.id.clone());
        assistant.update_provider_session_ref(&provider_ref);
        let assistant = state
            .chat_message_repo
            .create(assistant)
            .await
            .expect("create assistant message");

        let mut timeline = ChatTimelineItem::for_message_block(
            assistant.id.clone(),
            parent.id.clone(),
            0,
            MessageRole::Orchestrator,
            ChatTimelineItemKind::Text,
        );
        timeline.status = ChatTimelineItemStatus::Finalized;
        timeline.provider_harness = Some(provider_ref.harness);
        timeline.provider_session_id = Some(provider_ref.provider_session_id.clone());
        state
            .chat_timeline_repo
            .upsert_item(timeline)
            .await
            .expect("create timeline item");

        let result = fork_agent_conversation(&state, &parent.id)
            .await
            .expect("fork conversation");

        assert_eq!(result.copied_message_count, 2);
        assert_eq!(result.copied_timeline_item_count, 1);
        assert!(result.workspace.is_none());
        assert!(result.provider_session.is_none());
        assert_eq!(
            result.conversation.parent_conversation_id,
            Some(parent.id.as_str())
        );
        assert_eq!(
            result.conversation.title.as_deref(),
            Some("[Fork] Build fork flow")
        );
        assert_eq!(
            result.conversation.agent_mode,
            Some(AgentConversationWorkspaceMode::Chat)
        );

        let child_messages = state
            .chat_message_repo
            .get_by_conversation(&result.conversation.id)
            .await
            .expect("load child messages");
        assert_eq!(child_messages.len(), 2);
        assert!(child_messages.iter().all(|message| {
            message.session_id.is_none()
                && message.task_id.is_none()
                && message.project_id.as_ref() == Some(&project.id)
                && message.conversation_id.as_ref() == Some(&result.conversation.id)
        }));
        let child_user = child_messages
            .iter()
            .find(|message| message.role == MessageRole::User)
            .expect("child user message");
        let child_assistant = child_messages
            .iter()
            .find(|message| message.role == MessageRole::Orchestrator)
            .expect("child assistant message");
        assert_eq!(
            child_assistant.parent_message_id.as_ref(),
            Some(&child_user.id)
        );
        assert_eq!(
            child_assistant.provider_session_id.as_deref(),
            Some("parent-session")
        );

        let child_timeline = state
            .chat_timeline_repo
            .get_by_conversation(&result.conversation.id)
            .await
            .expect("load child timeline");
        assert_eq!(child_timeline.len(), 1);
        assert_eq!(
            child_timeline[0].message_id.as_ref(),
            Some(&child_assistant.id)
        );
        assert_eq!(child_timeline[0].run_id, None);
        assert_eq!(
            child_timeline[0].id,
            ChatTimelineItem::stable_message_block_id(&child_assistant.id, 0)
        );
    }

    #[tokio::test]
    async fn forks_edit_conversation_with_deferred_workspace() {
        let state = AppState::new_test();
        let temp_dir = tempfile::tempdir().expect("create temp dir");
        let repo_path = temp_dir.path().join("repo");
        init_repo(&repo_path);
        let mut project = create_project(&state, repo_path.to_string_lossy().as_ref()).await;
        project.worktree_parent_directory = Some(
            temp_dir
                .path()
                .join("worktrees")
                .to_string_lossy()
                .to_string(),
        );
        state
            .project_repo
            .update(&project)
            .await
            .expect("update project");
        let parent =
            create_parent_conversation(&state, &project, AgentConversationWorkspaceMode::Edit)
                .await;

        let result = fork_agent_conversation(&state, &parent.id)
            .await
            .expect("fork edit conversation");
        let workspace = result.workspace.expect("child workspace");

        assert_eq!(workspace.conversation_id, result.conversation.id);
        assert_eq!(workspace.project_id, project.id);
        assert_eq!(workspace.mode, AgentConversationWorkspaceMode::Edit);
        assert_eq!(
            workspace.base_ref_kind,
            IdeationAnalysisBaseRefKind::ProjectDefault
        );
        assert_eq!(workspace.base_ref, "main");
        assert!(std::path::Path::new(&workspace.worktree_path).exists());
        assert!(state
            .agent_conversation_workspace_repo
            .get_by_conversation_id(&result.conversation.id)
            .await
            .expect("load workspace")
            .is_some());
    }

    #[tokio::test]
    async fn rejects_non_project_parent_conversation() {
        let state = AppState::new_test();
        let parent = ChatConversation::new_ideation(
            crate::domain::entities::IdeationSessionId::from_string("session-1"),
        );
        let parent = state
            .chat_conversation_repo
            .create(parent)
            .await
            .expect("create parent conversation");

        let error = fork_agent_conversation(&state, &parent.id)
            .await
            .expect_err("ideation conversation should not fork");

        assert!(matches!(error, AppError::Validation(_)));
    }

    #[tokio::test]
    async fn rejects_running_parent_conversation() {
        let state = AppState::new_test();
        let project = create_project(&state, "/tmp/project").await;
        let parent =
            create_parent_conversation(&state, &project, AgentConversationWorkspaceMode::Chat)
                .await;
        state
            .running_agent_registry
            .register(
                RunningAgentKey::new("project", parent.id.as_str()),
                0,
                parent.id.as_str(),
                "run-1".to_string(),
                None,
                None,
            )
            .await;

        let error = fork_agent_conversation(&state, &parent.id)
            .await
            .expect_err("running conversation should not fork");

        assert!(matches!(error, AppError::Conflict(_)));
    }
}
