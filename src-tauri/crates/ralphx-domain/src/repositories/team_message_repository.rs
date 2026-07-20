// Team message repository trait — domain layer abstraction
//
// Defines the contract for team message persistence.
// Messages belong to a team session and track inter-agent communication.

use async_trait::async_trait;

use crate::domain::entities::team::{TeamMessageId, TeamMessageRecord, TeamSessionId};
use crate::error::AppResult;

/// Repository trait for TeamMessageRecord persistence
#[async_trait]
pub trait TeamMessageRepository: Send + Sync {
    /// Create a new team message
    async fn create(&self, message: TeamMessageRecord) -> AppResult<TeamMessageRecord>;

    /// Get all messages for a session, ordered by created_at ASC
    async fn get_by_session(&self, session_id: &TeamSessionId)
        -> AppResult<Vec<TeamMessageRecord>>;

    /// Get recent messages for a session (with limit), ordered oldest→newest
    async fn get_recent_by_session(
        &self,
        session_id: &TeamSessionId,
        limit: u32,
    ) -> AppResult<Vec<TeamMessageRecord>>;

    /// Count messages in a session
    async fn count_by_session(&self, session_id: &TeamSessionId) -> AppResult<u32>;

    /// Delete all messages for a session
    async fn delete_by_session(&self, session_id: &TeamSessionId) -> AppResult<()>;

    /// Delete a single message
    async fn delete(&self, id: &TeamMessageId) -> AppResult<()>;
}
