use std::sync::Arc;
use std::time::Duration;

use ralphx_events::RecordingEventSink;

use crate::application::proposal_generation_progress::{
    write_active_proposal_generation_progress_for_context, write_proposal_generation_progress,
    ProposalGenerationProgressTransition, PROPOSAL_GENERATION_PROGRESS_EVENT,
};
use crate::application::AppState;
use crate::domain::entities::{
    AgentConversationWorkspace, AgentConversationWorkspaceMode, ChatContextType, ChatConversation,
    ChatConversationId, IdeationAnalysisBaseRefKind, IdeationSession, IdeationSessionFlow,
    Priority, Project, ProjectId, ProposalCategory, ProposalGenerationPhase,
    ProposalGenerationStatus, TaskProposal,
};

async fn seeded_state() -> (AppState, IdeationSession, RecordingEventSink) {
    let mut state = AppState::new_sqlite_test();
    let sink = RecordingEventSink::new();
    state.events = Arc::new(sink.clone());

    let session = state
        .ideation_session_repo
        .create(IdeationSession::new(ProjectId::from_string(
            "project-proposal-progress".to_string(),
        )))
        .await
        .expect("session should be persisted");

    (state, session, sink)
}

async fn add_proposal(state: &AppState, session: &IdeationSession, title: &str) -> TaskProposal {
    state
        .task_proposal_repo
        .create(TaskProposal::new(
            session.id.clone(),
            title.to_string(),
            ProposalCategory::Feature,
            Priority::Medium,
        ))
        .await
        .expect("proposal should be persisted")
}

async fn linked_plan_workspace_state() -> (AppState, IdeationSession, ChatConversationId) {
    let state = AppState::new_sqlite_test();
    let project_id = ProjectId::from_string("project-proposal-runtime-progress".to_string());
    let mut project = Project::new(
        "Runtime Progress Project".to_string(),
        "/tmp/proposal-runtime-progress".to_string(),
    );
    project.id = project_id.clone();
    state
        .project_repo
        .create(project)
        .await
        .expect("project should be persisted");

    let session = state
        .ideation_session_repo
        .create(
            IdeationSession::builder()
                .project_id(project_id.clone())
                .session_flow(IdeationSessionFlow::Planning)
                .build(),
        )
        .await
        .expect("session should be persisted");

    let conversation = ChatConversation::new_project(project_id.clone());
    let conversation_id = conversation.id.clone();
    state
        .chat_conversation_repo
        .create(conversation)
        .await
        .expect("conversation should be persisted");

    let mut workspace = AgentConversationWorkspace::new(
        conversation_id.clone(),
        project_id,
        AgentConversationWorkspaceMode::Plan,
        IdeationAnalysisBaseRefKind::ProjectDefault,
        "main".to_string(),
        Some("Project default (main)".to_string()),
        None,
        "ralphx/project/proposal-runtime-progress".to_string(),
        "/tmp/proposal-runtime-progress-worktree".to_string(),
    );
    workspace.linked_ideation_session_id = Some(session.id.clone());
    state
        .agent_conversation_workspace_repo
        .create_or_update(workspace)
        .await
        .expect("workspace should be persisted");

    (state, session, conversation_id)
}

#[tokio::test]
async fn creating_progress_recalculates_counts_and_emits_event() {
    let (state, session, sink) = seeded_state().await;
    add_proposal(&state, &session, "First").await;
    add_proposal(&state, &session, "Second").await;

    let progress = write_proposal_generation_progress(
        &state,
        &session.id,
        ProposalGenerationProgressTransition::CreatingProposals {
            expected_count: Some(5),
        },
    )
    .await
    .expect("progress should be written");

    assert_eq!(progress.status, ProposalGenerationStatus::Running);
    assert_eq!(
        progress.phase,
        Some(ProposalGenerationPhase::CreatingProposals)
    );
    assert_eq!(progress.expected_count, Some(5));
    assert_eq!(progress.created_count, 2);
    assert_eq!(progress.dependency_count, None);
    assert!(progress.started_at.is_some());
    assert!(progress.updated_at.is_some());
    assert!(progress.completed_at.is_none());

    let updated = state
        .ideation_session_repo
        .get_by_id(&session.id)
        .await
        .expect("session lookup should succeed")
        .expect("session should exist")
        .proposal_generation_progress;
    assert_eq!(updated, progress);

    let events = sink.events();
    let event = events
        .iter()
        .find(|event| event.event == PROPOSAL_GENERATION_PROGRESS_EVENT)
        .expect("progress event should be emitted");
    assert_eq!(event.payload["sessionId"], session.id.as_str());
    assert_eq!(event.payload["progress"]["status"], "running");
    assert_eq!(event.payload["progress"]["phase"], "creating_proposals");
    assert_eq!(event.payload["progress"]["expected_count"], 5);
    assert_eq!(event.payload["progress"]["created_count"], 2);
}

#[tokio::test]
async fn dependency_progress_recalculates_dependency_count_and_preserves_expected_count() {
    let (state, session, _sink) = seeded_state().await;
    let first = add_proposal(&state, &session, "First").await;
    let second = add_proposal(&state, &session, "Second").await;

    write_proposal_generation_progress(
        &state,
        &session.id,
        ProposalGenerationProgressTransition::CreatingProposals {
            expected_count: Some(3),
        },
    )
    .await
    .expect("initial progress should be written");

    state
        .proposal_dependency_repo
        .add_dependency(
            &second.id,
            &first.id,
            Some("second depends on first"),
            Some("agent"),
        )
        .await
        .expect("dependency should be persisted");

    let progress = write_proposal_generation_progress(
        &state,
        &session.id,
        ProposalGenerationProgressTransition::AnalyzingDependencies,
    )
    .await
    .expect("dependency progress should be written");

    assert_eq!(progress.status, ProposalGenerationStatus::Running);
    assert_eq!(
        progress.phase,
        Some(ProposalGenerationPhase::AnalyzingDependencies)
    );
    assert_eq!(progress.expected_count, Some(3));
    assert_eq!(progress.created_count, 2);
    assert_eq!(progress.dependency_count, Some(1));
    assert!(progress.completed_at.is_none());
}

#[tokio::test]
async fn active_context_failure_marks_linked_plan_progress_failed() {
    let (state, session, conversation_id) = linked_plan_workspace_state().await;
    write_proposal_generation_progress(
        &state,
        &session.id,
        ProposalGenerationProgressTransition::Queued {
            expected_count: None,
        },
    )
    .await
    .expect("queued progress should be written");

    let changed = write_active_proposal_generation_progress_for_context(
        &state,
        ChatContextType::Project,
        session.project_id.as_str(),
        Some(&conversation_id),
        ProposalGenerationProgressTransition::Failed {
            error: "Agent failed before creating proposals".to_string(),
        },
    )
    .await
    .expect("active progress should be failed");

    assert!(changed);
    let progress = state
        .ideation_session_repo
        .get_by_id(&session.id)
        .await
        .expect("session lookup should succeed")
        .expect("session should exist")
        .proposal_generation_progress;
    assert_eq!(progress.status, ProposalGenerationStatus::Failed);
    assert_eq!(progress.phase, Some(ProposalGenerationPhase::Failed));
    assert!(progress
        .error
        .as_deref()
        .is_some_and(|message| message.contains("Agent failed")));
    assert!(progress.completed_at.is_some());
}

#[tokio::test]
async fn active_context_cancel_does_not_overwrite_completed_progress() {
    let (state, session, conversation_id) = linked_plan_workspace_state().await;
    write_proposal_generation_progress(
        &state,
        &session.id,
        ProposalGenerationProgressTransition::Completed,
    )
    .await
    .expect("completed progress should be written");

    let changed = write_active_proposal_generation_progress_for_context(
        &state,
        ChatContextType::Project,
        session.project_id.as_str(),
        Some(&conversation_id),
        ProposalGenerationProgressTransition::Cancelled {
            error: Some("Agent stopped by user".to_string()),
        },
    )
    .await
    .expect("terminal progress should be ignored");

    assert!(!changed);
    let progress = state
        .ideation_session_repo
        .get_by_id(&session.id)
        .await
        .expect("session lookup should succeed")
        .expect("session should exist")
        .proposal_generation_progress;
    assert_eq!(progress.status, ProposalGenerationStatus::Completed);
    assert_eq!(progress.phase, Some(ProposalGenerationPhase::Completed));
    assert!(progress.error.is_none());
}

#[tokio::test]
async fn active_context_failure_ignores_non_plan_project_workspace() {
    let (state, session, conversation_id) = linked_plan_workspace_state().await;
    let mut workspace = state
        .agent_conversation_workspace_repo
        .get_by_conversation_id(&conversation_id)
        .await
        .expect("workspace lookup should succeed")
        .expect("workspace should exist");
    workspace.mode = AgentConversationWorkspaceMode::Edit;
    state
        .agent_conversation_workspace_repo
        .create_or_update(workspace)
        .await
        .expect("workspace mode update should persist");
    write_proposal_generation_progress(
        &state,
        &session.id,
        ProposalGenerationProgressTransition::Queued {
            expected_count: None,
        },
    )
    .await
    .expect("queued progress should be written");

    let changed = write_active_proposal_generation_progress_for_context(
        &state,
        ChatContextType::Project,
        session.project_id.as_str(),
        Some(&conversation_id),
        ProposalGenerationProgressTransition::Failed {
            error: "Edit workspace stopped".to_string(),
        },
    )
    .await
    .expect("non-plan workspace should be ignored");

    assert!(!changed);
    let progress = state
        .ideation_session_repo
        .get_by_id(&session.id)
        .await
        .expect("session lookup should succeed")
        .expect("session should exist")
        .proposal_generation_progress;
    assert_eq!(progress.status, ProposalGenerationStatus::Queued);
    assert_eq!(progress.phase, Some(ProposalGenerationPhase::Queued));
    assert!(progress.error.is_none());
}

#[tokio::test]
async fn failed_progress_preserves_counts_and_sets_completed_timestamp() {
    let (state, session, _sink) = seeded_state().await;
    add_proposal(&state, &session, "Only").await;

    let progress = write_proposal_generation_progress(
        &state,
        &session.id,
        ProposalGenerationProgressTransition::Failed {
            error: "Proposal count mismatch".to_string(),
        },
    )
    .await
    .expect("failed progress should be written");

    assert_eq!(progress.status, ProposalGenerationStatus::Failed);
    assert_eq!(progress.phase, Some(ProposalGenerationPhase::Failed));
    assert_eq!(progress.created_count, 1);
    assert_eq!(progress.error.as_deref(), Some("Proposal count mismatch"));
    assert!(progress.started_at.is_some());
    assert!(progress.updated_at.is_some());
    assert!(progress.completed_at.is_some());
}

#[tokio::test]
async fn queued_retry_after_failure_starts_new_operation() {
    let (state, session, _sink) = seeded_state().await;

    let failed = write_proposal_generation_progress(
        &state,
        &session.id,
        ProposalGenerationProgressTransition::Failed {
            error: "First attempt failed".to_string(),
        },
    )
    .await
    .expect("failed progress should be written");
    tokio::time::sleep(Duration::from_millis(1)).await;

    let queued = write_proposal_generation_progress(
        &state,
        &session.id,
        ProposalGenerationProgressTransition::Queued {
            expected_count: Some(2),
        },
    )
    .await
    .expect("queued retry should be written");

    assert_eq!(queued.status, ProposalGenerationStatus::Queued);
    assert_eq!(queued.phase, Some(ProposalGenerationPhase::Queued));
    assert_eq!(queued.expected_count, Some(2));
    assert!(queued.error.is_none());
    assert!(queued.completed_at.is_none());
    assert!(
        queued.started_at > failed.started_at,
        "queued retry should start a fresh operation"
    );
}
