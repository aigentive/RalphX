// ExecutionPlan repository trait - domain layer abstraction
//
// Defines the contract for execution plan persistence.
// Implementations can use SQLite, PostgreSQL, in-memory, etc.

use async_trait::async_trait;

use crate::domain::entities::{
    ExecutionPlan, ExecutionPlanHaltMode, ExecutionPlanId, IdeationSessionId,
};
use crate::error::AppResult;

/// Repository trait for ExecutionPlan persistence.
/// Implementations can use SQLite, PostgreSQL, in-memory, etc.
#[async_trait]
pub trait ExecutionPlanRepository: Send + Sync {
    /// Create a new execution plan
    async fn create(&self, plan: ExecutionPlan) -> AppResult<ExecutionPlan>;

    /// Get execution plan by ID
    async fn get_by_id(&self, id: &ExecutionPlanId) -> AppResult<Option<ExecutionPlan>>;

    /// Get all execution plans for a session
    async fn get_by_session(&self, session_id: &IdeationSessionId)
        -> AppResult<Vec<ExecutionPlan>>;

    /// Get the active execution plan for a session (status = Active)
    async fn get_active_for_session(
        &self,
        session_id: &IdeationSessionId,
    ) -> AppResult<Option<ExecutionPlan>>;

    /// Mark an execution plan as superseded
    async fn mark_superseded(&self, id: &ExecutionPlanId) -> AppResult<()>;

    /// Set the runtime halt mode for an execution plan
    async fn set_halt_mode(
        &self,
        id: &ExecutionPlanId,
        halt_mode: ExecutionPlanHaltMode,
    ) -> AppResult<()>;

    /// Delete an execution plan
    async fn delete(&self, id: &ExecutionPlanId) -> AppResult<()>;
}
