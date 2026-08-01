use async_trait::async_trait;
use chrono::{DateTime, Utc};
use tokio::sync::Mutex;

use crate::domain::entities::{
    ChatConversationId, RemoteConversationModeSwitchRequest, RemoteConversationModeSwitchStatus,
};
use crate::domain::repositories::RemoteConversationModeSwitchRequestRepository;
use crate::error::AppResult;

pub struct MemoryRemoteConversationModeSwitchRequestRepository {
    requests: Mutex<Vec<RemoteConversationModeSwitchRequest>>,
}

impl MemoryRemoteConversationModeSwitchRequestRepository {
    pub fn new() -> Self {
        Self {
            requests: Mutex::new(Vec::new()),
        }
    }
}

impl Default for MemoryRemoteConversationModeSwitchRequestRepository {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl RemoteConversationModeSwitchRequestRepository
    for MemoryRemoteConversationModeSwitchRequestRepository
{
    async fn create_mode_switch_request(
        &self,
        request: RemoteConversationModeSwitchRequest,
    ) -> AppResult<RemoteConversationModeSwitchRequest> {
        let mut requests = self.requests.lock().await;
        requests.push(request.clone());
        Ok(request)
    }

    async fn get_mode_switch_request(
        &self,
        id: &str,
    ) -> AppResult<Option<RemoteConversationModeSwitchRequest>> {
        let requests = self.requests.lock().await;
        Ok(requests.iter().find(|request| request.id == id).cloned())
    }

    async fn find_unsettled_mode_switch_request_for_conversation(
        &self,
        conversation_id: &ChatConversationId,
    ) -> AppResult<Option<RemoteConversationModeSwitchRequest>> {
        let requests = self.requests.lock().await;
        Ok(requests
            .iter()
            .find(|request| {
                &request.conversation_id == conversation_id && !request.status.is_terminal()
            })
            .cloned())
    }

    async fn claim_pending_mode_switch_request(
        &self,
        claimed_at: DateTime<Utc>,
    ) -> AppResult<Option<RemoteConversationModeSwitchRequest>> {
        // Held across scan + flip → atomic; insertion order preserves oldest-first.
        let mut requests = self.requests.lock().await;
        let Some(request) = requests
            .iter_mut()
            .find(|request| request.status == RemoteConversationModeSwitchStatus::Pending)
        else {
            return Ok(None);
        };
        request.status = RemoteConversationModeSwitchStatus::Switching;
        request.claimed_at = Some(claimed_at);
        request.updated_at = claimed_at;
        Ok(Some(request.clone()))
    }

    async fn complete_mode_switch_request(
        &self,
        id: &str,
        updated_at: DateTime<Utc>,
    ) -> AppResult<()> {
        self.settle(
            id,
            RemoteConversationModeSwitchStatus::Switched,
            None,
            updated_at,
        )
        .await
    }

    async fn resolve_mode_switch_request_already_in_mode(
        &self,
        id: &str,
        updated_at: DateTime<Utc>,
    ) -> AppResult<()> {
        self.settle(
            id,
            RemoteConversationModeSwitchStatus::AlreadyInMode,
            None,
            updated_at,
        )
        .await
    }

    async fn fail_mode_switch_request(
        &self,
        id: &str,
        error_code: &str,
        updated_at: DateTime<Utc>,
    ) -> AppResult<()> {
        self.settle(
            id,
            RemoteConversationModeSwitchStatus::Failed,
            Some(error_code.to_string()),
            updated_at,
        )
        .await
    }

    async fn cancel_pending_mode_switch_requests_for_device(
        &self,
        device_id: &str,
        updated_at: DateTime<Utc>,
    ) -> AppResult<u64> {
        let mut requests = self.requests.lock().await;
        let mut changed = 0u64;
        for request in requests.iter_mut() {
            if request.requested_by_device_id == device_id
                && request.status == RemoteConversationModeSwitchStatus::Pending
            {
                request.status = RemoteConversationModeSwitchStatus::Cancelled;
                request.updated_at = updated_at;
                changed += 1;
            }
        }
        Ok(changed)
    }

    async fn fail_stale_switching_mode_switch_requests(
        &self,
        claimed_before: DateTime<Utc>,
        updated_at: DateTime<Utc>,
    ) -> AppResult<u64> {
        let mut requests = self.requests.lock().await;
        let mut changed = 0u64;
        for request in requests.iter_mut() {
            if request.status == RemoteConversationModeSwitchStatus::Switching
                && request
                    .claimed_at
                    .map(|value| value < claimed_before)
                    .unwrap_or(false)
            {
                request.status = RemoteConversationModeSwitchStatus::FailedStale;
                request.updated_at = updated_at;
                changed += 1;
            }
        }
        Ok(changed)
    }
}

impl MemoryRemoteConversationModeSwitchRequestRepository {
    /// Shared terminal write, guarded on `Switching` exactly like the SQLite `WHERE` clause: a
    /// late settle must never resurrect or downgrade an already-settled row.
    async fn settle(
        &self,
        id: &str,
        status: RemoteConversationModeSwitchStatus,
        error_code: Option<String>,
        updated_at: DateTime<Utc>,
    ) -> AppResult<()> {
        let mut requests = self.requests.lock().await;
        if let Some(request) = requests.iter_mut().find(|request| {
            request.id == id && request.status == RemoteConversationModeSwitchStatus::Switching
        }) {
            request.status = status;
            request.error_code = error_code;
            request.updated_at = updated_at;
        }
        Ok(())
    }
}
