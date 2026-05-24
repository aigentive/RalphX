use crate::application::AppState;
use crate::domain::entities::{
    ChatConversationId, IdeationSession, IdeationSessionFlow, IdeationSessionId,
};
use crate::error::AppResult;

const AGENT_CONVERSATION_SOURCE_CONTEXT: &str = "agent_conversation";
const AUTO_TITLE_SOURCE: &str = "auto";
const USER_TITLE_SOURCE: &str = "user";

fn title_text(title: &str) -> Option<String> {
    let title = title.trim();
    if title.is_empty() {
        None
    } else {
        Some(title.to_string())
    }
}

fn agent_conversation_id_for_planning_session(
    session: &IdeationSession,
) -> Option<ChatConversationId> {
    if session.session_flow != IdeationSessionFlow::Planning {
        return None;
    }
    if session.source_context_type.as_deref() != Some(AGENT_CONVERSATION_SOURCE_CONTEXT) {
        return None;
    }
    if session.title_source.as_deref() == Some(USER_TITLE_SOURCE) {
        return None;
    }

    session
        .source_context_id
        .as_ref()
        .map(|id| ChatConversationId::from_string(id.clone()))
}

/// Apply the owning agent conversation title to an agent Plan-mode planning session response.
///
/// This is intentionally read-side hydration: older Plan sessions can stay unmigrated while the UI
/// still renders the user-facing conversation title.
pub(crate) async fn hydrate_agent_conversation_planning_session_title(
    state: &AppState,
    mut session: IdeationSession,
) -> AppResult<IdeationSession> {
    let Some(conversation_id) = agent_conversation_id_for_planning_session(&session) else {
        return Ok(session);
    };
    let Some(conversation) = state
        .chat_conversation_repo
        .get_by_id(&conversation_id)
        .await?
    else {
        return Ok(session);
    };
    let Some(title) = conversation.title.as_deref().and_then(title_text) else {
        return Ok(session);
    };

    session.title = Some(title);
    session.title_source = Some(AUTO_TITLE_SOURCE.to_string());
    Ok(session)
}

pub(crate) async fn hydrate_agent_conversation_planning_session_titles(
    state: &AppState,
    sessions: Vec<IdeationSession>,
) -> AppResult<Vec<IdeationSession>> {
    let mut hydrated = Vec::with_capacity(sessions.len());
    for session in sessions {
        hydrated.push(hydrate_agent_conversation_planning_session_title(state, session).await?);
    }
    Ok(hydrated)
}

pub(crate) async fn sync_linked_planning_session_title_from_conversation(
    state: &AppState,
    conversation_id: &ChatConversationId,
    title: &str,
) -> AppResult<Option<IdeationSessionId>> {
    let Some(title) = title_text(title) else {
        return Ok(None);
    };
    let Some(workspace) = state
        .agent_conversation_workspace_repo
        .get_by_conversation_id(conversation_id)
        .await?
    else {
        return Ok(None);
    };
    let Some(session_id) = workspace.linked_ideation_session_id.as_ref() else {
        return Ok(None);
    };
    let Some(session) = state.ideation_session_repo.get_by_id(session_id).await? else {
        return Ok(None);
    };
    if agent_conversation_id_for_planning_session(&session).is_none() {
        return Ok(None);
    }

    state
        .ideation_session_repo
        .update_title(session_id, Some(title), AUTO_TITLE_SOURCE)
        .await?;
    Ok(Some(session_id.clone()))
}
