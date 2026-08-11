use crate::entities::{ChatContextType, InternalStatus, ProjectId};

use super::{
    context_matches_running_status, count_execution_status, ExecutionStatusCounts,
    ScopedExecutionSubject,
};

#[test]
fn context_matches_running_status_only_accepts_live_task_states() {
    assert!(context_matches_running_status(
        ChatContextType::TaskExecution,
        InternalStatus::Executing
    ));
    assert!(context_matches_running_status(
        ChatContextType::TaskExecution,
        InternalStatus::ReExecuting
    ));
    assert!(context_matches_running_status(
        ChatContextType::Review,
        InternalStatus::Reviewing
    ));
    assert!(context_matches_running_status(
        ChatContextType::Merge,
        InternalStatus::Merging
    ));
    assert!(context_matches_running_status(
        ChatContextType::BranchUpdate,
        InternalStatus::UpdatingPlanBranch
    ));
    assert!(context_matches_running_status(
        ChatContextType::BranchUpdate,
        InternalStatus::UpdatingTaskBranch
    ));

    assert!(!context_matches_running_status(
        ChatContextType::TaskExecution,
        InternalStatus::Failed
    ));
    assert!(!context_matches_running_status(
        ChatContextType::Ideation,
        InternalStatus::Executing
    ));
    assert!(!context_matches_running_status(
        ChatContextType::Task,
        InternalStatus::Executing
    ));
    assert!(!context_matches_running_status(
        ChatContextType::BranchUpdate,
        InternalStatus::BranchUpdateBlocked
    ));
}

#[test]
fn count_execution_status_scopes_ideation_and_execution_separately() {
    let project_a = ProjectId::from_string("proj-a".to_string());
    let project_b = ProjectId::from_string("proj-b".to_string());

    let counts = count_execution_status(
        vec![
            ScopedExecutionSubject::Ideation {
                project_id: project_a.clone(),
                is_idle: false,
            },
            ScopedExecutionSubject::Ideation {
                project_id: project_a.clone(),
                is_idle: true,
            },
            ScopedExecutionSubject::Ideation {
                project_id: project_b.clone(),
                is_idle: false,
            },
            ScopedExecutionSubject::Task {
                context_type: ChatContextType::TaskExecution,
                project_id: project_a.clone(),
                status: InternalStatus::Executing,
            },
            ScopedExecutionSubject::Task {
                context_type: ChatContextType::TaskExecution,
                project_id: project_b,
                status: InternalStatus::Failed,
            },
        ],
        Some(&project_a),
    );

    assert_eq!(
        counts,
        ExecutionStatusCounts {
            running_count: 1,
            total_project_active: 2,
            ideation_active: 1,
            ideation_idle: 1,
        }
    );
}

#[test]
fn count_execution_status_skips_out_of_scope_or_terminal_entries() {
    let project_a = ProjectId::from_string("proj-a".to_string());

    let counts = count_execution_status(
        vec![
            ScopedExecutionSubject::Task {
                context_type: ChatContextType::TaskExecution,
                project_id: project_a.clone(),
                status: InternalStatus::Merged,
            },
            ScopedExecutionSubject::Task {
                context_type: ChatContextType::Review,
                project_id: project_a.clone(),
                status: InternalStatus::Executing,
            },
            ScopedExecutionSubject::Ideation {
                project_id: project_a,
                is_idle: true,
            },
        ],
        None,
    );

    assert_eq!(
        counts,
        ExecutionStatusCounts {
            running_count: 0,
            total_project_active: 0,
            ideation_active: 0,
            ideation_idle: 1,
        }
    );
}
