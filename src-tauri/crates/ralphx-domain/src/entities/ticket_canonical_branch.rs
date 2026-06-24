use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::entities::ProjectId;

/// The single canonical branch that all RalphX conversations for a given ticket
/// base off of, so work for one ticket converges on one branch via PR merge.
///
/// The canonical branch is never checked out into a worktree — each conversation
/// gets its own per-conversation branch based off this canonical branch.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TicketCanonicalBranch {
    pub project_id: ProjectId,
    /// Provider slug, e.g. `jira` | `linear`.
    pub provider: String,
    /// Provider issue key, matching the link tables' `issue_key`.
    pub issue_key: String,
    /// The canonical branch name (e.g. `ralphx/ticket/linear-wise-24`).
    pub branch_name: String,
    /// Project default branch at canonical-branch creation time.
    pub base_branch: String,
    /// SHA the canonical branch was forged at.
    pub base_commit: Option<String>,
    /// `true` once the canonical branch has been confirmed pushed to origin.
    pub origin_pushed: bool,
    /// `true` once the canonical branch has been merged/closed; it must never be resurrected.
    pub terminal: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl TicketCanonicalBranch {
    /// Construct a freshly-forged canonical branch row (not yet pushed, not terminal).
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        project_id: ProjectId,
        provider: impl Into<String>,
        issue_key: impl Into<String>,
        branch_name: impl Into<String>,
        base_branch: impl Into<String>,
        base_commit: Option<String>,
        created_at: DateTime<Utc>,
    ) -> Self {
        Self {
            project_id,
            provider: provider.into(),
            issue_key: issue_key.into(),
            branch_name: branch_name.into(),
            base_branch: base_branch.into(),
            base_commit,
            origin_pushed: false,
            terminal: false,
            created_at,
            updated_at: created_at,
        }
    }
}
