use std::collections::HashMap;
use std::path::PathBuf;

use crate::application::agent_conversation_workspace::{
    prepare_agent_conversation_workspace_with_setup_mode, AgentConversationWorkspaceBaseSelection,
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

    let mut child_conversation = ChatConversation::new_project(project_id.clone());
    child_conversation.parent_conversation_id = Some(parent_conversation.id.as_str().to_string());
    child_conversation.set_title(forked_conversation_title(
        parent_conversation.title.as_deref(),
    ));
    child_conversation.set_agent_mode(Some(mode));
    child_conversation.upstream_provider = parent_conversation.upstream_provider.clone();
    child_conversation.provider_profile = parent_conversation.provider_profile.clone();

    let workspace = if agent_mode_requires_workspace(mode) {
        let selection = workspace_base_selection(parent_workspace.as_ref());
        Some(
            prepare_agent_conversation_workspace_with_setup_mode(
                &project,
                &child_conversation.id,
                mode,
                selection,
                AgentConversationWorkspaceSetupMode::Deferred,
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

async fn validate_forkable_parent(
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
        AgentConversationWorkspaceMode::Edit | AgentConversationWorkspaceMode::Ideation
    )
}

fn workspace_base_selection(
    workspace: Option<&AgentConversationWorkspace>,
) -> AgentConversationWorkspaceBaseSelection {
    workspace
        .map(|workspace| AgentConversationWorkspaceBaseSelection {
            kind: Some(workspace.base_ref_kind),
            base_ref: Some(workspace.base_ref.clone()),
            display_name: workspace.base_display_name.clone(),
            source_pull_request: workspace.source_pull_request.clone(),
        })
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

    let mut truncated = title
        .chars()
        .take(MAX_FORK_TITLE_CHARS)
        .collect::<String>();
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
    use crate::domain::entities::{ChatMessage, MessageRole};

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
}
