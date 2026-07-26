use crate::entities::{
    ArtifactId, IdeationAnalysisBaseRefKind, IdeationAnalysisState, IdeationAnalysisWorkspaceKind,
    IdeationSessionBuilder, ProjectId, SessionOrigin, SessionPurpose, TaskId,
};

use super::{
    build_child_session, matching_blocker_followup_session, resolve_child_origin,
    ChildSessionDraftInput,
};

fn sample_parent() -> crate::entities::IdeationSession {
    IdeationSessionBuilder::new()
        .project_id(ProjectId::from_string("project-1".to_string()))
        .title("Parent")
        .source_project_id("source-project")
        .source_session_id("source-session")
        .origin(SessionOrigin::Internal)
        .plan_artifact_id(ArtifactId::from_string("artifact-1".to_string()))
        .plan_blueprint_artifact_id(ArtifactId::from_string("blueprint-1".to_string()))
        .build()
}

#[test]
fn matching_blocker_followup_session_ignores_archived_or_mismatched_children() {
    let task_id = TaskId::from_string("task-1".to_string());
    let mut archived = IdeationSessionBuilder::new()
        .project_id(ProjectId::from_string("project-1".to_string()))
        .source_task_id(task_id.clone())
        .blocker_fingerprint("fingerprint-1")
        .build();
    archived.archived_at = Some(chrono::Utc::now());
    let live = IdeationSessionBuilder::new()
        .project_id(ProjectId::from_string("project-1".to_string()))
        .source_task_id(task_id)
        .blocker_fingerprint("fingerprint-1")
        .build();

    let matched =
        matching_blocker_followup_session(&[archived, live.clone()], "task-1", "fingerprint-1")
            .expect("live child should match");

    assert_eq!(matched.id, live.id);
}

#[test]
fn build_child_session_inherits_expected_parent_context() {
    let parent = sample_parent();
    let parent_id = parent.id.clone();
    let child = build_child_session(
        parent_id.clone(),
        &parent,
        ChildSessionDraftInput {
            title: Some("Child".to_string()),
            inherit_context: true,
            source_task_id: Some("task-1".to_string()),
            source_context_type: Some("review".to_string()),
            source_context_id: Some("review-1".to_string()),
            spawn_reason: Some("out_of_scope_failure".to_string()),
            blocker_fingerprint: Some("fp".to_string()),
            purpose: SessionPurpose::General,
            is_external_trigger: false,
        },
    );

    assert_eq!(child.parent_session_id, Some(parent_id));
    assert_eq!(child.inherited_plan_artifact_id, parent.plan_artifact_id);
    assert_eq!(
        child.inherited_plan_blueprint_artifact_id,
        parent.plan_blueprint_artifact_id
    );
    assert_eq!(child.source_project_id.as_deref(), Some("source-project"));
    assert_eq!(child.source_session_id.as_deref(), Some("source-session"));
    assert_eq!(
        child.source_task_id.as_ref().map(|id| id.as_str()),
        Some("task-1")
    );
    assert_eq!(child.source_context_type.as_deref(), Some("review"));
    assert_eq!(child.spawn_reason.as_deref(), Some("out_of_scope_failure"));
    assert_eq!(child.blocker_fingerprint.as_deref(), Some("fp"));
    assert_eq!(child.session_purpose, SessionPurpose::General);
    assert_eq!(child.origin, SessionOrigin::Internal);
}

#[test]
fn build_child_session_inherits_analysis_base_and_workspace() {
    let mut parent = sample_parent();
    parent.analysis = IdeationAnalysisState {
        base_ref_kind: Some(IdeationAnalysisBaseRefKind::LocalBranch),
        base_ref: Some("feature/base".to_string()),
        base_display_name: Some("feature/base".to_string()),
        workspace_kind: IdeationAnalysisWorkspaceKind::IdeationWorktree,
        workspace_path: Some("/tmp/ralphx-ideation-worktree".to_string()),
        base_commit: Some("abc123".to_string()),
        base_locked_at: Some(chrono::Utc::now()),
    };

    let child = build_child_session(
        parent.id.clone(),
        &parent,
        ChildSessionDraftInput {
            title: Some("Verifier".to_string()),
            inherit_context: true,
            source_task_id: None,
            source_context_type: None,
            source_context_id: None,
            spawn_reason: None,
            blocker_fingerprint: None,
            purpose: SessionPurpose::Verification,
            is_external_trigger: false,
        },
    );

    assert_eq!(child.analysis, parent.analysis);
}

#[test]
fn resolve_child_origin_keeps_verification_children_on_parent_origin() {
    assert_eq!(
        resolve_child_origin(SessionOrigin::External, SessionPurpose::Verification, false),
        SessionOrigin::External
    );
    assert_eq!(
        resolve_child_origin(SessionOrigin::Internal, SessionPurpose::General, true),
        SessionOrigin::External
    );
    assert_eq!(
        resolve_child_origin(SessionOrigin::External, SessionPurpose::General, false),
        SessionOrigin::Internal
    );
}
