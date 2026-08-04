use async_trait::async_trait;
use chrono::{DateTime, Utc};

use crate::domain::entities::{DelegatedSession, DelegatedSessionId};
use crate::error::AppResult;

#[async_trait]
pub trait DelegatedSessionRepository: Send + Sync {
    async fn create(&self, session: DelegatedSession) -> AppResult<DelegatedSession>;

    async fn get_by_id(&self, id: &DelegatedSessionId) -> AppResult<Option<DelegatedSession>>;

    async fn get_by_parent_context(
        &self,
        parent_context_type: &str,
        parent_context_id: &str,
    ) -> AppResult<Vec<DelegatedSession>>;

    async fn list_active_by_caller_conversation(
        &self,
        caller_conversation_id: &str,
    ) -> AppResult<Vec<DelegatedSession>>;

    async fn update_job_identity(
        &self,
        id: &DelegatedSessionId,
        job_id: String,
        parent_agent_run_id: Option<String>,
    ) -> AppResult<()>;

    async fn update_provider_session_id(
        &self,
        id: &DelegatedSessionId,
        provider_session_id: Option<String>,
    ) -> AppResult<()>;

    async fn update_status(
        &self,
        id: &DelegatedSessionId,
        status: &str,
        error: Option<String>,
        completed_at: Option<DateTime<Utc>>,
    ) -> AppResult<()>;
}
