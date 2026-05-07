use std::path::PathBuf;
use std::sync::Arc;

use crate::application::harness_runtime_registry::{
    default_repo_root_working_directory, resolve_harness_agent_bootstrap,
};
use crate::application::session_namer_prompt::build_session_namer_prompt;
use crate::application::AppState;
use crate::domain::agents::{
    AgentConfig, AgentHarnessKind, AgentRole, AgenticClient, DEFAULT_AGENT_HARNESS,
};
use crate::domain::entities::{
    ChatContextType, ChatConversation, ChatConversationId, DelegatedSessionId, IdeationSession,
    IdeationSessionId, ProjectId, TaskId,
};
use crate::error::{AppError, AppResult};
use crate::infrastructure::agents::claude::agent_names;

#[derive(Debug, Clone)]
pub(crate) enum SessionNamerTarget {
    SessionInitial {
        session_id: String,
        user_message: String,
    },
    ConversationInitial {
        conversation_id: String,
        user_message: String,
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
}

pub(crate) async fn spawn_session_namer_agent(
    state: &AppState,
    target: SessionNamerTarget,
) -> AppResult<()> {
    let spawn = build_session_namer_agent_spawn(state, target).await?;

    tokio::spawn(async move {
        tracing::info!(
            target = %spawn.target_label,
            project_id = spawn.project_id.as_deref().unwrap_or(""),
            harness = ?spawn.harness_for_log,
            "Spawning session namer agent"
        );
        match spawn.client.spawn_agent(spawn.config).await {
            Ok(handle) => {
                if let Err(error) = spawn.client.wait_for_completion(&handle).await {
                    tracing::warn!("Session namer agent failed: {}", error);
                }
            }
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
    let prompt = target.prompt();
    let target_label = target.target_label();
    let resolved = resolve_target_context(state, &target).await?;
    let working_directory =
        resolve_project_working_directory(state, resolved.project_id.as_deref()).await?;

    let bootstrap = resolve_harness_agent_bootstrap(
        resolved.runtime.harness.unwrap_or(DEFAULT_AGENT_HARNESS),
        agent_names::AGENT_SESSION_NAMER,
        working_directory,
    );

    let config = AgentConfig {
        role: AgentRole::Custom(bootstrap.agent_role.clone()),
        prompt,
        working_directory: bootstrap.working_directory,
        plugin_dir: Some(bootstrap.plugin_dir),
        agent: Some(bootstrap.agent_name),
        model: resolved.runtime.model,
        harness: resolved.runtime.harness,
        logical_effort: resolved.runtime.logical_effort,
        approval_policy: resolved.runtime.approval_policy,
        sandbox_mode: resolved.runtime.sandbox_mode,
        max_tokens: None,
        timeout_secs: Some(60),
        env: bootstrap.env,
    };

    Ok(SessionNamerAgentSpawn {
        client: resolved.client,
        config,
        target_label,
        project_id: resolved.project_id,
        harness_for_log: resolved.harness_for_log,
    })
}

struct ResolvedSessionNamerTarget {
    client: Arc<dyn AgenticClient>,
    runtime: super::app_state::ResolvedBackgroundAgentRuntime,
    project_id: Option<String>,
    harness_for_log: Option<AgentHarnessKind>,
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
            let runtime = state
                .resolve_session_namer_runtime_for_session(&session)
                .await?;
            let client = Arc::clone(&runtime.client);
            let harness_for_log = runtime.harness;
            Ok(ResolvedSessionNamerTarget {
                client,
                runtime,
                project_id,
                harness_for_log,
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
            let runtime = state
                .resolve_session_namer_runtime_for_conversation(
                    &conversation,
                    project_id.as_deref(),
                )
                .await?;
            let client = Arc::clone(&runtime.client);
            let harness_for_log = runtime.harness;
            Ok(ResolvedSessionNamerTarget {
                client,
                runtime,
                project_id,
                harness_for_log,
            })
        }
    }
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
        ChatContextType::Ideation => {
            let session = load_session(state, &conversation.context_id).await?;
            Ok(Some(session.project_id.as_str().to_string()))
        }
        ChatContextType::Task
        | ChatContextType::TaskExecution
        | ChatContextType::Review
        | ChatContextType::Merge => {
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
) -> AppResult<PathBuf> {
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
    fn prompt(&self) -> String {
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
            } => build_session_namer_prompt(&format!(
                "<conversation_id>{conversation_id}</conversation_id>\n<user_message>{user_message}</user_message>"
            )),
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
