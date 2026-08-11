use async_trait::async_trait;

use crate::entities::{ProjectId, TicketCanonicalBranch};
use crate::error::AppResult;

/// Persistence for the single canonical branch per (project, provider, issue_key)
/// that all RalphX conversations for a ticket base off of.
#[async_trait]
pub trait TicketCanonicalBranchRepository: Send + Sync {
    async fn get(
        &self,
        project_id: &ProjectId,
        provider: &str,
        issue_key: &str,
    ) -> AppResult<Option<TicketCanonicalBranch>>;

    async fn upsert(&self, branch: TicketCanonicalBranch) -> AppResult<TicketCanonicalBranch>;

    async fn mark_origin_pushed(
        &self,
        project_id: &ProjectId,
        provider: &str,
        issue_key: &str,
    ) -> AppResult<()>;

    async fn mark_terminal(
        &self,
        project_id: &ProjectId,
        provider: &str,
        issue_key: &str,
    ) -> AppResult<()>;
}
