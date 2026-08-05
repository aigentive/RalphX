use std::sync::Arc;

use async_trait::async_trait;
use chrono::{Duration, Utc};

use super::*;
use crate::domain::agents::ProviderSessionRef;
use crate::domain::entities::{
    AgentRun, AgentRunUsage, ChatMessage, ChatMessageAttribution, ChatMessageId, DelegationParkId,
    DelegationParkJob, DelegationParkState, DelegationWakePolicy, IdeationSessionId, ProjectId,
    TaskId,
};
use crate::domain::repositories::ChatMessageRepository;
use crate::error::{AppError, AppResult};
use crate::infrastructure::memory::MemoryChatMessageRepository;

struct TranscriptReadFailingChatMessageRepository {
    inner: MemoryChatMessageRepository,
}

impl TranscriptReadFailingChatMessageRepository {
    fn new() -> Self {
        Self {
            inner: MemoryChatMessageRepository::new(),
        }
    }
}

#[async_trait]
impl ChatMessageRepository for TranscriptReadFailingChatMessageRepository {
    async fn create(&self, message: ChatMessage) -> AppResult<ChatMessage> {
        self.inner.create(message).await
    }

    async fn get_by_id(&self, id: &ChatMessageId) -> AppResult<Option<ChatMessage>> {
        self.inner.get_by_id(id).await
    }

    async fn get_by_session(&self, id: &IdeationSessionId) -> AppResult<Vec<ChatMessage>> {
        self.inner.get_by_session(id).await
    }

    async fn get_by_project(&self, id: &ProjectId) -> AppResult<Vec<ChatMessage>> {
        self.inner.get_by_project(id).await
    }

    async fn get_by_task(&self, id: &TaskId) -> AppResult<Vec<ChatMessage>> {
        self.inner.get_by_task(id).await
    }

    async fn get_by_conversation(&self, id: &ChatConversationId) -> AppResult<Vec<ChatMessage>> {
        self.inner.get_by_conversation(id).await
    }

    async fn get_recent_by_conversation_paginated(
        &self,
        _id: &ChatConversationId,
        _limit: u32,
        _offset: u32,
    ) -> AppResult<Vec<ChatMessage>> {
        Err(AppError::Infrastructure(
            "sidebar unexpectedly hydrated a transcript".to_string(),
        ))
    }

    async fn delete_by_session(&self, id: &IdeationSessionId) -> AppResult<()> {
        self.inner.delete_by_session(id).await
    }

    async fn delete_by_project(&self, id: &ProjectId) -> AppResult<()> {
        self.inner.delete_by_project(id).await
    }

    async fn delete_by_task(&self, id: &TaskId) -> AppResult<()> {
        self.inner.delete_by_task(id).await
    }

    async fn delete(&self, id: &ChatMessageId) -> AppResult<()> {
        self.inner.delete(id).await
    }

    async fn count_by_session(&self, id: &IdeationSessionId) -> AppResult<u32> {
        self.inner.count_by_session(id).await
    }

    async fn get_recent_by_session(
        &self,
        id: &IdeationSessionId,
        limit: u32,
    ) -> AppResult<Vec<ChatMessage>> {
        self.inner.get_recent_by_session(id, limit).await
    }

    async fn get_recent_by_session_paginated(
        &self,
        id: &IdeationSessionId,
        limit: u32,
        offset: u32,
    ) -> AppResult<Vec<ChatMessage>> {
        self.inner
            .get_recent_by_session_paginated(id, limit, offset)
            .await
    }

    async fn update_content(
        &self,
        id: &ChatMessageId,
        content: &str,
        tool_calls: Option<&str>,
        content_blocks: Option<&str>,
    ) -> AppResult<()> {
        self.inner
            .update_content(id, content, tool_calls, content_blocks)
            .await
    }

    async fn update_provider_session_ref(
        &self,
        id: &ChatMessageId,
        session_ref: &ProviderSessionRef,
    ) -> AppResult<()> {
        self.inner
            .update_provider_session_ref(id, session_ref)
            .await
    }

    async fn update_usage(&self, id: &ChatMessageId, usage: &AgentRunUsage) -> AppResult<()> {
        self.inner.update_usage(id, usage).await
    }

    async fn update_attribution(
        &self,
        id: &ChatMessageId,
        attribution: &ChatMessageAttribution,
    ) -> AppResult<()> {
        self.inner.update_attribution(id, attribution).await
    }

    async fn count_unread_assistant_messages(
        &self,
        session_id: &str,
        after_message_id: Option<&str>,
    ) -> AppResult<u32> {
        self.inner
            .count_unread_assistant_messages(session_id, after_message_id)
            .await
    }

    async fn count_unread_messages(
        &self,
        session_id: &str,
        cursor_message_id: Option<&str>,
    ) -> AppResult<i64> {
        self.inner
            .count_unread_messages(session_id, cursor_message_id)
            .await
    }

    async fn get_first_user_message_by_context(
        &self,
        context_type: &str,
        context_id: &str,
    ) -> AppResult<Option<String>> {
        self.inner
            .get_first_user_message_by_context(context_type, context_id)
            .await
    }

    async fn get_latest_message_by_role(
        &self,
        session_id: &IdeationSessionId,
        role: &str,
    ) -> AppResult<Option<ChatMessage>> {
        self.inner
            .get_latest_message_by_role(session_id, role)
            .await
    }

    async fn exists_verification_result_in_conversation(
        &self,
        conversation_id: &ChatConversationId,
    ) -> AppResult<bool> {
        self.inner
            .exists_verification_result_in_conversation(conversation_id)
            .await
    }
}

fn sidebar_input(project_id: &ProjectId) -> AgentSidebarConversationsInput {
    AgentSidebarConversationsInput {
        project_ids: vec![project_id.as_str().to_string()],
        include_archived: None,
        archived_only: None,
        search: None,
        publication_states: None,
        group_by: Some("inbox".to_string()),
        sort: None,
        limit_per_group: Some(6),
        offsets: None,
        pinned_conversation_ids: None,
        priority_conversation_ids: None,
    }
}

#[tokio::test]
async fn sidebar_list_does_not_hydrate_conversation_transcripts() {
    let mut state = AppState::new_test();
    let project = state
        .project_repo
        .create(Project::new(
            "sidebar-summary".to_string(),
            "/tmp/sidebar-summary".to_string(),
        ))
        .await
        .unwrap();
    let conversation = state
        .chat_conversation_repo
        .create(ChatConversation::new_project(project.id.clone()))
        .await
        .unwrap();
    state.chat_message_repo = Arc::new(TranscriptReadFailingChatMessageRepository::new());

    let response =
        list_agent_sidebar_conversations_for_app_state(sidebar_input(&project.id), &state)
            .await
            .expect("sidebar summary should not depend on transcript hydration");

    assert!(response.groups.iter().any(|group| {
        group
            .rows
            .iter()
            .any(|row| row.conversation.id == conversation.id.as_str())
    }));
}

#[tokio::test]
async fn armed_park_keeps_completed_coordinator_working_and_counts_unsettled_delegates() {
    let state = AppState::new_test();
    let project = state
        .project_repo
        .create(Project::new(
            "parked-sidebar".to_string(),
            "/tmp/parked-sidebar".to_string(),
        ))
        .await
        .unwrap();
    let conversation = state
        .chat_conversation_repo
        .create(ChatConversation::new_project(project.id.clone()))
        .await
        .unwrap();
    let mut parent_run = AgentRun::new(conversation.id);
    parent_run.status = AgentRunStatus::Completed;
    let parent_run_id = parent_run.id.clone();
    state.agent_run_repo.create(parent_run).await.unwrap();

    let delegate_run = AgentRun::new(conversation.id);
    let now = Utc::now();
    state
        .delegation_park_repo
        .arm(DelegationPark {
            id: DelegationParkId::new(),
            parent_conversation_id: conversation.id,
            parent_agent_run_id: parent_run_id,
            generation: 0,
            wake_policy: DelegationWakePolicy::AllSettled,
            wake_on_failure: true,
            state: DelegationParkState::Armed,
            deadline_at: now + Duration::hours(1),
            wake_claimed_at: None,
            wake_attempts: 0,
            last_error: None,
            created_at: now,
            updated_at: now,
            jobs: vec![
                DelegationParkJob {
                    job_id: "settled".to_string(),
                    delegated_session_id: "delegate-session-1".to_string(),
                    delegated_agent_run_id: delegate_run.id.clone(),
                    settled_status: Some("completed".to_string()),
                },
                DelegationParkJob {
                    job_id: "waiting-1".to_string(),
                    delegated_session_id: "delegate-session-2".to_string(),
                    delegated_agent_run_id: AgentRun::new(conversation.id).id,
                    settled_status: None,
                },
                DelegationParkJob {
                    job_id: "waiting-2".to_string(),
                    delegated_session_id: "delegate-session-3".to_string(),
                    delegated_agent_run_id: AgentRun::new(conversation.id).id,
                    settled_status: None,
                },
            ],
        })
        .await
        .unwrap();

    let response =
        list_agent_sidebar_conversations_for_app_state(sidebar_input(&project.id), &state)
            .await
            .unwrap();
    let working_row = response
        .groups
        .iter()
        .find(|group| group.key == "working")
        .and_then(|group| {
            group
                .rows
                .iter()
                .find(|row| row.conversation.id == conversation.id.as_str())
        })
        .expect("completed parked coordinator should be working");

    assert_eq!(working_row.attention_lane, "working");
    assert_eq!(working_row.parked_delegate_count, 2);
    assert!(response
        .groups
        .iter()
        .find(|group| group.key == "needs")
        .is_none_or(|group| group
            .rows
            .iter()
            .all(|row| row.conversation.id != conversation.id.as_str())));
}
