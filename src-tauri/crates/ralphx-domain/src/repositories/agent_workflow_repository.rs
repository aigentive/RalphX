use async_trait::async_trait;
use chrono::{DateTime, Utc};

use crate::entities::{
    AgentWorkflowInvocation, AgentWorkflowLogEntry, AgentWorkflowPhase, AgentWorkflowProgress,
    AgentWorkflowRun, AgentWorkflowRunId, AgentWorkflowRunStatus, AgentWorkflowScript,
    AgentWorkflowScriptId, AgentWorkflowStepStatus,
};
use crate::error::AppResult;

#[async_trait]
pub trait AgentWorkflowRepository: Send + Sync {
    async fn save_script(&self, script: AgentWorkflowScript) -> AppResult<AgentWorkflowScript>;
    async fn get_script(
        &self,
        id: &AgentWorkflowScriptId,
    ) -> AppResult<Option<AgentWorkflowScript>>;
    async fn approve_script(
        &self,
        id: &AgentWorkflowScriptId,
        script_hash: &str,
        permission_hash: &str,
    ) -> AppResult<bool>;
    async fn create_run(&self, run: AgentWorkflowRun) -> AppResult<AgentWorkflowRun>;
    async fn get_run(&self, id: &AgentWorkflowRunId) -> AppResult<Option<AgentWorkflowRun>>;
    async fn get_latest_run_for_script(
        &self,
        script_id: &AgentWorkflowScriptId,
    ) -> AppResult<Option<AgentWorkflowRun>>;
    async fn get_progress(&self, id: &AgentWorkflowRunId) -> AppResult<AgentWorkflowProgress>;
    async fn claim_run(
        &self,
        id: &AgentWorkflowRunId,
        expected_attempt: u32,
        runner_instance_id: &str,
        lease_expires_at: DateTime<Utc>,
    ) -> AppResult<bool>;
    async fn heartbeat(
        &self,
        id: &AgentWorkflowRunId,
        attempt: u32,
        runner_instance_id: &str,
        lease_expires_at: DateTime<Utc>,
    ) -> AppResult<bool>;
    async fn transition_run(
        &self,
        id: &AgentWorkflowRunId,
        attempt: u32,
        runner_instance_id: &str,
        from: AgentWorkflowRunStatus,
        to: AgentWorkflowRunStatus,
        result_json: Option<String>,
        error: Option<String>,
    ) -> AppResult<bool>;
    async fn request_pause(&self, id: &AgentWorkflowRunId) -> AppResult<bool>;
    async fn resume_run(&self, id: &AgentWorkflowRunId) -> AppResult<bool>;
    async fn request_cancel(&self, id: &AgentWorkflowRunId) -> AppResult<bool>;
    async fn prepare_recovery(
        &self,
        id: &AgentWorkflowRunId,
        expected_attempt: u32,
        now: DateTime<Utc>,
    ) -> AppResult<bool>;
    async fn fail_unclaimed_run(
        &self,
        id: &AgentWorkflowRunId,
        expected_status: AgentWorkflowRunStatus,
        error: &str,
    ) -> AppResult<bool>;
    async fn begin_invocation(
        &self,
        invocation: AgentWorkflowInvocation,
    ) -> AppResult<AgentWorkflowInvocation>;
    async fn upsert_phase(
        &self,
        phase: AgentWorkflowPhase,
        attempt: u32,
        runner_instance_id: &str,
    ) -> AppResult<bool>;
    async fn settle_invocation(
        &self,
        invocation_id: &str,
        attempt: u32,
        runner_instance_id: &str,
        status: AgentWorkflowStepStatus,
        delegated_session_id: Option<String>,
        child_conversation_id: Option<String>,
        result_json: Option<String>,
        error: Option<String>,
    ) -> AppResult<bool>;
    async fn append_log(
        &self,
        run_id: &AgentWorkflowRunId,
        attempt: u32,
        runner_instance_id: &str,
        level: &str,
        message: &str,
    ) -> AppResult<Option<AgentWorkflowLogEntry>>;
    async fn list_recoverable(&self, now: DateTime<Utc>) -> AppResult<Vec<AgentWorkflowRun>>;
}
