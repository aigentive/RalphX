use chrono::{TimeZone, Utc};

use crate::entities::{
    ChatConversation, IdeationSessionBuilder, InternalStatus, ProjectId, StepProgressSummary, Task,
};
use crate::repositories::StatusTransition;

use super::{
    build_running_ideation_session, build_running_process,
    build_running_process_with_agent_workspace, build_running_workspace_session,
    elapsed_seconds_for_status, ideation_session_title, workspace_session_title,
    ExecutionTaskAgentWorkspace,
};

#[test]
fn ideation_session_title_falls_back_when_missing() {
    assert_eq!(ideation_session_title(None), "Untitled Session");
    assert_eq!(ideation_session_title(Some("Named")), "Named");
    assert_eq!(workspace_session_title(None), "Untitled Workspace");
    assert_eq!(workspace_session_title(Some("Workspace")), "Workspace");
}

#[test]
fn elapsed_seconds_for_status_uses_latest_matching_transition() {
    let now = Utc.with_ymd_and_hms(2026, 3, 29, 12, 0, 0).unwrap();
    let history = vec![
        StatusTransition::with_timestamp(
            InternalStatus::Ready,
            InternalStatus::Executing,
            "system",
            Utc.with_ymd_and_hms(2026, 3, 29, 11, 0, 0).unwrap(),
        ),
        StatusTransition::with_timestamp(
            InternalStatus::Reviewing,
            InternalStatus::Executing,
            "retry",
            Utc.with_ymd_and_hms(2026, 3, 29, 11, 30, 0).unwrap(),
        ),
    ];

    assert_eq!(
        elapsed_seconds_for_status(&history, InternalStatus::Executing, now),
        Some(1800)
    );
}

#[test]
fn build_running_views_shape_expected_fields() {
    let now = Utc.with_ymd_and_hms(2026, 3, 29, 12, 0, 0).unwrap();
    let mut session = IdeationSessionBuilder::new()
        .project_id(ProjectId::from_string("proj-1".to_string()))
        .title("Plan")
        .build();
    session.created_at = Utc.with_ymd_and_hms(2026, 3, 29, 11, 59, 0).unwrap();

    let ideation = build_running_ideation_session("session-1".to_string(), &session, true, now);
    assert_eq!(ideation.session_id, "session-1");
    assert_eq!(ideation.title, "Plan");
    assert_eq!(ideation.elapsed_seconds, Some(60));
    assert!(ideation.is_generating);

    let mut task = Task::new(
        ProjectId::from_string("proj-1".to_string()),
        "Task title".to_string(),
    );
    task.internal_status = InternalStatus::Reviewing;
    task.task_branch = Some("ralphx/proj/task-1".to_string());
    let process = build_running_process(
        &task,
        Some(StepProgressSummary {
            task_id: task.id.as_str().to_string(),
            total: 4,
            completed: 2,
            in_progress: 1,
            pending: 1,
            skipped: 0,
            failed: 0,
            current_step: None,
            next_step: None,
            percent_complete: 50.0,
        }),
        Some(45),
        Some("scheduler".to_string()),
    );
    assert_eq!(process.task_id, task.id.as_str());
    assert_eq!(process.title, "Task title");
    assert_eq!(process.internal_status, "reviewing");
    assert_eq!(process.elapsed_seconds, Some(45));
    assert_eq!(process.trigger_origin.as_deref(), Some("scheduler"));
    assert_eq!(process.task_branch.as_deref(), Some("ralphx/proj/task-1"));
    assert!(process.agent_workspace.is_none());

    let targeted_process = build_running_process_with_agent_workspace(
        &task,
        None,
        Some(12),
        Some("scheduler".to_string()),
        Some(ExecutionTaskAgentWorkspace {
            conversation_id: "conversation-1".to_string(),
            project_id: "proj-1".to_string(),
            title: "Workspace title".to_string(),
        }),
    );
    assert_eq!(
        targeted_process
            .agent_workspace
            .as_ref()
            .map(|workspace| workspace.title.as_str()),
        Some("Workspace title")
    );

    let mut conversation =
        ChatConversation::new_project(ProjectId::from_string("proj-1".to_string()));
    conversation.title = Some("Workspace run".to_string());
    conversation.automation_id = Some(crate::entities::AutomationId::from_string("automation-1"));
    conversation.automation_run_id = Some(crate::entities::AutomationRunId::from_string("run-1"));
    let workspace = build_running_workspace_session(
        &conversation,
        Utc.with_ymd_and_hms(2026, 3, 29, 11, 58, 0).unwrap(),
        Some("gpt-5.5".to_string()),
        now,
    );
    assert_eq!(workspace.conversation_id, conversation.id.as_str());
    assert_eq!(workspace.project_id, "proj-1");
    assert_eq!(workspace.automation_id.as_deref(), Some("automation-1"));
    assert_eq!(workspace.automation_run_id.as_deref(), Some("run-1"));
    assert_eq!(workspace.title, "Workspace run");
    assert_eq!(workspace.elapsed_seconds, Some(120));
    assert_eq!(workspace.model.as_deref(), Some("gpt-5.5"));
}
