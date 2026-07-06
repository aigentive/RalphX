// Permission state for handling UI-based permission approvals
// Used by the permission bridge system to coordinate between MCP tools and frontend

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{watch, Mutex};
use tracing::{error, info};

use crate::domain::repositories::PermissionRepository;
use crate::error::AppResult;

/// Permission decision made by the user in the UI
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PermissionDecision {
    pub decision: String, // "allow" or "deny"
    pub message: Option<String>,
}

/// Metadata for a pending permission request
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PendingPermissionInfo {
    pub request_id: String,
    pub tool_name: String,
    pub tool_input: serde_json::Value,
    pub context: Option<String>,
    // Agent identity fields (optional for backward compat)
    pub agent_type: Option<String>,
    pub task_id: Option<String>,
    pub context_type: Option<String>,
    pub context_id: Option<String>,
}

/// A pending permission request with its signaling channel
pub struct PendingPermissionRequest {
    pub info: PendingPermissionInfo,
    pub sender: watch::Sender<Option<PermissionDecision>>,
}

/// Shared state for managing pending permission requests
///
/// Uses tokio::sync::watch channels to allow long-polling:
/// - MCP server registers a request and waits on a receiver
/// - Frontend resolves the request by sending through the channel
///
/// Optionally backed by a repository for persistence (SQLite).
/// Repo calls are fire-and-forget: errors are logged but never block channel ops.
pub struct PermissionState {
    /// Map of request_id -> PendingPermissionRequest
    /// The PendingPermissionRequest contains both metadata and the signaling channel
    pub pending: Mutex<HashMap<String, PendingPermissionRequest>>,
    repo: Option<Arc<dyn PermissionRepository>>,
}

impl PermissionState {
    pub fn new() -> Self {
        Self {
            pending: Mutex::new(HashMap::new()),
            repo: None,
        }
    }

    pub fn with_repo(repo: Arc<dyn PermissionRepository>) -> Self {
        Self {
            pending: Mutex::new(HashMap::new()),
            repo: Some(repo),
        }
    }

    /// Get info about all pending permission requests
    pub async fn get_pending_info(&self) -> Vec<PendingPermissionInfo> {
        let pending = self.pending.lock().await;
        pending.values().map(|p| p.info.clone()).collect()
    }

    /// Log a repo operation error without blocking the channel signaling path.
    fn log_repo_err<T>(result: AppResult<T>, request_id: &str, context: &str) {
        if let Err(e) = result {
            error!("Failed to persist permission {} {}: {}", context, request_id, e);
        }
    }

    /// Register a new pending permission request
    pub async fn register(&self, info: PendingPermissionInfo) {
        let (tx, _rx) = watch::channel(None);
        let request_id = info.request_id.clone();

        // Fire-and-forget persist to repo
        if let Some(repo) = &self.repo {
            Self::log_repo_err(repo.create_pending(&info).await, &request_id, "pending");
        }

        let request = PendingPermissionRequest { info, sender: tx };
        self.pending.lock().await.insert(request_id, request);
    }

    /// Resolve a pending permission request with a decision
    /// Returns true if the request was found and resolved
    pub async fn resolve(&self, request_id: &str, decision: PermissionDecision) -> bool {
        let pending = self.pending.lock().await;
        if let Some(request) = pending.get(request_id) {
            let _ = request.sender.send(Some(decision.clone()));

            // Fire-and-forget persist to repo
            if let Some(repo) = &self.repo {
                Self::log_repo_err(repo.resolve(request_id, &decision).await, request_id, "resolution");
            }

            true
        } else {
            false
        }
    }

    /// Remove a pending permission request
    pub async fn remove(&self, request_id: &str) -> bool {
        let removed = self.pending.lock().await.remove(request_id).is_some();

        // Fire-and-forget persist to repo
        if removed {
            if let Some(repo) = &self.repo {
                Self::log_repo_err(repo.remove(request_id).await, request_id, "removal");
            }
        }

        removed
    }

    /// Expire all stale pending permissions in the repository on startup.
    /// Call this once after constructing with `with_repo()` to clean up
    /// permissions from agents that are no longer running.
    pub async fn expire_stale_on_startup(&self) {
        if let Some(repo) = &self.repo {
            match repo.expire_all_pending().await {
                Ok(count) if count > 0 => {
                    info!("Expired {} stale pending permissions on startup", count);
                }
                Ok(_) => {}
                Err(e) => {
                    error!("Failed to expire stale pending permissions: {}", e);
                }
            }
        }
    }
}

impl Default for PermissionState {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
#[path = "permission_state_tests.rs"]
mod tests;
