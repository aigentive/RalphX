use crate::{
    entities::{
        TeamMessage, TeamMessageDelivery, TeamMessageDeliveryId, TeamMessageDeliveryStatus,
        TeamMessageId, TeamSessionId,
    },
    error::AppResult,
};
use async_trait::async_trait;
#[async_trait]
pub trait TeamMessageRepository: Send + Sync {
    /// Persists one immutable envelope and all resolved recipient deliveries atomically.
    async fn create_envelope_with_deliveries(
        &self,
        message: TeamMessage,
        deliveries: Vec<TeamMessageDelivery>,
    ) -> AppResult<(TeamMessage, Vec<TeamMessageDelivery>)>;
    async fn get_message(&self, id: &TeamMessageId) -> AppResult<Option<TeamMessage>>;
    async fn get_delivery(
        &self,
        id: &TeamMessageDeliveryId,
    ) -> AppResult<Option<TeamMessageDelivery>>;
    /// Looks up a previously accepted request before a caller retries its
    /// transport-owned idempotency key.
    async fn get_envelope_by_idempotency_key(
        &self,
        team_id: &TeamSessionId,
        idempotency_key: &str,
    ) -> AppResult<Option<(TeamMessage, Vec<TeamMessageDelivery>)>>;
    /// Returns the next candidate sequence. Implementations still enforce the
    /// unique Team sequence at insert time, so callers must retry a typed
    /// conflict instead of treating this read as a lease.
    async fn next_message_sequence(&self, team_id: &TeamSessionId) -> AppResult<i64>;
    async fn list_messages_after(
        &self,
        team_id: &TeamSessionId,
        sequence: i64,
        limit: u32,
    ) -> AppResult<Vec<TeamMessage>>;
    async fn list_actionable_deliveries(
        &self,
        team_id: &TeamSessionId,
        limit: u32,
    ) -> AppResult<Vec<TeamMessageDelivery>>;
    async fn transition_delivery(
        &self,
        id: &TeamMessageDeliveryId,
        expected: TeamMessageDeliveryStatus,
        next: TeamMessageDeliveryStatus,
        delivery: TeamMessageDelivery,
    ) -> AppResult<bool>;
}
