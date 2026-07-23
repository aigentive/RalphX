use std::collections::HashMap;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use tokio::sync::RwLock;

use crate::domain::entities::{
    ProjectId, ProjectSkillEvidenceBatch, ProjectSkillEvidenceBatchId,
    ProjectSkillEvidenceBatchStatus, ProjectSkillId, TaskOutcomeId,
};
use crate::domain::repositories::ProjectSkillEvidenceBatchRepository;
use crate::error::{AppError, AppResult};

#[derive(Default)]
pub struct MemoryProjectSkillEvidenceBatchRepository {
    batches: RwLock<HashMap<ProjectSkillEvidenceBatchId, ProjectSkillEvidenceBatch>>,
}

impl MemoryProjectSkillEvidenceBatchRepository {
    pub fn new() -> Self {
        Self::default()
    }
}

#[async_trait]
impl ProjectSkillEvidenceBatchRepository for MemoryProjectSkillEvidenceBatchRepository {
    async fn insert_if_absent(
        &self,
        batch: ProjectSkillEvidenceBatch,
    ) -> AppResult<ProjectSkillEvidenceBatch> {
        batch.validate_for_insert()?;
        let mut batches = self.batches.write().await;
        if let Some(existing) = batches.values().find(|existing| {
            existing.project_id == batch.project_id && existing.fingerprint == batch.fingerprint
        }) {
            return Ok(existing.clone());
        }
        if batch.items.iter().any(|item| {
            batches.values().any(|existing| {
                existing
                    .items
                    .iter()
                    .any(|existing_item| existing_item.outcome_id == item.outcome_id)
            })
        }) {
            return Err(AppError::Conflict(
                "task outcome already belongs to an evidence batch".to_string(),
            ));
        }
        batches.insert(batch.id.clone(), batch.clone());
        Ok(batch)
    }

    async fn list_batched_outcome_ids(
        &self,
        project_id: &ProjectId,
    ) -> AppResult<Vec<TaskOutcomeId>> {
        let batches = self.batches.read().await;
        let mut outcome_ids = batches
            .values()
            .filter(|batch| &batch.project_id == project_id)
            .flat_map(|batch| batch.items.iter().map(|item| item.outcome_id.clone()))
            .collect::<Vec<_>>();
        outcome_ids.sort_by(|left, right| left.as_str().cmp(right.as_str()));
        Ok(outcome_ids)
    }

    async fn get_by_outcome_id(
        &self,
        project_id: &ProjectId,
        outcome_id: &TaskOutcomeId,
    ) -> AppResult<Option<ProjectSkillEvidenceBatch>> {
        Ok(self
            .batches
            .read()
            .await
            .values()
            .find(|batch| {
                &batch.project_id == project_id
                    && batch
                        .items
                        .iter()
                        .any(|item| &item.outcome_id == outcome_id)
            })
            .cloned())
    }

    async fn claim_oldest_pending(
        &self,
        project_id: &ProjectId,
        claim_token: &str,
        claimed_at: DateTime<Utc>,
    ) -> AppResult<Option<ProjectSkillEvidenceBatch>> {
        if claim_token.trim().is_empty() {
            return Err(AppError::Validation(
                "project skill evidence claim token is required".to_string(),
            ));
        }
        let mut batches = self.batches.write().await;
        let batch_id = batches
            .values()
            .filter(|batch| {
                &batch.project_id == project_id
                    && batch.status == ProjectSkillEvidenceBatchStatus::Pending
            })
            .min_by(|left, right| {
                left.created_at
                    .cmp(&right.created_at)
                    .then_with(|| left.id.as_str().cmp(right.id.as_str()))
            })
            .map(|batch| batch.id.clone());
        let Some(batch_id) = batch_id else {
            return Ok(None);
        };
        let batch = batches.get_mut(&batch_id).ok_or_else(|| {
            AppError::Database("evidence batch disappeared during claim".to_string())
        })?;
        batch.status = ProjectSkillEvidenceBatchStatus::Consumed;
        batch.claim_token = Some(claim_token.to_string());
        batch.claimed_at = Some(claimed_at);
        batch.updated_at = claimed_at;
        Ok(Some(batch.clone()))
    }

    async fn claim_pending_by_id(
        &self,
        project_id: &ProjectId,
        batch_id: &ProjectSkillEvidenceBatchId,
        claim_token: &str,
        claimed_at: DateTime<Utc>,
    ) -> AppResult<Option<ProjectSkillEvidenceBatch>> {
        if claim_token.trim().is_empty() {
            return Err(AppError::Validation(
                "project skill evidence claim token is required".to_string(),
            ));
        }
        let mut batches = self.batches.write().await;
        let Some(batch) = batches.get_mut(batch_id) else {
            return Ok(None);
        };
        if &batch.project_id != project_id
            || batch.status != ProjectSkillEvidenceBatchStatus::Pending
        {
            return Ok(None);
        }
        batch.status = ProjectSkillEvidenceBatchStatus::Consumed;
        batch.claim_token = Some(claim_token.to_string());
        batch.claimed_at = Some(claimed_at);
        batch.updated_at = claimed_at;
        Ok(Some(batch.clone()))
    }

    async fn release_claim(
        &self,
        batch_id: &ProjectSkillEvidenceBatchId,
        claim_token: &str,
        updated_at: DateTime<Utc>,
    ) -> AppResult<bool> {
        let mut batches = self.batches.write().await;
        let Some(batch) = batches.get_mut(batch_id) else {
            return Ok(false);
        };
        if batch.status != ProjectSkillEvidenceBatchStatus::Consumed
            || batch.claim_token.as_deref() != Some(claim_token)
            || batch.completed_at.is_some()
        {
            return Ok(false);
        }
        batch.status = ProjectSkillEvidenceBatchStatus::Pending;
        batch.claim_token = None;
        batch.claimed_at = None;
        batch.updated_at = updated_at;
        Ok(true)
    }

    async fn complete_claim(
        &self,
        batch_id: &ProjectSkillEvidenceBatchId,
        claim_token: &str,
        project_id: &ProjectId,
        project_skill_id: &ProjectSkillId,
        resolution_action: &str,
        completed_at: DateTime<Utc>,
    ) -> AppResult<bool> {
        let resolution_action = resolution_action.trim();
        if resolution_action.is_empty() {
            return Err(AppError::Validation(
                "project skill evidence resolution action is required".to_string(),
            ));
        }
        let mut batches = self.batches.write().await;
        let Some(batch) = batches.get_mut(batch_id) else {
            return Ok(false);
        };
        if batch.status != ProjectSkillEvidenceBatchStatus::Consumed
            || batch.claim_token.as_deref() != Some(claim_token)
            || &batch.project_id != project_id
            || batch.completed_at.is_some()
        {
            return Ok(false);
        }
        batch.completed_project_skill_id = Some(project_skill_id.clone());
        batch.resolution_action = Some(resolution_action.to_string());
        batch.completed_at = Some(completed_at);
        batch.updated_at = completed_at;
        Ok(true)
    }

    async fn requeue_stale_claims(
        &self,
        project_id: &ProjectId,
        stale_before: DateTime<Utc>,
        updated_at: DateTime<Utc>,
    ) -> AppResult<usize> {
        let mut batches = self.batches.write().await;
        let mut changed = 0;
        for batch in batches.values_mut() {
            if &batch.project_id == project_id
                && batch.status == ProjectSkillEvidenceBatchStatus::Consumed
                && batch.completed_at.is_none()
                && batch
                    .claimed_at
                    .is_some_and(|claimed_at| claimed_at < stale_before)
            {
                batch.status = ProjectSkillEvidenceBatchStatus::Pending;
                batch.claim_token = None;
                batch.claimed_at = None;
                batch.updated_at = updated_at;
                changed += 1;
            }
        }
        Ok(changed)
    }

    async fn get_by_id(
        &self,
        batch_id: &ProjectSkillEvidenceBatchId,
    ) -> AppResult<Option<ProjectSkillEvidenceBatch>> {
        Ok(self.batches.read().await.get(batch_id).cloned())
    }
}
