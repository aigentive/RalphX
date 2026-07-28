use async_trait::async_trait;

use crate::domain::entities::{
    AgentRunId, AgentTaskAssignmentId, AgentTaskAssignmentReservation,
    AgentTaskAssignmentSettlement, AgentTaskAssignmentTerminalStatus, AgentTaskAssignmentView,
    AgentTaskCreate, AgentTaskDetail, AgentTaskListId, AgentTaskListSummary,
    AgentTaskMutationResult, AgentTaskPatch, AgentTaskScope, AgentTaskSummary, DelegatedSessionId,
};
use crate::error::AppResult;

#[derive(Debug, Clone, Copy, Default)]
pub struct AgentTaskListOptions {
    pub include_done: bool,
}

#[async_trait]
pub trait AgentTaskRepository: Send + Sync {
    async fn create_task(
        &self,
        scope: &AgentTaskScope,
        input: AgentTaskCreate,
    ) -> AppResult<AgentTaskMutationResult>;

    async fn get_task(
        &self,
        scope: &AgentTaskScope,
        task_ref: &str,
    ) -> AppResult<Option<AgentTaskDetail>>;

    async fn list_tasks(
        &self,
        scope: &AgentTaskScope,
        options: AgentTaskListOptions,
    ) -> AppResult<Vec<AgentTaskSummary>>;

    async fn list_task_lists(&self, scope: &AgentTaskScope)
        -> AppResult<Vec<AgentTaskListSummary>>;

    async fn list_tasks_for_list(
        &self,
        scope: &AgentTaskScope,
        list_id: &AgentTaskListId,
        options: AgentTaskListOptions,
    ) -> AppResult<Vec<AgentTaskSummary>>;

    async fn update_task(
        &self,
        scope: &AgentTaskScope,
        task_ref: &str,
        patch: AgentTaskPatch,
    ) -> AppResult<Option<AgentTaskMutationResult>>;

    async fn reserve_assignment(
        &self,
        scope: &AgentTaskScope,
        task_ref: &str,
        delegated_session_id: &DelegatedSessionId,
        caller_agent_run_id: &AgentRunId,
        delegate_agent_name: &str,
    ) -> AppResult<Option<AgentTaskAssignmentReservation>>;

    async fn plan_assignment_run(
        &self,
        assignment_id: &AgentTaskAssignmentId,
        delegated_session_id: &DelegatedSessionId,
        delegated_agent_run_id: &AgentRunId,
    ) -> AppResult<Option<AgentTaskAssignmentView>>;

    /// Attaches durable Team member authority to the exact reserved assignment.
    /// Legacy delegate assignments keep all Team fields null.
    async fn set_assignment_team_identity(
        &self,
        assignment_id: &AgentTaskAssignmentId,
        delegated_session_id: &DelegatedSessionId,
        team_id: &crate::entities::TeamSessionId,
        team_member_id: &crate::entities::TeamMemberId,
        team_member_generation: i64,
    ) -> AppResult<Option<AgentTaskAssignmentView>>;

    async fn bind_assignment_run(
        &self,
        assignment_id: &AgentTaskAssignmentId,
        delegated_session_id: &DelegatedSessionId,
        delegated_agent_run_id: &AgentRunId,
    ) -> AppResult<Option<AgentTaskAssignmentView>>;

    async fn get_unresolved_assignment(
        &self,
        delegated_session_id: &DelegatedSessionId,
    ) -> AppResult<Option<AgentTaskAssignmentView>>;

    async fn request_assignment_completion(
        &self,
        delegated_session_id: &DelegatedSessionId,
        delegated_agent_run_id: &AgentRunId,
        local_scope: &AgentTaskScope,
        completion_metadata: Option<serde_json::Value>,
    ) -> AppResult<Option<AgentTaskAssignmentView>>;

    async fn request_assignment_release(
        &self,
        delegated_session_id: &DelegatedSessionId,
        delegated_agent_run_id: &AgentRunId,
        reason: &str,
    ) -> AppResult<Option<AgentTaskAssignmentView>>;

    async fn settle_assignment_for_run(
        &self,
        delegated_agent_run_id: &AgentRunId,
        terminal_status: AgentTaskAssignmentTerminalStatus,
        reason: Option<&str>,
    ) -> AppResult<Option<AgentTaskAssignmentSettlement>>;

    async fn get_assignment_for_run(
        &self,
        delegated_agent_run_id: &AgentRunId,
    ) -> AppResult<Option<AgentTaskAssignmentView>>;

    async fn fail_reserved_assignment(
        &self,
        delegated_session_id: &DelegatedSessionId,
        reason: &str,
    ) -> AppResult<Option<AgentTaskAssignmentSettlement>>;

    async fn list_unresolved_assignments(&self) -> AppResult<Vec<AgentTaskAssignmentView>>;
}
