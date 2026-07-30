use crate::domain::entities::{
    TeamMessage, TeamMessageDelivery, TeamMessageDeliveryId, TeamMessageDeliveryStatus,
    TeamMessageId, TeamSessionId,
};
use crate::domain::repositories::TeamMessageRepository;
use crate::error::{AppError, AppResult};
use async_trait::async_trait;
use std::collections::HashMap;
use tokio::sync::RwLock;
pub struct MemoryTeamMessageRepository {
    messages: RwLock<HashMap<TeamMessageId, TeamMessage>>,
    deliveries: RwLock<HashMap<TeamMessageDeliveryId, TeamMessageDelivery>>,
}
impl MemoryTeamMessageRepository {
    pub fn new() -> Self {
        Self {
            messages: RwLock::new(HashMap::new()),
            deliveries: RwLock::new(HashMap::new()),
        }
    }
}
impl Default for MemoryTeamMessageRepository {
    fn default() -> Self {
        Self::new()
    }
}
#[async_trait]
impl TeamMessageRepository for MemoryTeamMessageRepository {
    async fn create_envelope_with_deliveries(
        &self,
        message: TeamMessage,
        deliveries: Vec<TeamMessageDelivery>,
    ) -> AppResult<(TeamMessage, Vec<TeamMessageDelivery>)> {
        message.validate().map_err(AppError::Validation)?;
        if deliveries
            .iter()
            .any(|delivery| delivery.message_id != message.id)
        {
            return Err(AppError::Validation(
                "delivery message id mismatch".to_string(),
            ));
        }
        let mut messages = self.messages.write().await;
        let mut stored = self.deliveries.write().await;
        if messages.values().any(|value| {
            value.team_id == message.team_id
                && (value.sequence == message.sequence
                    || value.idempotency_key == message.idempotency_key)
        }) {
            return Err(AppError::Conflict(
                "duplicate team message sequence or idempotency key".to_string(),
            ));
        }
        if deliveries
            .iter()
            .any(|delivery| stored.contains_key(&delivery.id))
        {
            return Err(AppError::Validation("duplicate delivery".to_string()));
        }
        messages.insert(message.id.clone(), message.clone());
        for delivery in &deliveries {
            stored.insert(delivery.id.clone(), delivery.clone());
        }
        Ok((message, deliveries))
    }
    async fn get_message(&self, id: &TeamMessageId) -> AppResult<Option<TeamMessage>> {
        Ok(self.messages.read().await.get(id).cloned())
    }
    async fn get_envelope_by_idempotency_key(
        &self,
        team: &TeamSessionId,
        idempotency_key: &str,
    ) -> AppResult<Option<(TeamMessage, Vec<TeamMessageDelivery>)>> {
        let messages = self.messages.read().await;
        let Some(message) = messages
            .values()
            .find(|message| message.team_id == *team && message.idempotency_key == idempotency_key)
            .cloned()
        else {
            return Ok(None);
        };
        let mut deliveries: Vec<_> = self
            .deliveries
            .read()
            .await
            .values()
            .filter(|delivery| delivery.message_id == message.id)
            .cloned()
            .collect();
        deliveries.sort_by(|left, right| left.id.0.cmp(&right.id.0));
        Ok(Some((message, deliveries)))
    }
    async fn next_message_sequence(&self, team: &TeamSessionId) -> AppResult<i64> {
        Ok(self
            .messages
            .read()
            .await
            .values()
            .filter(|message| message.team_id == *team)
            .map(|message| message.sequence)
            .max()
            .unwrap_or(0)
            + 1)
    }
    async fn list_messages_after(
        &self,
        team: &TeamSessionId,
        sequence: i64,
        limit: u32,
    ) -> AppResult<Vec<TeamMessage>> {
        let mut values: Vec<_> = self
            .messages
            .read()
            .await
            .values()
            .filter(|value| value.team_id == *team && value.sequence > sequence)
            .cloned()
            .collect();
        values.sort_by_key(|value| value.sequence);
        values.truncate(limit as usize);
        Ok(values)
    }
    async fn list_actionable_deliveries(
        &self,
        team: &TeamSessionId,
        limit: u32,
    ) -> AppResult<Vec<TeamMessageDelivery>> {
        let messages = self.messages.read().await;
        let mut values: Vec<_> = self
            .deliveries
            .read()
            .await
            .values()
            .filter(|delivery| {
                messages
                    .get(&delivery.message_id)
                    .is_some_and(|message| message.team_id == *team)
                    && matches!(
                        delivery.status,
                        TeamMessageDeliveryStatus::Pending | TeamMessageDeliveryStatus::Failed
                    )
            })
            .cloned()
            .collect();
        values.truncate(limit as usize);
        Ok(values)
    }
    async fn transition_delivery(
        &self,
        id: &TeamMessageDeliveryId,
        expected: TeamMessageDeliveryStatus,
        next: TeamMessageDeliveryStatus,
        delivery: TeamMessageDelivery,
    ) -> AppResult<bool> {
        let mut values = self.deliveries.write().await;
        let Some(current) = values.get(id) else {
            return Ok(false);
        };
        if current.status != expected || delivery.status != next {
            return Ok(false);
        };
        values.insert(id.clone(), delivery);
        Ok(true)
    }
}
