// Team session repository trait — domain layer abstraction
//
// Defines the contract for team session persistence.
// Sessions track agent team composition, phase, and lifecycle.

use async_trait::async_trait;

use crate::domain::entities::team::{TeamSession, TeamSessionId, TeammateSnapshot};
use crate::error::AppResult;

/// Repository trait for TeamSession persistence
#[async_trait]
pub trait TeamSessionRepository: Send + Sync {
    /// Create a new team session
    async fn create(&self, session: TeamSession) -> AppResult<TeamSession>;

    /// Get session by ID
    async fn get_by_id(&self, id: &TeamSessionId) -> AppResult<Option<TeamSession>>;

    /// Get all sessions for a context (type + id)
    async fn get_by_context(
        &self,
        context_type: &str,
        context_id: &str,
    ) -> AppResult<Vec<TeamSession>>;

    /// Get the active (non-disbanded) session for a context
    async fn get_active_for_context(
        &self,
        context_type: &str,
        context_id: &str,
    ) -> AppResult<Option<TeamSession>>;

    /// Update the team phase
    async fn update_phase(&self, id: &TeamSessionId, phase: &str) -> AppResult<()>;

    /// Update the teammate snapshot list
    async fn update_teammates(
        &self,
        id: &TeamSessionId,
        teammates: &[TeammateSnapshot],
    ) -> AppResult<()>;

    /// Mark a session as disbanded
    async fn set_disbanded(&self, id: &TeamSessionId) -> AppResult<()>;

    /// Mark all active (non-disbanded) sessions as disbanded. Returns the count of rows affected.
    async fn disband_all_active(&self, reason: &str) -> AppResult<usize>;
}
