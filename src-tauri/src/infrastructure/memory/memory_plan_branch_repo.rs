// Memory-based PlanBranchRepository implementation for testing

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use std::collections::HashMap;
use std::sync::Arc;
#[cfg(test)]
use std::sync::Mutex;
use tokio::sync::RwLock;

use crate::domain::entities::plan_branch::{PrPushStatus, PrStatus};
use crate::domain::entities::{
    ArtifactId, ExecutionPlanId, IdeationSessionId, PlanBranch, PlanBranchId, PlanBranchStatus,
    ProjectId, TaskId,
};
use crate::domain::repositories::PlanBranchRepository;
#[cfg(test)]
use crate::error::AppError;
use crate::error::AppResult;

pub struct MemoryPlanBranchRepository {
    branches: Arc<RwLock<HashMap<String, PlanBranch>>>,
    local_cleanup_markers: Arc<RwLock<HashMap<String, (String, DateTime<Utc>)>>>,
    #[cfg(test)]
    next_merge_task_lookup_error: Mutex<Option<String>>,
    #[cfg(test)]
    next_pr_eligibility_update_error: Mutex<Option<String>>,
}

impl Default for MemoryPlanBranchRepository {
    fn default() -> Self {
        Self::new()
    }
}

impl MemoryPlanBranchRepository {
    pub fn new() -> Self {
        Self {
            branches: Arc::new(RwLock::new(HashMap::new())),
            local_cleanup_markers: Arc::new(RwLock::new(HashMap::new())),
            #[cfg(test)]
            next_merge_task_lookup_error: Mutex::new(None),
            #[cfg(test)]
            next_pr_eligibility_update_error: Mutex::new(None),
        }
    }

    #[cfg(test)]
    pub fn fail_next_merge_task_lookup(&self, message: impl Into<String>) {
        *self.next_merge_task_lookup_error.lock().unwrap() = Some(message.into());
    }

    #[cfg(test)]
    pub fn fail_next_pr_eligibility_update(&self, message: impl Into<String>) {
        *self.next_pr_eligibility_update_error.lock().unwrap() = Some(message.into());
    }
}

#[async_trait]
impl PlanBranchRepository for MemoryPlanBranchRepository {
    async fn create(&self, branch: PlanBranch) -> AppResult<PlanBranch> {
        let mut branches = self.branches.write().await;
        branches.insert(branch.id.as_str().to_string(), branch.clone());
        Ok(branch)
    }

    async fn create_or_update(&self, branch: PlanBranch) -> AppResult<PlanBranch> {
        let mut branches = self.branches.write().await;
        // Upsert by session_id: replace existing row for same session if present
        let existing_id = branches
            .values()
            .find(|b| b.session_id == branch.session_id)
            .map(|b| b.id.as_str().to_string());
        if let Some(old_id) = existing_id {
            branches.remove(&old_id);
        }
        branches.insert(branch.id.as_str().to_string(), branch.clone());
        Ok(branch)
    }

    async fn get_by_id(&self, id: &PlanBranchId) -> AppResult<Option<PlanBranch>> {
        let branches = self.branches.read().await;
        Ok(branches.get(id.as_str()).cloned())
    }

    async fn get_by_execution_plan_id(
        &self,
        id: &ExecutionPlanId,
    ) -> AppResult<Option<PlanBranch>> {
        let branches = self.branches.read().await;
        Ok(branches
            .values()
            .find(|b| b.execution_plan_id.as_ref() == Some(id))
            .cloned())
    }

    async fn get_by_plan_artifact_id(&self, id: &ArtifactId) -> AppResult<Vec<PlanBranch>> {
        let branches = self.branches.read().await;
        Ok(branches
            .values()
            .filter(|b| b.plan_artifact_id == *id)
            .cloned()
            .collect())
    }

    async fn get_by_session_id(
        &self,
        session_id: &IdeationSessionId,
    ) -> AppResult<Option<PlanBranch>> {
        let branches = self.branches.read().await;
        Ok(branches
            .values()
            .find(|b| b.session_id == *session_id)
            .cloned())
    }

    async fn get_by_merge_task_id(&self, task_id: &TaskId) -> AppResult<Option<PlanBranch>> {
        #[cfg(test)]
        if let Some(message) = self.next_merge_task_lookup_error.lock().unwrap().take() {
            return Err(AppError::Infrastructure(message));
        }
        let branches = self.branches.read().await;
        Ok(branches
            .values()
            .find(|b| b.merge_task_id.as_ref() == Some(task_id))
            .cloned())
    }

    async fn get_by_project_id(&self, project_id: &ProjectId) -> AppResult<Vec<PlanBranch>> {
        let branches = self.branches.read().await;
        Ok(branches
            .values()
            .filter(|b| b.project_id == *project_id)
            .cloned()
            .collect())
    }

    async fn mark_local_cleanup_status(
        &self,
        id: &PlanBranchId,
        status: &str,
        checked_at: DateTime<Utc>,
    ) -> AppResult<()> {
        self.local_cleanup_markers
            .write()
            .await
            .insert(id.as_str().to_string(), (status.to_string(), checked_at));
        Ok(())
    }

    async fn get_local_cleanup_status(&self, id: &PlanBranchId) -> AppResult<Option<String>> {
        Ok(self
            .local_cleanup_markers
            .read()
            .await
            .get(id.as_str())
            .map(|(status, _)| status.clone()))
    }

    async fn clear_local_cleanup_status(&self, id: &PlanBranchId) -> AppResult<()> {
        self.local_cleanup_markers.write().await.remove(id.as_str());
        Ok(())
    }

    async fn update_status(&self, id: &PlanBranchId, status: PlanBranchStatus) -> AppResult<()> {
        let mut branches = self.branches.write().await;
        if let Some(branch) = branches.get_mut(id.as_str()) {
            branch.status = status;
        }
        Ok(())
    }

    async fn update_pr_eligible(&self, id: &PlanBranchId, enabled: bool) -> AppResult<()> {
        #[cfg(test)]
        if let Some(message) = self.next_pr_eligibility_update_error.lock().unwrap().take() {
            return Err(AppError::Infrastructure(message));
        }
        let mut branches = self.branches.write().await;
        if let Some(branch) = branches.get_mut(id.as_str()) {
            branch.pr_eligible = enabled;
        }
        Ok(())
    }

    async fn set_merge_task_id(&self, id: &PlanBranchId, task_id: &TaskId) -> AppResult<()> {
        let mut branches = self.branches.write().await;
        if let Some(branch) = branches.get_mut(id.as_str()) {
            branch.merge_task_id = Some(task_id.clone());
        }
        Ok(())
    }

    async fn clear_merge_task_id(&self, id: &PlanBranchId) -> AppResult<()> {
        let mut branches = self.branches.write().await;
        if let Some(branch) = branches.get_mut(id.as_str()) {
            branch.merge_task_id = None;
        }
        Ok(())
    }

    async fn set_merged(&self, id: &PlanBranchId) -> AppResult<()> {
        let mut branches = self.branches.write().await;
        if let Some(branch) = branches.get_mut(id.as_str()) {
            branch.status = PlanBranchStatus::Merged;
            branch.merged_at = Some(chrono::Utc::now());
        }
        Ok(())
    }

    async fn abandon_active_for_artifact(&self, artifact_id: &ArtifactId) -> AppResult<u32> {
        let mut branches = self.branches.write().await;
        let mut count = 0u32;
        for branch in branches.values_mut() {
            if branch.plan_artifact_id == *artifact_id && branch.status == PlanBranchStatus::Active
            {
                branch.status = PlanBranchStatus::Abandoned;
                count += 1;
            }
        }
        Ok(count)
    }

    async fn delete(&self, id: &PlanBranchId) -> AppResult<()> {
        let mut branches = self.branches.write().await;
        branches.remove(id.as_str());
        Ok(())
    }

    async fn update_pr_info(
        &self,
        id: &PlanBranchId,
        pr_number: i64,
        pr_url: String,
        pr_status: PrStatus,
        pr_draft: bool,
    ) -> AppResult<()> {
        let mut branches = self.branches.write().await;
        if let Some(branch) = branches.get_mut(id.as_str()) {
            branch.pr_number = Some(pr_number);
            branch.pr_url = Some(pr_url);
            branch.pr_status = Some(pr_status);
            branch.pr_draft = Some(pr_draft);
            branch.pr_push_status = crate::domain::entities::plan_branch::PrPushStatus::Pushed;
        }
        Ok(())
    }

    async fn clear_pr_info(&self, id: &PlanBranchId) -> AppResult<()> {
        let mut branches = self.branches.write().await;
        if let Some(branch) = branches.get_mut(id.as_str()) {
            branch.pr_number = None;
            branch.pr_url = None;
            branch.pr_status = None;
            branch.pr_draft = None;
            branch.pr_push_status = crate::domain::entities::plan_branch::PrPushStatus::Pending;
            branch.pr_polling_active = false;
            branch.last_polled_at = None;
            branch.merge_commit_sha = None;
        }
        Ok(())
    }

    async fn update_pr_status(&self, id: &PlanBranchId, status: PrStatus) -> AppResult<()> {
        let mut branches = self.branches.write().await;
        if let Some(branch) = branches.get_mut(id.as_str()) {
            branch.pr_status = Some(status);
        }
        Ok(())
    }

    async fn set_merge_commit_sha(&self, id: &PlanBranchId, sha: String) -> AppResult<()> {
        let mut branches = self.branches.write().await;
        if let Some(branch) = branches.get_mut(id.as_str()) {
            branch.merge_commit_sha = Some(sha);
        }
        Ok(())
    }

    async fn update_last_polled_at(
        &self,
        id: &PlanBranchId,
        polled_at: DateTime<Utc>,
    ) -> AppResult<()> {
        let mut branches = self.branches.write().await;
        if let Some(branch) = branches.get_mut(id.as_str()) {
            branch.last_polled_at = Some(polled_at);
            branch.pr_polling_active = true;
        }
        Ok(())
    }

    async fn clear_polling_active_by_task(&self, task_id: &TaskId) -> AppResult<()> {
        let mut branches = self.branches.write().await;
        for branch in branches.values_mut() {
            if branch.merge_task_id.as_ref() == Some(task_id) {
                branch.pr_polling_active = false;
            }
        }
        Ok(())
    }

    async fn find_pr_polling_task_ids(&self) -> AppResult<Vec<TaskId>> {
        let branches = self.branches.read().await;
        let ids = branches
            .values()
            .filter(|b| b.pr_polling_active)
            .filter_map(|b| b.merge_task_id.clone())
            .collect();
        Ok(ids)
    }

    async fn update_pr_push_status(
        &self,
        id: &PlanBranchId,
        status: PrPushStatus,
    ) -> AppResult<()> {
        use crate::error::AppError;
        let mut branches = self.branches.write().await;
        let branch = branches
            .values_mut()
            .find(|b| b.id == *id)
            .ok_or_else(|| AppError::NotFound(format!("PlanBranch not found: {}", id.as_str())))?;
        branch.pr_push_status = status;
        Ok(())
    }
}
