use async_trait::async_trait;
use ralphx_lib::application::{
    CreateProposalOptions, IdeationService, UpdateProposalOptions,
};
use ralphx_lib::domain::agents::ProviderSessionRef;
use ralphx_lib::domain::entities::{self, *};
use ralphx_lib::domain::repositories::{self, *};
use ralphx_lib::error::{AppError, AppResult};
use chrono::Utc;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

// ========================================================================
// MOCK REPOSITORIES
// ========================================================================

struct MockSessionRepository {
    sessions: Mutex<HashMap<String, IdeationSession>>,
}

impl MockSessionRepository {
    fn new() -> Self {
        Self {
            sessions: Mutex::new(HashMap::new()),
        }
    }

    fn with_session(session: IdeationSession) -> Self {
        let repo = Self::new();
        repo.sessions
            .lock()
            .unwrap()
            .insert(session.id.to_string(), session);
        repo
    }
}

#[async_trait]
impl IdeationSessionRepository for MockSessionRepository {
    async fn create(&self, session: IdeationSession) -> AppResult<IdeationSession> {
        self.sessions
            .lock()
            .unwrap()
            .insert(session.id.to_string(), session.clone());
        Ok(session)
    }

    async fn get_by_id(&self, id: &IdeationSessionId) -> AppResult<Option<IdeationSession>> {
        Ok(self.sessions.lock().unwrap().get(&id.to_string()).cloned())
    }

    async fn get_by_project(&self, project_id: &ProjectId) -> AppResult<Vec<IdeationSession>> {
        Ok(self
            .sessions
            .lock()
            .unwrap()
            .values()
            .filter(|s| &s.project_id == project_id)
            .cloned()
            .collect())
    }

    async fn update_status(
        &self,
        id: &IdeationSessionId,
        status: IdeationSessionStatus,
    ) -> AppResult<()> {
        if let Some(session) = self.sessions.lock().unwrap().get_mut(&id.to_string()) {
            session.status = status;
            session.updated_at = Utc::now();
            if status == IdeationSessionStatus::Archived {
                session.archived_at = Some(Utc::now());
            }
            if status == IdeationSessionStatus::Accepted {
                session.converted_at = Some(Utc::now());
            }
        }
        Ok(())
    }

    async fn update_title(
        &self,
        id: &IdeationSessionId,
        title: Option<String>,
        _title_source: &str,
    ) -> AppResult<()> {
        if let Some(session) = self.sessions.lock().unwrap().get_mut(&id.to_string()) {
            session.title = title;
            session.updated_at = Utc::now();
        }
        Ok(())
    }

    async fn update_plan_artifact_id(
        &self,
        id: &IdeationSessionId,
        plan_artifact_id: Option<String>,
    ) -> AppResult<()> {
        if let Some(session) = self.sessions.lock().unwrap().get_mut(&id.to_string()) {
            session.plan_artifact_id =
                plan_artifact_id.map(entities::ArtifactId::from_string);
            session.updated_at = Utc::now();
        }
        Ok(())
    }

    async fn delete(&self, id: &IdeationSessionId) -> AppResult<()> {
        self.sessions.lock().unwrap().remove(&id.to_string());
        Ok(())
    }

    async fn get_active_by_project(
        &self,
        project_id: &ProjectId,
    ) -> AppResult<Vec<IdeationSession>> {
        Ok(self
            .sessions
            .lock()
            .unwrap()
            .values()
            .filter(|s| &s.project_id == project_id && s.status == IdeationSessionStatus::Active)
            .cloned()
            .collect())
    }

    async fn count_by_status(
        &self,
        project_id: &ProjectId,
        status: IdeationSessionStatus,
    ) -> AppResult<u32> {
        Ok(self
            .sessions
            .lock()
            .unwrap()
            .values()
            .filter(|s| &s.project_id == project_id && s.status == status)
            .count() as u32)
    }

    async fn get_by_plan_artifact_id(
        &self,
        plan_artifact_id: &str,
    ) -> AppResult<Vec<IdeationSession>> {
        Ok(self
            .sessions
            .lock()
            .unwrap()
            .values()
            .filter(|s| s.plan_artifact_id.as_ref().map(|id| id.as_str()) == Some(plan_artifact_id))
            .cloned()
            .collect())
    }

    async fn get_by_inherited_plan_artifact_id(
        &self,
        artifact_id: &str,
    ) -> AppResult<Vec<IdeationSession>> {
        Ok(self
            .sessions
            .lock()
            .unwrap()
            .values()
            .filter(|s| {
                s.inherited_plan_artifact_id
                    .as_ref()
                    .map(|id| id.as_str())
                    == Some(artifact_id)
            })
            .cloned()
            .collect())
    }

    async fn get_children(&self, parent_id: &IdeationSessionId) -> AppResult<Vec<IdeationSession>> {
        Ok(self
            .sessions
            .lock()
            .unwrap()
            .values()
            .filter(|s| s.parent_session_id.as_ref() == Some(parent_id))
            .cloned()
            .collect())
    }

    async fn get_ancestor_chain(
        &self,
        _session_id: &IdeationSessionId,
    ) -> AppResult<Vec<IdeationSession>> {
        Ok(Vec::new())
    }

    async fn set_parent(
        &self,
        _id: &IdeationSessionId,
        _parent_id: Option<&IdeationSessionId>,
    ) -> AppResult<()> {
        Ok(())
    }

    async fn update_verification_state(
        &self,
        _id: &IdeationSessionId,
        _status: VerificationStatus,
        _in_progress: bool,
    ) -> AppResult<()> {
        Ok(())
    }

    async fn reset_verification(&self, _id: &IdeationSessionId) -> AppResult<bool> {
        Ok(false)
    }

    async fn reset_and_begin_reverify(
        &self,
        _session_id: &str,
    ) -> AppResult<(i32, entities::VerificationRunSnapshot)> {
        unimplemented!()
    }

    async fn get_verification_status(
        &self,
        _id: &IdeationSessionId,
    ) -> AppResult<Option<(VerificationStatus, bool)>> {
        Ok(None)
    }

    async fn save_verification_run_snapshot(
        &self,
        _id: &IdeationSessionId,
        _snapshot: &entities::VerificationRunSnapshot,
    ) -> AppResult<()> {
        Ok(())
    }

    async fn get_verification_run_snapshot(
        &self,
        _id: &IdeationSessionId,
        _generation: i32,
    ) -> AppResult<Option<entities::VerificationRunSnapshot>> {
        Ok(None)
    }

    async fn revert_plan_and_skip_verification(
        &self,
        _id: &IdeationSessionId,
        _new_plan_artifact_id: String,
        _convergence_reason: String,
    ) -> AppResult<()> {
        Ok(())
    }

    async fn revert_plan_and_skip_with_artifact(
        &self,
        _session_id: &IdeationSessionId,
        _new_artifact_id: String,
        _artifact_type_str: String,
        _artifact_name: String,
        _content_text: String,
        _version: u32,
        _previous_version_id: String,
        _convergence_reason: String,
    ) -> AppResult<()> {
        Ok(())
    }

    async fn increment_verification_generation(
        &self,
        _session_id: &IdeationSessionId,
    ) -> AppResult<()> {
        Ok(())
    }

    async fn get_stale_in_progress_sessions(
        &self,
        stale_before: chrono::DateTime<chrono::Utc>,
    ) -> AppResult<Vec<IdeationSession>> {
        Ok(self
            .sessions
            .lock()
            .unwrap()
            .values()
            .filter(|s| s.verification_in_progress && s.updated_at < stale_before)
            .cloned()
            .collect())
    }

    async fn get_all_in_progress_sessions(&self) -> AppResult<Vec<IdeationSession>> {
        Ok(self
            .sessions
            .lock()
            .unwrap()
            .values()
            .filter(|s| s.verification_in_progress)
            .cloned()
            .collect())
    }

    async fn get_verification_children(
        &self,
        _parent_session_id: &IdeationSessionId,
    ) -> AppResult<Vec<IdeationSession>> {
        Ok(Vec::new())
    }

    async fn get_by_project_and_status(
        &self,
        project_id: &str,
        status: &str,
        limit: u32,
    ) -> AppResult<Vec<IdeationSession>> {
        let mut sessions: Vec<_> = self
            .sessions
            .lock()
            .unwrap()
            .values()
            .filter(|s| s.project_id.as_str() == project_id && s.status.to_string() == status)
            .cloned()
            .collect();
        sessions.sort_by(|a, b| b.created_at.cmp(&a.created_at));
        sessions.truncate(limit as usize);
        Ok(sessions)
    }

    async fn get_group_counts(
        &self,
        _project_id: &ProjectId,
        _search: Option<&str>,
    ) -> AppResult<repositories::ideation_session_repository::SessionGroupCounts> {
        unimplemented!()
    }

    async fn list_by_group(
        &self,
        _project_id: &ProjectId,
        _group: &str,
        _offset: u32,
        _limit: u32,
        _search: Option<&str>,
    ) -> AppResult<(
        Vec<repositories::ideation_session_repository::IdeationSessionWithProgress>,
        u32,
    )> {
        unimplemented!()
    }

    fn set_expected_proposal_count_sync(
        _conn: &rusqlite::Connection,
        _session_id: &str,
        _count: u32,
    ) -> AppResult<()>
    where
        Self: Sized,
    {
        unimplemented!()
    }

    async fn set_auto_accept_status(
        &self,
        _session_id: &str,
        _status: &str,
        _auto_accept_started_at: Option<String>,
    ) -> AppResult<()> {
        unimplemented!()
    }

    fn count_active_by_session_sync(
        _conn: &rusqlite::Connection,
        _session_id: &str,
    ) -> AppResult<i64>
    where
        Self: Sized,
    {
        unimplemented!()
    }

    async fn get_by_idempotency_key(
        &self,
        api_key_id: &str,
        idempotency_key: &str,
    ) -> AppResult<Option<IdeationSession>> {
        Ok(self
            .sessions
            .lock()
            .unwrap()
            .values()
            .find(|s| {
                s.api_key_id.as_deref() == Some(api_key_id)
                    && s.idempotency_key.as_deref() == Some(idempotency_key)
            })
            .cloned())
    }

    async fn update_external_activity_phase(
        &self,
        id: &IdeationSessionId,
        phase: Option<&str>,
    ) -> AppResult<()> {
        if let Some(session) = self.sessions.lock().unwrap().get_mut(&id.to_string()) {
            session.external_activity_phase = phase.map(|s| s.to_string());
            session.updated_at = Utc::now();
        }
        Ok(())
    }

    async fn update_external_last_read_message_id(
        &self,
        id: &IdeationSessionId,
        message_id: &str,
    ) -> AppResult<()> {
        if let Some(session) = self.sessions.lock().unwrap().get_mut(&id.to_string()) {
            session.external_last_read_message_id = Some(message_id.to_string());
            session.updated_at = Utc::now();
        }
        Ok(())
    }

    async fn list_active_external_by_project(
        &self,
        project_id: &ProjectId,
    ) -> AppResult<Vec<IdeationSession>> {
        Ok(self
            .sessions
            .lock()
            .unwrap()
            .values()
            .filter(|s| {
                &s.project_id == project_id
                    && s.status == IdeationSessionStatus::Active
                    && s.origin == SessionOrigin::External
            })
            .cloned()
            .collect())
    }

    async fn list_active_external_sessions_for_archival(
        &self,
        _stale_before: Option<chrono::DateTime<chrono::Utc>>,
    ) -> AppResult<Vec<IdeationSession>> {
        Ok(vec![])
    }

    async fn list_stalled_external_sessions(
        &self,
        _stalled_before: chrono::DateTime<chrono::Utc>,
    ) -> AppResult<Vec<IdeationSession>> {
        Ok(vec![])
    }

    async fn set_dependencies_acknowledged(&self, _session_id: &str) -> AppResult<()> {
        unimplemented!()
    }

    async fn reset_acceptance_cycle_fields(&self, _session_id: &str) -> AppResult<()> {
        Ok(())
    }

    async fn touch_updated_at(&self, _session_id: &str) -> AppResult<()> {
        Ok(())
    }

    async fn update_last_effective_model(
        &self,
        _session_id: &str,
        _model: &str,
    ) -> AppResult<()> {
        Ok(())
    }

    async fn list_active_verification_children(
        &self,
    ) -> AppResult<Vec<ralphx_lib::domain::entities::IdeationSession>> {
        Ok(vec![])
    }

    async fn set_pending_initial_prompt(
        &self,
        _session_id: &str,
        _prompt: Option<String>,
    ) -> AppResult<()> {
        Ok(())
    }

    async fn set_pending_initial_prompt_if_unset(
        &self,
        _session_id: &str,
        _prompt: String,
    ) -> AppResult<bool> {
        Ok(false)
    }

    async fn claim_pending_session_for_project(
        &self,
        _project_id: &str,
    ) -> AppResult<Option<(String, String)>> {
        Ok(None)
    }

    async fn list_projects_with_pending_sessions(&self) -> AppResult<Vec<String>> {
        Ok(vec![])
    }

    async fn count_pending_sessions_for_project(
        &self,
        _project_id: &ralphx_lib::domain::entities::ProjectId,
    ) -> AppResult<u32> {
        Ok(0)
    }

    async fn update_acceptance_status(
        &self,
        _session_id: &ralphx_lib::domain::entities::IdeationSessionId,
        _expected_current: Option<ralphx_lib::domain::entities::AcceptanceStatus>,
        _new_status: Option<ralphx_lib::domain::entities::AcceptanceStatus>,
    ) -> AppResult<bool> {
        Ok(false)
    }

    async fn get_sessions_with_pending_acceptance(
        &self,
        _project_id: &ralphx_lib::domain::entities::ProjectId,
    ) -> AppResult<Vec<ralphx_lib::domain::entities::IdeationSession>> {
        Ok(vec![])
    }

    async fn set_verification_confirmation_status(
        &self,
        _session_id: &ralphx_lib::domain::entities::IdeationSessionId,
        _status: Option<ralphx_lib::domain::entities::VerificationConfirmationStatus>,
    ) -> AppResult<()> {
        Ok(())
    }

    async fn get_pending_verification_confirmations(
        &self,
        _project_id: &ralphx_lib::domain::entities::ProjectId,
    ) -> AppResult<Vec<ralphx_lib::domain::entities::IdeationSession>> {
        Ok(vec![])
    }

    async fn count_active_proposals(
        &self,
        _session_id: &IdeationSessionId,
    ) -> AppResult<usize> {
        Ok(0)
    }

    async fn get_latest_verification_child(
        &self,
        _parent_id: &ralphx_lib::domain::entities::IdeationSessionId,
    ) -> AppResult<Option<ralphx_lib::domain::entities::IdeationSession>> {
        Ok(None)
    }
}

struct MockProposalRepository {
    proposals: Mutex<HashMap<String, TaskProposal>>,
}

impl MockProposalRepository {
    fn new() -> Self {
        Self {
            proposals: Mutex::new(HashMap::new()),
        }
    }
}

#[async_trait]
impl TaskProposalRepository for MockProposalRepository {
    async fn create(&self, proposal: TaskProposal) -> AppResult<TaskProposal> {
        self.proposals
            .lock()
            .unwrap()
            .insert(proposal.id.to_string(), proposal.clone());
        Ok(proposal)
    }

    async fn get_by_id(&self, id: &TaskProposalId) -> AppResult<Option<TaskProposal>> {
        Ok(self.proposals.lock().unwrap().get(&id.to_string()).cloned())
    }

    async fn get_by_session(&self, session_id: &IdeationSessionId) -> AppResult<Vec<TaskProposal>> {
        let mut proposals: Vec<_> = self
            .proposals
            .lock()
            .unwrap()
            .values()
            .filter(|p| &p.session_id == session_id && p.archived_at.is_none())
            .cloned()
            .collect();
        proposals.sort_by_key(|p| p.sort_order);
        Ok(proposals)
    }

    async fn update(&self, proposal: &TaskProposal) -> AppResult<()> {
        self.proposals
            .lock()
            .unwrap()
            .insert(proposal.id.to_string(), proposal.clone());
        Ok(())
    }

    async fn update_priority(
        &self,
        id: &TaskProposalId,
        assessment: &PriorityAssessment,
    ) -> AppResult<()> {
        if let Some(p) = self.proposals.lock().unwrap().get_mut(&id.to_string()) {
            p.suggested_priority = assessment.suggested_priority;
            p.priority_score = assessment.priority_score;
        }
        Ok(())
    }

    async fn update_selection(&self, id: &TaskProposalId, selected: bool) -> AppResult<()> {
        if let Some(p) = self.proposals.lock().unwrap().get_mut(&id.to_string()) {
            p.selected = selected;
        }
        Ok(())
    }

    async fn set_created_task_id(&self, id: &TaskProposalId, task_id: &TaskId) -> AppResult<()> {
        if let Some(p) = self.proposals.lock().unwrap().get_mut(&id.to_string()) {
            p.created_task_id = Some(task_id.clone());
        }
        Ok(())
    }

    async fn delete(&self, id: &TaskProposalId) -> AppResult<()> {
        self.proposals.lock().unwrap().remove(&id.to_string());
        Ok(())
    }

    async fn reorder(
        &self,
        _session_id: &IdeationSessionId,
        proposal_ids: Vec<TaskProposalId>,
    ) -> AppResult<()> {
        for (i, id) in proposal_ids.iter().enumerate() {
            if let Some(p) = self.proposals.lock().unwrap().get_mut(&id.to_string()) {
                p.sort_order = i as i32;
            }
        }
        Ok(())
    }

    async fn get_selected_by_session(
        &self,
        session_id: &IdeationSessionId,
    ) -> AppResult<Vec<TaskProposal>> {
        let mut proposals: Vec<_> = self
            .proposals
            .lock()
            .unwrap()
            .values()
            .filter(|p| &p.session_id == session_id && p.selected)
            .cloned()
            .collect();
        proposals.sort_by_key(|p| p.sort_order);
        Ok(proposals)
    }

    async fn count_by_session(&self, session_id: &IdeationSessionId) -> AppResult<u32> {
        Ok(self
            .proposals
            .lock()
            .unwrap()
            .values()
            .filter(|p| &p.session_id == session_id)
            .count() as u32)
    }

    async fn count_selected_by_session(&self, session_id: &IdeationSessionId) -> AppResult<u32> {
        Ok(self
            .proposals
            .lock()
            .unwrap()
            .values()
            .filter(|p| &p.session_id == session_id && p.selected)
            .count() as u32)
    }

    async fn get_by_plan_artifact_id(
        &self,
        artifact_id: &ArtifactId,
    ) -> AppResult<Vec<TaskProposal>> {
        Ok(self
            .proposals
            .lock()
            .unwrap()
            .values()
            .filter(|p| p.plan_artifact_id.as_ref() == Some(artifact_id))
            .cloned()
            .collect())
    }

    async fn clear_created_task_ids_by_session(
        &self,
        _session_id: &IdeationSessionId,
    ) -> AppResult<()> {
        Ok(())
    }

    async fn archive(&self, id: &TaskProposalId) -> AppResult<TaskProposal> {
        let mut proposals = self.proposals.lock().unwrap();
        let proposal = proposals
            .get_mut(&id.to_string())
            .ok_or_else(|| AppError::NotFound(format!("Proposal {} not found", id)))?;
        proposal.archived_at = Some(Utc::now());
        proposal.updated_at = Utc::now();
        Ok(proposal.clone())
    }
}

struct MockMessageRepository {
    messages: Mutex<HashMap<String, ChatMessage>>,
}

impl MockMessageRepository {
    fn new() -> Self {
        Self {
            messages: Mutex::new(HashMap::new()),
        }
    }
}

#[async_trait]
impl ChatMessageRepository for MockMessageRepository {
    async fn create(&self, message: ChatMessage) -> AppResult<ChatMessage> {
        self.messages
            .lock()
            .unwrap()
            .insert(message.id.to_string(), message.clone());
        Ok(message)
    }

    async fn get_by_id(&self, id: &ChatMessageId) -> AppResult<Option<ChatMessage>> {
        Ok(self.messages.lock().unwrap().get(&id.to_string()).cloned())
    }

    async fn get_by_session(&self, session_id: &IdeationSessionId) -> AppResult<Vec<ChatMessage>> {
        let mut messages: Vec<_> = self
            .messages
            .lock()
            .unwrap()
            .values()
            .filter(|m| m.session_id.as_ref() == Some(session_id))
            .cloned()
            .collect();
        messages.sort_by_key(|m| m.created_at);
        Ok(messages)
    }

    async fn get_by_project(&self, project_id: &ProjectId) -> AppResult<Vec<ChatMessage>> {
        let mut messages: Vec<_> = self
            .messages
            .lock()
            .unwrap()
            .values()
            .filter(|m| m.project_id.as_ref() == Some(project_id) && m.session_id.is_none())
            .cloned()
            .collect();
        messages.sort_by_key(|m| m.created_at);
        Ok(messages)
    }

    async fn get_by_task(&self, task_id: &TaskId) -> AppResult<Vec<ChatMessage>> {
        let mut messages: Vec<_> = self
            .messages
            .lock()
            .unwrap()
            .values()
            .filter(|m| m.task_id.as_ref() == Some(task_id))
            .cloned()
            .collect();
        messages.sort_by_key(|m| m.created_at);
        Ok(messages)
    }

    async fn get_by_conversation(
        &self,
        conversation_id: &ChatConversationId,
    ) -> AppResult<Vec<ChatMessage>> {
        let mut messages: Vec<_> = self
            .messages
            .lock()
            .unwrap()
            .values()
            .filter(|m| m.conversation_id.as_ref() == Some(conversation_id))
            .cloned()
            .collect();
        messages.sort_by_key(|m| m.created_at);
        Ok(messages)
    }

    async fn get_recent_by_conversation_paginated(
        &self,
        conversation_id: &ChatConversationId,
        limit: u32,
        offset: u32,
    ) -> AppResult<Vec<ChatMessage>> {
        let mut messages: Vec<_> = self
            .messages
            .lock()
            .unwrap()
            .values()
            .filter(|m| m.conversation_id.as_ref() == Some(conversation_id))
            .cloned()
            .collect();
        messages.sort_by_key(|m| std::cmp::Reverse(m.created_at));
        let mut messages: Vec<_> = messages
            .into_iter()
            .skip(offset as usize)
            .take(limit as usize)
            .collect();
        messages.reverse();
        Ok(messages)
    }

    async fn update_provider_session_ref(
        &self,
        id: &ChatMessageId,
        session_ref: &ProviderSessionRef,
    ) -> AppResult<()> {
        let mut messages = self.messages.lock().unwrap();
        let message = messages
            .get_mut(&id.to_string())
            .ok_or_else(|| AppError::NotFound(format!("Message {} not found", id)))?;
        message.update_provider_session_ref(session_ref);
        Ok(())
    }

    async fn update_usage(&self, id: &ChatMessageId, usage: &AgentRunUsage) -> AppResult<()> {
        let mut messages = self.messages.lock().unwrap();
        let message = messages
            .get_mut(&id.to_string())
            .ok_or_else(|| AppError::NotFound(format!("Message {} not found", id)))?;
        message.apply_usage(usage);
        Ok(())
    }

    async fn update_attribution(
        &self,
        id: &ChatMessageId,
        attribution: &ChatMessageAttribution,
    ) -> AppResult<()> {
        let mut messages = self.messages.lock().unwrap();
        let message = messages
            .get_mut(&id.to_string())
            .ok_or_else(|| AppError::NotFound(format!("Message {} not found", id)))?;
        message.apply_attribution(attribution);
        Ok(())
    }

    async fn delete_by_session(&self, session_id: &IdeationSessionId) -> AppResult<()> {
        self.messages
            .lock()
            .unwrap()
            .retain(|_, m| m.session_id.as_ref() != Some(session_id));
        Ok(())
    }

    async fn delete_by_project(&self, project_id: &ProjectId) -> AppResult<()> {
        self.messages
            .lock()
            .unwrap()
            .retain(|_, m| m.project_id.as_ref() != Some(project_id));
        Ok(())
    }

    async fn delete_by_task(&self, task_id: &TaskId) -> AppResult<()> {
        self.messages
            .lock()
            .unwrap()
            .retain(|_, m| m.task_id.as_ref() != Some(task_id));
        Ok(())
    }

    async fn delete(&self, id: &ChatMessageId) -> AppResult<()> {
        self.messages.lock().unwrap().remove(&id.to_string());
        Ok(())
    }

    async fn count_by_session(&self, session_id: &IdeationSessionId) -> AppResult<u32> {
        Ok(self
            .messages
            .lock()
            .unwrap()
            .values()
            .filter(|m| m.session_id.as_ref() == Some(session_id))
            .count() as u32)
    }

    async fn get_recent_by_session(
        &self,
        session_id: &IdeationSessionId,
        limit: u32,
    ) -> AppResult<Vec<ChatMessage>> {
        let mut messages: Vec<_> = self
            .messages
            .lock()
            .unwrap()
            .values()
            .filter(|m| m.session_id.as_ref() == Some(session_id))
            .cloned()
            .collect();
        messages.sort_by_key(|m| std::cmp::Reverse(m.created_at));
        messages.truncate(limit as usize);
        messages.reverse();
        Ok(messages)
    }

    async fn get_recent_by_session_paginated(
        &self,
        session_id: &IdeationSessionId,
        limit: u32,
        offset: u32,
    ) -> AppResult<Vec<ChatMessage>> {
        let mut messages: Vec<_> = self
            .messages
            .lock()
            .unwrap()
            .values()
            .filter(|m| m.session_id.as_ref() == Some(session_id))
            .cloned()
            .collect();
        messages.sort_by_key(|m| std::cmp::Reverse(m.created_at));
        let mut messages: Vec<_> = messages
            .into_iter()
            .skip(offset as usize)
            .take(limit as usize)
            .collect();
        messages.reverse();
        Ok(messages)
    }

    async fn update_content(
        &self,
        _id: &ChatMessageId,
        _content: &str,
        _tool_calls: Option<&str>,
        _content_blocks: Option<&str>,
    ) -> AppResult<()> {
        Ok(())
    }

    async fn count_unread_assistant_messages(
        &self,
        _session_id: &str,
        _after_message_id: Option<&str>,
    ) -> AppResult<u32> {
        Ok(0)
    }

    async fn count_unread_messages(
        &self,
        _session_id: &str,
        _cursor_message_id: Option<&str>,
    ) -> AppResult<i64> {
        Ok(0)
    }

    async fn get_first_user_message_by_context(
        &self,
        _context_type: &str,
        _context_id: &str,
    ) -> AppResult<Option<String>> {
        Ok(None)
    }

    async fn get_latest_message_by_role(
        &self,
        _session_id: &ralphx_lib::domain::entities::IdeationSessionId,
        _role: &str,
    ) -> AppResult<Option<ralphx_lib::domain::entities::ChatMessage>> {
        Ok(None)
    }

    async fn exists_verification_result_in_conversation(
        &self,
        conversation_id: &ralphx_lib::domain::entities::ChatConversationId,
    ) -> AppResult<bool> {
        Ok(self.messages.lock().unwrap().values().any(|m| {
            m.conversation_id.as_ref() == Some(conversation_id)
                && m.content.contains("<verification-result>")
        }))
    }
}

struct MockDependencyRepository {
    dependencies: Mutex<Vec<(String, String)>>,
}

impl MockDependencyRepository {
    fn new() -> Self {
        Self {
            dependencies: Mutex::new(Vec::new()),
        }
    }
}

#[async_trait]
impl ProposalDependencyRepository for MockDependencyRepository {
    async fn add_dependency(
        &self,
        proposal_id: &TaskProposalId,
        depends_on_id: &TaskProposalId,
        _reason: Option<&str>,
        _source: Option<&str>,
    ) -> AppResult<()> {
        self.dependencies
            .lock()
            .unwrap()
            .push((proposal_id.to_string(), depends_on_id.to_string()));
        Ok(())
    }

    async fn remove_dependency(
        &self,
        proposal_id: &TaskProposalId,
        depends_on_id: &TaskProposalId,
    ) -> AppResult<()> {
        self.dependencies
            .lock()
            .unwrap()
            .retain(|(p, d)| p != &proposal_id.to_string() || d != &depends_on_id.to_string());
        Ok(())
    }

    async fn get_dependencies(
        &self,
        proposal_id: &TaskProposalId,
    ) -> AppResult<Vec<TaskProposalId>> {
        Ok(self
            .dependencies
            .lock()
            .unwrap()
            .iter()
            .filter(|(p, _)| p == &proposal_id.to_string())
            .map(|(_, d)| TaskProposalId::from_string(d.clone()))
            .collect())
    }

    async fn get_dependents(&self, proposal_id: &TaskProposalId) -> AppResult<Vec<TaskProposalId>> {
        Ok(self
            .dependencies
            .lock()
            .unwrap()
            .iter()
            .filter(|(_, d)| d == &proposal_id.to_string())
            .map(|(p, _)| TaskProposalId::from_string(p.clone()))
            .collect())
    }

    async fn get_all_for_session(
        &self,
        _session_id: &IdeationSessionId,
    ) -> AppResult<Vec<(TaskProposalId, TaskProposalId, Option<String>)>> {
        Ok(self
            .dependencies
            .lock()
            .unwrap()
            .iter()
            .map(|(p, d)| {
                (
                    TaskProposalId::from_string(p.clone()),
                    TaskProposalId::from_string(d.clone()),
                    None,
                )
            })
            .collect())
    }

    async fn would_create_cycle(
        &self,
        _proposal_id: &TaskProposalId,
        _depends_on_id: &TaskProposalId,
    ) -> AppResult<bool> {
        Ok(false)
    }

    async fn clear_dependencies(&self, proposal_id: &TaskProposalId) -> AppResult<()> {
        self.dependencies
            .lock()
            .unwrap()
            .retain(|(p, d)| p != &proposal_id.to_string() && d != &proposal_id.to_string());
        Ok(())
    }

    async fn clear_session_dependencies(&self, _session_id: &IdeationSessionId) -> AppResult<()> {
        self.dependencies.lock().unwrap().clear();
        Ok(())
    }

    async fn get_all_for_session_with_source(
        &self,
        _session_id: &IdeationSessionId,
    ) -> AppResult<Vec<(TaskProposalId, TaskProposalId, Option<String>, String)>> {
        Ok(self
            .dependencies
            .lock()
            .unwrap()
            .iter()
            .map(|(p, d)| {
                (
                    TaskProposalId::from_string(p.clone()),
                    TaskProposalId::from_string(d.clone()),
                    None,
                    String::from("auto"),
                )
            })
            .collect())
    }

    async fn clear_auto_dependencies(&self, _session_id: &IdeationSessionId) -> AppResult<()> {
        self.dependencies.lock().unwrap().clear();
        Ok(())
    }

    async fn count_dependencies(&self, proposal_id: &TaskProposalId) -> AppResult<u32> {
        Ok(self
            .dependencies
            .lock()
            .unwrap()
            .iter()
            .filter(|(p, _)| p == &proposal_id.to_string())
            .count() as u32)
    }

    async fn count_dependents(&self, proposal_id: &TaskProposalId) -> AppResult<u32> {
        Ok(self
            .dependencies
            .lock()
            .unwrap()
            .iter()
            .filter(|(_, d)| d == &proposal_id.to_string())
            .count() as u32)
    }
}

// ========================================================================
// HELPER FUNCTIONS
// ========================================================================

fn create_service() -> IdeationService<
    MockSessionRepository,
    MockProposalRepository,
    MockMessageRepository,
    MockDependencyRepository,
> {
    IdeationService::new(
        Arc::new(MockSessionRepository::new()),
        Arc::new(MockProposalRepository::new()),
        Arc::new(MockMessageRepository::new()),
        Arc::new(MockDependencyRepository::new()),
    )
}

fn create_service_with_session(
    session: IdeationSession,
) -> IdeationService<
    MockSessionRepository,
    MockProposalRepository,
    MockMessageRepository,
    MockDependencyRepository,
> {
    IdeationService::new(
        Arc::new(MockSessionRepository::with_session(session)),
        Arc::new(MockProposalRepository::new()),
        Arc::new(MockMessageRepository::new()),
        Arc::new(MockDependencyRepository::new()),
    )
}

fn create_proposal_options() -> CreateProposalOptions {
    CreateProposalOptions {
        title: "Test Proposal".to_string(),
        description: Some("A test proposal".to_string()),
        category: ProposalCategory::Feature,
        suggested_priority: Priority::Medium,
        steps: None,
        acceptance_criteria: None,
        affected_paths: None,
        estimated_complexity: None,
        target_project: None,
        depends_on: vec![],
        expected_proposal_count: None,
    }
}

// ========================================================================
// SESSION TESTS
// ========================================================================

#[tokio::test]
async fn test_create_session_with_title() {
    let service = create_service();
    let project_id = ProjectId::new();

    let session = service
        .create_session(project_id.clone(), Some("My Session".to_string()))
        .await
        .unwrap();

    assert_eq!(session.project_id, project_id);
    assert_eq!(session.title, Some("My Session".to_string()));
    assert_eq!(session.status, IdeationSessionStatus::Active);
}

#[tokio::test]
async fn test_create_session_without_title_generates_default() {
    let service = create_service();
    let project_id = ProjectId::new();

    let session = service
        .create_session(project_id.clone(), None)
        .await
        .unwrap();

    assert_eq!(session.project_id, project_id);
    assert!(session.title.is_some());
    assert!(session.title.unwrap().starts_with("Ideation Session"));
}

#[tokio::test]
async fn test_get_session_returns_session() {
    let project_id = ProjectId::new();
    let session = IdeationSession::new_with_title(project_id.clone(), "Test");
    let session_id = session.id.clone();
    let service = create_service_with_session(session);

    let result = service.get_session(&session_id).await.unwrap();

    assert!(result.is_some());
    assert_eq!(result.unwrap().id, session_id);
}

#[tokio::test]
async fn test_get_session_returns_none_for_nonexistent() {
    let service = create_service();
    let session_id = IdeationSessionId::new();

    let result = service.get_session(&session_id).await.unwrap();

    assert!(result.is_none());
}

#[tokio::test]
async fn test_archive_session() {
    let project_id = ProjectId::new();
    let session = IdeationSession::new(project_id.clone());
    let session_id = session.id.clone();
    let service = create_service_with_session(session);

    service.archive_session(&session_id).await.unwrap();

    let updated = service.get_session(&session_id).await.unwrap().unwrap();
    assert_eq!(updated.status, IdeationSessionStatus::Archived);
    assert!(updated.archived_at.is_some());
}

#[tokio::test]
async fn test_update_session_title() {
    let project_id = ProjectId::new();
    let session = IdeationSession::new_with_title(project_id.clone(), "Original");
    let session_id = session.id.clone();
    let service = create_service_with_session(session);

    service
        .update_session_title(&session_id, Some("Updated".to_string()), "auto")
        .await
        .unwrap();

    let updated = service.get_session(&session_id).await.unwrap().unwrap();
    assert_eq!(updated.title, Some("Updated".to_string()));
}

#[tokio::test]
async fn test_delete_session() {
    let project_id = ProjectId::new();
    let session = IdeationSession::new(project_id.clone());
    let session_id = session.id.clone();
    let service = create_service_with_session(session);

    service.delete_session(&session_id).await.unwrap();

    let result = service.get_session(&session_id).await.unwrap();
    assert!(result.is_none());
}

#[tokio::test]
async fn test_get_sessions_by_project() {
    let project_id = ProjectId::new();
    let session1 = IdeationSession::new_with_title(project_id.clone(), "Session 1");
    let session2 = IdeationSession::new_with_title(project_id.clone(), "Session 2");

    let service = IdeationService::new(
        Arc::new(MockSessionRepository::new()),
        Arc::new(MockProposalRepository::new()),
        Arc::new(MockMessageRepository::new()),
        Arc::new(MockDependencyRepository::new()),
    );

    // Create sessions
    service.session_repo_for_test().create(session1.clone()).await.unwrap();
    service.session_repo_for_test().create(session2.clone()).await.unwrap();

    let sessions = service.get_sessions_by_project(&project_id).await.unwrap();
    assert_eq!(sessions.len(), 2);
}

#[tokio::test]
async fn test_get_active_sessions() {
    let project_id = ProjectId::new();
    let mut session1 = IdeationSession::new(project_id.clone());
    let session2 = IdeationSession::new(project_id.clone());
    session1.status = IdeationSessionStatus::Archived;

    let service = IdeationService::new(
        Arc::new(MockSessionRepository::new()),
        Arc::new(MockProposalRepository::new()),
        Arc::new(MockMessageRepository::new()),
        Arc::new(MockDependencyRepository::new()),
    );

    service.session_repo_for_test().create(session1.clone()).await.unwrap();
    service.session_repo_for_test().create(session2.clone()).await.unwrap();

    let active = service.get_active_sessions(&project_id).await.unwrap();
    assert_eq!(active.len(), 1);
    assert_eq!(active[0].status, IdeationSessionStatus::Active);
}

// ========================================================================
// PROPOSAL TESTS
// ========================================================================

#[tokio::test]
async fn test_create_proposal_in_active_session() {
    let project_id = ProjectId::new();
    let session = IdeationSession::new(project_id.clone());
    let session_id = session.id.clone();
    let service = create_service_with_session(session);

    let proposal = service
        .create_proposal(session_id.clone(), create_proposal_options())
        .await
        .unwrap();

    assert_eq!(proposal.session_id, session_id);
    assert_eq!(proposal.title, "Test Proposal");
    assert_eq!(proposal.category, ProposalCategory::Feature);
    assert_eq!(proposal.suggested_priority, Priority::Medium);
}

#[tokio::test]
async fn test_create_proposal_fails_for_nonexistent_session() {
    let service = create_service();
    let session_id = IdeationSessionId::new();

    let result = service
        .create_proposal(session_id, create_proposal_options())
        .await;

    assert!(result.is_err());
}

#[tokio::test]
async fn test_create_proposal_fails_for_archived_session() {
    let project_id = ProjectId::new();
    let mut session = IdeationSession::new(project_id.clone());
    session.archive();
    let session_id = session.id.clone();
    let service = create_service_with_session(session);

    let result = service
        .create_proposal(session_id, create_proposal_options())
        .await;

    assert!(result.is_err());
}

#[tokio::test]
async fn test_update_proposal_title() {
    let project_id = ProjectId::new();
    let session = IdeationSession::new(project_id.clone());
    let session_id = session.id.clone();
    let service = create_service_with_session(session);

    let proposal = service
        .create_proposal(session_id.clone(), create_proposal_options())
        .await
        .unwrap();

    let updated = service
        .update_proposal(
            &proposal.id,
            UpdateProposalOptions {
                title: Some("Updated Title".to_string()),
                ..Default::default()
            },
        )
        .await
        .unwrap();

    assert_eq!(updated.title, "Updated Title");
    assert!(updated.user_modified);
}

#[tokio::test]
async fn test_update_proposal_user_priority() {
    let project_id = ProjectId::new();
    let session = IdeationSession::new(project_id.clone());
    let session_id = session.id.clone();
    let service = create_service_with_session(session);

    let proposal = service
        .create_proposal(session_id.clone(), create_proposal_options())
        .await
        .unwrap();

    let updated = service
        .update_proposal(
            &proposal.id,
            UpdateProposalOptions {
                user_priority: Some(Priority::Critical),
                ..Default::default()
            },
        )
        .await
        .unwrap();

    assert_eq!(updated.user_priority, Some(Priority::Critical));
    assert_eq!(updated.status, ProposalStatus::Modified);
}

#[tokio::test]
async fn test_update_proposal_not_found() {
    let service = create_service();
    let proposal_id = TaskProposalId::new();

    let result = service
        .update_proposal(
            &proposal_id,
            UpdateProposalOptions {
                title: Some("New Title".to_string()),
                ..Default::default()
            },
        )
        .await;

    assert!(result.is_err());
}

#[tokio::test]
async fn test_archive_proposal() {
    let project_id = ProjectId::new();
    let session = IdeationSession::new(project_id.clone());
    let session_id = session.id.clone();
    let service = create_service_with_session(session);

    let proposal = service
        .create_proposal(session_id.clone(), create_proposal_options())
        .await
        .unwrap();

    service.archive_proposal(&proposal.id).await.unwrap();

    let proposals = service.get_proposals(&session_id).await.unwrap();
    assert!(proposals.is_empty());
}

#[tokio::test]
async fn test_toggle_proposal_selection() {
    let project_id = ProjectId::new();
    let session = IdeationSession::new(project_id.clone());
    let session_id = session.id.clone();
    let service = create_service_with_session(session);

    let proposal = service
        .create_proposal(session_id.clone(), create_proposal_options())
        .await
        .unwrap();

    // Initially not selected (default is false)
    assert!(!proposal.selected);

    // Toggle on
    let new_state = service
        .toggle_proposal_selection(&proposal.id)
        .await
        .unwrap();
    assert!(new_state);

    // Toggle back off
    let new_state = service
        .toggle_proposal_selection(&proposal.id)
        .await
        .unwrap();
    assert!(!new_state);
}

#[tokio::test]
async fn test_get_selected_proposals() {
    let project_id = ProjectId::new();
    let session = IdeationSession::new(project_id.clone());
    let session_id = session.id.clone();
    let service = create_service_with_session(session);

    let proposal1 = service
        .create_proposal(session_id.clone(), create_proposal_options())
        .await
        .unwrap();
    let _proposal2 = service
        .create_proposal(session_id.clone(), create_proposal_options())
        .await
        .unwrap();

    // Select proposal1 only (proposals start unselected by default)
    service
        .set_proposal_selection(&proposal1.id, true)
        .await
        .unwrap();

    let selected = service.get_selected_proposals(&session_id).await.unwrap();
    assert_eq!(selected.len(), 1);
    assert_eq!(selected[0].id, proposal1.id);
}

#[tokio::test]
async fn test_select_all_proposals() {
    let project_id = ProjectId::new();
    let session = IdeationSession::new(project_id.clone());
    let session_id = session.id.clone();
    let service = create_service_with_session(session);

    let proposal1 = service
        .create_proposal(session_id.clone(), create_proposal_options())
        .await
        .unwrap();
    let proposal2 = service
        .create_proposal(session_id.clone(), create_proposal_options())
        .await
        .unwrap();

    // Deselect both
    service
        .set_proposal_selection(&proposal1.id, false)
        .await
        .unwrap();
    service
        .set_proposal_selection(&proposal2.id, false)
        .await
        .unwrap();

    let count = service.select_all_proposals(&session_id).await.unwrap();
    assert_eq!(count, 2);

    let selected = service.get_selected_proposals(&session_id).await.unwrap();
    assert_eq!(selected.len(), 2);
}

#[tokio::test]
async fn test_deselect_all_proposals() {
    let project_id = ProjectId::new();
    let session = IdeationSession::new(project_id.clone());
    let session_id = session.id.clone();
    let service = create_service_with_session(session);

    let p1 = service
        .create_proposal(session_id.clone(), create_proposal_options())
        .await
        .unwrap();
    let p2 = service
        .create_proposal(session_id.clone(), create_proposal_options())
        .await
        .unwrap();

    // First select both (default is unselected)
    service.set_proposal_selection(&p1.id, true).await.unwrap();
    service.set_proposal_selection(&p2.id, true).await.unwrap();

    let count = service.deselect_all_proposals(&session_id).await.unwrap();
    assert_eq!(count, 2);

    let selected = service.get_selected_proposals(&session_id).await.unwrap();
    assert!(selected.is_empty());
}

// ========================================================================
// MESSAGE TESTS
// ========================================================================

#[tokio::test]
async fn test_add_user_message() {
    let project_id = ProjectId::new();
    let session = IdeationSession::new(project_id.clone());
    let session_id = session.id.clone();
    let service = create_service_with_session(session);

    let message = service
        .add_user_message(session_id.clone(), "Hello!")
        .await
        .unwrap();

    assert_eq!(message.session_id, Some(session_id));
    assert_eq!(message.role, MessageRole::User);
    assert_eq!(message.content, "Hello!");
}

#[tokio::test]
async fn test_add_orchestrator_message() {
    let project_id = ProjectId::new();
    let session = IdeationSession::new(project_id.clone());
    let session_id = session.id.clone();
    let service = create_service_with_session(session);

    let message = service
        .add_orchestrator_message(session_id.clone(), "I can help with that.")
        .await
        .unwrap();

    assert_eq!(message.session_id, Some(session_id));
    assert_eq!(message.role, MessageRole::Orchestrator);
}

#[tokio::test]
async fn test_add_system_message() {
    let project_id = ProjectId::new();
    let session = IdeationSession::new(project_id.clone());
    let session_id = session.id.clone();
    let service = create_service_with_session(session);

    let message = service
        .add_system_message(session_id.clone(), "Session started")
        .await
        .unwrap();

    assert_eq!(message.role, MessageRole::System);
}

#[tokio::test]
async fn test_get_session_messages() {
    let project_id = ProjectId::new();
    let session = IdeationSession::new(project_id.clone());
    let session_id = session.id.clone();
    let service = create_service_with_session(session);

    service
        .add_user_message(session_id.clone(), "Message 1")
        .await
        .unwrap();
    service
        .add_orchestrator_message(session_id.clone(), "Message 2")
        .await
        .unwrap();

    let messages = service.get_session_messages(&session_id).await.unwrap();
    assert_eq!(messages.len(), 2);
}

#[tokio::test]
async fn test_get_recent_messages() {
    let project_id = ProjectId::new();
    let session = IdeationSession::new(project_id.clone());
    let session_id = session.id.clone();
    let service = create_service_with_session(session);

    for i in 1..=5 {
        service
            .add_user_message(session_id.clone(), format!("Message {}", i))
            .await
            .unwrap();
    }

    let recent = service.get_recent_messages(&session_id, 3).await.unwrap();
    assert_eq!(recent.len(), 3);
}

// ========================================================================
// SESSION WITH DATA TESTS
// ========================================================================

#[tokio::test]
async fn test_get_session_with_data() {
    let project_id = ProjectId::new();
    let session = IdeationSession::new_with_title(project_id.clone(), "Test Session");
    let session_id = session.id.clone();
    let service = create_service_with_session(session);

    // Add some proposals and messages
    service
        .create_proposal(session_id.clone(), create_proposal_options())
        .await
        .unwrap();
    service
        .add_user_message(session_id.clone(), "Hello")
        .await
        .unwrap();
    service
        .add_orchestrator_message(session_id.clone(), "Hi there")
        .await
        .unwrap();

    let data = service
        .get_session_with_data(&session_id)
        .await
        .unwrap()
        .unwrap();

    assert_eq!(data.session.id, session_id);
    assert_eq!(data.proposals.len(), 1);
    assert_eq!(data.messages.len(), 2);
}

#[tokio::test]
async fn test_get_session_with_data_returns_none_for_nonexistent() {
    let service = create_service();
    let session_id = IdeationSessionId::new();

    let result = service.get_session_with_data(&session_id).await.unwrap();
    assert!(result.is_none());
}

// ========================================================================
// STATS TESTS
// ========================================================================

#[tokio::test]
async fn test_get_session_stats() {
    let project_id = ProjectId::new();
    let session = IdeationSession::new(project_id.clone());
    let session_id = session.id.clone();
    let service = create_service_with_session(session);

    // Add 2 proposals, select 1 (default is unselected)
    service
        .create_proposal(session_id.clone(), create_proposal_options())
        .await
        .unwrap();
    let proposal2 = service
        .create_proposal(session_id.clone(), create_proposal_options())
        .await
        .unwrap();
    service
        .set_proposal_selection(&proposal2.id, true)
        .await
        .unwrap();

    // Add 3 messages
    service
        .add_user_message(session_id.clone(), "1")
        .await
        .unwrap();
    service
        .add_user_message(session_id.clone(), "2")
        .await
        .unwrap();
    service
        .add_user_message(session_id.clone(), "3")
        .await
        .unwrap();

    let stats = service.get_session_stats(&session_id).await.unwrap();

    assert_eq!(stats.total_proposals, 2);
    assert_eq!(stats.selected_proposals, 1);
    assert_eq!(stats.message_count, 3);
}

// ========================================================================
// REORDER TESTS
// ========================================================================

#[tokio::test]
async fn test_reorder_proposals() {
    let project_id = ProjectId::new();
    let session = IdeationSession::new(project_id.clone());
    let session_id = session.id.clone();
    let service = create_service_with_session(session);

    let p1 = service
        .create_proposal(session_id.clone(), create_proposal_options())
        .await
        .unwrap();
    let p2 = service
        .create_proposal(session_id.clone(), create_proposal_options())
        .await
        .unwrap();
    let p3 = service
        .create_proposal(session_id.clone(), create_proposal_options())
        .await
        .unwrap();

    // Reorder: p3, p1, p2
    service
        .reorder_proposals(
            &session_id,
            vec![p3.id.clone(), p1.id.clone(), p2.id.clone()],
        )
        .await
        .unwrap();

    let proposals = service.get_proposals(&session_id).await.unwrap();
    assert_eq!(proposals[0].id, p3.id);
    assert_eq!(proposals[1].id, p1.id);
    assert_eq!(proposals[2].id, p2.id);
}
