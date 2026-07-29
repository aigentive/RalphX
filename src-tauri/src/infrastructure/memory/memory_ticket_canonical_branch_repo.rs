use std::collections::HashMap;

use async_trait::async_trait;
use chrono::Utc;
use tokio::sync::RwLock;

use crate::domain::entities::{ProjectId, TicketCanonicalBranch};
use crate::domain::repositories::TicketCanonicalBranchRepository;
use crate::error::AppResult;

type BranchKey = (String, String, String);

fn key(project_id: &ProjectId, provider: &str, issue_key: &str) -> BranchKey {
    (
        project_id.as_str().to_string(),
        provider.to_string(),
        issue_key.to_string(),
    )
}

pub struct MemoryTicketCanonicalBranchRepository {
    branches: RwLock<HashMap<BranchKey, TicketCanonicalBranch>>,
}

impl MemoryTicketCanonicalBranchRepository {
    pub fn new() -> Self {
        Self {
            branches: RwLock::new(HashMap::new()),
        }
    }
}

impl Default for MemoryTicketCanonicalBranchRepository {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl TicketCanonicalBranchRepository for MemoryTicketCanonicalBranchRepository {
    async fn get(
        &self,
        project_id: &ProjectId,
        provider: &str,
        issue_key: &str,
    ) -> AppResult<Option<TicketCanonicalBranch>> {
        Ok(self
            .branches
            .read()
            .await
            .get(&key(project_id, provider, issue_key))
            .cloned())
    }

    async fn upsert(
        &self,
        mut branch: TicketCanonicalBranch,
    ) -> AppResult<TicketCanonicalBranch> {
        let map_key = key(&branch.project_id, &branch.provider, &branch.issue_key);
        let mut branches = self.branches.write().await;
        if let Some(existing) = branches.get(&map_key) {
            branch.created_at = existing.created_at;
        }
        branch.updated_at = Utc::now();
        branches.insert(map_key, branch.clone());
        Ok(branch)
    }

    async fn mark_origin_pushed(
        &self,
        project_id: &ProjectId,
        provider: &str,
        issue_key: &str,
    ) -> AppResult<()> {
        let mut branches = self.branches.write().await;
        if let Some(branch) = branches.get_mut(&key(project_id, provider, issue_key)) {
            branch.origin_pushed = true;
            branch.updated_at = Utc::now();
        }
        Ok(())
    }

    async fn mark_terminal(
        &self,
        project_id: &ProjectId,
        provider: &str,
        issue_key: &str,
    ) -> AppResult<()> {
        let mut branches = self.branches.write().await;
        if let Some(branch) = branches.get_mut(&key(project_id, provider, issue_key)) {
            branch.terminal = true;
            branch.updated_at = Utc::now();
        }
        Ok(())
    }
}

#[cfg(test)]
#[path = "memory_ticket_canonical_branch_repo_tests.rs"]
mod tests;
