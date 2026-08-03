//! Application-owned port for reading live delegate jobs when arming a park.
//!
//! The delegate job registry lives in the transport layer, but the park service only needs the
//! ownership and durability facts required to validate an arm request. Keeping that contract here
//! lets the application layer stay free of transport imports.

use async_trait::async_trait;

/// Minimal delegate-job view the park needs to validate ownership before arming.
#[derive(Debug, Clone)]
pub struct ParkJobSnapshot {
    pub status: String,
    pub parent_conversation_id: Option<String>,
    pub parent_agent_run_id: Option<String>,
    pub delegated_session_id: String,
    pub delegated_agent_run_id: Option<String>,
}

/// Read-only source of live delegate jobs, implemented by the layer that owns the job registry.
#[async_trait]
pub trait DelegationJobSource: Send + Sync {
    /// Return the park-relevant view of one delegate job, or `None` when the job is unknown.
    async fn park_job_snapshot(&self, job_id: &str) -> Option<ParkJobSnapshot>;
}
