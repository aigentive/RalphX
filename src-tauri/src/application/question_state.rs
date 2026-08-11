// Question state for handling inline AskUserQuestion from agents
// Used by the question bridge system to coordinate between MCP tools and frontend
// Mirrors the permission_state.rs pattern exactly

use chrono::Utc;
use serde_json::Value;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::{watch, Mutex};
use tracing::{error, info};

use crate::domain::entities::permission_request::is_within_permission_request_ttl;
use crate::domain::repositories::QuestionRepository;

// The question records themselves are domain data — repositories persist them and
// the UI answers them. Re-exported here so existing `application::question_state`
// importers keep resolving.
pub use crate::domain::entities::question_request::{
    PendingQuestionInfo, QuestionAnswer, QuestionOption,
};

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

/// An exclusive, non-waking reservation to resolve a question.
///
/// A claim is intentionally consumed by either [`QuestionState::commit_claim`]
/// or [`QuestionState::release_claim`], so callers cannot reuse it after the
/// durable decision has been made.
pub struct QuestionClaim {
    request_id: String,
    info: PendingQuestionInfo,
    has_live_waiter: bool,
}

impl QuestionClaim {
    /// Returns the immutable metadata captured by this exclusive claim.
    ///
    /// Command handlers must validate this exact record before committing the
    /// claim instead of issuing a second, non-reserved question lookup.
    pub fn pending_question(&self) -> &PendingQuestionInfo {
        &self.info
    }
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
    claims: Mutex<HashSet<String>>,
    repo: Option<Arc<dyn QuestionRepository>>,
}

impl QuestionState {
    pub fn new() -> Self {
        Self {
            pending: Mutex::new(HashMap::new()),
            claims: Mutex::new(HashSet::new()),
            repo: None,
        }
    }

    pub fn with_repo(repo: Arc<dyn QuestionRepository>) -> Self {
        Self {
            pending: Mutex::new(HashMap::new()),
            claims: Mutex::new(HashSet::new()),
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

        let request = PendingQuestion {
            info: info.clone(),
            sender: tx,
            created_at: Instant::now(),
        };
        self.pending
            .lock()
            .await
            .insert(request_id.clone(), request);

        // Publish the live waiter before awaiting persistence so a concurrent
        // resolver claims the live question instead of racing an absent record.
        if let Some(repo) = &self.repo {
            if let Err(e) = repo.create_pending(&info).await {
                error!("Failed to persist pending question {}: {}", request_id, e);
            }
        }
        rx
    }

    /// Claim a question exclusively without sending an answer to its live waiter.
    ///
    /// A durable lookup failure is returned to callers instead of being treated
    /// as an absent question, preventing a stale or failed read from advancing
    /// resolution.
    pub async fn claim_pending(
        &self,
        request_id: &str,
    ) -> crate::error::AppResult<Option<QuestionClaim>> {
        if !self.claims.lock().await.insert(request_id.to_string()) {
            return Ok(None);
        }

        let live_question = self
            .pending
            .lock()
            .await
            .get(request_id)
            .map(|question| question.info.clone());
        if let Some(info) = live_question {
            return Ok(Some(QuestionClaim {
                request_id: request_id.to_string(),
                info,
                has_live_waiter: true,
            }));
        }

        let durable_question = match &self.repo {
            Some(repo) => match repo.get_by_request_id(request_id).await {
                Ok(question) => question,
                Err(error) => {
                    self.claims.lock().await.remove(request_id);
                    return Err(error);
                }
            },
            None => None,
        };
        let Some(question) = durable_question else {
            self.claims.lock().await.remove(request_id);
            return Ok(None);
        };

        Ok(Some(QuestionClaim {
            request_id: request_id.to_string(),
            info: question,
            has_live_waiter: false,
        }))
    }

    /// Release a previously acquired claim without sending an answer.
    pub async fn release_claim(&self, claim: QuestionClaim) -> bool {
        self.claims.lock().await.remove(&claim.request_id)
    }

    /// Persist a claimed answer before notifying any live waiter.
    ///
    /// A durable write failure releases the claim and leaves the live question
    /// untouched, allowing a later retry instead of waking an agent with an
    /// answer that cannot be recovered after restart.
    pub async fn commit_claim(
        &self,
        claim: QuestionClaim,
        answer: QuestionAnswer,
    ) -> QuestionResolveResult {
        if let Some(repo) = &self.repo {
            match repo.resolve(&claim.request_id, &answer).await {
                Ok(true) => {}
                Ok(false) => {
                    self.release_claim(claim).await;
                    return QuestionResolveResult {
                        resolved: false,
                        session_id: None,
                        delivered_to_waiting_agent: false,
                    };
                }
                Err(error) => {
                    error!(
                        "Failed to durably resolve question {} before live delivery: {}",
                        claim.request_id, error
                    );
                    self.release_claim(claim).await;
                    return QuestionResolveResult {
                        resolved: false,
                        session_id: None,
                        delivered_to_waiting_agent: false,
                    };
                }
            }
        }

        let delivered_to_waiting_agent = if claim.has_live_waiter {
            let mut pending = self.pending.lock().await;
            if let Some(question) = pending.get(&claim.request_id) {
                let _ = question.sender.send(Some(answer));
                pending.remove(&claim.request_id);
                true
            } else {
                false
            }
        } else {
            false
        };
        let session_id = claim.info.session_id.clone();
        self.release_claim(claim).await;

        QuestionResolveResult {
            resolved: true,
            session_id: Some(session_id),
            delivered_to_waiting_agent,
        }
    }

    /// Resolve a pending question with an answer.
    ///
    /// The claim/commit protocol excludes concurrent resolvers and commits the
    /// durable answer before the live watch channel is notified.
    pub async fn resolve(&self, request_id: &str, answer: QuestionAnswer) -> QuestionResolveResult {
        match self.claim_pending(request_id).await {
            Ok(Some(claim)) => self.commit_claim(claim, answer).await,
            Ok(None) => QuestionResolveResult {
                resolved: false,
                session_id: None,
                delivered_to_waiting_agent: false,
            },
            Err(error) => {
                error!(
                    "Failed to load question {} before durable resolution: {}",
                    request_id, error
                );
                QuestionResolveResult {
                    resolved: false,
                    session_id: None,
                    delivered_to_waiting_agent: false,
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
