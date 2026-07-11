// Question state for handling inline AskUserQuestion from agents
// Used by the question bridge system to coordinate between MCP tools and frontend
// Mirrors the permission_state.rs pattern exactly

use chrono::Utc;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::{watch, Mutex};
use tracing::{error, info};

use crate::application::permission_state::is_within_permission_request_ttl;
use crate::domain::repositories::QuestionRepository;

/// Answer provided by the user in the UI
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QuestionAnswer {
    pub selected_options: Vec<String>,
    pub text: Option<String>,
    #[serde(default)]
    pub skipped: bool,
}

/// Metadata for a pending question
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PendingQuestionInfo {
    pub request_id: String,
    pub session_id: String,
    pub question: String,
    pub header: Option<String>,
    pub options: Vec<QuestionOption>,
    pub multi_select: bool,
    #[serde(default = "default_allow_skip")]
    pub allow_skip: bool,
    pub batch_index: Option<u32>,
    pub batch_total: Option<u32>,
    #[serde(default)]
    pub metadata: Option<Value>,
    #[serde(default = "default_created_at")]
    pub created_at: String,
}

fn default_allow_skip() -> bool {
    true
}

fn default_created_at() -> String {
    Utc::now().to_rfc3339()
}

/// A single option in a question
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QuestionOption {
    pub value: String,
    pub label: String,
    pub description: Option<String>,
}

/// A pending question with its signaling channel
pub struct PendingQuestion {
    pub info: PendingQuestionInfo,
    pub sender: watch::Sender<Option<QuestionAnswer>>,
    pub created_at: Instant,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QuestionResolveResult {
    pub resolved: bool,
    pub session_id: Option<String>,
    pub delivered_to_waiting_agent: bool,
}

/// Shared state for managing pending questions from agents
///
/// Uses tokio::sync::watch channels to allow long-polling:
/// - MCP server registers a question and waits on a receiver
/// - Frontend resolves the question by sending through the channel
///
/// Optionally backed by a repository for persistence (SQLite).
/// Repo calls are fire-and-forget: errors are logged but never block channel ops.
pub struct QuestionState {
    pub pending: Mutex<HashMap<String, PendingQuestion>>,
    repo: Option<Arc<dyn QuestionRepository>>,
}

impl QuestionState {
    pub fn new() -> Self {
        Self {
            pending: Mutex::new(HashMap::new()),
            repo: None,
        }
    }

    pub fn with_repo(repo: Arc<dyn QuestionRepository>) -> Self {
        Self {
            pending: Mutex::new(HashMap::new()),
            repo: Some(repo),
        }
    }

    /// Get info about all pending questions
    pub async fn get_pending_info(&self) -> Vec<PendingQuestionInfo> {
        if let Some(repo) = &self.repo {
            match repo.get_pending().await {
                Ok(mut pending) => {
                    let durable_request_ids: HashSet<_> = pending
                        .iter()
                        .map(|question| question.request_id.clone())
                        .collect();
                    let live_pending = self.pending.lock().await;
                    pending.extend(
                        live_pending
                            .values()
                            .filter(|question| {
                                !durable_request_ids.contains(&question.info.request_id)
                            })
                            .map(|question| question.info.clone()),
                    );
                    return pending;
                }
                Err(e) => {
                    error!("Failed to load pending questions from repo: {}", e);
                }
            }
        }

        let pending = self.pending.lock().await;
        pending.values().map(|p| p.info.clone()).collect()
    }

    /// Load pending questions without turning a durable repository failure into an empty result.
    pub async fn get_pending_info_strict(
        &self,
    ) -> crate::error::AppResult<Vec<PendingQuestionInfo>> {
        if let Some(repo) = &self.repo {
            let mut pending: Vec<_> = repo
                .get_pending()
                .await?
                .into_iter()
                .filter(|question| is_within_permission_request_ttl(&question.created_at))
                .collect();
            let durable_request_ids: HashSet<_> = pending
                .iter()
                .map(|question| question.request_id.clone())
                .collect();
            let live_pending = self.pending.lock().await;
            pending.extend(
                live_pending
                    .values()
                    .filter(|question| {
                        is_within_permission_request_ttl(&question.info.created_at)
                            && !durable_request_ids.contains(&question.info.request_id)
                    })
                    .map(|question| question.info.clone()),
            );
            return Ok(pending);
        }
        Ok(self
            .pending
            .lock()
            .await
            .values()
            .filter(|question| is_within_permission_request_ttl(&question.info.created_at))
            .map(|question| question.info.clone())
            .collect())
    }

    /// Register a new pending question
    pub async fn register(
        &self,
        request_id: String,
        session_id: String,
        question: String,
        header: Option<String>,
        options: Vec<QuestionOption>,
        multi_select: bool,
    ) -> watch::Receiver<Option<QuestionAnswer>> {
        self.register_with_metadata(
            request_id,
            session_id,
            question,
            header,
            options,
            multi_select,
            true,
            None,
            None,
            None,
        )
        .await
    }

    /// Register a new pending question with UI metadata.
    pub async fn register_with_metadata(
        &self,
        request_id: String,
        session_id: String,
        question: String,
        header: Option<String>,
        options: Vec<QuestionOption>,
        multi_select: bool,
        allow_skip: bool,
        batch_index: Option<u32>,
        batch_total: Option<u32>,
        metadata: Option<Value>,
    ) -> watch::Receiver<Option<QuestionAnswer>> {
        let (tx, rx) = watch::channel(None);
        let info = PendingQuestionInfo {
            request_id: request_id.clone(),
            session_id,
            question,
            header,
            options,
            multi_select,
            allow_skip,
            batch_index,
            batch_total,
            metadata,
            created_at: Utc::now().to_rfc3339(),
        };

        // Fire-and-forget persist to repo
        if let Some(repo) = &self.repo {
            if let Err(e) = repo.create_pending(&info).await {
                error!("Failed to persist pending question {}: {}", request_id, e);
            }
        }

        let request = PendingQuestion {
            info,
            sender: tx,
            created_at: Instant::now(),
        };
        self.pending.lock().await.insert(request_id, request);
        rx
    }

    /// Resolve a pending question with an answer.
    ///
    /// Returns a result indicating whether the answer was delivered to a live
    /// waiter or only persisted for a later conversation resume.
    ///
    /// Phase 1 (lock held): send answer via watch channel, then remove from HashMap.
    /// Phase 2 (lock free): persist resolution to repo.
    ///
    /// IMPORTANT: send() happens BEFORE HashMap::remove() so any subscriber that
    /// holds a Receiver sees the value change before the Sender is dropped.
    /// HashMap removal is unconditional — if repo.resolve() fails, the entry stays
    /// removed (no re-insert) to avoid inconsistent in-memory state.
    pub async fn resolve(&self, request_id: &str, answer: QuestionAnswer) -> QuestionResolveResult {
        // Phase 1: lock held — signal channel and remove from HashMap atomically
        let session_id = {
            let mut pending = self.pending.lock().await;
            if let Some(question) = pending.get(request_id) {
                let session_id = question.info.session_id.clone();
                // send() BEFORE remove() so Receiver sees the value before Sender drops
                let _ = question.sender.send(Some(answer.clone()));
                pending.remove(request_id);
                Some(session_id)
            } else {
                None
            }
        };

        if let Some(ref sid) = session_id {
            // Phase 2: lock free — persist to repo (best-effort)
            if let Some(repo) = &self.repo {
                if let Err(e) = repo.resolve(request_id, &answer).await {
                    error!(
                        "Failed to persist question resolution {}: {}",
                        request_id, e
                    );
                }
            }
            QuestionResolveResult {
                resolved: true,
                session_id: Some(sid.clone()),
                delivered_to_waiting_agent: true,
            }
        } else {
            let Some(repo) = &self.repo else {
                return QuestionResolveResult {
                    resolved: false,
                    session_id: None,
                    delivered_to_waiting_agent: false,
                };
            };

            let question_info = match repo.get_by_request_id(request_id).await {
                Ok(info) => info,
                Err(e) => {
                    error!(
                        "Failed to load question {} before durable resolution: {}",
                        request_id, e
                    );
                    None
                }
            };
            let Some(question_info) = question_info else {
                return QuestionResolveResult {
                    resolved: false,
                    session_id: None,
                    delivered_to_waiting_agent: false,
                };
            };

            match repo.resolve(request_id, &answer).await {
                Ok(true) => QuestionResolveResult {
                    resolved: true,
                    session_id: Some(question_info.session_id),
                    delivered_to_waiting_agent: false,
                },
                Ok(false) => QuestionResolveResult {
                    resolved: false,
                    session_id: None,
                    delivered_to_waiting_agent: false,
                },
                Err(e) => {
                    error!(
                        "Failed to persist durable question resolution {}: {}",
                        request_id, e
                    );
                    QuestionResolveResult {
                        resolved: false,
                        session_id: None,
                        delivered_to_waiting_agent: false,
                    }
                }
            }
        }
    }

    /// Expire a pending question due to timeout.
    ///
    /// Returns the removed question metadata when the request_id existed in the
    /// in-memory map. Repo persistence is best-effort and marks the question as
    /// wait-expired instead of deleting audit history, so the UI can keep
    /// rendering the original question and accept a late answer.
    pub async fn expire(&self, request_id: &str) -> Option<PendingQuestionInfo> {
        let info = self
            .pending
            .lock()
            .await
            .remove(request_id)
            .map(|question| question.info);

        if info.is_some() {
            if let Some(repo) = &self.repo {
                if let Err(e) = repo.expire_by_request_id(request_id).await {
                    error!("Failed to persist question expiry {}: {}", request_id, e);
                }
            }
        }

        info
    }

    /// Get the answer for a resolved question from the repository.
    ///
    /// Returns `Ok(None)` when there is no repo (test mode without persistence).
    pub async fn get_resolved_answer(
        &self,
        request_id: &str,
    ) -> crate::error::AppResult<Option<QuestionAnswer>> {
        match &self.repo {
            Some(repo) => repo.get_resolved_answer(request_id).await,
            None => Ok(None),
        }
    }

    /// Remove a pending question
    pub async fn remove(&self, request_id: &str) -> bool {
        let removed = self.pending.lock().await.remove(request_id).is_some();

        // Fire-and-forget persist to repo
        if removed {
            if let Some(repo) = &self.repo {
                if let Err(e) = repo.remove(request_id).await {
                    error!("Failed to persist question removal {}: {}", request_id, e);
                }
            }
        }

        removed
    }

    /// Expire all stale pending questions in the repository on startup.
    /// Call this once after constructing with `with_repo()` to clean up
    /// questions from agents that are no longer running.
    pub async fn expire_stale_on_startup(&self) {
        if let Some(repo) = &self.repo {
            match repo.expire_all_pending().await {
                Ok(count) if count > 0 => {
                    info!(
                        "Marked {} stale pending questions as wait-expired on startup",
                        count
                    );
                }
                Ok(_) => {}
                Err(e) => {
                    error!("Failed to mark stale pending questions wait-expired: {}", e);
                }
            }
        }
    }

    /// Sweep stale in-memory pending questions and expire them in the repository.
    /// Call periodically (e.g., every 60 seconds) to clean up questions from agents
    /// that died without resolving them.
    pub async fn sweep_stale(&self, max_age: Duration) {
        let stale_ids: Vec<String> = {
            let pending = self.pending.lock().await;
            pending
                .iter()
                .filter(|(_, q)| q.created_at.elapsed() > max_age)
                .map(|(id, _)| id.clone())
                .collect()
        };

        if stale_ids.is_empty() {
            return;
        }

        info!(count = stale_ids.len(), "Sweeping stale pending questions");

        let mut pending = self.pending.lock().await;
        for request_id in &stale_ids {
            pending.remove(request_id);
            if let Some(repo) = &self.repo {
                if let Err(e) = repo.expire_by_request_id(request_id).await {
                    error!(
                        "Failed to expire stale question {} in repo: {}",
                        request_id, e
                    );
                }
            }
        }
    }

    /// Check if there's a pending question for the given session_id
    /// Used to suppress stream monitor timeout kills while agent is waiting for user input
    pub async fn has_pending_for_session(&self, session_id: &str) -> bool {
        let pending = self.pending.lock().await;
        pending.values().any(|q| q.info.session_id == session_id)
    }
}

impl Default for QuestionState {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
#[path = "question_state_tests.rs"]
mod tests;
