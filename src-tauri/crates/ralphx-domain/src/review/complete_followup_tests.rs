use crate::entities::{
    IdeationSessionBuilder, IdeationSessionStatus, InternalStatus, ProjectId, Task, TaskContext,
    TaskId, TaskProposalId,
};

use super::{
    build_followup_activity_event, build_unrelated_drift_followup_draft,
    matching_unrelated_drift_followup_session_id, should_spawn_unrelated_drift_followup,
    update_review_scope_metadata,
};
use crate::review::{
    compute_out_of_scope_blocker_fingerprint, ReviewSettings, ReviewToolOutcome,
    ScopeDriftClassification,
};

fn sample_task() -> Task {
    let project_id = ProjectId::from_string("project-1".to_string());
    let mut task = Task::new(project_id, "Refactor review flow".to_string());
    task.id = TaskId::from_string("task-1".to_string());
    task
}

fn sample_task_context() -> TaskContext {
    let task = sample_task();
    let _project_id = task.project_id.clone();

    TaskContext {
        task,
        source_proposal: Some(crate::entities::TaskProposalSummary {
            id: TaskProposalId::from_string("proposal-1"),
            title: "proposal".to_string(),
            description: "desc".to_string(),
            acceptance_criteria: Vec::new(),
            implementation_notes: None,
            plan_version_at_creation: None,
            priority_score: 0,
            affected_paths: vec!["src-tauri/src/http_server/handlers/reviews".to_string()],
        }),
        plan_artifact: None,
        blueprint_artifact: None,
        related_artifacts: Vec::new(),
        steps: Vec::new(),
        step_progress: None,
        context_hints: Vec::new(),
        blocked_by: Vec::new(),
        blocks: Vec::new(),
        tier: None,
        task_branch: None,
        worktree_path: None,
        validation_cache: None,
        actual_changed_files: vec![
            "src-tauri/src/http_server/handlers/reviews/complete.rs".to_string(),
            "ralphx.yaml".to_string(),
        ],
        scope_drift_status: crate::entities::ScopeDriftStatus::ScopeExpansion,
        out_of_scope_files: vec!["ralphx.yaml".to_string()],
        out_of_scope_blocker_fingerprint: None,
        followup_sessions: Vec::new(),
    }
}

#[test]
fn update_review_scope_metadata_clears_when_no_planned_paths() {
    let mut task_context = sample_task_context();
    task_context
        .source_proposal
        .as_mut()
        .unwrap()
        .affected_paths
        .clear();

    let result = update_review_scope_metadata(
        Some(r#"{"review_scope":{"planned_paths":["old"]},"keep":true}"#),
        &task_context,
        Some(ScopeDriftClassification::UnrelatedDrift),
        Some("note".to_string()),
    )
    .expect("metadata update should succeed");

    assert_eq!(result, Some(r#"{"keep":true}"#.to_string()));
}

#[test]
fn update_review_scope_metadata_persists_scope_snapshot() {
    let task_context = sample_task_context();

    let result = update_review_scope_metadata(
        Some(r#"{"keep":true}"#),
        &task_context,
        Some(ScopeDriftClassification::UnrelatedDrift),
        Some("reviewer note".to_string()),
    )
    .expect("metadata update should succeed")
    .expect("review scope snapshot should be stored");

    assert!(result.contains("\"review_scope\""));
    assert!(result.contains("ralphx.yaml"));
    assert!(result.contains("unrelated_drift"));
    assert!(result.contains("reviewer note"));
}

#[test]
fn should_spawn_unrelated_drift_followup_requires_exhausted_escalation() {
    let settings = ReviewSettings {
        max_revision_cycles: 2,
        ..ReviewSettings::default()
    };

    assert!(should_spawn_unrelated_drift_followup(
        ReviewToolOutcome::Escalate,
        Some(ScopeDriftClassification::UnrelatedDrift),
        2,
        &settings,
    ));
    assert!(!should_spawn_unrelated_drift_followup(
        ReviewToolOutcome::NeedsChanges,
        Some(ScopeDriftClassification::UnrelatedDrift),
        2,
        &settings,
    ));
    assert!(!should_spawn_unrelated_drift_followup(
        ReviewToolOutcome::Escalate,
        Some(ScopeDriftClassification::AdjacentScopeExpansion),
        2,
        &settings,
    ));
    assert!(!should_spawn_unrelated_drift_followup(
        ReviewToolOutcome::Escalate,
        Some(ScopeDriftClassification::UnrelatedDrift),
        1,
        &settings,
    ));
}

#[test]
fn build_unrelated_drift_followup_draft_carries_prompt_and_fingerprint() {
    let task_context = sample_task_context();
    // The draft builder takes the `Task` alongside the context, so pass the same entity the
    // context carries.
    let task = sample_task();
    let draft = build_unrelated_drift_followup_draft(
        &task,
        &task_context,
        Some("summary"),
        Some("feedback"),
        Some("escalation"),
        3,
        &ReviewSettings {
            max_revision_cycles: 3,
            ..ReviewSettings::default()
        },
    );

    assert_eq!(draft.title, "Follow-up: Refactor review flow");
    assert!(draft.description.contains("Separate follow-up"));
    assert!(draft.prompt.contains("summary"));
    assert_eq!(
        draft.blocker_fingerprint,
        compute_out_of_scope_blocker_fingerprint(
            &task_context.task.id,
            &task_context.out_of_scope_files
        )
    );
}

#[test]
fn matching_unrelated_drift_followup_session_id_prefers_fingerprint() {
    let task_id = TaskId::from_string("task-1".to_string());
    let session = IdeationSessionBuilder::new()
        .project_id(ProjectId::from_string("project-1".to_string()))
        .status(IdeationSessionStatus::Active)
        .source_task_id(task_id.clone())
        .source_context_type("worker")
        .spawn_reason("another_reason")
        .blocker_fingerprint("scope-drift:task-1:ralphx.yaml")
        .build();
    let expected_id = session.id.as_str().to_string();

    let found = matching_unrelated_drift_followup_session_id(
        &[session],
        &task_id,
        Some("scope-drift:task-1:ralphx.yaml"),
    );

    assert_eq!(found.as_deref(), Some(expected_id.as_str()));
}

#[test]
fn build_followup_activity_event_returns_system_event_with_metadata() {
    let event = build_followup_activity_event(
        TaskId::from_string("task-1".to_string()),
        InternalStatus::Escalated,
        Some("session-1"),
        "review-note-1",
    )
    .expect("activity event should be built");

    assert_eq!(event.role.to_string(), "system");
    assert_eq!(event.event_type.to_string(), "system");
    assert_eq!(event.internal_status, Some(InternalStatus::Escalated));
    assert!(event
        .metadata
        .expect("metadata")
        .contains("\"followupSessionId\":\"session-1\""));
}
