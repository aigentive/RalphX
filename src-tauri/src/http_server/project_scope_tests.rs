// Tests for ProjectScope extractor and ProjectScopeGuard trait
//
// Coverage:
// - parse_project_scope_header: empty, single, multiple, whitespace, empty segments
// - ProjectScopeGuard::assert_project_scope: None (unrestricted), match, mismatch, multi-project
// - Task / IdeationSession / Project / Review implementations

use axum::http::StatusCode;

use crate::domain::entities::types::ProjectId;
use crate::http_server::project_scope::{
    parse_project_scope_header, ProjectScope, ProjectScopeGuard,
};

// ============================================================================
// Helpers
// ============================================================================

fn make_task(project_id: &str) -> crate::domain::entities::task::Task {
    crate::domain::entities::task::Task::new(
        ProjectId::from_string(project_id.to_string()),
        "test task".to_string(),
    )
}

fn make_session(project_id: &str) -> crate::domain::entities::ideation::IdeationSession {
    crate::domain::entities::ideation::IdeationSession::new(ProjectId::from_string(
        project_id.to_string(),
    ))
}

fn make_project(id: &str) -> crate::domain::entities::project::Project {
    use crate::domain::entities::project::Project;
    Project {
        id: ProjectId::from_string(id.to_string()),
        name: "test".to_string(),
        working_directory: "/tmp".to_string(),
        git_mode: crate::domain::entities::project::GitMode::Worktree,
        base_branch: None,
        worktree_parent_directory: None,
        use_feature_branches: true,
        merge_validation_mode: Default::default(),
        merge_strategy: Default::default(),
        detected_analysis: None,
        custom_analysis: None,
        analyzed_at: None,
        github_pr_enabled: true,
        created_at: chrono::Utc::now(),
        updated_at: chrono::Utc::now(),
        archived_at: None,
    }
}

fn make_review(project_id: &str, task_id: &str) -> crate::domain::entities::review::Review {
    use crate::domain::entities::{review::Review, review::ReviewerType, TaskId};
    Review::new(
        ProjectId::from_string(project_id.to_string()),
        TaskId::from_string(task_id.to_string()),
        ReviewerType::Ai,
    )
}

fn make_project_skill(project_id: &str) -> crate::domain::entities::ProjectSkill {
    use crate::domain::entities::{ProjectSkill, ProjectSkillId, ProjectSkillLifecycleStatus};
    ProjectSkill {
        id: ProjectSkillId::new(),
        project_id: ProjectId::from_string(project_id.to_string()),
        title: "test skill".to_string(),
        bucket: "execution".to_string(),
        stage: "execution".to_string(),
        status: ProjectSkillLifecycleStatus::Approved,
        pinned: false,
        archived: false,
        scope_paths: Vec::new(),
        compact_guidance: "test guidance".to_string(),
        body_markdown: "test body".to_string(),
        predicted_effect: Some("test effect".to_string()),
        provenance_json: serde_json::json!({}),
        companion_of_skill_id: None,
        content_hash: String::new(),
        evidence_hash: String::new(),
        created_by: crate::domain::entities::ProjectSkillCreatedBy::User,
        pipeline_role: None,
        created_at: chrono::Utc::now(),
        updated_at: chrono::Utc::now(),
    }
}

fn scoped(ids: &[&str]) -> ProjectScope {
    let vec: Vec<ProjectId> = ids
        .iter()
        .map(|s| ProjectId::from_string(s.to_string()))
        .collect();
    ProjectScope(Some(vec))
}

fn unrestricted() -> ProjectScope {
    ProjectScope(None)
}

// ============================================================================
// parse_project_scope_header tests
// ============================================================================

#[test]
fn parse_single_id() {
    let ids = parse_project_scope_header("proj-abc");
    assert_eq!(ids.len(), 1);
    assert_eq!(ids[0].as_str(), "proj-abc");
}

#[test]
fn parse_multiple_ids() {
    let ids = parse_project_scope_header("proj-a,proj-b,proj-c");
    assert_eq!(ids.len(), 3);
    assert_eq!(ids[0].as_str(), "proj-a");
    assert_eq!(ids[1].as_str(), "proj-b");
    assert_eq!(ids[2].as_str(), "proj-c");
}

#[test]
fn parse_trims_whitespace() {
    let ids = parse_project_scope_header("  proj-a ,  proj-b  ");
    assert_eq!(ids.len(), 2);
    assert_eq!(ids[0].as_str(), "proj-a");
    assert_eq!(ids[1].as_str(), "proj-b");
}

#[test]
fn parse_skips_empty_segments() {
    let ids = parse_project_scope_header("proj-a,,proj-b,");
    assert_eq!(ids.len(), 2);
    assert_eq!(ids[0].as_str(), "proj-a");
    assert_eq!(ids[1].as_str(), "proj-b");
}

#[test]
fn parse_all_empty_gives_empty_ids() {
    let ids = parse_project_scope_header(",, ,");
    assert!(ids.is_empty());
}

// ============================================================================
// ProjectScope helper tests
// ============================================================================

#[test]
fn is_unrestricted_when_none() {
    assert!(unrestricted().is_unrestricted());
}

#[test]
fn is_not_unrestricted_when_some() {
    assert!(!scoped(&["proj-a"]).is_unrestricted());
}

// ============================================================================
// ProjectScopeGuard::assert_project_scope tests
// ============================================================================

#[test]
fn unrestricted_scope_always_allows() {
    let task = make_task("proj-abc");
    assert!(task.assert_project_scope(&unrestricted()).is_ok());
}

#[test]
fn allowed_when_project_in_scope() {
    let task = make_task("proj-abc");
    assert!(task.assert_project_scope(&scoped(&["proj-abc"])).is_ok());
}

#[test]
fn forbidden_when_project_not_in_scope() {
    let task = make_task("proj-abc");
    let err = task
        .assert_project_scope(&scoped(&["proj-other"]))
        .unwrap_err();
    assert_eq!(err.status, StatusCode::FORBIDDEN);
    assert!(err
        .message
        .as_deref()
        .unwrap_or("")
        .contains("does not have access"));
}

#[test]
fn allowed_when_one_of_multiple_projects_matches() {
    let task = make_task("proj-abc");
    let scope = scoped(&["proj-other", "proj-abc"]);
    assert!(task.assert_project_scope(&scope).is_ok());
}

#[test]
fn forbidden_when_scope_has_empty_allowed_list() {
    let task = make_task("proj-abc");
    let scope = ProjectScope(Some(vec![]));
    let err = task.assert_project_scope(&scope).unwrap_err();
    assert_eq!(err.status, StatusCode::FORBIDDEN);
}

// ============================================================================
// Entity-specific implementation tests
// ============================================================================

#[test]
fn ideation_session_scope_guard() {
    let session = make_session("proj-session");
    assert_eq!(session.project_id().as_str(), "proj-session");

    assert!(session
        .assert_project_scope(&scoped(&["proj-other"]))
        .is_err());
    assert!(session
        .assert_project_scope(&scoped(&["proj-session"]))
        .is_ok());
    assert!(session.assert_project_scope(&unrestricted()).is_ok());
}

#[test]
fn project_scope_guard_uses_own_id() {
    let project = make_project("proj-xyz");
    assert_eq!(project.project_id().as_str(), "proj-xyz");

    assert!(project.assert_project_scope(&scoped(&["proj-xyz"])).is_ok());
    assert!(project
        .assert_project_scope(&scoped(&["proj-other"]))
        .is_err());
    assert!(project.assert_project_scope(&unrestricted()).is_ok());
}

#[test]
fn review_scope_guard() {
    let review = make_review("proj-review", "task-001");
    assert_eq!(review.project_id().as_str(), "proj-review");

    assert!(review
        .assert_project_scope(&scoped(&["proj-review"]))
        .is_ok());
    assert!(review
        .assert_project_scope(&scoped(&["proj-other"]))
        .is_err());
}

#[test]
fn project_skill_scope_guard() {
    let skill = make_project_skill("proj-skill");
    assert_eq!(skill.project_id().as_str(), "proj-skill");

    assert!(skill.assert_project_scope(&scoped(&["proj-skill"])).is_ok());
    assert!(skill
        .assert_project_scope(&scoped(&["proj-other"]))
        .is_err());
    assert!(skill.assert_project_scope(&unrestricted()).is_ok());
}

#[test]
fn cross_project_rejection() {
    let task = make_task("proj-b");
    let err = task.assert_project_scope(&scoped(&["proj-a"])).unwrap_err();
    assert_eq!(err.status, StatusCode::FORBIDDEN);
}

#[test]
fn multi_project_key_allows_all_listed() {
    let task_a = make_task("proj-a");
    let task_b = make_task("proj-b");
    let task_c = make_task("proj-c");

    let scope = scoped(&["proj-a", "proj-b"]);

    assert!(task_a.assert_project_scope(&scope).is_ok());
    assert!(task_b.assert_project_scope(&scope).is_ok());
    assert!(task_c.assert_project_scope(&scope).is_err());
}

#[test]
fn missing_header_is_backward_compatible() {
    // No scope header → all existing internal routes work without changes
    let task = make_task("any-project");
    let session = make_session("any-project");
    let project = make_project("any-project");
    let review = make_review("any-project", "task-001");

    assert!(task.assert_project_scope(&unrestricted()).is_ok());
    assert!(session.assert_project_scope(&unrestricted()).is_ok());
    assert!(project.assert_project_scope(&unrestricted()).is_ok());
    assert!(review.assert_project_scope(&unrestricted()).is_ok());
}
