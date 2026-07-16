use async_trait::async_trait;

use crate::entities::{
    ProjectId, TicketCanonicalBranch, TicketCanonicalBranchCycle, TicketCanonicalBranchCycleState,
};
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

    /// Resolve a persisted policy from the exact workspace branch identity.
    async fn get_by_branch_name(
        &self,
        project_id: &ProjectId,
        branch_name: &str,
    ) -> AppResult<Option<TicketCanonicalBranch>>;

    /// Legacy canonical-base mutation path. Implementations must reject any
    /// attempt to create or overwrite a strict binding through this method.
    async fn upsert(&self, branch: TicketCanonicalBranch) -> AppResult<TicketCanonicalBranch>;

    /// Insert a binding once and return the winner when the ticket key already
    /// exists. A different ticket using the same project/branch is a conflict.
    async fn create_if_absent(
        &self,
        branch: TicketCanonicalBranch,
    ) -> AppResult<TicketCanonicalBranch>;

    /// Replace strict per-cycle state only when both generation and state still
    /// match the caller's snapshot. Returns `false` without mutation when stale.
    #[allow(clippy::too_many_arguments)]
    async fn compare_and_swap_cycle(
        &self,
        project_id: &ProjectId,
        provider: &str,
        issue_key: &str,
        expected_generation: i64,
        expected_state: TicketCanonicalBranchCycleState,
        replacement: TicketCanonicalBranchCycle,
    ) -> AppResult<bool>;

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
