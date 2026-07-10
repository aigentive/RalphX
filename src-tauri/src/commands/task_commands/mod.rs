// Tauri commands for Task CRUD operations
// Modular structure: types, helpers, query (read), mutation (write), tests

pub mod helpers;
mod execution_plan_control_service;
pub mod execution_plan_controls;
pub mod mutation;
pub mod query;
pub mod types;
#[cfg(test)]
mod execution_plan_control_service_tests;

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
    get_archived_count, get_task, get_task_dependency_graph, get_task_state_transitions,
    get_task_timeline_events, get_tasks_awaiting_review, get_valid_transitions, list_tasks,
    search_tasks,
};

// Re-export mutation commands
pub use mutation::{
    answer_user_question, archive_task, cancel_tasks_in_group, cleanup_task,
    cleanup_tasks_in_group, create_task, inject_task, move_task, pause_task, restore_task,
    resume_task, stop_task, update_task,
};
