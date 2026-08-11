// Question repository trait - domain layer abstraction for question persistence
//
// This trait defines the contract for persisting pending questions (from AskUserQuestion).
// SQLite stores data for restart resilience + audit trail; in-memory channels remain for signaling.
// Records live in crate::domain::entities::question_request.

use async_trait::async_trait;

use crate::domain::entities::question_request::{PendingQuestionInfo, QuestionAnswer};
use crate::error::AppResult;

/// Repository trait for pending question persistence.
/// Implementations can use SQLite, in-memory, etc.
#[async_trait]
pub trait QuestionRepository: Send + Sync {
    /// Persist a new pending question
    async fn create_pending(&self, info: &PendingQuestionInfo) -> AppResult<()>;

    /// Mark a question as resolved with the given answer
    async fn resolve(&self, request_id: &str, answer: &QuestionAnswer) -> AppResult<bool>;

    /// Get all user-visible unresolved questions, including questions whose
    /// agent-side wait has timed out but can still accept a late answer.
    async fn get_pending(&self) -> AppResult<Vec<PendingQuestionInfo>>;

    /// Get a single question by its request_id
    async fn get_by_request_id(&self, request_id: &str) -> AppResult<Option<PendingQuestionInfo>>;

    /// Mark all active waits as expired while keeping questions user-visible.
    async fn expire_all_pending(&self) -> AppResult<u64>;

    /// Mark a single active wait as expired while keeping the question user-visible.
    async fn expire_by_request_id(&self, request_id: &str) -> AppResult<()>;

    /// Remove a question record by request_id
    async fn remove(&self, request_id: &str) -> AppResult<bool>;

    /// Get the answer for a resolved question (returns None if not found or not yet resolved)
    async fn get_resolved_answer(&self, request_id: &str) -> AppResult<Option<QuestionAnswer>>;
}

#[cfg(test)]
#[path = "question_repository_tests.rs"]
mod tests;
