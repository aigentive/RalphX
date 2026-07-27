use std::collections::HashMap;

use async_trait::async_trait;
use chrono::Utc;
use tokio::sync::RwLock;

use crate::domain::entities::{
    ProjectId, TicketCanonicalBranch, TicketCanonicalBranchCycle, TicketCanonicalBranchCycleState,
    TicketCanonicalBranchPolicyKind,
};
use crate::domain::repositories::TicketCanonicalBranchRepository;
use crate::error::{AppError, AppResult};

type BranchKey = (String, String, String);

fn key(project_id: &ProjectId, provider: &str, issue_key: &str) -> BranchKey {
    (
        project_id.as_str().to_string(),
        provider.to_string(),
        issue_key.to_string(),
    )
}

fn branch_collision<'a>(
    branches: &'a HashMap<BranchKey, TicketCanonicalBranch>,
    candidate_key: &BranchKey,
    project_id: &ProjectId,
    branch_name: &str,
) -> Option<&'a TicketCanonicalBranch> {
    branches.iter().find_map(|(stored_key, branch)| {
        (stored_key != candidate_key
            && branch.project_id == *project_id
            && branch.branch_name == branch_name)
            .then_some(branch)
    })
}

fn branch_name_conflict(
    candidate: &TicketCanonicalBranch,
    existing: &TicketCanonicalBranch,
) -> AppError {
    AppError::Conflict(format!(
        "Ticket branch '{}' is already bound to {}:{} in project {}",
        candidate.branch_name, existing.provider, existing.issue_key, candidate.project_id
    ))
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

    async fn get_by_branch_name(
        &self,
        project_id: &ProjectId,
        branch_name: &str,
    ) -> AppResult<Option<TicketCanonicalBranch>> {
        Ok(self
            .branches
            .read()
            .await
            .values()
            .find(|branch| branch.project_id == *project_id && branch.branch_name == branch_name)
            .cloned())
    }

    async fn upsert(&self, mut branch: TicketCanonicalBranch) -> AppResult<TicketCanonicalBranch> {
        if branch.policy_kind != TicketCanonicalBranchPolicyKind::LegacyCanonicalBase {
            return Err(AppError::Conflict(
                "Strict ticket bindings must be created with create_if_absent".to_string(),
            ));
        }
        branch.validate_policy().map_err(AppError::Validation)?;
        let map_key = key(&branch.project_id, &branch.provider, &branch.issue_key);
        let mut branches = self.branches.write().await;
        if let Some(existing) = branches.get(&map_key) {
            if existing.policy_kind == TicketCanonicalBranchPolicyKind::StrictGitConvention {
                return Err(AppError::Conflict(format!(
                    "Strict ticket binding {}:{} is immutable",
                    existing.provider, existing.issue_key
                )));
            }
            branch.created_at = existing.created_at;
        }
        if let Some(existing) =
            branch_collision(&branches, &map_key, &branch.project_id, &branch.branch_name)
        {
            return Err(branch_name_conflict(&branch, existing));
        }
        branch.updated_at = Utc::now();
        branches.insert(map_key, branch.clone());
        Ok(branch)
    }

    async fn create_if_absent(
        &self,
        branch: TicketCanonicalBranch,
    ) -> AppResult<TicketCanonicalBranch> {
        branch.validate_policy().map_err(AppError::Validation)?;
        let map_key = key(&branch.project_id, &branch.provider, &branch.issue_key);
        let mut branches = self.branches.write().await;
        if let Some(existing) = branches.get(&map_key) {
            return Ok(existing.clone());
        }
        if let Some(existing) =
            branch_collision(&branches, &map_key, &branch.project_id, &branch.branch_name)
        {
            return Err(branch_name_conflict(&branch, existing));
        }
        branches.insert(map_key, branch.clone());
        Ok(branch)
    }

    async fn compare_and_swap_cycle(
        &self,
        project_id: &ProjectId,
        provider: &str,
        issue_key: &str,
        expected_generation: i64,
        expected_state: TicketCanonicalBranchCycleState,
        replacement: TicketCanonicalBranchCycle,
    ) -> AppResult<bool> {
        replacement
            .validate_replacement(expected_generation)
            .map_err(AppError::Validation)?;
        let mut branches = self.branches.write().await;
        let Some(branch) = branches.get_mut(&key(project_id, provider, issue_key)) else {
            return Ok(false);
        };
        if branch.policy_kind != TicketCanonicalBranchPolicyKind::StrictGitConvention
            || branch.cycle.generation != expected_generation
            || branch.cycle.state != expected_state
        {
            return Ok(false);
        }
        branch.cycle = replacement;
        branch.updated_at = Utc::now();
        Ok(true)
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
            if branch.policy_kind == TicketCanonicalBranchPolicyKind::StrictGitConvention {
                return Err(AppError::Conflict(
                    "Strict ticket bindings use per-cycle state instead of terminal".to_string(),
                ));
            }
            branch.terminal = true;
            branch.updated_at = Utc::now();
        }
        Ok(())
    }
}

#[cfg(test)]
#[path = "memory_ticket_canonical_branch_repo_tests.rs"]
mod tests;
