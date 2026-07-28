use crate::domain::entities::{TeamWakeBatch, TeamWakeBatchId, TeamWakeBatchStatus};
use crate::domain::repositories::TeamWakeBatchRepository;
use crate::error::{AppError, AppResult};
use async_trait::async_trait;
use std::collections::HashMap;
use tokio::sync::RwLock;
pub struct MemoryTeamWakeBatchRepository {
    values: RwLock<HashMap<TeamWakeBatchId, TeamWakeBatch>>,
}
impl MemoryTeamWakeBatchRepository {
    pub fn new() -> Self {
        Self {
            values: RwLock::new(HashMap::new()),
        }
    }
}
impl Default for MemoryTeamWakeBatchRepository {
    fn default() -> Self {
        Self::new()
    }
}
#[async_trait]
impl TeamWakeBatchRepository for MemoryTeamWakeBatchRepository {
    async fn create_or_extend_active(&self, batch: TeamWakeBatch) -> AppResult<TeamWakeBatch> {
        batch.validate(i64::MAX).map_err(AppError::Validation)?;
        let mut values = self.values.write().await;
        if let Some(existing) = values.values_mut().find(|value| {
            value.team_id == batch.team_id
                && value.recipient_kind == batch.recipient_kind
                && value.recipient_member_id == batch.recipient_member_id
                && value.recipient_generation == batch.recipient_generation
                && value.status.is_active()
        }) {
            existing.first_message_sequence = existing
                .first_message_sequence
                .min(batch.first_message_sequence);
            existing.last_message_sequence = existing
                .last_message_sequence
                .max(batch.last_message_sequence);
            for id in batch.delivery_ids {
                if !existing.delivery_ids.contains(&id) {
                    existing.delivery_ids.push(id)
                }
            }
            return Ok(existing.clone());
        }
        values.insert(batch.id.clone(), batch.clone());
        Ok(batch)
    }
    async fn get_by_id(&self, id: &TeamWakeBatchId) -> AppResult<Option<TeamWakeBatch>> {
        Ok(self.values.read().await.get(id).cloned())
    }
    async fn list_queued_for_team(
        &self,
        team_id: &crate::domain::entities::TeamSessionId,
        limit: u32,
    ) -> AppResult<Vec<TeamWakeBatch>> {
        let mut batches: Vec<_> = self
            .values
            .read()
            .await
            .values()
            .filter(|batch| {
                batch.team_id == *team_id && batch.status == TeamWakeBatchStatus::Queued
            })
            .cloned()
            .collect();
        batches.sort_by(|left, right| {
            left.created_at
                .cmp(&right.created_at)
                .then_with(|| left.id.0.cmp(&right.id.0))
        });
        batches.truncate(limit as usize);
        Ok(batches)
    }
    async fn transition(
        &self,
        id: &TeamWakeBatchId,
        expected_version: i64,
        expected: TeamWakeBatchStatus,
        batch: TeamWakeBatch,
    ) -> AppResult<bool> {
        let mut values = self.values.write().await;
        let Some(current) = values.get(id) else {
            return Ok(false);
        };
        if current.version != expected_version || current.status != expected {
            return Ok(false);
        };
        values.insert(id.clone(), batch);
        Ok(true)
    }
}
