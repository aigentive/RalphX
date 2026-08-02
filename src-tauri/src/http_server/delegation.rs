use chrono::Utc;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{watch, RwLock};

use crate::application::delegation_park::{DelegationJobSource, ParkJobSnapshot};
use crate::domain::agents::AgentHarnessKind;
use crate::domain::entities::{
    AgentRunId, ChatConversationId, ChatTimelineItem, ChatTimelineItemId, ChatTimelineItemKind,
    ChatTimelineItemStatus, MessageRole,
};
use crate::domain::repositories::ChatTimelineRepository;
use crate::error::AppResult;
use crate::http_server::types::{DelegatedRunSummary, DelegatedSessionStatusResponse};

#[derive(Debug, Clone, serde::Serialize)]
pub struct DelegationHistoryEntry {
    pub status: String,
    pub timestamp: String,
    pub detail: Option<String>,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct DelegationAssignmentSummary {
    pub task_number: i64,
    pub title: String,
    pub task_state: String,
    pub assignment_state: String,
    pub delegate_agent_name: String,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct DelegationJobSnapshot {
    pub job_id: String,
    pub parent_context_type: String,
    pub parent_context_id: String,
    pub parent_turn_id: Option<String>,
    pub parent_message_id: Option<String>,
    pub parent_conversation_id: Option<String>,
    pub parent_agent_run_id: Option<String>,
    pub parent_tool_use_id: Option<String>,
    pub delegated_session_id: String,
    pub delegated_conversation_id: Option<String>,
    pub delegated_agent_run_id: Option<String>,
    pub agent_name: String,
    pub assignment: Option<DelegationAssignmentSummary>,
    pub harness: String,
    pub provider_session_id: Option<String>,
    pub upstream_provider: Option<String>,
    pub provider_profile: Option<String>,
    pub logical_model: Option<String>,
    pub effective_model_id: Option<String>,
    pub logical_effort: Option<String>,
    pub effective_effort: Option<String>,
    pub approval_policy: Option<String>,
    pub sandbox_mode: Option<String>,
    pub status: String,
    pub content: Option<String>,
    pub error: Option<String>,
    pub started_at: String,
    pub completed_at: Option<String>,
    pub history: Vec<DelegationHistoryEntry>,
    pub delegated_status: Option<DelegatedSessionStatusResponse>,
    /// Set only when a bounded `delegate_wait` block hit its deadline without any
    /// watched job settling. Absent on every immediate-return path.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timed_out: Option<bool>,
}

#[derive(Debug, Clone)]
struct DelegationJobRecord {
    snapshot: DelegationJobSnapshot,
    cancel_requested: bool,
    settlement_status: Option<String>,
    /// Broadcasts the committed terminal status exactly once, after `commit_terminal`
    /// accepts the CAS. `None` while the job is still running.
    settled_tx: Arc<watch::Sender<Option<String>>>,
}

#[derive(Clone, Default)]
pub struct DelegationService {
    jobs: Arc<RwLock<HashMap<String, DelegationJobRecord>>>,
}

impl DelegationService {
    pub fn new() -> Self {
        Self::default()
    }

    pub async fn register_running(
        &self,
        job_id: String,
        parent_context_type: String,
        parent_context_id: String,
        parent_turn_id: Option<String>,
        parent_message_id: Option<String>,
        parent_conversation_id: Option<String>,
        parent_agent_run_id: Option<String>,
        parent_tool_use_id: Option<String>,
        delegated_session_id: String,
        delegated_conversation_id: Option<String>,
        delegated_agent_run_id: Option<String>,
        agent_name: String,
        assignment: Option<DelegationAssignmentSummary>,
        harness: impl Into<String>,
        provider_session_id: Option<String>,
        upstream_provider: Option<String>,
        provider_profile: Option<String>,
        logical_model: Option<String>,
        effective_model_id: Option<String>,
        logical_effort: Option<String>,
        effective_effort: Option<String>,
        approval_policy: Option<String>,
        sandbox_mode: Option<String>,
    ) -> DelegationJobSnapshot {
        let started_at = Utc::now().to_rfc3339();
        let snapshot = DelegationJobSnapshot {
            job_id: job_id.clone(),
            parent_context_type,
            parent_context_id,
            parent_turn_id,
            parent_message_id,
            parent_conversation_id,
            parent_agent_run_id,
            parent_tool_use_id,
            delegated_session_id,
            delegated_conversation_id,
            delegated_agent_run_id,
            agent_name,
            assignment,
            harness: harness.into(),
            provider_session_id,
            upstream_provider,
            provider_profile,
            logical_model,
            effective_model_id,
            logical_effort,
            effective_effort,
            approval_policy,
            sandbox_mode,
            status: "running".to_string(),
            content: None,
            error: None,
            started_at: started_at.clone(),
            completed_at: None,
            history: vec![DelegationHistoryEntry {
                status: "running".to_string(),
                timestamp: started_at,
                detail: None,
            }],
            delegated_status: None,
            timed_out: None,
        };

        let (settled_tx, _settled_rx) = watch::channel(None);
        self.jobs.write().await.insert(
            job_id,
            DelegationJobRecord {
                snapshot: snapshot.clone(),
                cancel_requested: false,
                settlement_status: None,
                settled_tx: Arc::new(settled_tx),
            },
        );

        snapshot
    }

    /// Subscribes to the terminal settlement signal for `job_id`.
    ///
    /// The receiver observes `Some(status)` only after `commit_terminal` accepts the
    /// terminal CAS, so a signal is proof of committed settlement, never of a
    /// speculative `terminal_candidate`. Returns `None` for unknown jobs.
    pub async fn subscribe_settlement(
        &self,
        job_id: &str,
    ) -> Option<watch::Receiver<Option<String>>> {
        self.jobs
            .read()
            .await
            .get(job_id)
            .map(|record| record.settled_tx.subscribe())
    }

    pub async fn snapshot(&self, job_id: &str) -> Option<DelegationJobSnapshot> {
        self.jobs
            .read()
            .await
            .get(job_id)
            .map(|record| record.snapshot.clone())
    }

    #[doc(hidden)]
    pub async fn job_count_for_test(&self) -> usize {
        self.jobs.read().await.len()
    }

    pub async fn begin_cancellation(&self, job_id: &str) -> Option<DelegationJobSnapshot> {
        let mut jobs = self.jobs.write().await;
        let record = jobs.get_mut(job_id)?;
        if record.snapshot.status != "running"
            || record.cancel_requested
            || record.settlement_status.is_some()
        {
            return None;
        }
        record.cancel_requested = true;
        Some(record.snapshot.clone())
    }

    pub async fn is_cancellation_pending(&self, job_id: &str) -> bool {
        self.jobs
            .read()
            .await
            .get(job_id)
            .is_some_and(|record| record.cancel_requested)
    }

    pub async fn abort_cancellation(&self, job_id: &str) {
        let mut jobs = self.jobs.write().await;
        if let Some(record) = jobs.get_mut(job_id) {
            if record.snapshot.status == "running" {
                record.cancel_requested = false;
            }
        }
    }

    pub async fn terminal_candidate(
        &self,
        job_id: &str,
        status: &str,
        content: Option<String>,
        error: Option<String>,
    ) -> Option<DelegationJobSnapshot> {
        if !matches!(status, "completed" | "failed" | "cancelled") {
            return None;
        }
        let mut jobs = self.jobs.write().await;
        let record = jobs.get_mut(job_id)?;
        if record.snapshot.status != "running" || (record.cancel_requested && status != "cancelled")
        {
            return None;
        }
        match record.settlement_status.as_deref() {
            Some(claimed_status) if claimed_status != status => return None,
            Some(_) => {}
            None => record.settlement_status = Some(status.to_string()),
        }
        let mut candidate = record.snapshot.clone();
        candidate.status = status.to_string();
        candidate.content = if status == "completed" { content } else { None };
        candidate.error = if status == "failed" {
            error.clone()
        } else {
            None
        };
        let completed_at = Utc::now().to_rfc3339();
        candidate.completed_at = Some(completed_at.clone());
        candidate.history.push(DelegationHistoryEntry {
            status: status.to_string(),
            timestamp: completed_at,
            detail: if status == "failed" { error } else { None },
        });
        Some(candidate)
    }

    pub async fn commit_terminal(&self, candidate: DelegationJobSnapshot) -> bool {
        let mut jobs = self.jobs.write().await;
        let Some(record) = jobs.get_mut(&candidate.job_id) else {
            return false;
        };
        if record.snapshot.status != "running" {
            return false;
        }
        if record.snapshot.delegated_agent_run_id != candidate.delegated_agent_run_id
            || (record.cancel_requested && candidate.status != "cancelled")
            || record.settlement_status.as_deref() != Some(candidate.status.as_str())
            || !matches!(
                candidate.status.as_str(),
                "completed" | "failed" | "cancelled"
            )
        {
            return false;
        }
        let settled_status = candidate.status.clone();
        record.snapshot = candidate;
        record.cancel_requested = false;
        record.settlement_status = None;
        // Effects strictly after authority: the wake signal fires only once the CAS above
        // has accepted this terminal, never from `terminal_candidate`.
        let _ = record.settled_tx.send(Some(settled_status));
        true
    }
}

/// The live job registry is the park's source of delegate ownership and durable-run facts.
#[async_trait::async_trait]
impl DelegationJobSource for DelegationService {
    async fn park_job_snapshot(&self, job_id: &str) -> Option<ParkJobSnapshot> {
        self.snapshot(job_id).await.map(|snapshot| ParkJobSnapshot {
            status: snapshot.status,
            parent_conversation_id: snapshot.parent_conversation_id,
            parent_agent_run_id: snapshot.parent_agent_run_id,
            delegated_session_id: snapshot.delegated_session_id,
            delegated_agent_run_id: snapshot.delegated_agent_run_id,
        })
    }
}

pub async fn persist_terminal_projection(
    repo: &Arc<dyn ChatTimelineRepository>,
    snapshot: &DelegationJobSnapshot,
    latest_run: Option<&DelegatedRunSummary>,
) -> AppResult<()> {
    let Some(parent_conversation_id) = snapshot.parent_conversation_id.as_deref() else {
        return Ok(());
    };

    let run_id = latest_run
        .map(|run| run.agent_run_id.clone())
        .or_else(|| snapshot.delegated_agent_run_id.clone());
    let provider_harness = latest_run
        .and_then(|run| run.harness.as_deref())
        .unwrap_or(snapshot.harness.as_str())
        .parse::<AgentHarnessKind>()
        .ok();
    let provider_session_id = latest_run
        .and_then(|run| run.provider_session_id.clone())
        .or_else(|| snapshot.provider_session_id.clone());
    let result = serde_json::json!({
        "job_id": snapshot.job_id,
        "status": snapshot.status,
        "content": snapshot.content,
        "error": snapshot.error,
        "delegated_session_id": snapshot.delegated_session_id,
        "delegated_conversation_id": snapshot.delegated_conversation_id,
        "delegated_agent_run_id": run_id.clone(),
        "harness": provider_harness.map(|value| value.to_string()).unwrap_or_else(|| snapshot.harness.clone()),
        "provider_session_id": provider_session_id.clone(),
        "upstream_provider": latest_run.and_then(|run| run.upstream_provider.clone()).or_else(|| snapshot.upstream_provider.clone()),
        "provider_profile": latest_run.and_then(|run| run.provider_profile.clone()).or_else(|| snapshot.provider_profile.clone()),
        "logical_model": latest_run.and_then(|run| run.logical_model.clone()).or_else(|| snapshot.logical_model.clone()),
        "effective_model_id": latest_run.and_then(|run| run.effective_model_id.clone()).or_else(|| snapshot.effective_model_id.clone()),
        "logical_effort": latest_run.and_then(|run| run.logical_effort.clone()).or_else(|| snapshot.logical_effort.clone()),
        "effective_effort": latest_run.and_then(|run| run.effective_effort.clone()).or_else(|| snapshot.effective_effort.clone()),
        "approval_policy": latest_run.and_then(|run| run.approval_policy.clone()).or_else(|| snapshot.approval_policy.clone()),
        "sandbox_mode": latest_run.and_then(|run| run.sandbox_mode.clone()).or_else(|| snapshot.sandbox_mode.clone()),
        "input_tokens": latest_run.and_then(|run| run.input_tokens),
        "output_tokens": latest_run.and_then(|run| run.output_tokens),
        "cache_creation_tokens": latest_run.and_then(|run| run.cache_creation_tokens),
        "cache_read_tokens": latest_run.and_then(|run| run.cache_read_tokens),
        "estimated_usd": latest_run.and_then(|run| run.estimated_usd),
        "started_at": snapshot.started_at,
        "completed_at": snapshot.completed_at,
    });
    let result_json = result.to_string();
    let now = Utc::now();
    let item_id = format!("delegation-terminal:{}", snapshot.job_id);
    let item = ChatTimelineItem {
        id: ChatTimelineItemId::from_string(item_id.clone()),
        conversation_id: ChatConversationId::from_string(parent_conversation_id),
        message_id: None,
        run_id: run_id.map(AgentRunId::from_string),
        sequence: 0,
        block_index: 0,
        role: MessageRole::Orchestrator,
        kind: ChatTimelineItemKind::ToolUse,
        status: ChatTimelineItemStatus::Finalized,
        text: None,
        tool_call_id: Some(item_id),
        tool_name: Some("delegate_terminal".to_string()),
        tool_status: Some(snapshot.status.clone()),
        tool_input_preview: Some(serde_json::json!({ "job_id": snapshot.job_id }).to_string()),
        tool_result_preview: Some(result_json.chars().take(1_000).collect()),
        input_json: Some(serde_json::json!({ "job_id": snapshot.job_id }).to_string()),
        result_json: Some(result_json),
        raw_block_json: None,
        metadata: Some(
            serde_json::json!({
                "synthetic_delegation_lifecycle": true,
                "delegated_job_id": snapshot.job_id,
            })
            .to_string(),
        ),
        provider_harness,
        provider_session_id,
        created_at: snapshot.started_at.parse().unwrap_or(now),
        updated_at: now,
        finalized_at: Some(now),
    };
    let mut attempt = 0_u8;
    loop {
        attempt += 1;
        match repo.upsert_item(item.clone()).await {
            Ok(_) => return Ok(()),
            Err(error) if attempt < 3 => {
                tracing::warn!(
                    job_id = snapshot.job_id,
                    attempt,
                    %error,
                    "Retrying delegated terminal timeline projection"
                );
                tokio::time::sleep(std::time::Duration::from_millis(25)).await;
            }
            Err(error) => return Err(error),
        }
    }
}
