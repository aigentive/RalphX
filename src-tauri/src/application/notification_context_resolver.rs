use std::sync::Arc;

use crate::application::AppState;
use crate::domain::entities::{
    AgentConversationWorkspaceStatus, ChatContextType, ChatConversation, ChatConversationId,
    IdeationSession, IdeationSessionId, NotificationTarget, NotificationTargetKind, ProjectId,
    TaskId,
};
use crate::domain::repositories::{
    AgentConversationWorkspaceRepository, ChatConversationRepository, IdeationSessionRepository,
    ProjectRepository, TaskRepository,
};
use crate::error::AppResult;

/// Notification navigation and copy context resolved from authoritative state.
pub struct ResolvedNotificationTarget {
    pub project_id: Option<String>,
    pub target: NotificationTarget,
    pub context_label: Option<String>,
    pub project_name: Option<String>,
    pub context_kind: Option<ChatContextType>,
}

/// Shared resolver for notification producers and attention aggregation.
///
/// It keeps project/target resolution and the workspace-plan ownership predicates in one place so
/// durable notifications and live attention cannot drift.
pub struct NotificationContextResolver {
    task_repo: Arc<dyn TaskRepository>,
    ideation_session_repo: Arc<dyn IdeationSessionRepository>,
    chat_conversation_repo: Arc<dyn ChatConversationRepository>,
    project_repo: Arc<dyn ProjectRepository>,
    agent_workspace_repo: Arc<dyn AgentConversationWorkspaceRepository>,
}

impl NotificationContextResolver {
    pub fn from_app_state(state: &AppState) -> Self {
        Self {
            task_repo: Arc::clone(&state.task_repo),
            ideation_session_repo: Arc::clone(&state.ideation_session_repo),
            chat_conversation_repo: Arc::clone(&state.chat_conversation_repo),
            project_repo: Arc::clone(&state.project_repo),
            agent_workspace_repo: Arc::clone(&state.agent_conversation_workspace_repo),
        }
    }

    pub async fn resolve_permission_target_with_trusted_conversation(
        &self,
        task_id: Option<&str>,
        context_type: Option<&str>,
        context_id: Option<&str>,
        trusted_conversation_id: Option<&str>,
    ) -> AppResult<ResolvedNotificationTarget> {
        if let Some(conversation_id) = trusted_conversation_id {
            let expected_project_id = self
                .expected_project_id(task_id, context_type, context_id)
                .await?;
            let expected_context_type = context_type.and_then(|value| value.parse().ok());
            if let Some(resolved) = self
                .resolve_trusted_agent_conversation_target(
                    conversation_id,
                    expected_project_id.as_deref(),
                    expected_context_type,
                    context_id,
                )
                .await?
            {
                return Ok(resolved);
            }
            return Ok(unresolved_target(expected_project_id));
        }
        self.resolve_permission_target(task_id, context_id).await
    }

    pub async fn resolve_permission_target(
        &self,
        task_id: Option<&str>,
        context_id: Option<&str>,
    ) -> AppResult<ResolvedNotificationTarget> {
        if let Some(task_id) = task_id {
            if let Some(task) = self
                .task_repo
                .get_by_id(&TaskId::from_string(task_id.to_string()))
                .await?
            {
                let project_id = task.project_id.to_string();
                let project_name = self.project_name(&project_id).await;
                return Ok(ResolvedNotificationTarget {
                    project_id: Some(project_id.clone()),
                    target: NotificationTarget {
                        kind: NotificationTargetKind::Task,
                        project_id: Some(project_id),
                        task_id: Some(task.id.to_string()),
                        conversation_id: None,
                        setup_conversation_id: None,
                        automation_id: None,
                        run_id: None,
                    },
                    context_label: Some(task.title),
                    project_name,
                    context_kind: Some(ChatContextType::Task),
                });
            }
        }
        let Some(context_id) = context_id else {
            return Ok(ResolvedNotificationTarget {
                project_id: None,
                target: NotificationTarget::none(),
                context_label: None,
                project_name: None,
                context_kind: None,
            });
        };
        self.resolve_conversation_target(&ChatConversationId::from_string(context_id.to_string()))
            .await
    }

    pub async fn resolve_conversation_target(
        &self,
        conversation_id: &ChatConversationId,
    ) -> AppResult<ResolvedNotificationTarget> {
        let conversation = self
            .chat_conversation_repo
            .get_by_id(conversation_id)
            .await?;
        let Some(conversation) = conversation else {
            return Ok(ResolvedNotificationTarget {
                project_id: None,
                target: NotificationTarget::none(),
                context_label: None,
                project_name: None,
                context_kind: None,
            });
        };
        let project_id = self.project_id_for_conversation(&conversation).await?;
        let context_label = self.conversation_context_label(&conversation).await?;
        let project_name = match project_id.as_deref() {
            Some(project_id) => self.project_name(project_id).await,
            None => None,
        };
        Ok(ResolvedNotificationTarget {
            target: conversation_target(&conversation, project_id.clone()),
            project_id,
            context_label,
            project_name,
            context_kind: Some(conversation.context_type.clone()),
        })
    }

    pub async fn resolve_ideation_session_target(
        &self,
        session: &IdeationSession,
    ) -> AppResult<ResolvedNotificationTarget> {
        if let Some(workspace) = self
            .agent_workspace_repo
            .get_by_linked_ideation_session_id(&session.id)
            .await?
            .filter(|workspace| {
                workspace.status == AgentConversationWorkspaceStatus::Active
                    && workspace.project_id == session.project_id
            })
        {
            let workspace_conversation_id = workspace.conversation_id.to_string();
            if let Some(resolved) = self
                .resolve_trusted_agent_conversation_target(
                    &workspace_conversation_id,
                    Some(session.project_id.as_str()),
                    Some(ChatContextType::Ideation),
                    Some(session.id.as_str()),
                )
                .await?
            {
                return Ok(resolved);
            }
        }
        let conversation = self
            .chat_conversation_repo
            .get_by_context(ChatContextType::Ideation, session.id.as_str())
            .await?
            .into_iter()
            .max_by_key(|conversation| conversation.updated_at);
        match conversation {
            Some(conversation) => self.resolve_conversation_target(&conversation.id).await,
            None => Ok(ResolvedNotificationTarget {
                project_id: Some(session.project_id.to_string()),
                target: NotificationTarget::none(),
                context_label: session.title.clone(),
                project_name: self.project_name(session.project_id.as_str()).await,
                context_kind: Some(ChatContextType::Ideation),
            }),
        }
    }

    pub async fn resolve_context_target(
        &self,
        context_type: &str,
        context_id: &str,
    ) -> AppResult<ResolvedNotificationTarget> {
        let Ok(context_type) = context_type.parse::<ChatContextType>() else {
            return Ok(ResolvedNotificationTarget {
                project_id: None,
                target: NotificationTarget::none(),
                context_label: None,
                project_name: None,
                context_kind: None,
            });
        };
        let conversation = self
            .chat_conversation_repo
            .get_by_context(context_type, context_id)
            .await?
            .into_iter()
            .max_by_key(|conversation| conversation.updated_at);
        match conversation {
            Some(conversation) => self.resolve_conversation_target(&conversation.id).await,
            None => Ok(ResolvedNotificationTarget {
                project_id: None,
                target: NotificationTarget::none(),
                context_label: None,
                project_name: None,
                context_kind: None,
            }),
        }
    }

    pub async fn resolve_context_target_with_trusted_conversation(
        &self,
        context_type: &str,
        context_id: &str,
        trusted_conversation_id: Option<&str>,
    ) -> AppResult<ResolvedNotificationTarget> {
        if let Some(conversation_id) = trusted_conversation_id {
            let expected_project_id = self
                .expected_project_id(None, Some(context_type), Some(context_id))
                .await?;
            let expected_context_type = context_type.parse().ok();
            if let Some(resolved) = self
                .resolve_trusted_agent_conversation_target(
                    conversation_id,
                    expected_project_id.as_deref(),
                    expected_context_type,
                    Some(context_id),
                )
                .await?
            {
                return Ok(resolved);
            }
            return Ok(unresolved_target(expected_project_id));
        }
        self.resolve_context_target(context_type, context_id).await
    }

    pub async fn session_is_automation_owned(&self, session: &IdeationSession) -> AppResult<bool> {
        Ok(self
            .chat_conversation_repo
            .get_by_context(ChatContextType::Ideation, session.id.as_str())
            .await?
            .into_iter()
            .any(|conversation| conversation.automation_run_id.is_some()))
    }

    pub async fn session_has_implementation_task(
        &self,
        session: &IdeationSession,
    ) -> AppResult<bool> {
        Ok(self
            .task_repo
            .get_by_ideation_session(&session.id)
            .await?
            .into_iter()
            .any(|task| task.archived_at.is_none()))
    }

    async fn project_id_for_conversation(
        &self,
        conversation: &ChatConversation,
    ) -> AppResult<Option<String>> {
        match conversation.context_type {
            ChatContextType::Task
            | ChatContextType::TaskExecution
            | ChatContextType::Review
            | ChatContextType::Merge
            | ChatContextType::BranchUpdate => Ok(self
                .task_repo
                .get_by_id(&TaskId::from_string(conversation.context_id.clone()))
                .await?
                .map(|task| task.project_id.to_string())),
            ChatContextType::Ideation => Ok(self
                .ideation_session_repo
                .get_by_id(&IdeationSessionId::from_string(
                    conversation.context_id.clone(),
                ))
                .await?
                .map(|session| session.project_id.to_string())),
            ChatContextType::Project => Ok(Some(conversation.context_id.clone())),
            ChatContextType::Delegation | ChatContextType::Standalone => Ok(None),
        }
    }

    async fn conversation_context_label(
        &self,
        conversation: &ChatConversation,
    ) -> AppResult<Option<String>> {
        match conversation.context_type {
            ChatContextType::Task
            | ChatContextType::TaskExecution
            | ChatContextType::Review
            | ChatContextType::Merge
            | ChatContextType::BranchUpdate => Ok(self
                .task_repo
                .get_by_id(&TaskId::from_string(conversation.context_id.clone()))
                .await?
                .map(|task| task.title)),
            ChatContextType::Ideation => Ok(self
                .ideation_session_repo
                .get_by_id(&IdeationSessionId::from_string(
                    conversation.context_id.clone(),
                ))
                .await?
                .and_then(|session| session.title)),
            ChatContextType::Project
            | ChatContextType::Delegation
            | ChatContextType::Standalone => Ok(conversation.title.clone()),
        }
    }

    async fn project_name(&self, project_id: &str) -> Option<String> {
        match self
            .project_repo
            .get_by_id(&ProjectId::from_string(project_id.to_string()))
            .await
        {
            Ok(project) => project.map(|project| project.name),
            Err(error) => {
                tracing::warn!(error = %error, project_id, "Failed to resolve notification project name");
                None
            }
        }
    }

    async fn resolve_trusted_agent_conversation_target(
        &self,
        conversation_id: &str,
        expected_project_id: Option<&str>,
        expected_context_type: Option<ChatContextType>,
        expected_context_id: Option<&str>,
    ) -> AppResult<Option<ResolvedNotificationTarget>> {
        let conversation_id = ChatConversationId::from_string(conversation_id.to_string());
        let Some(conversation) = self
            .chat_conversation_repo
            .get_by_id(&conversation_id)
            .await?
        else {
            return Ok(None);
        };
        if conversation.archived_at.is_some() {
            return Ok(None);
        }
        if conversation.context_type == ChatContextType::Standalone {
            let stored_conversation_id = conversation.id.as_str();
            if expected_context_type != Some(ChatContextType::Standalone)
                || expected_context_id != Some(stored_conversation_id.as_str())
                || conversation.context_id != stored_conversation_id
            {
                return Ok(None);
            }
            return Ok(Some(
                self.resolve_conversation_target(&conversation_id).await?,
            ));
        }
        let Some(workspace) = self
            .agent_workspace_repo
            .get_by_conversation_id(&conversation_id)
            .await?
        else {
            return Ok(None);
        };
        if workspace.status != AgentConversationWorkspaceStatus::Active
            || expected_project_id
                .is_some_and(|project_id| workspace.project_id.as_str() != project_id)
        {
            return Ok(None);
        }
        Ok(Some(
            self.resolve_conversation_target(&conversation_id).await?,
        ))
    }

    async fn expected_project_id(
        &self,
        task_id: Option<&str>,
        context_type: Option<&str>,
        context_id: Option<&str>,
    ) -> AppResult<Option<String>> {
        if let Some(task_id) = task_id {
            return Ok(self
                .task_repo
                .get_by_id(&TaskId::from_string(task_id.to_string()))
                .await?
                .map(|task| task.project_id.to_string()));
        }
        let (Some(context_type), Some(context_id)) = (context_type, context_id) else {
            return Ok(None);
        };
        let Ok(context_type) = context_type.parse::<ChatContextType>() else {
            return Ok(None);
        };
        match context_type {
            ChatContextType::Project => Ok(Some(context_id.to_string())),
            ChatContextType::Ideation => Ok(self
                .ideation_session_repo
                .get_by_id(&IdeationSessionId::from_string(context_id.to_string()))
                .await?
                .map(|session| session.project_id.to_string())),
            ChatContextType::Task
            | ChatContextType::TaskExecution
            | ChatContextType::Review
            | ChatContextType::Merge
            | ChatContextType::BranchUpdate => Ok(self
                .task_repo
                .get_by_id(&TaskId::from_string(context_id.to_string()))
                .await?
                .map(|task| task.project_id.to_string())),
            ChatContextType::Delegation | ChatContextType::Standalone => Ok(None),
        }
    }
}

fn unresolved_target(project_id: Option<String>) -> ResolvedNotificationTarget {
    ResolvedNotificationTarget {
        project_id,
        target: NotificationTarget::none(),
        context_label: None,
        project_name: None,
        context_kind: None,
    }
}

pub fn conversation_target(
    conversation: &ChatConversation,
    project_id: Option<String>,
) -> NotificationTarget {
    NotificationTarget {
        kind: NotificationTargetKind::AgentConversation,
        project_id,
        task_id: None,
        conversation_id: Some(conversation.id.to_string()),
        setup_conversation_id: None,
        automation_id: conversation.automation_id.as_ref().map(ToString::to_string),
        run_id: conversation
            .automation_run_id
            .as_ref()
            .map(ToString::to_string),
    }
}
