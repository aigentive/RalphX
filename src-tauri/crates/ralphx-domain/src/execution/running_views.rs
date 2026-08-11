use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::entities::{
    ChatConversation, IdeationSession, InternalStatus, StepProgressSummary, Task,
};
use crate::repositories::StatusTransition;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExecutionTaskAgentWorkspace {
    pub conversation_id: String,
    pub project_id: String,
    pub title: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunningProcess {
    pub task_id: String,
    pub title: String,
    pub internal_status: String,
    pub step_progress: Option<StepProgressSummary>,
    pub elapsed_seconds: Option<i64>,
    pub trigger_origin: Option<String>,
    pub task_branch: Option<String>,
    pub agent_workspace: Option<ExecutionTaskAgentWorkspace>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunningIdeationSession {
    pub session_id: String,
    pub title: String,
    pub elapsed_seconds: Option<i64>,
    pub is_generating: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunningWorkspaceSession {
    pub conversation_id: String,
    pub project_id: String,
    pub automation_id: Option<String>,
    pub automation_run_id: Option<String>,
    pub title: String,
    pub elapsed_seconds: Option<i64>,
    pub model: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionLaneUsage {
    pub lane: String,
    pub active: u32,
    pub idle: u32,
    pub waiting: u32,
    pub max: u32,
    pub borrowed: u32,
    pub priority_rank: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionCapacitySummary {
    pub total_active: u32,
    pub global_max_concurrent: u32,
    pub borrowing_enabled: bool,
    pub priority: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunningProcessesResponse {
    pub processes: Vec<RunningProcess>,
    pub ideation_sessions: Vec<RunningIdeationSession>,
    pub workspace_sessions: Vec<RunningWorkspaceSession>,
    pub lanes: Vec<ExecutionLaneUsage>,
    pub capacity: ExecutionCapacitySummary,
}

pub fn ideation_session_title(title: Option<&str>) -> String {
    title.unwrap_or("Untitled Session").to_string()
}

pub fn workspace_session_title(title: Option<&str>) -> String {
    title.unwrap_or("Untitled Workspace").to_string()
}

pub fn elapsed_seconds_since(timestamp: DateTime<Utc>, now: DateTime<Utc>) -> i64 {
    now.signed_duration_since(timestamp).num_seconds()
}

pub fn elapsed_seconds_for_status(
    history: &[StatusTransition],
    current_status: InternalStatus,
    now: DateTime<Utc>,
) -> Option<i64> {
    history
        .iter()
        .rev()
        .find(|transition| transition.to == current_status)
        .map(|transition| elapsed_seconds_since(transition.timestamp, now))
}

pub fn build_running_ideation_session(
    session_id: String,
    session: &IdeationSession,
    is_generating: bool,
    now: DateTime<Utc>,
) -> RunningIdeationSession {
    RunningIdeationSession {
        session_id,
        title: ideation_session_title(session.title.as_deref()),
        elapsed_seconds: Some(elapsed_seconds_since(session.created_at, now)),
        is_generating,
    }
}

pub fn build_running_workspace_session(
    conversation: &ChatConversation,
    started_at: DateTime<Utc>,
    model: Option<String>,
    now: DateTime<Utc>,
) -> RunningWorkspaceSession {
    RunningWorkspaceSession {
        conversation_id: conversation.id.as_str().to_string(),
        project_id: conversation.context_id.clone(),
        automation_id: conversation
            .automation_id
            .as_ref()
            .map(|id| id.as_str().to_string()),
        automation_run_id: conversation
            .automation_run_id
            .as_ref()
            .map(|id| id.as_str().to_string()),
        title: workspace_session_title(conversation.title.as_deref()),
        elapsed_seconds: Some(elapsed_seconds_since(started_at, now)),
        model,
    }
}

pub fn build_running_process(
    task: &Task,
    step_progress: Option<StepProgressSummary>,
    elapsed_seconds: Option<i64>,
    trigger_origin: Option<String>,
) -> RunningProcess {
    build_running_process_with_agent_workspace(
        task,
        step_progress,
        elapsed_seconds,
        trigger_origin,
        None,
    )
}

pub fn build_running_process_with_agent_workspace(
    task: &Task,
    step_progress: Option<StepProgressSummary>,
    elapsed_seconds: Option<i64>,
    trigger_origin: Option<String>,
    agent_workspace: Option<ExecutionTaskAgentWorkspace>,
) -> RunningProcess {
    RunningProcess {
        task_id: task.id.as_str().to_string(),
        title: task.title.clone(),
        internal_status: task.internal_status.as_str().to_string(),
        step_progress,
        elapsed_seconds,
        trigger_origin,
        task_branch: task.task_branch.clone(),
        agent_workspace,
    }
}

#[cfg(test)]
#[path = "running_views_tests.rs"]
mod tests;
