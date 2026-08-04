// Tauri commands for Task CRUD operations
// Modular structure: types, helpers, query (read), mutation (write), tests

mod execution_plan_control_service;
#[cfg(test)]
mod execution_plan_control_service_tests;
pub mod execution_plan_controls;
#[cfg(test)]
mod execution_plan_controls_tests;
pub mod helpers;
pub mod mutation;
pub mod query;
#[cfg(test)]
mod query_tests;
pub mod types;

// Re-export types
pub use types::{
    AnswerUserQuestionInput,
    AnswerUserQuestionResponse,
    BulkCancelResponse,
    CleanupReportResponse,
    CreateTaskInput,
    ExecutionPlanControlInput,
    ExecutionPlanControlResponse,
    InjectTaskInput,
    InjectTaskResponse,
    PlanGroupInfo,
    StateTransitionResponse,
    StatusSummary,
    StatusTransition,
    TaskDependencyGraphResponse,
    TaskGraphEdge,
    // Task graph types (Phase 67)
    TaskGraphNode,
    TaskHistoryAvailabilityResponse,
    TaskListResponse,
    TaskResponse,
    // Timeline event types (Phase 67 - Task D.1)
    TimelineEvent,
    TimelineEventType,
    TimelineEventsResponse,
    UpdateTaskInput,
};

pub use execution_plan_controls::{
    pause_execution_plan, resume_execution_plan, stop_execution_plan,
};

// Re-export helpers (for use by other command modules)
pub use helpers::{default_target, emit_queue_changed, emit_task_lifecycle_event, status_to_label};

// Re-export query commands
pub use query::{
    get_archived_count, get_session_task_history_availability, get_task, get_task_agent_workspace,
    get_task_dependency_graph, get_task_state_transitions, get_task_timeline_events,
    get_tasks_awaiting_review, get_valid_transitions, list_tasks, search_tasks,
};

// Re-export mutation commands
pub use mutation::{
    answer_user_question, archive_task, cancel_tasks_in_group, cleanup_task,
    cleanup_tasks_in_group, create_task, inject_task, move_task, pause_task, restore_task,
    resume_task, resume_tasks_in_group, retry_branch_update, stop_task, update_task,
};
pub(crate) use mutation::{resume_task_for_state, resume_tasks_in_group_for_state};
