use async_trait::async_trait;
use chrono::{DateTime, Utc};

use crate::domain::entities::{
    ProjectId, ProjectSkillEvidenceBatch, ProjectSkillEvidenceBatchId, ProjectSkillId,
    TaskOutcomeId,
};
use crate::error::AppResult;

#[async_trait]
pub trait ProjectSkillEvidenceBatchRepository: Send + Sync {
    async fn insert_if_absent(
        &self,
        batch: ProjectSkillEvidenceBatch,
    ) -> AppResult<ProjectSkillEvidenceBatch>;

    async fn list_batched_outcome_ids(
        &self,
        project_id: &ProjectId,
    ) -> AppResult<Vec<TaskOutcomeId>>;

    async fn get_by_outcome_id(
        &self,
        project_id: &ProjectId,
        outcome_id: &TaskOutcomeId,
    ) -> AppResult<Option<ProjectSkillEvidenceBatch>>;

    async fn claim_oldest_pending(
        &self,
        project_id: &ProjectId,
        claim_token: &str,
        claimed_at: DateTime<Utc>,
    ) -> AppResult<Option<ProjectSkillEvidenceBatch>>;

    async fn claim_pending_by_id(
        &self,
        project_id: &ProjectId,
        batch_id: &ProjectSkillEvidenceBatchId,
        claim_token: &str,
        claimed_at: DateTime<Utc>,
    ) -> AppResult<Option<ProjectSkillEvidenceBatch>>;

    async fn release_claim(
        &self,
        batch_id: &ProjectSkillEvidenceBatchId,
        claim_token: &str,
        updated_at: DateTime<Utc>,
    ) -> AppResult<bool>;

    async fn complete_claim(
        &self,
        batch_id: &ProjectSkillEvidenceBatchId,
        claim_token: &str,
        project_id: &ProjectId,
        project_skill_id: &ProjectSkillId,
        resolution_action: &str,
        completed_at: DateTime<Utc>,
    ) -> AppResult<bool>;

    async fn requeue_stale_claims(
        &self,
        project_id: &ProjectId,
        stale_before: DateTime<Utc>,
        updated_at: DateTime<Utc>,
    ) -> AppResult<usize>;

    async fn get_by_id(
        &self,
        batch_id: &ProjectSkillEvidenceBatchId,
    ) -> AppResult<Option<ProjectSkillEvidenceBatch>>;
}
