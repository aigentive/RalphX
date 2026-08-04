use async_trait::async_trait;
use chrono::{DateTime, Utc};
use std::sync::RwLock;

use crate::domain::entities::{DelegatedSession, DelegatedSessionId};
use crate::domain::repositories::DelegatedSessionRepository;
use crate::error::AppResult;

pub struct MemoryDelegatedSessionRepository {
    sessions: RwLock<Vec<DelegatedSession>>,
}

impl MemoryDelegatedSessionRepository {
    pub fn new() -> Self {
        Self {
            sessions: RwLock::new(Vec::new()),
        }
    }
}

impl Default for MemoryDelegatedSessionRepository {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl DelegatedSessionRepository for MemoryDelegatedSessionRepository {
    async fn create(&self, session: DelegatedSession) -> AppResult<DelegatedSession> {
        self.sessions.write().unwrap().push(session.clone());
        Ok(session)
    }

    async fn get_by_id(&self, id: &DelegatedSessionId) -> AppResult<Option<DelegatedSession>> {
        Ok(self
            .sessions
            .read()
            .unwrap()
            .iter()
            .find(|session| session.id == *id)
            .cloned())
    }

    async fn get_by_parent_context(
        &self,
        parent_context_type: &str,
        parent_context_id: &str,
    ) -> AppResult<Vec<DelegatedSession>> {
        let mut sessions: Vec<_> = self
            .sessions
            .read()
            .unwrap()
            .iter()
            .filter(|session| {
                session.parent_context_type == parent_context_type
                    && session.parent_context_id == parent_context_id
            })
            .cloned()
            .collect();
        sessions.sort_by(|left, right| right.created_at.cmp(&left.created_at));
        Ok(sessions)
    }

    async fn list_active_by_caller_conversation(
        &self,
        caller_conversation_id: &str,
    ) -> AppResult<Vec<DelegatedSession>> {
        let mut sessions: Vec<_> = self
            .sessions
            .read()
            .unwrap()
            .iter()
            .filter(|session| {
                session.caller_conversation_id.as_deref() == Some(caller_conversation_id)
                    && !matches!(
                        session.status.as_str(),
                        "completed" | "failed" | "cancelled"
                    )
            })
            .cloned()
            .collect();
        sessions.sort_by(|left, right| right.created_at.cmp(&left.created_at));
        Ok(sessions)
    }

    async fn update_job_identity(
        &self,
        id: &DelegatedSessionId,
        caller_conversation_id: Option<String>,
        job_id: String,
        parent_agent_run_id: Option<String>,
    ) -> AppResult<()> {
        let mut sessions = self.sessions.write().unwrap();
        if let Some(session) = sessions.iter_mut().find(|session| session.id == *id) {
            session.caller_conversation_id = caller_conversation_id;
            session.job_id = Some(job_id);
            session.parent_agent_run_id = parent_agent_run_id;
            session.updated_at = Utc::now();
        }
        Ok(())
    }

    async fn update_provider_session_id(
        &self,
        id: &DelegatedSessionId,
        provider_session_id: Option<String>,
    ) -> AppResult<()> {
        let mut sessions = self.sessions.write().unwrap();
        if let Some(session) = sessions.iter_mut().find(|session| session.id == *id) {
            session.provider_session_id = provider_session_id;
            session.updated_at = Utc::now();
        }
        Ok(())
    }

    async fn update_status(
        &self,
        id: &DelegatedSessionId,
        status: &str,
        error: Option<String>,
        completed_at: Option<DateTime<Utc>>,
    ) -> AppResult<()> {
        let mut sessions = self.sessions.write().unwrap();
        if let Some(session) = sessions.iter_mut().find(|session| session.id == *id) {
            session.status = status.to_string();
            session.error = error;
            session.completed_at = completed_at;
            session.updated_at = Utc::now();
        }
        Ok(())
    }
}

#[cfg(test)]
#[path = "memory_delegated_session_repo_tests.rs"]
mod tests;
