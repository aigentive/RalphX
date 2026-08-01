use std::collections::HashMap;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use tokio::sync::RwLock;

use crate::domain::entities::{
    AgentRunId, ChatConversationId, DelegationPark, DelegationParkId, DelegationParkState,
};
use crate::domain::repositories::DelegationParkRepository;
use crate::error::{AppError, AppResult};

pub struct MemoryDelegationParkRepo {
    parks: RwLock<HashMap<DelegationParkId, DelegationPark>>,
}

impl MemoryDelegationParkRepo {
    pub fn new() -> Self {
        Self {
            parks: RwLock::new(HashMap::new()),
        }
    }
}

impl Default for MemoryDelegationParkRepo {
    fn default() -> Self {
        Self::new()
    }
}

fn sort_parks(parks: &mut [DelegationPark]) {
    parks.sort_by(|left, right| {
        left.deadline_at
            .cmp(&right.deadline_at)
            .then_with(|| left.id.as_str().cmp(&right.id.as_str()))
    });
}

#[async_trait]
impl DelegationParkRepository for MemoryDelegationParkRepo {
    async fn arm(&self, park: DelegationPark) -> AppResult<DelegationPark> {
        let mut parks = self.parks.write().await;
        if parks.contains_key(&park.id) {
            return Err(AppError::Database(format!(
                "delegation park {} already exists",
                park.id
            )));
        }
        parks.insert(park.id, park.clone());
        Ok(park)
    }

    async fn get(&self, id: &DelegationParkId) -> AppResult<Option<DelegationPark>> {
        Ok(self.parks.read().await.get(id).cloned())
    }

    async fn get_armed_for_conversation(
        &self,
        conversation_id: &ChatConversationId,
    ) -> AppResult<Option<DelegationPark>> {
        let mut parks = self
            .parks
            .read()
            .await
            .values()
            .filter(|park| {
                park.parent_conversation_id == *conversation_id
                    && park.state == DelegationParkState::Armed
            })
            .cloned()
            .collect::<Vec<_>>();
        parks.sort_by(|left, right| left.created_at.cmp(&right.created_at));
        Ok(parks.into_iter().next())
    }

    async fn get_settlement_blocking_for_conversation(
        &self,
        conversation_id: &ChatConversationId,
    ) -> AppResult<Option<DelegationPark>> {
        Ok(self
            .parks
            .read()
            .await
            .values()
            .filter(|park| {
                park.parent_conversation_id == *conversation_id
                    && matches!(
                        park.state,
                        DelegationParkState::Armed
                            | DelegationParkState::Waking
                            | DelegationParkState::Woken
                    )
            })
            .max_by(|left, right| {
                left.updated_at
                    .cmp(&right.updated_at)
                    .then_with(|| left.id.as_str().cmp(&right.id.as_str()))
            })
            .cloned())
    }

    async fn list_armed(&self) -> AppResult<Vec<DelegationPark>> {
        let mut parks = self
            .parks
            .read()
            .await
            .values()
            .filter(|park| park.state == DelegationParkState::Armed)
            .cloned()
            .collect::<Vec<_>>();
        sort_parks(&mut parks);
        Ok(parks)
    }

    async fn list_armed_for_delegated_run(
        &self,
        run_id: &AgentRunId,
    ) -> AppResult<Vec<DelegationPark>> {
        let mut parks = self
            .parks
            .read()
            .await
            .values()
            .filter(|park| {
                park.state == DelegationParkState::Armed
                    && park
                        .jobs
                        .iter()
                        .any(|job| job.delegated_agent_run_id == *run_id)
            })
            .cloned()
            .collect::<Vec<_>>();
        sort_parks(&mut parks);
        Ok(parks)
    }

    async fn record_job_settled(
        &self,
        id: &DelegationParkId,
        delegated_run_id: &AgentRunId,
        status: &str,
    ) -> AppResult<()> {
        if let Some(park) = self.parks.write().await.get_mut(id) {
            if let Some(job) = park
                .jobs
                .iter_mut()
                .find(|job| job.delegated_agent_run_id == *delegated_run_id)
            {
                job.settled_status = Some(status.to_string());
            }
            park.updated_at = Utc::now();
        }
        Ok(())
    }

    async fn claim_wake(&self, id: &DelegationParkId, expected_generation: i64) -> AppResult<bool> {
        let mut parks = self.parks.write().await;
        let Some(park) = parks.get_mut(id) else {
            return Ok(false);
        };
        if park.state != DelegationParkState::Armed || park.generation != expected_generation {
            return Ok(false);
        }
        park.state = DelegationParkState::Waking;
        park.updated_at = Utc::now();
        Ok(true)
    }

    async fn record_wake_failure(&self, id: &DelegationParkId, error: &str) -> AppResult<i32> {
        let mut parks = self.parks.write().await;
        let Some(park) = parks.get_mut(id) else {
            return Err(AppError::NotFound(format!(
                "delegation park not found: {id}"
            )));
        };
        park.wake_attempts += 1;
        park.last_error = Some(error.to_string());
        park.updated_at = Utc::now();
        Ok(park.wake_attempts)
    }

    async fn list_wake_stalled(&self, older_than: DateTime<Utc>) -> AppResult<Vec<DelegationPark>> {
        let mut parks = self
            .parks
            .read()
            .await
            .values()
            .filter(|park| {
                park.state == DelegationParkState::Waking && park.updated_at <= older_than
            })
            .cloned()
            .collect::<Vec<_>>();
        sort_parks(&mut parks);
        Ok(parks)
    }

    async fn reset_wake_claim(&self, id: &DelegationParkId) -> AppResult<bool> {
        let mut parks = self.parks.write().await;
        let Some(park) = parks.get_mut(id) else {
            return Ok(false);
        };
        if park.state != DelegationParkState::Waking {
            return Ok(false);
        }
        park.state = DelegationParkState::Armed;
        park.wake_attempts = 0;
        park.updated_at = Utc::now();
        Ok(true)
    }

    async fn settle(
        &self,
        id: &DelegationParkId,
        state: DelegationParkState,
        error: Option<&str>,
    ) -> AppResult<()> {
        if let Some(park) = self.parks.write().await.get_mut(id) {
            park.state = state;
            park.last_error = error.map(str::to_string);
            park.updated_at = Utc::now();
        }
        Ok(())
    }

    async fn supersede_for_conversation(
        &self,
        conversation_id: &ChatConversationId,
    ) -> AppResult<usize> {
        let mut count = 0;
        for park in self.parks.write().await.values_mut() {
            if park.parent_conversation_id == *conversation_id
                && matches!(
                    park.state,
                    DelegationParkState::Armed | DelegationParkState::Waking
                )
            {
                park.state = DelegationParkState::Superseded;
                park.updated_at = Utc::now();
                count += 1;
            }
        }
        Ok(count)
    }

    async fn list_expired(&self, now: DateTime<Utc>) -> AppResult<Vec<DelegationPark>> {
        let mut parks = self
            .parks
            .read()
            .await
            .values()
            .filter(|park| park.state == DelegationParkState::Armed && park.deadline_at <= now)
            .cloned()
            .collect::<Vec<_>>();
        sort_parks(&mut parks);
        Ok(parks)
    }
}
