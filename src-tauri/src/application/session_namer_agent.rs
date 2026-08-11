use std::path::PathBuf;
use std::sync::Arc;

use crate::application::harness_runtime_registry::{
    default_repo_root_working_directory, resolve_harness_agent_bootstrap,
};
use crate::application::session_namer_prompt::build_session_namer_prompt;
use crate::application::standalone_workspace;
use crate::application::AppState;
use crate::domain::agents::{
    AgentConfig, AgentHarnessKind, AgentOutput, AgentRole, AgenticClient, RoutingRole,
    DEFAULT_AGENT_HARNESS,
};
use crate::domain::entities::{
    AgentConversationWorkspaceMode, AgentWorkspaceSourcePullRequest, ChatContextType,
    ChatConversation, ChatConversationId, ChatMessage, DelegatedSessionId, IdeationSession,
    IdeationSessionFlow, IdeationSessionId, ProjectId, TaskId,
};
use crate::domain::repositories::{
    AgentConversationWorkspaceRepository, ChatConversationRepository, IdeationSessionRepository,
};
use crate::error::{AppError, AppResult};
use crate::infrastructure::agents::claude::agent_names;
use tauri::Emitter;

const AUTO_TITLE_SOURCE: &str = "auto";
const USER_TITLE_SOURCE: &str = "user";
const AGENT_CONVERSATION_SOURCE_CONTEXT: &str = "agent_conversation";
const CONVERSATION_CONTEXT_MESSAGE_LIMIT: u32 = 6;
const CONVERSATION_CONTEXT_MESSAGE_CHARS: usize = 600;

#[derive(Debug, Clone)]
pub(crate) enum SessionNamerTarget {
    SessionInitial {
        session_id: String,
        user_message: String,
    },
    ConversationInitial {
        conversation_id: String,
        user_message: String,
        requested_harness: Option<AgentHarnessKind>,
        service_tier_override: Option<String>,
    },
    AcceptedSession {
        session_id: String,
        accepted_proposals: String,
    },
}

pub(crate) struct SessionNamerAgentSpawn {
    pub client: Arc<dyn AgenticClient>,
    pub config: AgentConfig,
    pub target_label: String,
    pub project_id: Option<String>,
    pub harness_for_log: Option<AgentHarnessKind>,
    target: SessionNamerTarget,
    title_updater: SessionNamerTitleUpdater,
}

#[derive(Clone)]
struct SessionNamerTitleUpdater {
    ideation_session_repo: Arc<dyn IdeationSessionRepository>,
    chat_conversation_repo: Arc<dyn ChatConversationRepository>,
    agent_conversation_workspace_repo: Arc<dyn AgentConversationWorkspaceRepository>,
    app_handle: Option<tauri::AppHandle>,
}

pub(crate) async fn spawn_session_namer_agent(
    state: &AppState,
    target: SessionNamerTarget,
) -> AppResult<()> {
    let spawn = match build_session_namer_agent_spawn(state, target).await {
        Ok(spawn) => spawn,
        Err(AppError::SessionNamerStandaloneWorkspaceUnavailable {
            conversation_id,
            detail,
        }) => {
            tracing::warn!(
                conversation_id,
                detail,
                "Skipping standalone session namer because its app-owned workspace is unavailable"
            );
            return Ok(());
        }
        Err(error) => return Err(error),
    };

    tokio::spawn(async move {
        tracing::info!(
            target = %spawn.target_label,
            project_id = spawn.project_id.as_deref().unwrap_or(""),
            harness = ?spawn.harness_for_log,
            "Spawning session namer agent"
        );
        match spawn.client.spawn_agent(spawn.config).await {
            Ok(handle) => match spawn.client.wait_for_completion(&handle).await {
                Ok(output) => {
                    if let Err(error) = spawn
                        .title_updater
                        .persist_output_title(&spawn.target, &output)
                        .await
                    {
                        tracing::warn!(
                            target = %spawn.target_label,
                            error = %error,
                            "Session namer title persistence failed"
                        );
                    }
                }
                Err(error) => {
                    tracing::warn!("Session namer agent failed: {}", error);
                }
            },
            Err(error) => {
                tracing::warn!("Failed to spawn session namer agent: {}", error);
            }
        }
    });

    Ok(())
}

pub(crate) async fn build_session_namer_agent_spawn(
    state: &AppState,
    target: SessionNamerTarget,
) -> AppResult<SessionNamerAgentSpawn> {
    let target_label = target.target_label();
    let resolved = resolve_target_context(state, &target).await?;
    let prompt = target.prompt(
        resolved.review_pull_request.as_ref(),
        resolved.conversation_context.as_deref(),
    );
    let working_directory = resolve_project_working_directory(
        state,
        resolved.project_id.as_deref(),
        resolved.standalone_conversation_id.as_deref(),
    )
    .await?;
    let harness_override = match &target {
        SessionNamerTarget::ConversationInitial {
            requested_harness, ..
        } => requested_harness.or(resolved.conversation_harness),
        _ => None,
    };
    let mut runtime = state
        .resolve_manual_role_background_agent_runtime(
            resolved.project_id.as_deref(),
            resolved
                .project_id
                .as_ref()
                .map(|_| working_directory.as_path()),
            RoutingRole::UtilityLightweight,
            None,
            agent_names::AGENT_SESSION_NAMER,
            "session namer",
            harness_override,
        )
        .await?;
    if let SessionNamerTarget::ConversationInitial {
        service_tier_override: Some(service_tier_override),
        ..
    } = &target
    {
        runtime.service_tier = normalize_session_namer_service_tier_override(service_tier_override);
    }

    let bootstrap = resolve_harness_agent_bootstrap(
        runtime.harness.unwrap_or(DEFAULT_AGENT_HARNESS),
        agent_names::AGENT_SESSION_NAMER,
        working_directory,
    );
    let env = runtime.env_with_overrides(bootstrap.env);

    let config = AgentConfig {
        role: AgentRole::Custom(bootstrap.agent_role.clone()),
        prompt,
        working_directory: bootstrap.working_directory,
        plugin_dir: Some(bootstrap.plugin_dir),
        agent: Some(bootstrap.agent_name),
        model: runtime.model,
        harness: runtime.harness,
        cli_path_override: runtime.cli_path_override,
        logical_effort: runtime.logical_effort,
        approval_policy: runtime.approval_policy,
        sandbox_mode: runtime.sandbox_mode,
        service_tier: runtime.service_tier,
        max_tokens: None,
        timeout_secs: Some(60),
        env,
        mcp_launch_policy: Default::default(),
    };

    Ok(SessionNamerAgentSpawn {
        client: runtime.client,
        config,
        target_label,
        project_id: resolved.project_id,
        harness_for_log: runtime.harness,
        target,
        title_updater: SessionNamerTitleUpdater::from_state(state),
    })
}

struct ResolvedSessionNamerTarget {
    project_id: Option<String>,
    conversation_harness: Option<AgentHarnessKind>,
    review_pull_request: Option<AgentWorkspaceSourcePullRequest>,
    conversation_context: Option<String>,
    standalone_conversation_id: Option<String>,
}

async fn resolve_target_context(
    state: &AppState,
    target: &SessionNamerTarget,
) -> AppResult<ResolvedSessionNamerTarget> {
    match target {
        SessionNamerTarget::SessionInitial { session_id, .. }
        | SessionNamerTarget::AcceptedSession { session_id, .. } => {
            let session = load_session(state, session_id).await?;
            let project_id = Some(session.project_id.as_str().to_string());
            Ok(ResolvedSessionNamerTarget {
                project_id,
                conversation_harness: None,
                review_pull_request: None,
                conversation_context: None,
                standalone_conversation_id: None,
            })
        }
        SessionNamerTarget::ConversationInitial {
            conversation_id, ..
        } => {
            let conversation = state
                .chat_conversation_repo
                .get_by_id(&ChatConversationId::from_string(conversation_id.clone()))
                .await?
                .ok_or_else(|| {
                    AppError::NotFound(format!("Conversation not found: {conversation_id}"))
                })?;
            let project_id = resolve_conversation_project_id(state, &conversation).await?;
            let review_pull_request =
                resolve_review_pull_request_context(state, &conversation.id).await?;
            let conversation_context = format_conversation_context(state, &conversation).await?;
            let standalone_conversation_id = (conversation.context_type
                == ChatContextType::Standalone)
                .then(|| conversation.id.as_str());
            Ok(ResolvedSessionNamerTarget {
                project_id,
                conversation_harness: conversation.provider_harness,
                review_pull_request,
                conversation_context,
                standalone_conversation_id,
            })
        }
    }
}

async fn format_conversation_context(
    state: &AppState,
    conversation: &ChatConversation,
) -> AppResult<Option<String>> {
    let messages = state
        .chat_message_repo
        .get_recent_by_conversation_paginated(
            &conversation.id,
            CONVERSATION_CONTEXT_MESSAGE_LIMIT,
            0,
        )
        .await?;
    if conversation.parent_conversation_id.is_none() && messages.is_empty() {
        return Ok(None);
    }

    let mut context = String::from("\n<conversation_context>");
    append_optional_xml_tag(
        &mut context,
        "parent_conversation_id",
        conversation.parent_conversation_id.as_deref(),
    );
    append_optional_xml_tag(&mut context, "current_title", conversation.title.as_deref());
    append_recent_messages_context(&mut context, messages);
    context.push_str("\n</conversation_context>");
    Ok(Some(context))
}

fn append_recent_messages_context(context: &mut String, messages: Vec<ChatMessage>) {
    let mut wrote_messages = false;
    for message in messages {
        let content = compact_context_text(&message.content, CONVERSATION_CONTEXT_MESSAGE_CHARS);
        if content.is_empty() {
            continue;
        }
        if !wrote_messages {
            context.push_str("\n<recent_messages>");
            wrote_messages = true;
        }
        let role = message.role.to_string();
        context.push_str("\n<message>");
        append_optional_xml_tag(context, "role", Some(role.as_str()));
        append_optional_xml_tag(context, "content", Some(content.as_str()));
        context.push_str("\n</message>");
    }
    if wrote_messages {
        context.push_str("\n</recent_messages>");
    }
}

fn compact_context_text(value: &str, max_chars: usize) -> String {
    let compact = value.split_whitespace().collect::<Vec<_>>().join(" ");
    if compact.chars().count() <= max_chars {
        return compact;
    }

    let mut truncated = compact.chars().take(max_chars).collect::<String>();
    if let Some((prefix, _)) = truncated.rsplit_once(char::is_whitespace) {
        if !prefix.trim().is_empty() {
            truncated = prefix.trim_end().to_string();
        }
    }
    truncated.push_str("...");
    truncated
}

async fn resolve_review_pull_request_context(
    state: &AppState,
    conversation_id: &ChatConversationId,
) -> AppResult<Option<AgentWorkspaceSourcePullRequest>> {
    let Some(workspace) = state
        .agent_conversation_workspace_repo
        .get_by_conversation_id(conversation_id)
        .await?
    else {
        return Ok(None);
    };
    if workspace.mode != AgentConversationWorkspaceMode::ReviewPr {
        return Ok(None);
    }
    Ok(workspace.source_pull_request)
}

async fn load_session(state: &AppState, session_id: &str) -> AppResult<IdeationSession> {
    state
        .ideation_session_repo
        .get_by_id(&IdeationSessionId::from_string(session_id.to_string()))
        .await?
        .ok_or_else(|| AppError::NotFound(format!("Ideation session not found: {session_id}")))
}

async fn resolve_conversation_project_id(
    state: &AppState,
    conversation: &ChatConversation,
) -> AppResult<Option<String>> {
    match conversation.context_type {
        ChatContextType::Project => Ok(Some(conversation.context_id.clone())),
        ChatContextType::Standalone => Ok(None),
        ChatContextType::Ideation => {
            let session = load_session(state, &conversation.context_id).await?;
            Ok(Some(session.project_id.as_str().to_string()))
        }
        ChatContextType::Task
        | ChatContextType::TaskExecution
        | ChatContextType::Review
        | ChatContextType::Merge
        | ChatContextType::BranchUpdate => {
            let task = state
                .task_repo
                .get_by_id(&TaskId::from_string(conversation.context_id.clone()))
                .await?;
            Ok(task.map(|task| task.project_id.as_str().to_string()))
        }
        ChatContextType::Delegation => {
            let delegated = state
                .delegated_session_repo
                .get_by_id(&DelegatedSessionId::from_string(
                    conversation.context_id.clone(),
                ))
                .await?;
            Ok(delegated.map(|session| session.project_id.as_str().to_string()))
        }
    }
}

async fn resolve_project_working_directory(
    state: &AppState,
    project_id: Option<&str>,
    standalone_conversation_id: Option<&str>,
) -> AppResult<PathBuf> {
    if let Some(conversation_id) = standalone_conversation_id {
        return standalone_workspace::resolve_workspace(
            state.app_paths.app_data_dir(),
            conversation_id,
        )
        .map_err(
            |error| AppError::SessionNamerStandaloneWorkspaceUnavailable {
                conversation_id: conversation_id.to_string(),
                detail: error.to_string(),
            },
        );
    }
    let Some(project_id) = project_id else {
        return Ok(default_repo_root_working_directory());
    };

    let project = state
        .project_repo
        .get_by_id(&ProjectId::from_string(project_id.to_string()))
        .await?
        .ok_or_else(|| AppError::NotFound(format!("Project not found: {project_id}")))?;
    Ok(PathBuf::from(project.working_directory))
}

impl SessionNamerTarget {
    pub(crate) fn session_initial(
        session_id: impl Into<String>,
        user_message: impl Into<String>,
    ) -> Self {
        Self::SessionInitial {
            session_id: session_id.into(),
            user_message: user_message.into(),
        }
    }

    pub(crate) fn accepted_session(
        session_id: impl Into<String>,
        accepted_proposals: impl Into<String>,
    ) -> Self {
        Self::AcceptedSession {
            session_id: session_id.into(),
            accepted_proposals: accepted_proposals.into(),
        }
    }

    pub(crate) fn from_initial_request(
        session_id: Option<String>,
        conversation_id: Option<String>,
        user_message: String,
        requested_harness: Option<AgentHarnessKind>,
        service_tier_override: Option<String>,
    ) -> Result<Self, &'static str> {
        match (session_id, conversation_id) {
            (Some(session_id), None) => Ok(Self::session_initial(session_id, user_message)),
            (None, Some(conversation_id)) => Ok(Self::ConversationInitial {
                conversation_id,
                user_message,
                requested_harness,
                service_tier_override,
            }),
            (Some(_), Some(_)) | (None, None) => {
                Err("spawn_session_namer requires exactly one of sessionId or conversationId")
            }
        }
    }

    fn prompt(
        &self,
        review_pull_request: Option<&AgentWorkspaceSourcePullRequest>,
        conversation_context: Option<&str>,
    ) -> String {
        match self {
            Self::SessionInitial {
                session_id,
                user_message,
            } => build_session_namer_prompt(&format!(
                "<session_id>{session_id}</session_id>\n<user_message>{user_message}</user_message>"
            )),
            Self::ConversationInitial {
                conversation_id,
                user_message,
                ..
            } => {
                let review_pull_request_context = review_pull_request
                    .map(format_review_pull_request_context)
                    .unwrap_or_default();
                let conversation_context = conversation_context.unwrap_or_default();
                build_session_namer_prompt(&format!(
                    "<conversation_id>{conversation_id}</conversation_id>\n<user_message>{user_message}</user_message>{conversation_context}{review_pull_request_context}"
                ))
            }
            Self::AcceptedSession {
                session_id,
                accepted_proposals,
            } => build_session_namer_prompt(&format!(
                "<session_id>{session_id}</session_id>\n<accepted_proposals>{accepted_proposals}</accepted_proposals>"
            )),
        }
    }

    fn target_label(&self) -> String {
        match self {
            Self::SessionInitial { session_id, .. } | Self::AcceptedSession { session_id, .. } => {
                format!("session:{session_id}")
            }
            Self::ConversationInitial {
                conversation_id, ..
            } => format!("conversation:{conversation_id}"),
        }
    }
}

fn normalize_session_namer_service_tier_override(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() || trimmed.eq_ignore_ascii_case("standard") {
        return Some("standard".to_string());
    }
    Some(trimmed.to_ascii_lowercase())
}

fn format_review_pull_request_context(pull_request: &AgentWorkspaceSourcePullRequest) -> String {
    let mut context = format!(
        "\n<review_pull_request>\n<number>{}</number>",
        pull_request.number
    );
    append_optional_xml_tag(&mut context, "title", pull_request.title.as_deref());
    append_optional_xml_tag(
        &mut context,
        "head_ref_name",
        Some(pull_request.head_ref_name.as_str()),
    );
    append_optional_xml_tag(
        &mut context,
        "base_ref_name",
        pull_request.base_ref_name.as_deref(),
    );
    append_optional_xml_tag(
        &mut context,
        "head_ref_oid",
        pull_request.head_ref_oid.as_deref(),
    );
    append_optional_xml_tag(&mut context, "url", pull_request.url.as_deref());
    context.push_str("\n</review_pull_request>");
    context
}

fn append_optional_xml_tag(context: &mut String, tag: &str, value: Option<&str>) {
    let Some(value) = value.map(str::trim).filter(|value| !value.is_empty()) else {
        return;
    };
    context.push('\n');
    context.push('<');
    context.push_str(tag);
    context.push('>');
    context.push_str(&escape_xml_text(value));
    context.push_str("</");
    context.push_str(tag);
    context.push('>');
}

fn escape_xml_text(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

impl SessionNamerTitleUpdater {
    fn from_state(state: &AppState) -> Self {
        Self {
            ideation_session_repo: Arc::clone(&state.ideation_session_repo),
            chat_conversation_repo: Arc::clone(&state.chat_conversation_repo),
            agent_conversation_workspace_repo: Arc::clone(&state.agent_conversation_workspace_repo),
            app_handle: state.app_handle.clone(),
        }
    }

    async fn persist_output_title(
        &self,
        target: &SessionNamerTarget,
        output: &AgentOutput,
    ) -> AppResult<()> {
        let Some(title) = extract_session_namer_title(&output.content) else {
            tracing::warn!(
                success = output.success,
                exit_code = output.exit_code,
                content_len = output.content.len(),
                "Session namer completed without a parseable title"
            );
            return Ok(());
        };

        match target {
            SessionNamerTarget::SessionInitial { session_id, .. }
            | SessionNamerTarget::AcceptedSession { session_id, .. } => {
                let session_id = IdeationSessionId::from_string(session_id.clone());
                self.ideation_session_repo
                    .update_title(&session_id, Some(title.clone()), AUTO_TITLE_SOURCE)
                    .await?;
                if let Some(app_handle) = &self.app_handle {
                    let _ = app_handle.emit(
                        "ideation:session_title_updated",
                        serde_json::json!({
                            "sessionId": session_id.as_str(),
                            "title": title,
                        }),
                    );
                }
            }
            SessionNamerTarget::ConversationInitial {
                conversation_id, ..
            } => {
                let conversation_id = ChatConversationId::from_string(conversation_id.clone());
                self.chat_conversation_repo
                    .update_title(&conversation_id, &title)
                    .await?;
                let conversation = self
                    .chat_conversation_repo
                    .get_by_id(&conversation_id)
                    .await?;
                let synced_planning_session_id = self
                    .sync_linked_planning_session_title_from_conversation(&conversation_id, &title)
                    .await?;

                if let Some(app_handle) = &self.app_handle {
                    if let Some(session_id) = synced_planning_session_id {
                        let _ = app_handle.emit(
                            "ideation:session_title_updated",
                            serde_json::json!({
                                "sessionId": session_id.as_str(),
                                "title": title,
                            }),
                        );
                    }
                    if let Some(conversation) = conversation {
                        let _ = app_handle.emit(
                            "agent:conversation_title_updated",
                            serde_json::json!({
                                "conversationId": conversation.id.as_str(),
                                "contextType": conversation.context_type.to_string(),
                                "contextId": conversation.context_id,
                                "title": title,
                            }),
                        );
                    }
                }
            }
        }

        Ok(())
    }

    async fn sync_linked_planning_session_title_from_conversation(
        &self,
        conversation_id: &ChatConversationId,
        title: &str,
    ) -> AppResult<Option<IdeationSessionId>> {
        let Some(workspace) = self
            .agent_conversation_workspace_repo
            .get_by_conversation_id(conversation_id)
            .await?
        else {
            return Ok(None);
        };
        let Some(session_id) = workspace.linked_ideation_session_id.as_ref() else {
            return Ok(None);
        };
        let Some(session) = self.ideation_session_repo.get_by_id(session_id).await? else {
            return Ok(None);
        };
        if session.session_flow != IdeationSessionFlow::Planning {
            return Ok(None);
        }
        if session.source_context_type.as_deref() != Some(AGENT_CONVERSATION_SOURCE_CONTEXT) {
            return Ok(None);
        }
        if session.title_source.as_deref() == Some(USER_TITLE_SOURCE) {
            return Ok(None);
        }

        self.ideation_session_repo
            .update_title(
                session_id,
                Some(title.trim().to_string()),
                AUTO_TITLE_SOURCE,
            )
            .await?;
        Ok(Some(session_id.clone()))
    }
}

pub(crate) fn extract_session_namer_title(output: &str) -> Option<String> {
    let trimmed = output.trim();
    if trimmed.is_empty() {
        return None;
    }

    if let Some(title) = extract_title_from_json_text(trimmed) {
        return Some(title);
    }

    for line in trimmed.lines() {
        if let Some(title) = extract_title_from_json_text(line.trim()) {
            return Some(title);
        }
    }

    extract_title_from_text(trimmed)
}

fn extract_title_from_json_text(text: &str) -> Option<String> {
    let value: serde_json::Value = serde_json::from_str(text).ok()?;
    extract_title_from_json_value(&value)
}

fn extract_title_from_json_value(value: &serde_json::Value) -> Option<String> {
    if let Some(title) = value.get("title").and_then(|value| value.as_str()) {
        if let Some(title) = normalize_title_candidate(title) {
            return Some(title);
        }
    }

    if let Some(result) = value.get("result").and_then(|value| value.as_str()) {
        if let Some(title) = extract_title_from_text(result) {
            return Some(title);
        }
    }

    if let Some(content) = value
        .get("message")
        .and_then(|message| message.get("content"))
        .and_then(|content| content.as_array())
    {
        for block in content {
            if block.get("type").and_then(|value| value.as_str()) == Some("tool_use") {
                if let Some(title) = block
                    .get("input")
                    .and_then(|input| input.get("title"))
                    .and_then(|title| title.as_str())
                    .and_then(normalize_title_candidate)
                {
                    return Some(title);
                }
            }
            if let Some(title) = block
                .get("text")
                .and_then(|text| text.as_str())
                .and_then(extract_title_from_text)
            {
                return Some(title);
            }
        }
    }

    None
}

fn extract_title_from_text(text: &str) -> Option<String> {
    if let Some(title) = extract_between(text, "<parameter name=\"title\">", "</parameter>")
        .and_then(normalize_title_candidate)
    {
        return Some(title);
    }

    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        if let Some((_, rest)) = line.split_once("Generated Title:") {
            if let Some(title) =
                extract_backticked(rest).or_else(|| normalize_title_candidate(rest))
            {
                return Some(title);
            }
        }
        if let Some((_, rest)) = line.split_once("Title:") {
            if let Some(title) =
                extract_backticked(rest).or_else(|| normalize_title_candidate(rest))
            {
                return Some(title);
            }
        }
    }

    let non_empty_lines: Vec<&str> = text
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect();
    if non_empty_lines.len() == 1 {
        return normalize_title_candidate(non_empty_lines[0]);
    }

    None
}

fn extract_between<'a>(text: &'a str, start: &str, end: &str) -> Option<&'a str> {
    let start_idx = text.find(start)? + start.len();
    let end_idx = text[start_idx..].find(end)? + start_idx;
    Some(&text[start_idx..end_idx])
}

fn extract_backticked(text: &str) -> Option<String> {
    let start = text.find('`')? + 1;
    let end = text[start..].find('`')? + start;
    normalize_title_candidate(&text[start..end])
}

fn normalize_title_candidate(candidate: &str) -> Option<String> {
    let mut title = candidate.trim();
    title = title.trim_matches(|c| matches!(c, '`' | '"' | '\'' | '*' | ' '));
    title = title
        .strip_prefix("Generated Title:")
        .unwrap_or(title)
        .trim();
    title = title.strip_prefix("Title:").unwrap_or(title).trim();
    title = title.trim_matches(|c| matches!(c, '`' | '"' | '\'' | '*' | ' '));

    if title.is_empty()
        || title.contains('\n')
        || title.contains('\r')
        || title.contains("<invoke")
        || title.contains("</invoke>")
    {
        return None;
    }

    let char_count = title.chars().count();
    if char_count > 80 {
        return None;
    }
    let title = if char_count > 50 {
        title.chars().take(50).collect::<String>()
    } else {
        title.to_string()
    };
    Some(title.trim().to_string()).filter(|title| !title.is_empty())
}
