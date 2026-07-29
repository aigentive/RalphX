use super::SqliteTaskProposalRepository;
use crate::domain::entities::{
    ArtifactId, BusinessValueFactor, Complexity, ComplexityFactor, CriticalPathFactor,
    DependencyFactor, IdeationSessionId, Priority, PriorityAssessment,
    PriorityAssessmentFactors, ProposalCategory, ProposalStatus, TaskId, TaskProposalId,
    UserHintFactor, ProjectId, TaskProposal,
};
use crate::domain::repositories::TaskProposalRepository;
use crate::testing::SqliteTestDb;

fn setup_test_db() -> SqliteTestDb {
    SqliteTestDb::new("sqlite-task-proposal-repo")
}

fn create_test_project(db: &SqliteTestDb, id: &ProjectId, name: &str, path: &str) {
    db.with_connection(|conn| {
        conn.execute(
            "INSERT INTO projects (id, name, working_directory, git_mode, created_at, updated_at)
             VALUES (?1, ?2, ?3, 'single_branch', strftime('%Y-%m-%dT%H:%M:%S+00:00', 'now'), strftime('%Y-%m-%dT%H:%M:%S+00:00', 'now'))",
            rusqlite::params![id.as_str(), name, path],
        )
        .unwrap();
    });
}

fn create_test_session(
    db: &SqliteTestDb,
    session_id: &IdeationSessionId,
    project_id: &ProjectId,
) {
    db.with_connection(|conn| {
        conn.execute(
            "INSERT INTO ideation_sessions (id, project_id, status, created_at, updated_at)
             VALUES (?1, ?2, 'active', strftime('%Y-%m-%dT%H:%M:%S+00:00', 'now'), strftime('%Y-%m-%dT%H:%M:%S+00:00', 'now'))",
            rusqlite::params![session_id.as_str(), project_id.as_str()],
        )
        .unwrap();
    });
}

fn create_test_proposal(session_id: &IdeationSessionId, title: &str) -> TaskProposal {
    TaskProposal::new(
        session_id.clone(),
        title,
        ProposalCategory::Feature,
        Priority::Medium,
    )
}

fn create_test_assessment(proposal_id: &TaskProposalId) -> PriorityAssessment {
    let factors = PriorityAssessmentFactors {
        dependency_factor: DependencyFactor {
            score: 15,
            blocks_count: 2,
            reason: "Blocks 2 tasks".to_string(),
        },
        critical_path_factor: CriticalPathFactor {
            score: 20,
            is_on_critical_path: true,
            path_length: 3,
            reason: "On critical path".to_string(),
        },
        business_value_factor: BusinessValueFactor {
            score: 15,
            keywords: vec!["core".to_string()],
            reason: "Core functionality".to_string(),
        },
        complexity_factor: ComplexityFactor {
            score: 10,
            complexity: Complexity::Simple,
            reason: "Simple task".to_string(),
        },
        user_hint_factor: UserHintFactor {
            score: 5,
            hints: vec!["important".to_string()],
            reason: "User marked important".to_string(),
        },
    };
    PriorityAssessment::new(proposal_id.clone(), factors)
}

// ==================== CREATE TESTS ====================

#[tokio::test]
async fn test_create_inserts_proposal_and_returns_it() {
    let db = setup_test_db();
    let project_id = ProjectId::new();
    let session_id = IdeationSessionId::new();
    create_test_project(&db, &project_id, "Test Project", "/test/path");
    create_test_session(&db, &session_id, &project_id);

    let repo = SqliteTaskProposalRepository::new(db.new_connection());
    let proposal = create_test_proposal(&session_id, "Test Proposal");

    let result = repo.create(proposal.clone()).await;

    assert!(result.is_ok());
    let created = result.unwrap();
    assert_eq!(created.id, proposal.id);
    assert_eq!(created.title, "Test Proposal");
    assert_eq!(created.status, ProposalStatus::Pending);
}

#[tokio::test]
async fn test_create_with_all_fields() {
    let db = setup_test_db();
    let project_id = ProjectId::new();
    let session_id = IdeationSessionId::new();
    create_test_project(&db, &project_id, "Test Project", "/test/path");
    create_test_session(&db, &session_id, &project_id);

    let repo = SqliteTaskProposalRepository::new(db.new_connection());
    let mut proposal = create_test_proposal(&session_id, "Full Proposal");
    proposal.description = Some("Detailed description".to_string());
    proposal.steps = Some(r#"["Step 1", "Step 2"]"#.to_string());
    proposal.acceptance_criteria = Some(r#"["AC 1", "AC 2"]"#.to_string());
    proposal.priority_reason = Some("Important feature".to_string());
    proposal.estimated_complexity = Complexity::Complex;

    let result = repo.create(proposal.clone()).await;

    assert!(result.is_ok());
    let created = result.unwrap();
    assert_eq!(created.description, Some("Detailed description".to_string()));
    assert_eq!(created.steps, Some(r#"["Step 1", "Step 2"]"#.to_string()));
    assert_eq!(created.estimated_complexity, Complexity::Complex);
}

#[tokio::test]
async fn test_create_duplicate_id_fails() {
    let db = setup_test_db();
    let project_id = ProjectId::new();
    let session_id = IdeationSessionId::new();
    create_test_project(&db, &project_id, "Test Project", "/test/path");
    create_test_session(&db, &session_id, &project_id);

    let repo = SqliteTaskProposalRepository::new(db.new_connection());
    let proposal = create_test_proposal(&session_id, "Duplicate");

    repo.create(proposal.clone()).await.unwrap();
    let result = repo.create(proposal).await;

    assert!(result.is_err());
}

// ==================== GET BY ID TESTS ====================

#[tokio::test]
async fn test_get_by_id_retrieves_proposal_correctly() {
    let db = setup_test_db();
    let project_id = ProjectId::new();
    let session_id = IdeationSessionId::new();
    create_test_project(&db, &project_id, "Test Project", "/test/path");
    create_test_session(&db, &session_id, &project_id);

    let repo = SqliteTaskProposalRepository::new(db.new_connection());
    let proposal = create_test_proposal(&session_id, "Get By ID Test");

    repo.create(proposal.clone()).await.unwrap();
    let result = repo.get_by_id(&proposal.id).await;

    assert!(result.is_ok());
    let found = result.unwrap();
    assert!(found.is_some());
    let found_proposal = found.unwrap();
    assert_eq!(found_proposal.id, proposal.id);
    assert_eq!(found_proposal.title, "Get By ID Test");
    assert_eq!(found_proposal.session_id, session_id);
}

#[tokio::test]
async fn test_get_by_id_returns_none_for_nonexistent() {
    let db = setup_test_db();
    let repo = SqliteTaskProposalRepository::new(db.new_connection());
    let id = TaskProposalId::new();

    let result = repo.get_by_id(&id).await;

    assert!(result.is_ok());
    assert!(result.unwrap().is_none());
}

#[tokio::test]
async fn test_get_by_id_preserves_all_fields() {
    let db = setup_test_db();
    let project_id = ProjectId::new();
    let session_id = IdeationSessionId::new();
    create_test_project(&db, &project_id, "Test Project", "/test/path");
    create_test_session(&db, &session_id, &project_id);

    let repo = SqliteTaskProposalRepository::new(db.new_connection());
    let mut proposal = create_test_proposal(&session_id, "Full Fields");
    proposal.description = Some("Description".to_string());
    proposal.steps = Some(r#"["step1"]"#.to_string());
    proposal.acceptance_criteria = Some(r#"["ac1"]"#.to_string());
    proposal.priority_reason = Some("Reason".to_string());
    proposal.estimated_complexity = Complexity::VeryComplex;
    proposal.user_priority = Some(Priority::High);
    proposal.user_modified = true;
    proposal.status = ProposalStatus::Modified;
    proposal.selected = false;
    proposal.sort_order = 5;

    repo.create(proposal.clone()).await.unwrap();
    let found = repo.get_by_id(&proposal.id).await.unwrap().unwrap();

    assert_eq!(found.id, proposal.id);
    assert_eq!(found.description, Some("Description".to_string()));
    assert_eq!(found.steps, Some(r#"["step1"]"#.to_string()));
    assert_eq!(found.acceptance_criteria, Some(r#"["ac1"]"#.to_string()));
    assert_eq!(found.priority_reason, Some("Reason".to_string()));
    assert_eq!(found.estimated_complexity, Complexity::VeryComplex);
    assert_eq!(found.user_priority, Some(Priority::High));
    assert!(found.user_modified);
    assert_eq!(found.status, ProposalStatus::Modified);
    assert!(!found.selected);
    assert_eq!(found.sort_order, 5);
}

// ==================== GET BY SESSION TESTS ====================

#[tokio::test]
async fn test_get_by_session_returns_all_proposals() {
    let db = setup_test_db();
    let project_id = ProjectId::new();
    let session_id = IdeationSessionId::new();
    create_test_project(&db, &project_id, "Test Project", "/test/path");
    create_test_session(&db, &session_id, &project_id);

    let repo = SqliteTaskProposalRepository::new(db.new_connection());

    let proposal1 = create_test_proposal(&session_id, "Proposal 1");
    let proposal2 = create_test_proposal(&session_id, "Proposal 2");
    let proposal3 = create_test_proposal(&session_id, "Proposal 3");

    repo.create(proposal1).await.unwrap();
    repo.create(proposal2).await.unwrap();
    repo.create(proposal3).await.unwrap();

    let result = repo.get_by_session(&session_id).await;

    assert!(result.is_ok());
    let proposals = result.unwrap();
    assert_eq!(proposals.len(), 3);
}

#[tokio::test]
async fn test_get_by_session_ordered_by_sort_order() {
    let db = setup_test_db();
    let project_id = ProjectId::new();
    let session_id = IdeationSessionId::new();
    create_test_project(&db, &project_id, "Test Project", "/test/path");
    create_test_session(&db, &session_id, &project_id);

    let repo = SqliteTaskProposalRepository::new(db.new_connection());

    let mut proposal1 = create_test_proposal(&session_id, "Third");
    proposal1.sort_order = 3;
    let mut proposal2 = create_test_proposal(&session_id, "First");
    proposal2.sort_order = 1;
    let mut proposal3 = create_test_proposal(&session_id, "Second");
    proposal3.sort_order = 2;

    // Insert in non-order
    repo.create(proposal1).await.unwrap();
    repo.create(proposal3).await.unwrap();
    repo.create(proposal2).await.unwrap();

    let proposals = repo.get_by_session(&session_id).await.unwrap();

    assert_eq!(proposals.len(), 3);
    assert_eq!(proposals[0].title, "First");
    assert_eq!(proposals[0].sort_order, 1);
    assert_eq!(proposals[1].title, "Second");
    assert_eq!(proposals[1].sort_order, 2);
    assert_eq!(proposals[2].title, "Third");
    assert_eq!(proposals[2].sort_order, 3);
}

#[tokio::test]
async fn test_get_by_session_returns_empty_for_no_proposals() {
    let db = setup_test_db();
    let project_id = ProjectId::new();
    let session_id = IdeationSessionId::new();
    create_test_project(&db, &project_id, "Test Project", "/test/path");
    create_test_session(&db, &session_id, &project_id);

    let repo = SqliteTaskProposalRepository::new(db.new_connection());

    let result = repo.get_by_session(&session_id).await;

    assert!(result.is_ok());
    assert!(result.unwrap().is_empty());
}

#[tokio::test]
async fn test_get_by_session_filters_by_session() {
    let db = setup_test_db();
    let project_id = ProjectId::new();
    let session_id1 = IdeationSessionId::new();
    let session_id2 = IdeationSessionId::new();
    create_test_project(&db, &project_id, "Test Project", "/test/path");
    create_test_session(&db, &session_id1, &project_id);
    create_test_session(&db, &session_id2, &project_id);

    let repo = SqliteTaskProposalRepository::new(db.new_connection());

    let proposal1 = create_test_proposal(&session_id1, "Session 1 Proposal");
    let proposal2 = create_test_proposal(&session_id2, "Session 2 Proposal");

    repo.create(proposal1).await.unwrap();
    repo.create(proposal2).await.unwrap();

    let proposals = repo.get_by_session(&session_id1).await.unwrap();

    assert_eq!(proposals.len(), 1);
    assert_eq!(proposals[0].session_id, session_id1);
}

// ==================== UPDATE TESTS ====================

#[tokio::test]
async fn test_update_modifies_proposal() {
    let db = setup_test_db();
    let project_id = ProjectId::new();
    let session_id = IdeationSessionId::new();
    create_test_project(&db, &project_id, "Test Project", "/test/path");
    create_test_session(&db, &session_id, &project_id);

    let repo = SqliteTaskProposalRepository::new(db.new_connection());
    let mut proposal = create_test_proposal(&session_id, "Original");

    repo.create(proposal.clone()).await.unwrap();

    proposal.title = "Updated Title".to_string();
    proposal.description = Some("Updated description".to_string());
    proposal.category = ProposalCategory::Fix;
    proposal.status = ProposalStatus::Accepted;

    repo.update(&proposal).await.unwrap();

    let found = repo.get_by_id(&proposal.id).await.unwrap().unwrap();
    assert_eq!(found.title, "Updated Title");
    assert_eq!(found.description, Some("Updated description".to_string()));
    assert_eq!(found.category, ProposalCategory::Fix);
    assert_eq!(found.status, ProposalStatus::Accepted);
}

#[tokio::test]
async fn test_update_updates_updated_at() {
    let db = setup_test_db();
    let project_id = ProjectId::new();
    let session_id = IdeationSessionId::new();
    create_test_project(&db, &project_id, "Test Project", "/test/path");
    create_test_session(&db, &session_id, &project_id);

    let repo = SqliteTaskProposalRepository::new(db.new_connection());
    let proposal = create_test_proposal(&session_id, "Timestamp Test");
    let original_updated = proposal.updated_at;

    repo.create(proposal.clone()).await.unwrap();

    tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;

    let mut updated_proposal = proposal.clone();
    updated_proposal.title = "Changed".to_string();
    repo.update(&updated_proposal).await.unwrap();

    let found = repo.get_by_id(&proposal.id).await.unwrap().unwrap();
    assert!(found.updated_at >= original_updated);
}

// ==================== UPDATE PRIORITY TESTS ====================

#[tokio::test]
async fn test_update_priority_sets_assessment_fields() {
    let db = setup_test_db();
    let project_id = ProjectId::new();
    let session_id = IdeationSessionId::new();
    create_test_project(&db, &project_id, "Test Project", "/test/path");
    create_test_session(&db, &session_id, &project_id);

    let repo = SqliteTaskProposalRepository::new(db.new_connection());
    let proposal = create_test_proposal(&session_id, "Priority Update");

    repo.create(proposal.clone()).await.unwrap();

    let assessment = create_test_assessment(&proposal.id);
    repo.update_priority(&proposal.id, &assessment).await.unwrap();

    let found = repo.get_by_id(&proposal.id).await.unwrap().unwrap();
    assert_eq!(found.suggested_priority, assessment.suggested_priority);
    assert_eq!(found.priority_score, assessment.priority_score);
    assert_eq!(found.priority_reason, Some(assessment.priority_reason.clone()));
}

#[tokio::test]
async fn test_update_priority_stores_factors_as_json() {
    let db = setup_test_db();
    let project_id = ProjectId::new();
    let session_id = IdeationSessionId::new();
    create_test_project(&db, &project_id, "Test Project", "/test/path");
    create_test_session(&db, &session_id, &project_id);

    let repo = SqliteTaskProposalRepository::new(db.new_connection());
    let proposal = create_test_proposal(&session_id, "Factors JSON");

    repo.create(proposal.clone()).await.unwrap();

    let assessment = create_test_assessment(&proposal.id);
    repo.update_priority(&proposal.id, &assessment).await.unwrap();

    let found = repo.get_by_id(&proposal.id).await.unwrap().unwrap();
    assert_eq!(found.priority_reason, Some(assessment.priority_reason.clone()));
    assert_eq!(found.suggested_priority, assessment.suggested_priority);
    assert_eq!(found.priority_score, assessment.priority_score);
}

// ==================== UPDATE SELECTION TESTS ====================

#[tokio::test]
async fn test_update_selection_toggles_selected() {
    let db = setup_test_db();
    let project_id = ProjectId::new();
    let session_id = IdeationSessionId::new();
    create_test_project(&db, &project_id, "Test Project", "/test/path");
    create_test_session(&db, &session_id, &project_id);

    let repo = SqliteTaskProposalRepository::new(db.new_connection());
    let proposal = create_test_proposal(&session_id, "Selection Test");

    repo.create(proposal.clone()).await.unwrap();
    let found = repo.get_by_id(&proposal.id).await.unwrap().unwrap();
    assert!(!found.selected);

    // Select
    repo.update_selection(&proposal.id, true).await.unwrap();
    let found = repo.get_by_id(&proposal.id).await.unwrap().unwrap();
    assert!(found.selected);

    // Deselect again
    repo.update_selection(&proposal.id, false).await.unwrap();
    let found = repo.get_by_id(&proposal.id).await.unwrap().unwrap();
    assert!(!found.selected);
}

#[tokio::test]
async fn test_update_selection_updates_timestamp() {
    let db = setup_test_db();
    let project_id = ProjectId::new();
    let session_id = IdeationSessionId::new();
    create_test_project(&db, &project_id, "Test Project", "/test/path");
    create_test_session(&db, &session_id, &project_id);

    let repo = SqliteTaskProposalRepository::new(db.new_connection());
    let proposal = create_test_proposal(&session_id, "Selection Timestamp");
    let original = proposal.updated_at;

    repo.create(proposal.clone()).await.unwrap();

    tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;

    repo.update_selection(&proposal.id, false).await.unwrap();

    let found = repo.get_by_id(&proposal.id).await.unwrap().unwrap();
    assert!(found.updated_at >= original);
}

// ==================== SET CREATED TASK ID TESTS ====================

fn create_test_task(db: &SqliteTestDb, task_id: &TaskId, project_id: &ProjectId, title: &str) {
    db.with_connection(|conn| {
        conn.execute(
            "INSERT INTO tasks (id, project_id, title, category, internal_status, created_at, updated_at)
             VALUES (?1, ?2, ?3, 'feature', 'Ready', strftime('%Y-%m-%dT%H:%M:%S+00:00', 'now'), strftime('%Y-%m-%dT%H:%M:%S+00:00', 'now'))",
            rusqlite::params![task_id.as_str(), project_id.as_str(), title],
        )
        .unwrap();
    });
}

#[tokio::test]
async fn test_set_created_task_id_links_proposal_to_task() {
    let db = setup_test_db();
    let project_id = ProjectId::new();
    let session_id = IdeationSessionId::new();
    let task_id = TaskId::new();
    create_test_project(&db, &project_id, "Test Project", "/test/path");
    create_test_session(&db, &session_id, &project_id);
    create_test_task(&db, &task_id, &project_id, "Created Task");

    let repo = SqliteTaskProposalRepository::new(db.new_connection());
    let proposal = create_test_proposal(&session_id, "Link Task");

    repo.create(proposal.clone()).await.unwrap();

    repo.set_created_task_id(&proposal.id, &task_id).await.unwrap();

    let found = repo.get_by_id(&proposal.id).await.unwrap().unwrap();
    assert_eq!(found.created_task_id, Some(task_id));
}

#[tokio::test]
async fn test_set_created_task_id_updates_timestamp() {
    let db = setup_test_db();
    let project_id = ProjectId::new();
    let session_id = IdeationSessionId::new();
    let task_id = TaskId::new();
    create_test_project(&db, &project_id, "Test Project", "/test/path");
    create_test_session(&db, &session_id, &project_id);
    create_test_task(&db, &task_id, &project_id, "Timestamp Task");

    let repo = SqliteTaskProposalRepository::new(db.new_connection());
    let proposal = create_test_proposal(&session_id, "Task Link Timestamp");
    let original = proposal.updated_at;

    repo.create(proposal.clone()).await.unwrap();

    tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;

    repo.set_created_task_id(&proposal.id, &task_id).await.unwrap();

    let found = repo.get_by_id(&proposal.id).await.unwrap().unwrap();
    assert!(found.updated_at >= original);
}

// ==================== DELETE TESTS ====================

#[tokio::test]
async fn test_delete_removes_proposal_from_database() {
    let db = setup_test_db();
    let project_id = ProjectId::new();
    let session_id = IdeationSessionId::new();
    create_test_project(&db, &project_id, "Test Project", "/test/path");
    create_test_session(&db, &session_id, &project_id);

    let repo = SqliteTaskProposalRepository::new(db.new_connection());
    let proposal = create_test_proposal(&session_id, "To Delete");

    repo.create(proposal.clone()).await.unwrap();

    let delete_result = repo.delete(&proposal.id).await;
    assert!(delete_result.is_ok());

    let found = repo.get_by_id(&proposal.id).await.unwrap();
    assert!(found.is_none());
}

#[tokio::test]
async fn test_delete_nonexistent_succeeds() {
    let db = setup_test_db();
    let repo = SqliteTaskProposalRepository::new(db.new_connection());
    let id = TaskProposalId::new();

    let result = repo.delete(&id).await;
    assert!(result.is_ok());
}

// ==================== REORDER TESTS ====================

#[tokio::test]
async fn test_reorder_updates_sort_order() {
    let db = setup_test_db();
    let project_id = ProjectId::new();
    let session_id = IdeationSessionId::new();
    create_test_project(&db, &project_id, "Test Project", "/test/path");
    create_test_session(&db, &session_id, &project_id);

    let repo = SqliteTaskProposalRepository::new(db.new_connection());

    let mut proposal1 = create_test_proposal(&session_id, "First");
    proposal1.sort_order = 1;
    let mut proposal2 = create_test_proposal(&session_id, "Second");
    proposal2.sort_order = 2;
    let mut proposal3 = create_test_proposal(&session_id, "Third");
    proposal3.sort_order = 3;

    repo.create(proposal1.clone()).await.unwrap();
    repo.create(proposal2.clone()).await.unwrap();
    repo.create(proposal3.clone()).await.unwrap();

    // Reorder: move third to first position
    let new_order = vec![proposal3.id.clone(), proposal1.id.clone(), proposal2.id.clone()];
    repo.reorder(&session_id, new_order).await.unwrap();

    let proposals = repo.get_by_session(&session_id).await.unwrap();
    assert_eq!(proposals[0].title, "Third");
    assert_eq!(proposals[0].sort_order, 0);
    assert_eq!(proposals[1].title, "First");
    assert_eq!(proposals[1].sort_order, 1);
    assert_eq!(proposals[2].title, "Second");
    assert_eq!(proposals[2].sort_order, 2);
}

#[tokio::test]
async fn test_reorder_only_affects_specified_session() {
    let db = setup_test_db();
    let project_id = ProjectId::new();
    let session_id1 = IdeationSessionId::new();
    let session_id2 = IdeationSessionId::new();
    create_test_project(&db, &project_id, "Test Project", "/test/path");
    create_test_session(&db, &session_id1, &project_id);
    create_test_session(&db, &session_id2, &project_id);

    let repo = SqliteTaskProposalRepository::new(db.new_connection());

    let mut proposal1 = create_test_proposal(&session_id1, "Session1");
    proposal1.sort_order = 1;
    let mut proposal2 = create_test_proposal(&session_id2, "Session2");
    proposal2.sort_order = 1;

    repo.create(proposal1.clone()).await.unwrap();
    repo.create(proposal2.clone()).await.unwrap();

    // Reorder session1 only
    repo.reorder(&session_id1, vec![proposal1.id.clone()]).await.unwrap();

    // Session 2 should be unaffected
    let found = repo.get_by_id(&proposal2.id).await.unwrap().unwrap();
    assert_eq!(found.sort_order, 1);
}

// ==================== GET SELECTED BY SESSION TESTS ====================

#[tokio::test]
async fn test_get_selected_by_session_returns_only_selected() {
    let db = setup_test_db();
    let project_id = ProjectId::new();
    let session_id = IdeationSessionId::new();
    create_test_project(&db, &project_id, "Test Project", "/test/path");
    create_test_session(&db, &session_id, &project_id);

    let repo = SqliteTaskProposalRepository::new(db.new_connection());

    let mut selected = create_test_proposal(&session_id, "Selected");
    selected.selected = true;
    let mut unselected = create_test_proposal(&session_id, "Unselected");
    unselected.selected = false;

    repo.create(selected.clone()).await.unwrap();
    repo.create(unselected.clone()).await.unwrap();

    let result = repo.get_selected_by_session(&session_id).await;

    assert!(result.is_ok());
    let proposals = result.unwrap();
    assert_eq!(proposals.len(), 1);
    assert_eq!(proposals[0].id, selected.id);
    assert!(proposals[0].selected);
}

#[tokio::test]
async fn test_get_selected_by_session_returns_empty_when_none_selected() {
    let db = setup_test_db();
    let project_id = ProjectId::new();
    let session_id = IdeationSessionId::new();
    create_test_project(&db, &project_id, "Test Project", "/test/path");
    create_test_session(&db, &session_id, &project_id);

    let repo = SqliteTaskProposalRepository::new(db.new_connection());

    let mut unselected = create_test_proposal(&session_id, "Unselected");
    unselected.selected = false;

    repo.create(unselected).await.unwrap();

    let result = repo.get_selected_by_session(&session_id).await;

    assert!(result.is_ok());
    assert!(result.unwrap().is_empty());
}

#[tokio::test]
async fn test_get_selected_by_session_ordered_by_sort_order() {
    let db = setup_test_db();
    let project_id = ProjectId::new();
    let session_id = IdeationSessionId::new();
    create_test_project(&db, &project_id, "Test Project", "/test/path");
    create_test_session(&db, &session_id, &project_id);

    let repo = SqliteTaskProposalRepository::new(db.new_connection());

    let mut proposal1 = create_test_proposal(&session_id, "Third");
    proposal1.sort_order = 3;
    proposal1.selected = true;
    let mut proposal2 = create_test_proposal(&session_id, "First");
    proposal2.sort_order = 1;
    proposal2.selected = true;

    repo.create(proposal1).await.unwrap();
    repo.create(proposal2).await.unwrap();

    let proposals = repo.get_selected_by_session(&session_id).await.unwrap();

    assert_eq!(proposals.len(), 2);
    assert_eq!(proposals[0].title, "First");
    assert_eq!(proposals[1].title, "Third");
}

// ==================== COUNT TESTS ====================

#[tokio::test]
async fn test_count_by_session_returns_zero_for_no_proposals() {
    let db = setup_test_db();
    let project_id = ProjectId::new();
    let session_id = IdeationSessionId::new();
    create_test_project(&db, &project_id, "Test Project", "/test/path");
    create_test_session(&db, &session_id, &project_id);

    let repo = SqliteTaskProposalRepository::new(db.new_connection());

    let result = repo.count_by_session(&session_id).await;

    assert!(result.is_ok());
    assert_eq!(result.unwrap(), 0);
}

#[tokio::test]
async fn test_count_by_session_counts_correctly() {
    let db = setup_test_db();
    let project_id = ProjectId::new();
    let session_id = IdeationSessionId::new();
    create_test_project(&db, &project_id, "Test Project", "/test/path");
    create_test_session(&db, &session_id, &project_id);

    let repo = SqliteTaskProposalRepository::new(db.new_connection());

    let proposal1 = create_test_proposal(&session_id, "One");
    let proposal2 = create_test_proposal(&session_id, "Two");
    let proposal3 = create_test_proposal(&session_id, "Three");

    repo.create(proposal1).await.unwrap();
    repo.create(proposal2).await.unwrap();
    repo.create(proposal3).await.unwrap();

    let count = repo.count_by_session(&session_id).await.unwrap();
    assert_eq!(count, 3);
}

#[tokio::test]
async fn test_count_by_session_filters_by_session() {
    let db = setup_test_db();
    let project_id = ProjectId::new();
    let session_id1 = IdeationSessionId::new();
    let session_id2 = IdeationSessionId::new();
    create_test_project(&db, &project_id, "Test Project", "/test/path");
    create_test_session(&db, &session_id1, &project_id);
    create_test_session(&db, &session_id2, &project_id);

    let repo = SqliteTaskProposalRepository::new(db.new_connection());

    let proposal1 = create_test_proposal(&session_id1, "Session 1");
    let proposal2 = create_test_proposal(&session_id2, "Session 2 A");
    let proposal3 = create_test_proposal(&session_id2, "Session 2 B");

    repo.create(proposal1).await.unwrap();
    repo.create(proposal2).await.unwrap();
    repo.create(proposal3).await.unwrap();

    let count1 = repo.count_by_session(&session_id1).await.unwrap();
    let count2 = repo.count_by_session(&session_id2).await.unwrap();

    assert_eq!(count1, 1);
    assert_eq!(count2, 2);
}

#[tokio::test]
async fn test_count_selected_by_session_counts_correctly() {
    let db = setup_test_db();
    let project_id = ProjectId::new();
    let session_id = IdeationSessionId::new();
    create_test_project(&db, &project_id, "Test Project", "/test/path");
    create_test_session(&db, &session_id, &project_id);

    let repo = SqliteTaskProposalRepository::new(db.new_connection());

    let mut selected1 = create_test_proposal(&session_id, "Selected 1");
    selected1.selected = true;
    let mut selected2 = create_test_proposal(&session_id, "Selected 2");
    selected2.selected = true;
    let mut unselected = create_test_proposal(&session_id, "Unselected");
    unselected.selected = false;

    repo.create(selected1).await.unwrap();
    repo.create(selected2).await.unwrap();
    repo.create(unselected).await.unwrap();

    let total_count = repo.count_by_session(&session_id).await.unwrap();
    let selected_count = repo.count_selected_by_session(&session_id).await.unwrap();

    assert_eq!(total_count, 3);
    assert_eq!(selected_count, 2);
}

// ==================== SHARED CONNECTION TESTS ====================

#[tokio::test]
async fn test_from_shared_works_correctly() {
    let db = setup_test_db();
    let project_id = ProjectId::new();
    let session_id = IdeationSessionId::new();
    create_test_project(&db, &project_id, "Test Project", "/test/path");
    create_test_session(&db, &session_id, &project_id);

    let repo = SqliteTaskProposalRepository::from_shared(db.shared_conn());

    let proposal = create_test_proposal(&session_id, "Shared Connection");

    let result = repo.create(proposal.clone()).await;
    assert!(result.is_ok());

    let found = repo.get_by_id(&proposal.id).await.unwrap();
    assert!(found.is_some());
}

// ==================== PRIORITY FACTORS JSON TESTS ====================

#[tokio::test]
async fn test_create_with_priority_factors() {
    let db = setup_test_db();
    let project_id = ProjectId::new();
    let session_id = IdeationSessionId::new();
    create_test_project(&db, &project_id, "Test Project", "/test/path");
    create_test_session(&db, &session_id, &project_id);

    let repo = SqliteTaskProposalRepository::new(db.new_connection());
    let mut proposal = create_test_proposal(&session_id, "With Factors");
    proposal.priority_factors = Some(crate::domain::entities::PriorityFactors {
        dependency: 10,
        business_value: 20,
        technical_risk: 5,
        user_demand: 15,
    });

    repo.create(proposal.clone()).await.unwrap();

    let found = repo.get_by_id(&proposal.id).await.unwrap().unwrap();
    assert!(found.priority_factors.is_some());
    let factors = found.priority_factors.unwrap();
    assert_eq!(factors.dependency, 10);
    assert_eq!(factors.business_value, 20);
}

// ==================== GET_BY_PLAN_ARTIFACT_ID TESTS ====================

#[tokio::test]
async fn test_get_by_plan_artifact_id_returns_matching_proposals() {
    let db = setup_test_db();
    let project_id = ProjectId::new();
    let session_id = IdeationSessionId::new();
    create_test_project(&db, &project_id, "Test Project", "/test/path");
    create_test_session(&db, &session_id, &project_id);

    let repo = SqliteTaskProposalRepository::new(db.new_connection());

    let mut p1 = create_test_proposal(&session_id, "Proposal With Artifact");
    p1.plan_artifact_id = Some(ArtifactId::from_string("plan-artifact-1"));
    let mut p2 = create_test_proposal(&session_id, "Proposal Same Artifact");
    p2.plan_artifact_id = Some(ArtifactId::from_string("plan-artifact-1"));
    let p3 = create_test_proposal(&session_id, "Proposal No Artifact");

    repo.create(p1.clone()).await.unwrap();
    repo.create(p2.clone()).await.unwrap();
    repo.create(p3).await.unwrap();

    let results = repo
        .get_by_plan_artifact_id(&ArtifactId::from_string("plan-artifact-1"))
        .await
        .unwrap();

    assert_eq!(results.len(), 2);
    let ids: Vec<_> = results.iter().map(|p| &p.id).collect();
    assert!(ids.contains(&&p1.id));
    assert!(ids.contains(&&p2.id));
}

#[tokio::test]
async fn test_get_by_plan_artifact_id_returns_empty_when_no_match() {
    let db = setup_test_db();
    let project_id = ProjectId::new();
    let session_id = IdeationSessionId::new();
    create_test_project(&db, &project_id, "Test Project", "/test/path");
    create_test_session(&db, &session_id, &project_id);

    let repo = SqliteTaskProposalRepository::new(db.new_connection());
    let p = create_test_proposal(&session_id, "No artifact");
    repo.create(p).await.unwrap();

    let results = repo
        .get_by_plan_artifact_id(&ArtifactId::from_string("nonexistent-artifact"))
        .await
        .unwrap();
    assert!(results.is_empty());
}

// ==================== CLEAR_CREATED_TASK_IDS_BY_SESSION TESTS ====================

#[tokio::test]
async fn test_clear_created_task_ids_by_session_nullifies_all_in_session() {
    let db = setup_test_db();
    let project_id = ProjectId::new();
    let session_id = IdeationSessionId::new();
    create_test_project(&db, &project_id, "Test Project", "/test/path");
    create_test_session(&db, &session_id, &project_id);

    let task_id = TaskId::new();
    create_test_task(&db, &task_id, &project_id, "FK Task");

    let repo = SqliteTaskProposalRepository::new(db.new_connection());

    let p1 = create_test_proposal(&session_id, "Proposal 1");
    let p2 = create_test_proposal(&session_id, "Proposal 2");
    repo.create(p1.clone()).await.unwrap();
    repo.create(p2.clone()).await.unwrap();

    // Link proposals to tasks
    repo.set_created_task_id(&p1.id, &task_id).await.unwrap();
    repo.set_created_task_id(&p2.id, &task_id).await.unwrap();

    // Verify they're set
    let found_p1 = repo.get_by_id(&p1.id).await.unwrap().unwrap();
    assert!(found_p1.created_task_id.is_some());

    // Clear them all
    repo.clear_created_task_ids_by_session(&session_id)
        .await
        .unwrap();

    let cleared_p1 = repo.get_by_id(&p1.id).await.unwrap().unwrap();
    let cleared_p2 = repo.get_by_id(&p2.id).await.unwrap().unwrap();
    assert!(cleared_p1.created_task_id.is_none());
    assert!(cleared_p2.created_task_id.is_none());
}

// ==================== SYNC HELPER TESTS ====================

#[test]
fn test_create_sync_inserts_and_returns_proposal() {
    let db = setup_test_db();
    let project_id = ProjectId::new();
    let session_id = IdeationSessionId::new();
    create_test_project(&db, &project_id, "Test Project", "/test/path");
    create_test_session(&db, &session_id, &project_id);

    let proposal = create_test_proposal(&session_id, "Sync Create");
    let result = db.with_connection(|conn| {
        SqliteTaskProposalRepository::create_sync(conn, proposal.clone())
    });

    assert!(result.is_ok());
    let created = result.unwrap();
    assert_eq!(created.id, proposal.id);
    assert_eq!(created.title, "Sync Create");
}

#[test]
fn test_count_by_session_sync_returns_correct_count() {
    let db = setup_test_db();
    let project_id = ProjectId::new();
    let session_id = IdeationSessionId::new();
    create_test_project(&db, &project_id, "Test Project", "/test/path");
    create_test_session(&db, &session_id, &project_id);

    let p1 = create_test_proposal(&session_id, "One");
    let p2 = create_test_proposal(&session_id, "Two");
    db.with_connection(|conn| {
        SqliteTaskProposalRepository::create_sync(conn, p1).unwrap();
        SqliteTaskProposalRepository::create_sync(conn, p2).unwrap();
    });

    let count = db
        .with_connection(|conn| SqliteTaskProposalRepository::count_by_session_sync(conn, session_id.as_str()));
    assert!(count.is_ok());
    assert_eq!(count.unwrap(), 2);
}

#[test]
fn test_count_by_session_sync_returns_zero_when_empty() {
    let db = setup_test_db();
    let project_id = ProjectId::new();
    let session_id = IdeationSessionId::new();
    create_test_project(&db, &project_id, "Test Project", "/test/path");
    create_test_session(&db, &session_id, &project_id);

    let count = db
        .with_connection(|conn| SqliteTaskProposalRepository::count_by_session_sync(conn, session_id.as_str()));
    assert!(count.is_ok());
    assert_eq!(count.unwrap(), 0);
}

#[test]
fn test_update_sync_modifies_proposal_and_returns_updated() {
    let db = setup_test_db();
    let project_id = ProjectId::new();
    let session_id = IdeationSessionId::new();
    create_test_project(&db, &project_id, "Test Project", "/test/path");
    create_test_session(&db, &session_id, &project_id);

    let proposal = create_test_proposal(&session_id, "Before Update");
    db.with_connection(|conn| {
        SqliteTaskProposalRepository::create_sync(conn, proposal.clone()).unwrap();
    });

    let mut updated = proposal.clone();
    updated.title = "After Update".to_string();
    let result = db.with_connection(|conn| SqliteTaskProposalRepository::update_sync(conn, &updated));

    assert!(result.is_ok());
    let returned = result.unwrap();
    assert_eq!(returned.title, "After Update");
    // updated_at should be refreshed by update_sync
    assert!(returned.updated_at >= proposal.updated_at);
}

#[test]
fn test_delete_sync_removes_proposal_scoped_to_session() {
    let db = setup_test_db();
    let project_id = ProjectId::new();
    let session_id = IdeationSessionId::new();
    create_test_project(&db, &project_id, "Test Project", "/test/path");
    create_test_session(&db, &session_id, &project_id);

    let proposal = create_test_proposal(&session_id, "To Delete Sync");
    db.with_connection(|conn| {
        SqliteTaskProposalRepository::create_sync(conn, proposal.clone()).unwrap();
    });

    let result = db.with_connection(|conn| {
        SqliteTaskProposalRepository::delete_sync(conn, proposal.id.as_str(), session_id.as_str())
    });
    assert!(result.is_ok());

    // Verify gone
    let count = db.with_connection(|conn| {
        SqliteTaskProposalRepository::count_by_session_sync(conn, session_id.as_str()).unwrap()
    });
    assert_eq!(count, 0);
}

#[test]
fn test_delete_sync_with_wrong_session_is_noop() {
    let db = setup_test_db();
    let project_id = ProjectId::new();
    let session_id = IdeationSessionId::new();
    let other_session = IdeationSessionId::new();
    create_test_project(&db, &project_id, "Test Project", "/test/path");
    create_test_session(&db, &session_id, &project_id);

    let proposal = create_test_proposal(&session_id, "Wrong Session Delete");
    db.with_connection(|conn| {
        SqliteTaskProposalRepository::create_sync(conn, proposal.clone()).unwrap();
    });

    // Delete with wrong session_id — should not remove the row
    let result = db.with_connection(|conn| {
        SqliteTaskProposalRepository::delete_sync(conn, proposal.id.as_str(), other_session.as_str())
    });
    assert!(result.is_ok()); // no error, just no-op

    // Row still exists
    let count = db.with_connection(|conn| {
        SqliteTaskProposalRepository::count_by_session_sync(conn, session_id.as_str()).unwrap()
    });
    assert_eq!(count, 1);
}

#[tokio::test]
async fn test_clear_created_task_ids_by_session_only_affects_target_session() {
    let db = setup_test_db();
    let project_id = ProjectId::new();
    let session_a = IdeationSessionId::new();
    let session_b = IdeationSessionId::new();
    create_test_project(&db, &project_id, "Test Project", "/test/path");
    create_test_session(&db, &session_a, &project_id);
    create_test_session(&db, &session_b, &project_id);

    let task_id = TaskId::new();
    create_test_task(&db, &task_id, &project_id, "FK Task");

    let repo = SqliteTaskProposalRepository::new(db.new_connection());

    let p_a = create_test_proposal(&session_a, "Proposal A");
    let p_b = create_test_proposal(&session_b, "Proposal B");
    repo.create(p_a.clone()).await.unwrap();
    repo.create(p_b.clone()).await.unwrap();

    repo.set_created_task_id(&p_a.id, &task_id).await.unwrap();
    repo.set_created_task_id(&p_b.id, &task_id).await.unwrap();

    // Clear only session A
    repo.clear_created_task_ids_by_session(&session_a)
        .await
        .unwrap();

    let cleared_a = repo.get_by_id(&p_a.id).await.unwrap().unwrap();
    let untouched_b = repo.get_by_id(&p_b.id).await.unwrap().unwrap();
    assert!(cleared_a.created_task_id.is_none());
    assert!(untouched_b.created_task_id.is_some()); // session B unaffected
}
