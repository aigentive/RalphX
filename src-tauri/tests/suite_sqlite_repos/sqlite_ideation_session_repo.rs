use ralphx_lib::domain::entities::ideation::{SessionOrigin, SessionPurpose};
use ralphx_lib::domain::entities::{
    ArtifactId, IdeationAnalysisBaseRefKind, IdeationAnalysisState, IdeationAnalysisWorkspaceKind,
    IdeationSession, IdeationSessionId, IdeationSessionStatus, ProjectId, VerificationGap,
    VerificationRoundSnapshot, VerificationRunSnapshot, VerificationStatus,
};
use ralphx_lib::domain::repositories::IdeationSessionRepository;
use ralphx_lib::infrastructure::sqlite::SqliteIdeationSessionRepository;
use ralphx_lib::testing::SqliteTestDb;
use rusqlite::Connection;

fn setup_test_db() -> SqliteTestDb {
    SqliteTestDb::new("sqlite-ideation-session-repo")
}

fn insert_test_project(conn: &Connection, id: &ProjectId, name: &str, path: &str) {
    conn.execute(
        "INSERT INTO projects (id, name, working_directory, git_mode, created_at, updated_at)
         VALUES (?1, ?2, ?3, 'single_branch', strftime('%Y-%m-%dT%H:%M:%S+00:00', 'now'), strftime('%Y-%m-%dT%H:%M:%S+00:00', 'now'))",
        rusqlite::params![id.as_str(), name, path],
    )
    .unwrap();
}

fn create_test_project(db: &SqliteTestDb, id: &ProjectId, name: &str, path: &str) {
    db.with_connection(|conn| insert_test_project(conn, id, name, path));
}

fn create_test_session(project_id: &ProjectId, title: Option<&str>) -> IdeationSession {
    let mut builder = IdeationSession::builder().project_id(project_id.clone());

    if let Some(t) = title {
        builder = builder.title(t);
    }

    builder.build()
}

// ==================== CREATE TESTS ====================

#[tokio::test]
async fn test_create_inserts_session_and_returns_it() {
    let db = setup_test_db();
    let project_id = ProjectId::new();
    create_test_project(&db, &project_id, "Test Project", "/test/path");

    let repo = SqliteIdeationSessionRepository::new(db.new_connection());
    let session = create_test_session(&project_id, Some("My Ideation"));

    let result = repo.create(session.clone()).await;

    assert!(result.is_ok());
    let created = result.unwrap();
    assert_eq!(created.id, session.id);
    assert_eq!(created.title, Some("My Ideation".to_string()));
    assert_eq!(created.status, IdeationSessionStatus::Active);
}

#[tokio::test]
async fn test_create_session_without_title() {
    let db = setup_test_db();
    let project_id = ProjectId::new();
    create_test_project(&db, &project_id, "Test Project", "/test/path");

    let repo = SqliteIdeationSessionRepository::new(db.new_connection());
    let session = create_test_session(&project_id, None);

    let result = repo.create(session.clone()).await;

    assert!(result.is_ok());
    let created = result.unwrap();
    assert_eq!(created.title, None);
}

#[tokio::test]
async fn test_create_duplicate_id_fails() {
    let db = setup_test_db();
    let project_id = ProjectId::new();
    create_test_project(&db, &project_id, "Test Project", "/test/path");

    let repo = SqliteIdeationSessionRepository::new(db.new_connection());
    let session = create_test_session(&project_id, Some("Original"));

    repo.create(session.clone()).await.unwrap();
    let result = repo.create(session).await;

    assert!(result.is_err());
}

// ==================== GET BY ID TESTS ====================

#[tokio::test]
async fn test_get_by_id_retrieves_session_correctly() {
    let db = setup_test_db();
    let project_id = ProjectId::new();
    create_test_project(&db, &project_id, "Test Project", "/test/path");

    let repo = SqliteIdeationSessionRepository::new(db.new_connection());
    let session = create_test_session(&project_id, Some("Test Session"));

    repo.create(session.clone()).await.unwrap();
    let result = repo.get_by_id(&session.id).await;

    assert!(result.is_ok());
    let found = result.unwrap();
    assert!(found.is_some());
    let found_session = found.unwrap();
    assert_eq!(found_session.id, session.id);
    assert_eq!(found_session.title, Some("Test Session".to_string()));
    assert_eq!(found_session.project_id, project_id);
}

#[tokio::test]
async fn test_get_by_id_returns_none_for_nonexistent() {
    let db = setup_test_db();
    let repo = SqliteIdeationSessionRepository::new(db.new_connection());
    let id = IdeationSessionId::new();

    let result = repo.get_by_id(&id).await;

    assert!(result.is_ok());
    assert!(result.unwrap().is_none());
}

#[tokio::test]
async fn test_get_by_id_preserves_all_fields() {
    let db = setup_test_db();
    let project_id = ProjectId::new();
    create_test_project(&db, &project_id, "Test Project", "/test/path");

    let repo = SqliteIdeationSessionRepository::new(db.new_connection());

    // Create a session with all fields set
    let mut session = create_test_session(&project_id, Some("Full Session"));
    session.archive(); // This sets archived_at

    repo.create(session.clone()).await.unwrap();
    let found = repo.get_by_id(&session.id).await.unwrap().unwrap();

    assert_eq!(found.id, session.id);
    assert_eq!(found.project_id, session.project_id);
    assert_eq!(found.title, session.title);
    assert_eq!(found.status, IdeationSessionStatus::Archived);
    assert!(found.archived_at.is_some());
}

#[tokio::test]
async fn test_create_preserves_analysis_base_metadata() {
    let db = setup_test_db();
    let project_id = ProjectId::new();
    create_test_project(&db, &project_id, "Test Project", "/test/path");

    let repo = SqliteIdeationSessionRepository::new(db.new_connection());
    let mut session = create_test_session(&project_id, Some("Base-aware Session"));
    session.analysis = IdeationAnalysisState {
        base_ref_kind: Some(IdeationAnalysisBaseRefKind::LocalBranch),
        base_ref: Some("feature/analysis".to_string()),
        base_display_name: Some("feature/analysis".to_string()),
        workspace_kind: IdeationAnalysisWorkspaceKind::IdeationWorktree,
        workspace_path: Some("/tmp/ralphx/ideation-session".to_string()),
        base_commit: Some("0123456789abcdef".to_string()),
        base_locked_at: Some(chrono::Utc::now()),
    };

    repo.create(session.clone()).await.unwrap();
    let found = repo.get_by_id(&session.id).await.unwrap().unwrap();

    assert_eq!(found.analysis, session.analysis);
}

// ==================== GET BY PROJECT TESTS ====================

#[tokio::test]
async fn test_get_by_project_returns_all_sessions() {
    let db = setup_test_db();
    let project_id = ProjectId::new();
    create_test_project(&db, &project_id, "Test Project", "/test/path");

    let repo = SqliteIdeationSessionRepository::new(db.new_connection());

    let session1 = create_test_session(&project_id, Some("Session 1"));
    let session2 = create_test_session(&project_id, Some("Session 2"));
    let session3 = create_test_session(&project_id, Some("Session 3"));

    repo.create(session1).await.unwrap();
    repo.create(session2).await.unwrap();
    repo.create(session3).await.unwrap();

    let result = repo.get_by_project(&project_id).await;

    assert!(result.is_ok());
    let sessions = result.unwrap();
    assert_eq!(sessions.len(), 3);
}

#[tokio::test]
async fn test_get_by_project_ordered_by_updated_at_desc() {
    let db = setup_test_db();
    let project_id = ProjectId::new();
    create_test_project(&db, &project_id, "Test Project", "/test/path");

    let repo = SqliteIdeationSessionRepository::new(db.new_connection());

    // Create sessions with different timestamps
    let session1 = IdeationSession::builder()
        .project_id(project_id.clone())
        .title("Oldest")
        .created_at(chrono::Utc::now() - chrono::Duration::hours(3))
        .updated_at(chrono::Utc::now() - chrono::Duration::hours(3))
        .build();
    let session2 = IdeationSession::builder()
        .project_id(project_id.clone())
        .title("Middle")
        .created_at(chrono::Utc::now() - chrono::Duration::hours(2))
        .updated_at(chrono::Utc::now() - chrono::Duration::hours(2))
        .build();
    let session3 = IdeationSession::builder()
        .project_id(project_id.clone())
        .title("Newest")
        .created_at(chrono::Utc::now() - chrono::Duration::hours(1))
        .updated_at(chrono::Utc::now() - chrono::Duration::hours(1))
        .build();

    // Insert in non-order (oldest first, then newest, then middle)
    repo.create(session1).await.unwrap();
    repo.create(session3).await.unwrap();
    repo.create(session2).await.unwrap();

    let sessions = repo.get_by_project(&project_id).await.unwrap();

    // Should be ordered newest first
    assert_eq!(sessions.len(), 3);
    assert_eq!(sessions[0].title, Some("Newest".to_string()));
    assert_eq!(sessions[1].title, Some("Middle".to_string()));
    assert_eq!(sessions[2].title, Some("Oldest".to_string()));
}

#[tokio::test]
async fn test_get_by_project_returns_empty_for_no_sessions() {
    let db = setup_test_db();
    let project_id = ProjectId::new();
    create_test_project(&db, &project_id, "Test Project", "/test/path");

    let repo = SqliteIdeationSessionRepository::new(db.new_connection());

    let result = repo.get_by_project(&project_id).await;

    assert!(result.is_ok());
    assert!(result.unwrap().is_empty());
}

#[tokio::test]
async fn test_get_by_project_filters_by_project() {
    let db = setup_test_db();
    let project_id1 = ProjectId::new();
    let project_id2 = ProjectId::new();
    create_test_project(&db, &project_id1, "Project 1", "/path1");
    create_test_project(&db, &project_id2, "Project 2", "/path2");

    let repo = SqliteIdeationSessionRepository::new(db.new_connection());

    let session1 = create_test_session(&project_id1, Some("Session for P1"));
    let session2 = create_test_session(&project_id2, Some("Session for P2"));

    repo.create(session1).await.unwrap();
    repo.create(session2).await.unwrap();

    let sessions = repo.get_by_project(&project_id1).await.unwrap();

    assert_eq!(sessions.len(), 1);
    assert_eq!(sessions[0].project_id, project_id1);
}

// ==================== UPDATE STATUS TESTS ====================

#[tokio::test]
async fn test_update_status_to_archived() {
    let db = setup_test_db();
    let project_id = ProjectId::new();
    create_test_project(&db, &project_id, "Test Project", "/test/path");

    let repo = SqliteIdeationSessionRepository::new(db.new_connection());
    let session = create_test_session(&project_id, Some("To Archive"));

    repo.create(session.clone()).await.unwrap();

    let result = repo
        .update_status(&session.id, IdeationSessionStatus::Archived)
        .await;
    assert!(result.is_ok());

    let found = repo.get_by_id(&session.id).await.unwrap().unwrap();
    assert_eq!(found.status, IdeationSessionStatus::Archived);
    assert!(found.archived_at.is_some());
}

#[tokio::test]
async fn test_update_status_to_converted() {
    let db = setup_test_db();
    let project_id = ProjectId::new();
    create_test_project(&db, &project_id, "Test Project", "/test/path");

    let repo = SqliteIdeationSessionRepository::new(db.new_connection());
    let session = create_test_session(&project_id, Some("To Convert"));

    repo.create(session.clone()).await.unwrap();

    let result = repo
        .update_status(&session.id, IdeationSessionStatus::Accepted)
        .await;
    assert!(result.is_ok());

    let found = repo.get_by_id(&session.id).await.unwrap().unwrap();
    assert_eq!(found.status, IdeationSessionStatus::Accepted);
    assert!(found.converted_at.is_some());
}

#[tokio::test]
async fn test_update_status_back_to_active() {
    let db = setup_test_db();
    let project_id = ProjectId::new();
    create_test_project(&db, &project_id, "Test Project", "/test/path");

    let repo = SqliteIdeationSessionRepository::new(db.new_connection());
    let mut session = create_test_session(&project_id, Some("Reactivate"));
    session.archive();

    repo.create(session.clone()).await.unwrap();

    let result = repo
        .update_status(&session.id, IdeationSessionStatus::Active)
        .await;
    assert!(result.is_ok());

    let found = repo.get_by_id(&session.id).await.unwrap().unwrap();
    assert_eq!(found.status, IdeationSessionStatus::Active);
}

#[tokio::test]
async fn test_update_status_updates_updated_at() {
    let db = setup_test_db();
    let project_id = ProjectId::new();
    create_test_project(&db, &project_id, "Test Project", "/test/path");

    let repo = SqliteIdeationSessionRepository::new(db.new_connection());
    let session = create_test_session(&project_id, Some("Check Timestamp"));
    let original_updated = session.updated_at;

    repo.create(session.clone()).await.unwrap();

    // Small delay to ensure timestamp difference
    tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;

    repo.update_status(&session.id, IdeationSessionStatus::Archived)
        .await
        .unwrap();

    let found = repo.get_by_id(&session.id).await.unwrap().unwrap();
    assert!(found.updated_at >= original_updated);
}

// ==================== UPDATE TITLE TESTS ====================

#[tokio::test]
async fn test_update_title_sets_new_title() {
    let db = setup_test_db();
    let project_id = ProjectId::new();
    create_test_project(&db, &project_id, "Test Project", "/test/path");

    let repo = SqliteIdeationSessionRepository::new(db.new_connection());
    let session = create_test_session(&project_id, Some("Original Title"));

    repo.create(session.clone()).await.unwrap();

    let result = repo
        .update_title(&session.id, Some("New Title".to_string()), "auto")
        .await;
    assert!(result.is_ok());

    let found = repo.get_by_id(&session.id).await.unwrap().unwrap();
    assert_eq!(found.title, Some("New Title".to_string()));
    assert_eq!(found.title_source, Some("auto".to_string()));
}

#[tokio::test]
async fn test_update_title_clears_title() {
    let db = setup_test_db();
    let project_id = ProjectId::new();
    create_test_project(&db, &project_id, "Test Project", "/test/path");

    let repo = SqliteIdeationSessionRepository::new(db.new_connection());
    let session = create_test_session(&project_id, Some("Has Title"));

    repo.create(session.clone()).await.unwrap();

    let result = repo.update_title(&session.id, None, "auto").await;
    assert!(result.is_ok());

    let found = repo.get_by_id(&session.id).await.unwrap().unwrap();
    assert_eq!(found.title, None);
}

#[tokio::test]
async fn test_update_title_user_source() {
    let db = setup_test_db();
    let project_id = ProjectId::new();
    create_test_project(&db, &project_id, "Test Project", "/test/path");

    let repo = SqliteIdeationSessionRepository::new(db.new_connection());
    let session = create_test_session(&project_id, Some("Original"));

    repo.create(session.clone()).await.unwrap();

    repo.update_title(&session.id, Some("User Renamed".to_string()), "user")
        .await
        .unwrap();

    let found = repo.get_by_id(&session.id).await.unwrap().unwrap();
    assert_eq!(found.title, Some("User Renamed".to_string()));
    assert_eq!(found.title_source, Some("user".to_string()));
}

#[tokio::test]
async fn test_update_title_updates_updated_at() {
    let db = setup_test_db();
    let project_id = ProjectId::new();
    create_test_project(&db, &project_id, "Test Project", "/test/path");

    let repo = SqliteIdeationSessionRepository::new(db.new_connection());
    let session = create_test_session(&project_id, Some("Check Timestamp"));
    let original_updated = session.updated_at;

    repo.create(session.clone()).await.unwrap();

    tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;

    repo.update_title(&session.id, Some("Updated".to_string()), "auto")
        .await
        .unwrap();

    let found = repo.get_by_id(&session.id).await.unwrap().unwrap();
    assert!(found.updated_at >= original_updated);
}

// ==================== DELETE TESTS ====================

#[tokio::test]
async fn test_delete_removes_session_from_database() {
    let db = setup_test_db();
    let project_id = ProjectId::new();
    create_test_project(&db, &project_id, "Test Project", "/test/path");

    let repo = SqliteIdeationSessionRepository::new(db.new_connection());
    let session = create_test_session(&project_id, Some("To Delete"));

    repo.create(session.clone()).await.unwrap();

    let delete_result = repo.delete(&session.id).await;
    assert!(delete_result.is_ok());

    let found = repo.get_by_id(&session.id).await.unwrap();
    assert!(found.is_none());
}

#[tokio::test]
async fn test_delete_nonexistent_succeeds() {
    let db = setup_test_db();
    let repo = SqliteIdeationSessionRepository::new(db.new_connection());
    let id = IdeationSessionId::new();

    // Deleting a non-existent session should not error
    let result = repo.delete(&id).await;
    assert!(result.is_ok());
}

// ==================== GET ACTIVE BY PROJECT TESTS ====================

#[tokio::test]
async fn test_get_active_by_project_returns_only_active() {
    let db = setup_test_db();
    let project_id = ProjectId::new();
    create_test_project(&db, &project_id, "Test Project", "/test/path");

    let repo = SqliteIdeationSessionRepository::new(db.new_connection());

    let active = create_test_session(&project_id, Some("Active"));
    let mut archived = create_test_session(&project_id, Some("Archived"));
    archived.archive();
    let mut accepted = create_test_session(&project_id, Some("Accepted"));
    accepted.mark_accepted();

    repo.create(active.clone()).await.unwrap();
    repo.create(archived).await.unwrap();
    repo.create(accepted).await.unwrap();

    let result = repo.get_active_by_project(&project_id).await;

    assert!(result.is_ok());
    let sessions = result.unwrap();
    assert_eq!(sessions.len(), 1);
    assert_eq!(sessions[0].id, active.id);
    assert_eq!(sessions[0].status, IdeationSessionStatus::Active);
}

#[tokio::test]
async fn test_get_active_by_project_returns_empty_when_none_active() {
    let db = setup_test_db();
    let project_id = ProjectId::new();
    create_test_project(&db, &project_id, "Test Project", "/test/path");

    let repo = SqliteIdeationSessionRepository::new(db.new_connection());

    let mut archived = create_test_session(&project_id, Some("Archived"));
    archived.archive();

    repo.create(archived).await.unwrap();

    let result = repo.get_active_by_project(&project_id).await;

    assert!(result.is_ok());
    assert!(result.unwrap().is_empty());
}

#[tokio::test]
async fn test_get_active_by_project_ordered_by_updated_at_desc() {
    let db = setup_test_db();
    let project_id = ProjectId::new();
    create_test_project(&db, &project_id, "Test Project", "/test/path");

    let repo = SqliteIdeationSessionRepository::new(db.new_connection());

    let session1 = IdeationSession::builder()
        .project_id(project_id.clone())
        .title("Oldest Active")
        .updated_at(chrono::Utc::now() - chrono::Duration::hours(2))
        .build();
    let session2 = IdeationSession::builder()
        .project_id(project_id.clone())
        .title("Newest Active")
        .updated_at(chrono::Utc::now() - chrono::Duration::hours(1))
        .build();

    repo.create(session1).await.unwrap();
    repo.create(session2).await.unwrap();

    let sessions = repo.get_active_by_project(&project_id).await.unwrap();

    assert_eq!(sessions.len(), 2);
    assert_eq!(sessions[0].title, Some("Newest Active".to_string()));
    assert_eq!(sessions[1].title, Some("Oldest Active".to_string()));
}

// ==================== COUNT BY STATUS TESTS ====================

#[tokio::test]
async fn test_count_by_status_returns_zero_for_no_sessions() {
    let db = setup_test_db();
    let project_id = ProjectId::new();
    create_test_project(&db, &project_id, "Test Project", "/test/path");

    let repo = SqliteIdeationSessionRepository::new(db.new_connection());

    let result = repo
        .count_by_status(&project_id, IdeationSessionStatus::Active)
        .await;

    assert!(result.is_ok());
    assert_eq!(result.unwrap(), 0);
}

#[tokio::test]
async fn test_count_by_status_counts_correctly() {
    let db = setup_test_db();
    let project_id = ProjectId::new();
    create_test_project(&db, &project_id, "Test Project", "/test/path");

    let repo = SqliteIdeationSessionRepository::new(db.new_connection());

    let active1 = create_test_session(&project_id, Some("Active 1"));
    let active2 = create_test_session(&project_id, Some("Active 2"));
    let mut archived = create_test_session(&project_id, Some("Archived"));
    archived.archive();
    let mut accepted = create_test_session(&project_id, Some("Accepted"));
    accepted.mark_accepted();

    repo.create(active1).await.unwrap();
    repo.create(active2).await.unwrap();
    repo.create(archived).await.unwrap();
    repo.create(accepted).await.unwrap();

    let active_count = repo
        .count_by_status(&project_id, IdeationSessionStatus::Active)
        .await
        .unwrap();
    let archived_count = repo
        .count_by_status(&project_id, IdeationSessionStatus::Archived)
        .await
        .unwrap();
    let accepted_count = repo
        .count_by_status(&project_id, IdeationSessionStatus::Accepted)
        .await
        .unwrap();

    assert_eq!(active_count, 2);
    assert_eq!(archived_count, 1);
    assert_eq!(accepted_count, 1);
}

#[tokio::test]
async fn test_count_by_status_filters_by_project() {
    let db = setup_test_db();
    let project_id1 = ProjectId::new();
    let project_id2 = ProjectId::new();
    create_test_project(&db, &project_id1, "Project 1", "/path1");
    create_test_project(&db, &project_id2, "Project 2", "/path2");

    let repo = SqliteIdeationSessionRepository::new(db.new_connection());

    let session1 = create_test_session(&project_id1, Some("P1 Session"));
    let session2 = create_test_session(&project_id2, Some("P2 Session 1"));
    let session3 = create_test_session(&project_id2, Some("P2 Session 2"));

    repo.create(session1).await.unwrap();
    repo.create(session2).await.unwrap();
    repo.create(session3).await.unwrap();

    let count_p1 = repo
        .count_by_status(&project_id1, IdeationSessionStatus::Active)
        .await
        .unwrap();
    let count_p2 = repo
        .count_by_status(&project_id2, IdeationSessionStatus::Active)
        .await
        .unwrap();

    assert_eq!(count_p1, 1);
    assert_eq!(count_p2, 2);
}

// ==================== SHARED CONNECTION TESTS ====================

#[tokio::test]
async fn test_from_shared_works_correctly() {
    let db = setup_test_db();
    let project_id = ProjectId::new();
    create_test_project(&db, &project_id, "Test Project", "/test/path");

    let repo = SqliteIdeationSessionRepository::from_shared(db.shared_conn());

    let session = create_test_session(&project_id, Some("Shared Connection"));

    let result = repo.create(session.clone()).await;
    assert!(result.is_ok());

    let found = repo.get_by_id(&session.id).await.unwrap();
    assert!(found.is_some());
}

// ==================== GET CHILDREN TESTS ====================

#[tokio::test]
async fn test_get_children_returns_all_direct_children() {
    let db = setup_test_db();
    let project_id = ProjectId::new();
    create_test_project(&db, &project_id, "Test Project", "/test/path");

    let repo = SqliteIdeationSessionRepository::new(db.new_connection());

    let parent = create_test_session(&project_id, Some("Parent"));
    let mut child1 = create_test_session(&project_id, Some("Child 1"));
    child1.parent_session_id = Some(parent.id.clone());
    let mut child2 = create_test_session(&project_id, Some("Child 2"));
    child2.parent_session_id = Some(parent.id.clone());

    repo.create(parent.clone()).await.unwrap();
    repo.create(child1.clone()).await.unwrap();
    repo.create(child2.clone()).await.unwrap();

    let children = repo.get_children(&parent.id).await.unwrap();
    assert_eq!(children.len(), 2);
}

#[tokio::test]
async fn test_get_children_returns_empty_for_sessions_without_children() {
    let db = setup_test_db();
    let project_id = ProjectId::new();
    create_test_project(&db, &project_id, "Test Project", "/test/path");

    let repo = SqliteIdeationSessionRepository::new(db.new_connection());

    let session = create_test_session(&project_id, Some("No Children"));
    repo.create(session.clone()).await.unwrap();

    let children = repo.get_children(&session.id).await.unwrap();
    assert!(children.is_empty());
}

// ==================== GET ANCESTOR CHAIN TESTS ====================

#[tokio::test]
async fn test_get_ancestor_chain_three_levels_deep() {
    let db = setup_test_db();
    let project_id = ProjectId::new();
    create_test_project(&db, &project_id, "Test Project", "/test/path");

    let repo = SqliteIdeationSessionRepository::new(db.new_connection());

    let level1 = create_test_session(&project_id, Some("Level 1"));
    let mut level2 = create_test_session(&project_id, Some("Level 2"));
    level2.parent_session_id = Some(level1.id.clone());
    let mut level3 = create_test_session(&project_id, Some("Level 3"));
    level3.parent_session_id = Some(level2.id.clone());

    repo.create(level1.clone()).await.unwrap();
    repo.create(level2.clone()).await.unwrap();
    repo.create(level3.clone()).await.unwrap();

    let chain = repo.get_ancestor_chain(&level3.id).await.unwrap();
    // Should return: [level2, level1] (direct parent to root)
    assert_eq!(chain.len(), 2);
    assert_eq!(chain[0].id, level2.id);
    assert_eq!(chain[1].id, level1.id);
}

#[tokio::test]
async fn test_get_ancestor_chain_single_parent() {
    let db = setup_test_db();
    let project_id = ProjectId::new();
    create_test_project(&db, &project_id, "Test Project", "/test/path");

    let repo = SqliteIdeationSessionRepository::new(db.new_connection());

    let parent = create_test_session(&project_id, Some("Parent"));
    let mut child = create_test_session(&project_id, Some("Child"));
    child.parent_session_id = Some(parent.id.clone());

    repo.create(parent.clone()).await.unwrap();
    repo.create(child.clone()).await.unwrap();

    let chain = repo.get_ancestor_chain(&child.id).await.unwrap();
    assert_eq!(chain.len(), 1);
    assert_eq!(chain[0].id, parent.id);
}

#[tokio::test]
async fn test_get_ancestor_chain_no_parent() {
    let db = setup_test_db();
    let project_id = ProjectId::new();
    create_test_project(&db, &project_id, "Test Project", "/test/path");

    let repo = SqliteIdeationSessionRepository::new(db.new_connection());

    let session = create_test_session(&project_id, Some("Root Session"));
    repo.create(session.clone()).await.unwrap();

    let chain = repo.get_ancestor_chain(&session.id).await.unwrap();
    assert!(chain.is_empty());
}

// ==================== SET PARENT TESTS ====================

#[tokio::test]
async fn test_set_parent_establishes_parent_child_relationship() {
    let db = setup_test_db();
    let project_id = ProjectId::new();
    create_test_project(&db, &project_id, "Test Project", "/test/path");

    let repo = SqliteIdeationSessionRepository::new(db.new_connection());

    let parent = create_test_session(&project_id, Some("Parent"));
    let child = create_test_session(&project_id, Some("Child"));

    repo.create(parent.clone()).await.unwrap();
    repo.create(child.clone()).await.unwrap();

    repo.set_parent(&child.id, Some(&parent.id)).await.unwrap();

    let updated_child = repo.get_by_id(&child.id).await.unwrap().unwrap();
    assert_eq!(updated_child.parent_session_id, Some(parent.id.clone()));
}

#[tokio::test]
async fn test_set_parent_with_null_clears_parent() {
    let db = setup_test_db();
    let project_id = ProjectId::new();
    create_test_project(&db, &project_id, "Test Project", "/test/path");

    let repo = SqliteIdeationSessionRepository::new(db.new_connection());

    let parent = create_test_session(&project_id, Some("Parent"));
    let mut child = create_test_session(&project_id, Some("Child"));
    child.parent_session_id = Some(parent.id.clone());

    repo.create(parent.clone()).await.unwrap();
    repo.create(child.clone()).await.unwrap();

    // Clear the parent
    repo.set_parent(&child.id, None).await.unwrap();

    let updated_child = repo.get_by_id(&child.id).await.unwrap().unwrap();
    assert!(updated_child.parent_session_id.is_none());
}

#[tokio::test]
async fn test_set_parent_updates_updated_at() {
    let db = setup_test_db();
    let project_id = ProjectId::new();
    create_test_project(&db, &project_id, "Test Project", "/test/path");

    let repo = SqliteIdeationSessionRepository::new(db.new_connection());

    let parent = create_test_session(&project_id, Some("Parent"));
    let child = create_test_session(&project_id, Some("Child"));
    let original_updated_at = child.updated_at;

    repo.create(parent.clone()).await.unwrap();
    repo.create(child.clone()).await.unwrap();

    tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;

    repo.set_parent(&child.id, Some(&parent.id)).await.unwrap();

    let updated_child = repo.get_by_id(&child.id).await.unwrap().unwrap();
    assert!(updated_child.updated_at >= original_updated_at);
}

// ==================== UPDATE_PLAN_ARTIFACT_ID TESTS ====================

#[tokio::test]
async fn test_update_plan_artifact_id_sets_value() {
    let db = setup_test_db();
    let project_id = ProjectId::new();
    create_test_project(&db, &project_id, "Test Project", "/test/path");

    let repo = SqliteIdeationSessionRepository::new(db.new_connection());
    let session = create_test_session(&project_id, Some("Plan Session"));
    repo.create(session.clone()).await.unwrap();

    repo.update_plan_artifact_id(&session.id, Some("artifact-abc".to_string()))
        .await
        .unwrap();

    let found = repo.get_by_id(&session.id).await.unwrap().unwrap();
    assert_eq!(
        found.plan_artifact_id,
        Some(ArtifactId::from_string("artifact-abc"))
    );
}

#[tokio::test]
async fn test_update_plan_artifact_id_clears_value() {
    let db = setup_test_db();
    let project_id = ProjectId::new();
    create_test_project(&db, &project_id, "Test Project", "/test/path");

    let repo = SqliteIdeationSessionRepository::new(db.new_connection());
    let session = create_test_session(&project_id, Some("Plan Session"));
    repo.create(session.clone()).await.unwrap();

    // Set then clear
    repo.update_plan_artifact_id(&session.id, Some("artifact-abc".to_string()))
        .await
        .unwrap();
    repo.update_plan_artifact_id(&session.id, None)
        .await
        .unwrap();

    let found = repo.get_by_id(&session.id).await.unwrap().unwrap();
    assert!(found.plan_artifact_id.is_none());
}

// ==================== GET_BY_PLAN_ARTIFACT_ID TESTS ====================

#[tokio::test]
async fn test_get_by_plan_artifact_id_returns_matching_sessions() {
    let db = setup_test_db();
    let project_id = ProjectId::new();
    create_test_project(&db, &project_id, "Test Project", "/test/path");

    let repo = SqliteIdeationSessionRepository::new(db.new_connection());
    let session1 = create_test_session(&project_id, Some("Session 1"));
    let session2 = create_test_session(&project_id, Some("Session 2"));
    let session3 = create_test_session(&project_id, Some("Session 3 Different Artifact"));
    repo.create(session1.clone()).await.unwrap();
    repo.create(session2.clone()).await.unwrap();
    repo.create(session3.clone()).await.unwrap();

    repo.update_plan_artifact_id(&session1.id, Some("plan-artifact-xyz".to_string()))
        .await
        .unwrap();
    repo.update_plan_artifact_id(&session3.id, Some("plan-artifact-other".to_string()))
        .await
        .unwrap();

    let results = repo
        .get_by_plan_artifact_id("plan-artifact-xyz")
        .await
        .unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].id, session1.id);
}

#[tokio::test]
async fn test_get_by_plan_artifact_id_returns_empty_when_no_match() {
    let db = setup_test_db();
    let project_id = ProjectId::new();
    create_test_project(&db, &project_id, "Test Project", "/test/path");

    let repo = SqliteIdeationSessionRepository::new(db.new_connection());
    let session = create_test_session(&project_id, Some("Session"));
    repo.create(session).await.unwrap();

    let results = repo
        .get_by_plan_artifact_id("nonexistent-artifact")
        .await
        .unwrap();
    assert!(results.is_empty());
}

// ==================== VERIFICATION STATE TESTS ====================

#[tokio::test]
async fn test_update_verification_state_roundtrip() {
    let db = setup_test_db();
    let project_id = ProjectId::new();
    create_test_project(&db, &project_id, "Test Project", "/test/path");

    let repo = SqliteIdeationSessionRepository::new(db.new_connection());
    let session = create_test_session(&project_id, Some("Verify Session"));
    repo.create(session.clone()).await.unwrap();

    // Default state
    let (status, in_progress) = repo
        .get_verification_status(&session.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(status, VerificationStatus::Unverified);
    assert!(!in_progress);

    // Update to reviewing + in_progress
    repo.update_verification_state(&session.id, VerificationStatus::Reviewing, true)
        .await
        .unwrap();

    let found = repo.get_by_id(&session.id).await.unwrap().unwrap();
    assert_eq!(found.verification_status, VerificationStatus::Reviewing);
    assert!(found.verification_in_progress);

    let (status2, in_progress2) = repo
        .get_verification_status(&session.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(status2, VerificationStatus::Reviewing);
    assert!(in_progress2);
}

#[tokio::test]
async fn test_update_verification_state_all_status_variants() {
    let db = setup_test_db();
    let project_id = ProjectId::new();
    create_test_project(&db, &project_id, "Test Project", "/test/path");

    let repo = SqliteIdeationSessionRepository::new(db.new_connection());
    let session = create_test_session(&project_id, Some("All Statuses"));
    repo.create(session.clone()).await.unwrap();

    for status in [
        VerificationStatus::Reviewing,
        VerificationStatus::NeedsRevision,
        VerificationStatus::Verified,
        VerificationStatus::Skipped,
        VerificationStatus::Unverified,
    ] {
        repo.update_verification_state(&session.id, status, false)
            .await
            .unwrap();
        let (s, _) = repo
            .get_verification_status(&session.id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(s, status);
    }
}

#[tokio::test]
async fn test_reset_verification_clears_all_3_columns_when_not_in_progress() {
    let db = setup_test_db();
    let project_id = ProjectId::new();
    create_test_project(&db, &project_id, "Test Project", "/test/path");

    let repo = SqliteIdeationSessionRepository::new(db.new_connection());
    let session = create_test_session(&project_id, Some("Reset Session"));
    repo.create(session.clone()).await.unwrap();

    // Set to needs_revision, not in progress
    repo.update_verification_state(&session.id, VerificationStatus::NeedsRevision, false)
        .await
        .unwrap();

    // Reset should clear all 3 columns and return true
    let reset = repo.reset_verification(&session.id).await.unwrap();
    assert!(
        reset,
        "reset_verification must return true when in_progress=0"
    );

    let found = repo.get_by_id(&session.id).await.unwrap().unwrap();
    assert_eq!(found.verification_status, VerificationStatus::Unverified);
    assert!(!found.verification_in_progress);
    assert_eq!(
        found.verification_generation, 1,
        "reset_verification must increment generation to fence stale verifier callbacks"
    );
}

#[tokio::test]
async fn test_reset_verification_is_noop_when_in_progress() {
    let db = setup_test_db();
    let project_id = ProjectId::new();
    create_test_project(&db, &project_id, "Test Project", "/test/path");

    let repo = SqliteIdeationSessionRepository::new(db.new_connection());
    let session = create_test_session(&project_id, Some("In Progress Session"));
    repo.create(session.clone()).await.unwrap();

    // Set to reviewing with in_progress = true
    repo.update_verification_state(&session.id, VerificationStatus::Reviewing, true)
        .await
        .unwrap();

    // Reset should be a no-op because in_progress = 1 and return false
    let reset = repo.reset_verification(&session.id).await.unwrap();
    assert!(
        !reset,
        "reset_verification must return false when in_progress=1"
    );

    // Status flags remain unchanged while no native verification snapshot is present
    let found = repo.get_by_id(&session.id).await.unwrap().unwrap();
    assert_eq!(found.verification_status, VerificationStatus::Reviewing);
    assert!(found.verification_in_progress);
}

#[tokio::test]
async fn test_reset_verification_returns_false_for_nonexistent_session() {
    let db = setup_test_db();
    let repo = SqliteIdeationSessionRepository::new(db.new_connection());
    let fake_id = IdeationSessionId::new();

    let reset = repo.reset_verification(&fake_id).await.unwrap();
    assert!(
        !reset,
        "reset_verification must return false for nonexistent session"
    );
}

#[tokio::test]
async fn test_get_verification_status_returns_none_for_nonexistent_session() {
    let db = setup_test_db();
    let repo = SqliteIdeationSessionRepository::new(db.new_connection());
    let id = IdeationSessionId::new();

    let result = repo.get_verification_status(&id).await.unwrap();
    assert!(result.is_none());
}

#[tokio::test]
async fn test_get_verification_status_prefers_active_generation_snapshot_over_stale_summary() {
    let db = setup_test_db();
    let project_id = ProjectId::new();
    create_test_project(&db, &project_id, "Test Project", "/test/path");

    let repo = SqliteIdeationSessionRepository::new(db.new_connection());
    let mut session = create_test_session(&project_id, Some("Snapshot First"));
    session.verification_generation = 3;
    let session_id = session.id.clone();
    repo.create(session).await.unwrap();

    repo.save_verification_run_snapshot(
        &session_id,
        &VerificationRunSnapshot {
            generation: 3,
            status: VerificationStatus::Reviewing,
            in_progress: true,
            current_round: 2,
            max_rounds: 5,
            best_round_index: None,
            convergence_reason: None,
            current_gaps: vec![VerificationGap {
                severity: "high".to_string(),
                category: "testing".to_string(),
                description: "Missing regression".to_string(),
                why_it_matters: None,
                source: Some("completeness".to_string()),
            }],
            rounds: vec![],
        },
    )
    .await
    .unwrap();

    repo.update_verification_state(&session_id, VerificationStatus::Unverified, false)
        .await
        .unwrap();

    let (status, in_progress) = repo
        .get_verification_status(&session_id)
        .await
        .unwrap()
        .expect("session should exist");
    assert_eq!(
        status,
        VerificationStatus::Reviewing,
        "active-generation snapshot must override stale summary status"
    );
    assert!(
        in_progress,
        "active-generation snapshot must override stale summary in_progress flag"
    );
}

#[tokio::test]
async fn test_save_and_get_verification_run_snapshot_roundtrip() {
    let db = setup_test_db();
    let project_id = ProjectId::new();
    create_test_project(&db, &project_id, "Test Project", "/test/path");

    let repo = SqliteIdeationSessionRepository::new(db.new_connection());
    let session = create_test_session(&project_id, Some("Verification Snapshot"));
    repo.create(session.clone()).await.unwrap();

    let snapshot = VerificationRunSnapshot {
        generation: 7,
        status: VerificationStatus::NeedsRevision,
        in_progress: false,
        current_round: 2,
        max_rounds: 5,
        best_round_index: Some(1),
        convergence_reason: Some("escalated_to_parent".to_string()),
        current_gaps: vec![VerificationGap {
            severity: "high".to_string(),
            category: "testing".to_string(),
            description: "Missing regression".to_string(),
            why_it_matters: Some("Plan can regress at runtime".to_string()),
            source: Some("completeness".to_string()),
        }],
        rounds: vec![
            VerificationRoundSnapshot {
                round: 1,
                gap_score: 10,
                fingerprints: vec!["gap-auth".to_string()],
                gaps: vec![VerificationGap {
                    severity: "critical".to_string(),
                    category: "security".to_string(),
                    description: "Auth missing".to_string(),
                    why_it_matters: None,
                    source: Some("completeness".to_string()),
                }],
                parse_failed: false,
            },
            VerificationRoundSnapshot {
                round: 2,
                gap_score: 3,
                fingerprints: vec!["gap-regression".to_string()],
                gaps: vec![VerificationGap {
                    severity: "high".to_string(),
                    category: "testing".to_string(),
                    description: "Missing regression".to_string(),
                    why_it_matters: Some("Plan can regress at runtime".to_string()),
                    source: Some("feasibility".to_string()),
                }],
                parse_failed: true,
            },
        ],
    };

    repo.save_verification_run_snapshot(&session.id, &snapshot)
        .await
        .unwrap();

    let found = repo
        .get_verification_run_snapshot(&session.id, 7)
        .await
        .unwrap()
        .expect("snapshot must exist");
    assert_eq!(found, snapshot);
}

// ==================== CIRCULAR IMPORT VALIDATION TESTS ====================

fn create_test_session_with_source(
    conn: &Connection,
    project_id: &ProjectId,
    source_session_id: Option<&str>,
    source_project_id: Option<&str>,
) -> IdeationSession {
    let mut builder = IdeationSession::builder()
        .project_id(project_id.clone())
        .verification_status(VerificationStatus::Verified);

    if let Some(sid) = source_session_id {
        builder = builder.source_session_id(sid.to_string());
    }
    if let Some(pid) = source_project_id {
        builder = builder.source_project_id(pid.to_string());
    }

    let session = builder.build();
    SqliteIdeationSessionRepository::insert_sync(conn, &session).unwrap();
    session
}

fn create_test_session_with_source_in_db(
    db: &SqliteTestDb,
    project_id: &ProjectId,
    source_session_id: Option<&str>,
    source_project_id: Option<&str>,
) -> IdeationSession {
    db.with_connection(|conn| {
        create_test_session_with_source(conn, project_id, source_session_id, source_project_id)
    })
}

#[test]
fn test_validate_no_circular_import_happy_path() {
    let db = setup_test_db();
    let source_project_id = ProjectId::new();
    let target_project_id = ProjectId::new();
    create_test_project(&db, &source_project_id, "Source", "/source");
    create_test_project(&db, &target_project_id, "Target", "/target");

    let source = create_test_session_with_source_in_db(&db, &source_project_id, None, None);

    let result = db.with_connection(|conn| {
        SqliteIdeationSessionRepository::validate_no_circular_import_sync(
            conn,
            source.id.as_str(),
            target_project_id.as_str(),
            10,
        )
    });

    assert!(
        result.is_ok(),
        "Simple cross-project import should be allowed"
    );
}

#[test]
fn test_validate_no_circular_import_self_reference() {
    let db = setup_test_db();
    let project_id = ProjectId::new();
    create_test_project(&db, &project_id, "Project", "/project");

    let session = create_test_session_with_source_in_db(&db, &project_id, None, None);

    // Trying to import from the same project (self-reference)
    let result = db.with_connection(|conn| {
        SqliteIdeationSessionRepository::validate_no_circular_import_sync(
            conn,
            session.id.as_str(),
            project_id.as_str(), // target == source project
            10,
        )
    });

    assert!(result.is_err());
    let err_msg = result.unwrap_err().to_string();
    assert!(
        err_msg.contains("SELF_REFERENCE"),
        "Expected SELF_REFERENCE error, got: {err_msg}"
    );
}

#[test]
fn test_validate_no_circular_import_a_to_b_to_a() {
    let db = setup_test_db();
    let project_a = ProjectId::new();
    let project_b = ProjectId::new();
    create_test_project(&db, &project_a, "Project A", "/project-a");
    create_test_project(&db, &project_b, "Project B", "/project-b");

    // Session in B that was imported from A
    let _session_b =
        create_test_session_with_source_in_db(&db, &project_b, None, Some(project_a.as_str()));

    // Now trying to import from session_b into project_a would create A→B→A
    // session_b is in project_b, which is NOT project_a, so no SELF_REFERENCE.
    // But session_b.source_project_id == project_a → CIRCULAR_IMPORT.
    // Note: validate_no_circular_import walks session_b.source_session_id (which is None here).
    // The cycle is detected via project membership checks.
    // Source session_b.project_id = project_b ≠ project_a → ok on first check.
    // session_b.source_session_id = None → chain ends.
    // No cycle detected at the session-chain level (because source_project_id is not walked).
    //
    // Actually: the CIRCULAR_IMPORT detection works when session_b has a source_session_id
    // pointing to a session in project_a. Let's set up a proper 2-hop cycle.

    // Create a session in project_a (the "original") with no parent
    let session_a_original = create_test_session_with_source_in_db(&db, &project_a, None, None);

    // session_b2 was imported from session_a_original
    let session_b2 = create_test_session_with_source_in_db(
        &db,
        &project_b,
        Some(session_a_original.id.as_str()),
        Some(project_a.as_str()),
    );

    // Now project_a tries to import from session_b2 (which itself came from project_a)
    // Walk: session_b2.source_session_id = session_a_original, which is in project_a = target
    let result = db.with_connection(|conn| {
        SqliteIdeationSessionRepository::validate_no_circular_import_sync(
            conn,
            session_b2.id.as_str(),
            project_a.as_str(),
            10,
        )
    });

    assert!(result.is_err());
    let err_msg = result.unwrap_err().to_string();
    assert!(
        err_msg.contains("CIRCULAR_IMPORT"),
        "Expected CIRCULAR_IMPORT error for A→B→A cycle, got: {err_msg}"
    );
}

#[test]
fn test_validate_no_circular_import_three_hop_cycle() {
    let db = setup_test_db();
    let project_a = ProjectId::new();
    let project_b = ProjectId::new();
    let project_c = ProjectId::new();
    create_test_project(&db, &project_a, "A", "/a");
    create_test_project(&db, &project_b, "B", "/b");
    create_test_project(&db, &project_c, "C", "/c");

    // session_a in A
    let session_a = create_test_session_with_source_in_db(&db, &project_a, None, None);
    // session_b in B, imported from session_a
    let session_b = create_test_session_with_source_in_db(
        &db,
        &project_b,
        Some(session_a.id.as_str()),
        Some(project_a.as_str()),
    );
    // session_c in C, imported from session_b
    let session_c = create_test_session_with_source_in_db(
        &db,
        &project_c,
        Some(session_b.id.as_str()),
        Some(project_b.as_str()),
    );

    // Now A tries to import from session_c: A→C→B→A (3-hop)
    // Walk: session_c.source_session_id = session_b (project_b ≠ project_a → ok)
    //       session_b.source_session_id = session_a (project_a == target → CIRCULAR_IMPORT)
    let result = db.with_connection(|conn| {
        SqliteIdeationSessionRepository::validate_no_circular_import_sync(
            conn,
            session_c.id.as_str(),
            project_a.as_str(),
            10,
        )
    });

    assert!(result.is_err());
    let err_msg = result.unwrap_err().to_string();
    assert!(
        err_msg.contains("CIRCULAR_IMPORT"),
        "Expected CIRCULAR_IMPORT for 3-hop cycle, got: {err_msg}"
    );
}

#[test]
fn test_validate_no_circular_import_depth_limit_9_ok() {
    let db = setup_test_db();

    // Build a chain of 9 projects: P1 ← P2 ← P3 ← … ← P9 ← P10
    // Then P10 tries to import from the end of the chain (depth = 9 hops): should PASS
    let projects: Vec<ProjectId> = (0..10).map(|_| ProjectId::new()).collect();
    for (i, pid) in projects.iter().enumerate() {
        create_test_project(&db, pid, &format!("P{i}"), &format!("/p{i}"));
    }

    // Create sessions: each session points to the previous project's session
    let session_0 = create_test_session_with_source_in_db(&db, &projects[0], None, None);
    let mut prev_session = session_0;
    for i in 1..9 {
        let s = create_test_session_with_source_in_db(
            &db,
            &projects[i],
            Some(prev_session.id.as_str()),
            Some(projects[i - 1].as_str()),
        );
        prev_session = s;
    }

    // session at depth 9 (prev_session), target = projects[9]
    // Walk depth 9 (should succeed since max is 10)
    let result = db.with_connection(|conn| {
        SqliteIdeationSessionRepository::validate_no_circular_import_sync(
            conn,
            prev_session.id.as_str(),
            projects[9].as_str(),
            10,
        )
    });

    assert!(
        result.is_ok(),
        "9-hop chain should be within depth limit of 10"
    );
}

#[test]
fn test_validate_no_circular_import_depth_limit_exceeded() {
    let db = setup_test_db();

    // Build a chain of 11 projects so the walk exceeds depth 10
    let projects: Vec<ProjectId> = (0..12).map(|_| ProjectId::new()).collect();
    for (i, pid) in projects.iter().enumerate() {
        create_test_project(&db, pid, &format!("P{i}"), &format!("/p{i}"));
    }

    let session_0 = create_test_session_with_source_in_db(&db, &projects[0], None, None);
    let mut prev_session = session_0;
    for i in 1..11 {
        let s = create_test_session_with_source_in_db(
            &db,
            &projects[i],
            Some(prev_session.id.as_str()),
            Some(projects[i - 1].as_str()),
        );
        prev_session = s;
    }

    // Session at depth 11, target = projects[11] (no cycle, just too deep)
    let result = db.with_connection(|conn| {
        SqliteIdeationSessionRepository::validate_no_circular_import_sync(
            conn,
            prev_session.id.as_str(),
            projects[11].as_str(),
            10,
        )
    });

    assert!(result.is_err());
    let err_msg = result.unwrap_err().to_string();
    assert!(
        err_msg.contains("CHAIN_TOO_DEEP"),
        "Expected CHAIN_TOO_DEEP error, got: {err_msg}"
    );
}

#[test]
fn test_validate_no_circular_import_dangling_source_is_ok() {
    let db = setup_test_db();
    let source_project = ProjectId::new();
    let target_project = ProjectId::new();
    create_test_project(&db, &source_project, "Source", "/source");
    create_test_project(&db, &target_project, "Target", "/target");

    // Source session points to a non-existent (deleted) session — dangling reference
    let nonexistent_id = IdeationSessionId::new();
    let source = create_test_session_with_source_in_db(
        &db,
        &source_project,
        Some(nonexistent_id.as_str()),
        None,
    );

    let result = db.with_connection(|conn| {
        SqliteIdeationSessionRepository::validate_no_circular_import_sync(
            conn,
            source.id.as_str(),
            target_project.as_str(),
            10,
        )
    });

    assert!(
        result.is_ok(),
        "Dangling source reference should be handled gracefully"
    );
}

#[test]
fn test_insert_sync_and_get_by_id_sync() {
    let db = setup_test_db();
    let project_id = ProjectId::new();
    create_test_project(&db, &project_id, "Test", "/test");

    let session = IdeationSession::builder()
        .project_id(project_id.clone())
        .title("Cross-project session")
        .verification_status(VerificationStatus::ImportedVerified)
        .source_project_id("source-proj-123")
        .source_session_id("source-sess-456")
        .build();

    let inserted = db.with_connection(|conn| {
        SqliteIdeationSessionRepository::insert_sync(conn, &session).unwrap()
    });
    assert_eq!(inserted.id, session.id);
    assert_eq!(
        inserted.verification_status,
        VerificationStatus::ImportedVerified
    );
    assert_eq!(
        inserted.source_project_id,
        Some("source-proj-123".to_string())
    );
    assert_eq!(
        inserted.source_session_id,
        Some("source-sess-456".to_string())
    );

    let fetched = db.with_connection(|conn| {
        SqliteIdeationSessionRepository::get_by_id_sync(conn, session.id.as_str())
            .unwrap()
            .unwrap()
    });
    assert_eq!(
        fetched.verification_status,
        VerificationStatus::ImportedVerified
    );
    assert_eq!(
        fetched.source_session_id,
        Some("source-sess-456".to_string())
    );
}

// ==================== GROUP COUNT TESTS ====================

use std::sync::Arc;
use tokio::sync::Mutex as TokioMutex;

fn setup_shared_test_db() -> (
    SqliteTestDb,
    Arc<TokioMutex<Connection>>,
    SqliteIdeationSessionRepository,
) {
    let db = setup_test_db();
    let shared = db.shared_conn();
    let repo = SqliteIdeationSessionRepository::from_shared(Arc::clone(&shared));
    (db, shared, repo)
}

async fn create_task_in_db(
    shared: &Arc<TokioMutex<Connection>>,
    id: &str,
    project_id: &str,
    session_id: &str,
    internal_status: &str,
) {
    let id = id.to_string();
    let project_id = project_id.to_string();
    let session_id = session_id.to_string();
    let internal_status = internal_status.to_string();
    let conn = shared.lock().await;
    conn.execute(
        "INSERT INTO tasks (id, project_id, category, title, internal_status, ideation_session_id, created_at, updated_at) \
         VALUES (?1, ?2, 'regular', 'Test Task', ?3, ?4, strftime('%Y-%m-%dT%H:%M:%S+00:00', 'now'), strftime('%Y-%m-%dT%H:%M:%S+00:00', 'now'))",
        rusqlite::params![id, project_id, internal_status, session_id],
    )
    .unwrap();
}

async fn create_task_in_db_with_execution_plan(
    shared: &Arc<TokioMutex<Connection>>,
    id: &str,
    project_id: &str,
    session_id: &str,
    internal_status: &str,
    execution_plan_id: &str,
    archived: bool,
) {
    let id = id.to_string();
    let project_id = project_id.to_string();
    let session_id = session_id.to_string();
    let internal_status = internal_status.to_string();
    let execution_plan_id = execution_plan_id.to_string();
    let archived_at = archived.then(|| "2026-04-16T00:00:00+00:00".to_string());
    let conn = shared.lock().await;
    conn.execute(
        "INSERT INTO tasks (id, project_id, category, title, internal_status, ideation_session_id, execution_plan_id, archived_at, created_at, updated_at) \
         VALUES (?1, ?2, 'regular', 'Test Task', ?3, ?4, ?5, ?6, strftime('%Y-%m-%dT%H:%M:%S+00:00', 'now'), strftime('%Y-%m-%dT%H:%M:%S+00:00', 'now'))",
        rusqlite::params![id, project_id, internal_status, session_id, execution_plan_id, archived_at],
    )
    .unwrap();
}

async fn create_execution_plan_in_db(
    shared: &Arc<TokioMutex<Connection>>,
    id: &str,
    session_id: &str,
    status: &str,
) {
    let conn = shared.lock().await;
    conn.execute(
        "INSERT INTO execution_plans (id, session_id, status, created_at) \
         VALUES (?1, ?2, ?3, strftime('%Y-%m-%dT%H:%M:%f+00:00', 'now'))",
        rusqlite::params![id, session_id, status],
    )
    .unwrap();
}

async fn update_session_status(
    shared: &Arc<TokioMutex<Connection>>,
    session_id: &str,
    status: &str,
) {
    let conn = shared.lock().await;
    conn.execute(
        "UPDATE ideation_sessions SET status = ?1 WHERE id = ?2",
        rusqlite::params![status, session_id],
    )
    .unwrap();
}

#[tokio::test]
async fn test_get_group_counts_empty_project() {
    let db = setup_test_db();
    let project_id = ProjectId::new();
    create_test_project(&db, &project_id, "Test Project", "/test/path");

    let repo = SqliteIdeationSessionRepository::new(db.new_connection());
    let counts = repo.get_group_counts(&project_id, None).await.unwrap();

    assert_eq!(counts.drafts, 0);
    assert_eq!(counts.in_progress, 0);
    assert_eq!(counts.accepted, 0);
    assert_eq!(counts.done, 0);
    assert_eq!(counts.archived, 0);
}

#[tokio::test]
async fn test_get_group_counts_active_sessions_drafts() {
    let db = setup_test_db();
    let project_id = ProjectId::new();
    create_test_project(&db, &project_id, "Test Project", "/test/path");

    let repo = SqliteIdeationSessionRepository::new(db.new_connection());
    let s1 = create_test_session(&project_id, Some("Draft 1"));
    let s2 = create_test_session(&project_id, Some("Draft 2"));
    repo.create(s1).await.unwrap();
    repo.create(s2).await.unwrap();

    let counts = repo.get_group_counts(&project_id, None).await.unwrap();

    assert_eq!(counts.drafts, 2);
    assert_eq!(counts.in_progress, 0);
    assert_eq!(counts.accepted, 0);
    assert_eq!(counts.done, 0);
    assert_eq!(counts.archived, 0);
}

#[tokio::test]
async fn test_get_group_counts_accepted_with_active_tasks_in_progress() {
    let project_id = ProjectId::new();
    let (_db, shared, repo) = setup_shared_test_db();
    {
        let conn = shared.lock().await;
        insert_test_project(&conn, &project_id, "Test Project", "/test/path");
    }

    let session = create_test_session(&project_id, Some("Accepted Session"));
    let session_id = session.id.as_str().to_string();
    repo.create(session).await.unwrap();

    // Mark as accepted
    update_session_status(&shared, &session_id, "accepted").await;

    // Add an executing task (active status — not idle, not terminal)
    create_task_in_db(
        &shared,
        "task-001",
        project_id.as_str(),
        &session_id,
        "executing",
    )
    .await;

    let counts = repo.get_group_counts(&project_id, None).await.unwrap();

    assert_eq!(counts.drafts, 0);
    assert_eq!(counts.in_progress, 1);
    assert_eq!(counts.accepted, 0);
    assert_eq!(counts.done, 0);
    assert_eq!(counts.archived, 0);
}

#[tokio::test]
async fn test_get_group_counts_accepted_with_all_terminal_tasks_done() {
    let project_id = ProjectId::new();
    let (_db, shared, repo) = setup_shared_test_db();
    {
        let conn = shared.lock().await;
        insert_test_project(&conn, &project_id, "Test Project", "/test/path");
    }

    let session = create_test_session(&project_id, Some("Done Session"));
    let session_id = session.id.as_str().to_string();
    repo.create(session).await.unwrap();

    update_session_status(&shared, &session_id, "accepted").await;

    // All tasks are terminal
    create_task_in_db(
        &shared,
        "task-002",
        project_id.as_str(),
        &session_id,
        "merged",
    )
    .await;

    let counts = repo.get_group_counts(&project_id, None).await.unwrap();

    assert_eq!(counts.drafts, 0);
    assert_eq!(counts.in_progress, 0);
    assert_eq!(counts.accepted, 0);
    assert_eq!(counts.done, 1);
    assert_eq!(counts.archived, 0);
}

#[tokio::test]
async fn test_get_group_counts_done_ignores_archived_and_superseded_execution_plan_tasks() {
    let project_id = ProjectId::new();
    let (_db, shared, repo) = setup_shared_test_db();
    {
        let conn = shared.lock().await;
        insert_test_project(&conn, &project_id, "Test Project", "/test/path");
    }

    let session = create_test_session(&project_id, Some("Repeated Reaccept Session"));
    let session_id = session.id.as_str().to_string();
    repo.create(session).await.unwrap();
    update_session_status(&shared, &session_id, "accepted").await;

    create_execution_plan_in_db(&shared, "ep-superseded", &session_id, "superseded").await;
    create_execution_plan_in_db(&shared, "ep-current", &session_id, "active").await;

    create_task_in_db_with_execution_plan(
        &shared,
        "task-old-unarchived",
        project_id.as_str(),
        &session_id,
        "paused",
        "ep-superseded",
        false,
    )
    .await;
    create_task_in_db_with_execution_plan(
        &shared,
        "task-current-archived",
        project_id.as_str(),
        &session_id,
        "paused",
        "ep-current",
        true,
    )
    .await;
    create_task_in_db_with_execution_plan(
        &shared,
        "task-current-merged",
        project_id.as_str(),
        &session_id,
        "merged",
        "ep-current",
        false,
    )
    .await;

    let counts = repo.get_group_counts(&project_id, None).await.unwrap();

    assert_eq!(
        counts.in_progress, 0,
        "superseded/archived tasks must not keep the session in_progress"
    );
    assert_eq!(counts.accepted, 0);
    assert_eq!(
        counts.done, 1,
        "only the current unarchived execution-plan tasks should classify the session"
    );
}

#[tokio::test]
async fn test_get_group_counts_accepted_no_tasks_accepted() {
    let project_id = ProjectId::new();
    let (_db, shared, repo) = setup_shared_test_db();
    {
        let conn = shared.lock().await;
        insert_test_project(&conn, &project_id, "Test Project", "/test/path");
    }

    let session = create_test_session(&project_id, Some("Accepted No Tasks"));
    let session_id = session.id.as_str().to_string();
    repo.create(session).await.unwrap();

    update_session_status(&shared, &session_id, "accepted").await;

    // No tasks — falls into accepted sub-group
    let counts = repo.get_group_counts(&project_id, None).await.unwrap();

    assert_eq!(counts.drafts, 0);
    assert_eq!(counts.in_progress, 0);
    assert_eq!(counts.accepted, 1);
    assert_eq!(counts.done, 0);
    assert_eq!(counts.archived, 0);
}

#[tokio::test]
async fn test_get_group_counts_accepted_with_mix_active_and_idle() {
    let project_id = ProjectId::new();
    let (_db, shared, repo) = setup_shared_test_db();
    {
        let conn = shared.lock().await;
        insert_test_project(&conn, &project_id, "Test Project", "/test/path");
    }

    let session = create_test_session(&project_id, Some("Mix Active Idle"));
    let session_id = session.id.as_str().to_string();
    repo.create(session).await.unwrap();

    update_session_status(&shared, &session_id, "accepted").await;

    // Mix: one active (executing) + one idle (backlog) — active takes precedence
    create_task_in_db(
        &shared,
        "task-003",
        project_id.as_str(),
        &session_id,
        "executing",
    )
    .await;
    create_task_in_db(
        &shared,
        "task-004",
        project_id.as_str(),
        &session_id,
        "backlog",
    )
    .await;

    let counts = repo.get_group_counts(&project_id, None).await.unwrap();

    assert_eq!(
        counts.in_progress, 1,
        "Session with active tasks should be in_progress"
    );
    assert_eq!(counts.accepted, 0);
    assert_eq!(counts.done, 0);
}

#[tokio::test]
async fn test_get_group_counts_multiple_groups_simultaneously() {
    let project_id = ProjectId::new();
    let (_db, shared, repo) = setup_shared_test_db();
    {
        let conn = shared.lock().await;
        insert_test_project(&conn, &project_id, "Test Project", "/test/path");
    }

    // Draft (active)
    let draft = create_test_session(&project_id, Some("Draft"));
    repo.create(draft).await.unwrap();

    // Archived
    let archived = create_test_session(&project_id, Some("Archived"));
    let archived_id = archived.id.as_str().to_string();
    repo.create(archived).await.unwrap();
    update_session_status(&shared, &archived_id, "archived").await;

    // In progress (accepted + active task)
    let in_prog = create_test_session(&project_id, Some("In Progress"));
    let in_prog_id = in_prog.id.as_str().to_string();
    repo.create(in_prog).await.unwrap();
    update_session_status(&shared, &in_prog_id, "accepted").await;
    create_task_in_db(
        &shared,
        "task-005",
        project_id.as_str(),
        &in_prog_id,
        "reviewing",
    )
    .await;

    // Accepted (no tasks)
    let accepted = create_test_session(&project_id, Some("Accepted"));
    let accepted_id = accepted.id.as_str().to_string();
    repo.create(accepted).await.unwrap();
    update_session_status(&shared, &accepted_id, "accepted").await;

    // Done (all terminal)
    let done = create_test_session(&project_id, Some("Done"));
    let done_id = done.id.as_str().to_string();
    repo.create(done).await.unwrap();
    update_session_status(&shared, &done_id, "accepted").await;
    create_task_in_db(
        &shared,
        "task-006",
        project_id.as_str(),
        &done_id,
        "approved",
    )
    .await;

    let counts = repo.get_group_counts(&project_id, None).await.unwrap();

    assert_eq!(counts.drafts, 1);
    assert_eq!(counts.in_progress, 1);
    assert_eq!(counts.accepted, 1);
    assert_eq!(counts.done, 1);
    assert_eq!(counts.archived, 1);
}

#[tokio::test]
async fn test_get_group_counts_archived() {
    let project_id = ProjectId::new();
    let (_db, shared, repo) = setup_shared_test_db();
    {
        let conn = shared.lock().await;
        insert_test_project(&conn, &project_id, "Test Project", "/test/path");
    }

    let session = create_test_session(&project_id, Some("Archived Session"));
    let session_id = session.id.as_str().to_string();
    repo.create(session).await.unwrap();

    update_session_status(&shared, &session_id, "archived").await;

    let counts = repo.get_group_counts(&project_id, None).await.unwrap();

    assert_eq!(counts.archived, 1);
    assert_eq!(counts.drafts, 0);
}

// ==================== LIST BY GROUP TESTS ====================

#[tokio::test]
async fn test_list_by_group_pagination_first_page() {
    let db = setup_test_db();
    let project_id = ProjectId::new();
    create_test_project(&db, &project_id, "Test Project", "/test/path");

    let repo = SqliteIdeationSessionRepository::new(db.new_connection());

    // Create 25 draft sessions
    for i in 0..25 {
        let session = create_test_session(&project_id, Some(&format!("Draft {i}")));
        repo.create(session).await.unwrap();
    }

    let (sessions, total) = repo
        .list_by_group(&project_id, "drafts", 0, 20, None)
        .await
        .unwrap();

    assert_eq!(total, 25);
    assert_eq!(sessions.len(), 20);
}

#[tokio::test]
async fn test_list_by_group_empty_group() {
    let db = setup_test_db();
    let project_id = ProjectId::new();
    create_test_project(&db, &project_id, "Test Project", "/test/path");

    let repo = SqliteIdeationSessionRepository::new(db.new_connection());

    let (sessions, total) = repo
        .list_by_group(&project_id, "drafts", 0, 20, None)
        .await
        .unwrap();

    assert_eq!(total, 0);
    assert!(sessions.is_empty());
}

#[tokio::test]
async fn test_list_by_group_invalid_group_returns_error() {
    let db = setup_test_db();
    let project_id = ProjectId::new();
    create_test_project(&db, &project_id, "Test Project", "/test/path");

    let repo = SqliteIdeationSessionRepository::new(db.new_connection());

    let result = repo
        .list_by_group(&project_id, "nonexistent_group", 0, 20, None)
        .await;

    assert!(result.is_err());
    let err_msg = result.unwrap_err().to_string();
    assert!(
        err_msg.contains("Unknown session group"),
        "Expected 'Unknown session group' error, got: {err_msg}"
    );
}

#[tokio::test]
async fn test_list_by_group_sort_order_updated_at_desc() {
    let project_id = ProjectId::new();
    let (_db, shared, repo) = setup_shared_test_db();
    {
        let conn = shared.lock().await;
        insert_test_project(&conn, &project_id, "Test Project", "/test/path");
    }

    let s1 = create_test_session(&project_id, Some("First Created"));
    let s2 = create_test_session(&project_id, Some("Second Created"));
    let s1_id = s1.id.as_str().to_string();
    let s2_id = s2.id.as_str().to_string();
    repo.create(s1).await.unwrap();
    repo.create(s2).await.unwrap();

    // Update s1 after s2 so it has a later updated_at
    tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;
    {
        let conn = shared.lock().await;
        conn.execute(
            "UPDATE ideation_sessions SET updated_at = strftime('%Y-%m-%dT%H:%M:%f+00:00', 'now') WHERE id = ?1",
            rusqlite::params![s1_id],
        )
        .unwrap();
    }

    let (sessions, _) = repo
        .list_by_group(&project_id, "drafts", 0, 20, None)
        .await
        .unwrap();

    assert_eq!(sessions.len(), 2);
    assert_eq!(
        sessions[0].session.id.as_str(),
        s1_id,
        "Most recently updated session should be first"
    );
    assert_eq!(sessions[1].session.id.as_str(), s2_id);
}

#[tokio::test]
async fn test_list_by_group_progress_data_for_accepted_subgroups() {
    let project_id = ProjectId::new();
    let (_db, shared, repo) = setup_shared_test_db();
    {
        let conn = shared.lock().await;
        insert_test_project(&conn, &project_id, "Test Project", "/test/path");
    }

    let session = create_test_session(&project_id, Some("In Progress Session"));
    let session_id = session.id.as_str().to_string();
    repo.create(session).await.unwrap();

    update_session_status(&shared, &session_id, "accepted").await;

    // Add tasks: 1 active, 1 idle, 1 terminal
    create_task_in_db(
        &shared,
        "task-p1",
        project_id.as_str(),
        &session_id,
        "executing",
    )
    .await;
    create_task_in_db(
        &shared,
        "task-p2",
        project_id.as_str(),
        &session_id,
        "backlog",
    )
    .await;
    create_task_in_db(
        &shared,
        "task-p3",
        project_id.as_str(),
        &session_id,
        "merged",
    )
    .await;

    let (sessions, total) = repo
        .list_by_group(&project_id, "in_progress", 0, 20, None)
        .await
        .unwrap();

    assert_eq!(total, 1);
    assert_eq!(sessions.len(), 1);

    let progress = sessions[0]
        .progress
        .as_ref()
        .expect("Progress should be populated for in_progress group");
    assert_eq!(progress.active, 1, "Should have 1 active task");
    assert_eq!(progress.idle, 1, "Should have 1 idle task");
    assert_eq!(progress.done, 1, "Should have 1 done task");
    assert_eq!(progress.total, 3, "Should have 3 total tasks");
}

#[tokio::test]
async fn test_list_by_group_done_scopes_progress_to_current_unarchived_execution_plan_tasks() {
    let project_id = ProjectId::new();
    let (_db, shared, repo) = setup_shared_test_db();
    {
        let conn = shared.lock().await;
        insert_test_project(&conn, &project_id, "Test Project", "/test/path");
    }

    let session = create_test_session(&project_id, Some("Done Current Plan Only"));
    let session_id = session.id.as_str().to_string();
    repo.create(session.clone()).await.unwrap();
    update_session_status(&shared, &session_id, "accepted").await;

    create_execution_plan_in_db(&shared, "ep-old", &session_id, "superseded").await;
    create_execution_plan_in_db(&shared, "ep-live", &session_id, "active").await;

    create_task_in_db_with_execution_plan(
        &shared,
        "task-old-paused",
        project_id.as_str(),
        &session_id,
        "paused",
        "ep-old",
        false,
    )
    .await;
    create_task_in_db_with_execution_plan(
        &shared,
        "task-live-paused-archived",
        project_id.as_str(),
        &session_id,
        "paused",
        "ep-live",
        true,
    )
    .await;
    create_task_in_db_with_execution_plan(
        &shared,
        "task-live-merged",
        project_id.as_str(),
        &session_id,
        "merged",
        "ep-live",
        false,
    )
    .await;

    let (in_progress_sessions, in_progress_total) = repo
        .list_by_group(&project_id, "in_progress", 0, 20, None)
        .await
        .unwrap();
    assert_eq!(
        in_progress_total, 0,
        "stale historical tasks must not place the session in in_progress"
    );
    assert!(in_progress_sessions.is_empty());

    let (done_sessions, done_total) = repo
        .list_by_group(&project_id, "done", 0, 20, None)
        .await
        .unwrap();

    assert_eq!(done_total, 1);
    assert_eq!(done_sessions.len(), 1);
    assert_eq!(done_sessions[0].session.id, session.id);

    let progress = done_sessions[0]
        .progress
        .as_ref()
        .expect("done group should include progress");
    assert_eq!(progress.active, 0);
    assert_eq!(progress.idle, 0);
    assert_eq!(progress.done, 1);
    assert_eq!(progress.total, 1);
}

#[tokio::test]
async fn test_list_by_group_parent_title_resolved() {
    let project_id = ProjectId::new();
    let (_db, shared, repo) = setup_shared_test_db();
    {
        let conn = shared.lock().await;
        insert_test_project(&conn, &project_id, "Test Project", "/test/path");
    }

    let parent = create_test_session(&project_id, Some("Parent Session Title"));
    let parent_id = parent.id.as_str().to_string();
    repo.create(parent).await.unwrap();

    // Child session linked to parent
    let child = IdeationSession::builder()
        .project_id(project_id.clone())
        .title("Child Session")
        .build();
    let child_id = child.id.as_str().to_string();
    repo.create(child).await.unwrap();

    // Set parent relationship
    {
        let conn = shared.lock().await;
        conn.execute(
            "UPDATE ideation_sessions SET parent_session_id = ?1 WHERE id = ?2",
            rusqlite::params![parent_id, child_id],
        )
        .unwrap();
    }

    let (sessions, _) = repo
        .list_by_group(&project_id, "drafts", 0, 20, None)
        .await
        .unwrap();

    // Find the child session in results
    let child_result = sessions
        .iter()
        .find(|s| s.session.id.as_str() == child_id)
        .expect("Child session should be in results");

    assert_eq!(
        child_result.parent_session_title.as_deref(),
        Some("Parent Session Title"),
        "Parent session title should be resolved"
    );
}

// ==================== RESET_AND_BEGIN_REVERIFY TESTS ====================

/// SQLite-specific: reset_and_begin_reverify uses atomic read-modify-write transaction.
///
/// Uses a real in-memory SQLite connection to verify atomicity and correct
/// SQL serialization. This catches bugs that memory-repo tests miss, such as
/// JSON field ordering differences, serde round-trip errors, and integer
/// storage precision issues.
///
/// Verifies:
/// - Returned (new_gen, cleared_snapshot) tuple is correct
/// - DB is updated atomically: status=reviewing, in_progress=true, gen=N+1
/// - All stale metadata fields are cleared in the stored JSON
#[tokio::test]
async fn test_reset_and_begin_reverify_sqlite_atomicity() {
    let db = setup_test_db();
    let project_id = ProjectId::new();
    create_test_project(&db, &project_id, "Test Project", "/test/path");
    let repo = SqliteIdeationSessionRepository::new(db.new_connection());

    // Create session — SQLite create() does not persist verification_generation from the builder,
    // so the session starts at generation=0 (the DB default). We test 0 → 1 increment.
    let session = IdeationSession::builder()
        .project_id(project_id.clone())
        .verification_status(VerificationStatus::Verified)
        .build();
    let session_id_obj = session.id.clone();
    repo.create(session).await.unwrap();

    // Set stale metadata: 2 rounds, 1 gap, convergence_reason set, parse_failures non-empty
    repo.update_verification_state(&session_id_obj, VerificationStatus::Verified, false)
        .await
        .unwrap();

    // Call reset_and_begin_reverify — must atomically clear metadata, increment gen, set Reviewing
    let (new_gen, cleared_snapshot) = repo
        .reset_and_begin_reverify(session_id_obj.as_str())
        .await
        .unwrap();

    // Assert returned tuple (DB starts at generation=0, so reset increments to 1)
    assert_eq!(new_gen, 1, "generation must be incremented from 0 to 1");
    assert_eq!(cleared_snapshot.generation, 1);
    assert_eq!(cleared_snapshot.status, VerificationStatus::Reviewing);
    assert!(cleared_snapshot.in_progress);
    assert!(
        cleared_snapshot.current_gaps.is_empty(),
        "returned current_gaps must be empty"
    );
    assert!(
        cleared_snapshot.rounds.is_empty(),
        "returned rounds must be empty"
    );
    assert!(
        cleared_snapshot.convergence_reason.is_none(),
        "returned convergence_reason must be None"
    );
    assert!(
        cleared_snapshot.best_round_index.is_none(),
        "returned best_round_index must be None"
    );
    assert_eq!(
        cleared_snapshot.current_round, 0,
        "returned current_round must be 0"
    );

    // Assert DB was updated atomically
    let updated = repo.get_by_id(&session_id_obj).await.unwrap().unwrap();
    assert_eq!(
        updated.verification_status,
        VerificationStatus::Reviewing,
        "DB status must be Reviewing"
    );
    assert!(
        updated.verification_in_progress,
        "DB in_progress must be true"
    );
    assert_eq!(
        updated.verification_generation, 1,
        "DB generation must be 1"
    );

    // Native reverify state lives in the returned snapshot rather than the session row.
    assert_eq!(updated.verification_current_round, None);
    assert_eq!(updated.verification_max_rounds, None);
    assert_eq!(updated.verification_gap_count, 0);
    assert_eq!(updated.verification_gap_score, None);
    assert_eq!(updated.verification_convergence_reason, None);
}

// ==================== SESSION PURPOSE FILTER TESTS ====================

/// Regression guard: verification child sessions must be excluded from list_by_group results,
/// verification_child_count must be correct, and existing column positions must not be corrupted.
#[tokio::test]
async fn test_list_by_group_excludes_verification_sessions_and_counts_children() {
    let db = setup_test_db();
    let project_id = ProjectId::new();
    create_test_project(&db, &project_id, "Test Project", "/test/purpose");

    let repo = SqliteIdeationSessionRepository::new(db.new_connection());

    // Create a regular (general) ideation session
    let parent_session = IdeationSession::builder()
        .project_id(project_id.clone())
        .title("Parent General Session")
        .build();
    repo.create(parent_session.clone()).await.unwrap();

    // Create a verification child session (should be excluded from list_by_group)
    let verification_child = IdeationSession::builder()
        .project_id(project_id.clone())
        .title("Verification Child")
        .session_purpose(SessionPurpose::Verification)
        .parent_session_id(parent_session.id.clone())
        .build();
    repo.create(verification_child.clone()).await.unwrap();

    // List drafts group (active sessions)
    let (sessions, total) = repo
        .list_by_group(&project_id, "drafts", 0, 50, None)
        .await
        .unwrap();

    // Verification session must be excluded
    assert_eq!(
        total, 1,
        "Only 1 session should be visible (verification child excluded)"
    );
    assert_eq!(sessions.len(), 1);

    let result = &sessions[0];

    // Regression guard: title must survive column-index changes
    assert_eq!(
        result.session.title,
        Some("Parent General Session".to_string()),
        "parent_session_title must not be corrupted by new columns"
    );

    // verification_child_count must be 1 for the parent session
    assert_eq!(
        result.verification_child_count, 1,
        "parent session should report 1 verification child"
    );

    // parent_session_title should be None (no parent for this session)
    assert!(
        result.parent_session_title.is_none(),
        "parent_session_title should be None for a root session"
    );
}

/// Regression guard: get_group_counts must exclude verification sessions from all counts.
#[tokio::test]
async fn test_get_group_counts_excludes_verification_sessions() {
    let db = setup_test_db();
    let project_id = ProjectId::new();
    create_test_project(&db, &project_id, "Test Project", "/test/counts");

    let repo = SqliteIdeationSessionRepository::new(db.new_connection());

    // Create 2 regular sessions
    for i in 0..2u32 {
        let session = IdeationSession::builder()
            .project_id(project_id.clone())
            .title(format!("General Session {i}"))
            .build();
        repo.create(session).await.unwrap();
    }

    // Create a verification session (must not be counted)
    let verification_session = IdeationSession::builder()
        .project_id(project_id.clone())
        .title("Verification Session")
        .session_purpose(SessionPurpose::Verification)
        .build();
    repo.create(verification_session).await.unwrap();

    let counts = repo.get_group_counts(&project_id, None).await.unwrap();

    // Only 2 general sessions should appear as drafts
    assert_eq!(
        counts.drafts, 2,
        "Drafts count must exclude verification sessions"
    );
}

// ==================== ARCHIVE CLEARS VERIFICATION_IN_PROGRESS TESTS ====================

#[tokio::test]
async fn test_archive_clears_verification_in_progress_when_set() {
    let db = setup_test_db();
    let project_id = ProjectId::new();
    create_test_project(&db, &project_id, "Test Project", "/test/path");
    let repo = SqliteIdeationSessionRepository::new(db.new_connection());

    let session = IdeationSession::builder()
        .project_id(project_id.clone())
        .build();
    repo.create(session.clone()).await.unwrap();

    // Set verification_in_progress = true
    repo.update_verification_state(&session.id, VerificationStatus::Reviewing, true)
        .await
        .unwrap();

    // Verify flag is set
    let before = repo.get_by_id(&session.id).await.unwrap().unwrap();
    assert!(before.verification_in_progress);

    // Archive should atomically clear the flag
    repo.update_status(&session.id, IdeationSessionStatus::Archived)
        .await
        .unwrap();

    let updated = repo.get_by_id(&session.id).await.unwrap().unwrap();
    assert_eq!(updated.status, IdeationSessionStatus::Archived);
    assert!(updated.archived_at.is_some());
    assert!(
        !updated.verification_in_progress,
        "verification_in_progress must be cleared on archive"
    );
}

#[tokio::test]
async fn test_archive_does_not_regress_when_verification_in_progress_already_false() {
    let db = setup_test_db();
    let project_id = ProjectId::new();
    create_test_project(&db, &project_id, "Test Project", "/test/path");
    let repo = SqliteIdeationSessionRepository::new(db.new_connection());

    let session = IdeationSession::builder()
        .project_id(project_id.clone())
        .build();
    repo.create(session.clone()).await.unwrap();

    // Verify flag is already false (default)
    let before = repo.get_by_id(&session.id).await.unwrap().unwrap();
    assert!(!before.verification_in_progress);

    // Archive — flag must remain false
    repo.update_status(&session.id, IdeationSessionStatus::Archived)
        .await
        .unwrap();

    let updated = repo.get_by_id(&session.id).await.unwrap().unwrap();
    assert_eq!(updated.status, IdeationSessionStatus::Archived);
    assert!(
        !updated.verification_in_progress,
        "verification_in_progress must remain false after archive"
    );
}

// ==================== STALE QUERY EXCLUDES ARCHIVED SESSIONS TESTS ====================

/// Defense-in-depth: archived session with verification_in_progress=1 must NOT appear in
/// get_stale_in_progress_sessions results, even if the flag was somehow set after archiving.
#[tokio::test]
async fn test_get_stale_in_progress_sessions_excludes_archived() {
    let (_db, shared, repo) = setup_shared_test_db();
    let project_id = ProjectId::new();
    {
        let conn = shared.lock().await;
        insert_test_project(&conn, &project_id, "Test Project", "/test/path");
    }

    // Create an archived session and force verification_in_progress=1 via raw SQL
    // to simulate the defense-in-depth scenario (bypassing normal update_status guard).
    let session = IdeationSession::builder()
        .project_id(project_id.clone())
        .build();
    repo.create(session.clone()).await.unwrap();

    let stale_before = "2020-01-01T00:00:00+00:00";
    {
        let conn = shared.lock().await;
        conn.execute(
            "UPDATE ideation_sessions SET status = 'archived', verification_in_progress = 1, updated_at = ?1 WHERE id = ?2",
            rusqlite::params![stale_before, session.id.as_str()],
        )
        .unwrap();
    }

    let stale_cutoff = chrono::Utc::now();
    let results = repo
        .get_stale_in_progress_sessions(stale_cutoff)
        .await
        .unwrap();
    assert!(
        results.iter().all(|s| s.id != session.id),
        "archived session must be excluded from stale query even with verification_in_progress=1"
    );
}

/// Active session with stale verification_in_progress=1 MUST appear in results.
#[tokio::test]
async fn test_get_stale_in_progress_sessions_includes_active() {
    let (_db, shared, repo) = setup_shared_test_db();
    let project_id = ProjectId::new();
    {
        let conn = shared.lock().await;
        insert_test_project(&conn, &project_id, "Test Project", "/test/path");
    }

    let session = IdeationSession::builder()
        .project_id(project_id.clone())
        .build();
    repo.create(session.clone()).await.unwrap();

    // Force stale updated_at and verification_in_progress=1 while keeping status='active'
    let stale_before = "2020-01-01T00:00:00+00:00";
    {
        let conn = shared.lock().await;
        conn.execute(
            "UPDATE ideation_sessions SET verification_in_progress = 1, updated_at = ?1 WHERE id = ?2",
            rusqlite::params![stale_before, session.id.as_str()],
        )
        .unwrap();
    }

    let stale_cutoff = chrono::Utc::now();
    let results = repo
        .get_stale_in_progress_sessions(stale_cutoff)
        .await
        .unwrap();
    assert!(
        results.iter().any(|s| s.id == session.id),
        "active stale session must be included in stale query"
    );
}

#[tokio::test]
async fn test_touch_updated_at_bumps_timestamp() {
    let db = setup_test_db();
    let shared = db.shared_conn();
    let repo = SqliteIdeationSessionRepository::from_shared(shared.clone());
    let project_id = ProjectId::new();
    {
        let conn = shared.lock().await;
        insert_test_project(&conn, &project_id, "Test Project", "/test/touch");
    }

    let session = IdeationSession::builder()
        .project_id(project_id.clone())
        .build();
    repo.create(session.clone()).await.unwrap();

    // Force a stale updated_at
    let stale_ts = "2020-01-01T00:00:00+00:00";
    {
        let conn = shared.lock().await;
        conn.execute(
            "UPDATE ideation_sessions SET updated_at = ?1 WHERE id = ?2",
            rusqlite::params![stale_ts, session.id.as_str()],
        )
        .unwrap();
    }

    // Verify it is stale
    let before = repo.get_by_id(&session.id).await.unwrap().unwrap();
    assert_eq!(
        before.updated_at.format("%Y").to_string(),
        "2020",
        "updated_at must be stale before touch"
    );

    repo.touch_updated_at(session.id.as_str()).await.unwrap();

    let after = repo.get_by_id(&session.id).await.unwrap().unwrap();
    assert!(
        after.updated_at > before.updated_at,
        "touch_updated_at must bump updated_at"
    );
}

#[tokio::test]
async fn test_touch_updated_at_keeps_session_out_of_archival() {
    let db = setup_test_db();
    let shared = db.shared_conn();
    let repo = SqliteIdeationSessionRepository::from_shared(shared.clone());
    let project_id = ProjectId::new();
    {
        let conn = shared.lock().await;
        insert_test_project(&conn, &project_id, "Test Project", "/test/archival");
    }

    // Create an external session with phase='created'
    let session = IdeationSession::builder()
        .project_id(project_id.clone())
        .origin(SessionOrigin::External)
        .external_activity_phase("created")
        .build();
    repo.create(session.clone()).await.unwrap();

    // Force a stale updated_at so it would normally be archived
    let stale_ts = "2020-01-01T00:00:00+00:00";
    {
        let conn = shared.lock().await;
        conn.execute(
            "UPDATE ideation_sessions SET updated_at = ?1 WHERE id = ?2",
            rusqlite::params![stale_ts, session.id.as_str()],
        )
        .unwrap();
    }

    // Confirm it is eligible for archival before touch
    let stale_cutoff = chrono::Utc::now();
    let before_touch = repo
        .list_active_external_sessions_for_archival(Some(stale_cutoff))
        .await
        .unwrap();
    assert!(
        before_touch.iter().any(|s| s.id == session.id),
        "stale external session must appear in archival list before touch"
    );

    // Simulate message creation: touch updated_at
    repo.touch_updated_at(session.id.as_str()).await.unwrap();

    // Now it must NOT appear in archival list because updated_at is fresh
    let after_touch = repo
        .list_active_external_sessions_for_archival(Some(stale_cutoff))
        .await
        .unwrap();
    assert!(
        !after_touch.iter().any(|s| s.id == session.id),
        "external session with recent activity must not appear in archival list after touch"
    );
}

// ==================== PENDING INITIAL PROMPT TESTS ====================

#[tokio::test]
async fn test_set_pending_initial_prompt_stores_value() {
    let db = setup_test_db();
    let project_id = ProjectId::new();
    create_test_project(&db, &project_id, "Test Project", "/test/path");

    let repo = SqliteIdeationSessionRepository::new(db.new_connection());
    let session = create_test_session(&project_id, None);
    repo.create(session.clone()).await.unwrap();

    repo.set_pending_initial_prompt(session.id.as_str(), Some("Hello world".to_string()))
        .await
        .unwrap();

    let fetched = repo.get_by_id(&session.id).await.unwrap().unwrap();
    assert_eq!(
        fetched.pending_initial_prompt,
        Some("Hello world".to_string())
    );
}

#[tokio::test]
async fn test_set_pending_initial_prompt_clears_value() {
    let db = setup_test_db();
    let project_id = ProjectId::new();
    create_test_project(&db, &project_id, "Test Project", "/test/path");

    let repo = SqliteIdeationSessionRepository::new(db.new_connection());
    let session = create_test_session(&project_id, None);
    repo.create(session.clone()).await.unwrap();

    repo.set_pending_initial_prompt(session.id.as_str(), Some("Initial prompt".to_string()))
        .await
        .unwrap();
    repo.set_pending_initial_prompt(session.id.as_str(), None)
        .await
        .unwrap();

    let fetched = repo.get_by_id(&session.id).await.unwrap().unwrap();
    assert_eq!(fetched.pending_initial_prompt, None);
}

#[tokio::test]
async fn test_claim_pending_session_returns_none_when_empty() {
    let db = setup_test_db();
    let project_id = ProjectId::new();
    create_test_project(&db, &project_id, "Test Project", "/test/path");

    let repo = SqliteIdeationSessionRepository::new(db.new_connection());

    let result = repo
        .claim_pending_session_for_project(project_id.as_str())
        .await
        .unwrap();
    assert!(result.is_none());
}

#[tokio::test]
async fn test_claim_pending_session_returns_session_and_clears_prompt() {
    let db = setup_test_db();
    let project_id = ProjectId::new();
    create_test_project(&db, &project_id, "Test Project", "/test/path");

    let repo = SqliteIdeationSessionRepository::new(db.new_connection());
    let session = create_test_session(&project_id, None);
    repo.create(session.clone()).await.unwrap();
    repo.set_pending_initial_prompt(session.id.as_str(), Some("Auto-launch me".to_string()))
        .await
        .unwrap();

    let result = repo
        .claim_pending_session_for_project(project_id.as_str())
        .await
        .unwrap();

    assert!(result.is_some());
    let (claimed_id, prompt) = result.unwrap();
    assert_eq!(claimed_id, session.id.as_str());
    assert_eq!(prompt, "Auto-launch me");

    // Prompt must be cleared after claim
    let fetched = repo.get_by_id(&session.id).await.unwrap().unwrap();
    assert_eq!(fetched.pending_initial_prompt, None);
}

#[tokio::test]
async fn test_claim_pending_session_is_idempotent_second_claim_returns_none() {
    let db = setup_test_db();
    let project_id = ProjectId::new();
    create_test_project(&db, &project_id, "Test Project", "/test/path");

    let repo = SqliteIdeationSessionRepository::new(db.new_connection());
    let session = create_test_session(&project_id, None);
    repo.create(session.clone()).await.unwrap();
    repo.set_pending_initial_prompt(session.id.as_str(), Some("Once only".to_string()))
        .await
        .unwrap();

    let first = repo
        .claim_pending_session_for_project(project_id.as_str())
        .await
        .unwrap();
    assert!(first.is_some());

    let second = repo
        .claim_pending_session_for_project(project_id.as_str())
        .await
        .unwrap();
    assert!(
        second.is_none(),
        "second claim must return None after prompt cleared"
    );
}

#[tokio::test]
async fn test_claim_pending_session_respects_status_active_filter() {
    let db = setup_test_db();
    let project_id = ProjectId::new();
    create_test_project(&db, &project_id, "Test Project", "/test/path");

    let repo = SqliteIdeationSessionRepository::new(db.new_connection());
    let session = create_test_session(&project_id, None);
    repo.create(session.clone()).await.unwrap();
    repo.set_pending_initial_prompt(session.id.as_str(), Some("Archived session".to_string()))
        .await
        .unwrap();

    // Archive the session — status changes from 'active' to 'archived'
    repo.update_status(&session.id, IdeationSessionStatus::Archived)
        .await
        .unwrap();

    // Claim must ignore non-active sessions
    let result = repo
        .claim_pending_session_for_project(project_id.as_str())
        .await
        .unwrap();
    assert!(result.is_none(), "archived session must not be claimed");
}

#[tokio::test]
async fn test_claim_pending_session_uses_fifo_ordering() {
    let db = setup_test_db();
    let project_id = ProjectId::new();
    create_test_project(&db, &project_id, "Test Project", "/test/path");

    let repo = SqliteIdeationSessionRepository::new(db.new_connection());

    // Create two sessions — insert them in order; oldest gets claimed first
    let session_a = create_test_session(&project_id, Some("First"));
    let session_b = create_test_session(&project_id, Some("Second"));
    repo.create(session_a.clone()).await.unwrap();
    repo.create(session_b.clone()).await.unwrap();

    repo.set_pending_initial_prompt(session_a.id.as_str(), Some("Prompt A".to_string()))
        .await
        .unwrap();
    repo.set_pending_initial_prompt(session_b.id.as_str(), Some("Prompt B".to_string()))
        .await
        .unwrap();

    let first_claim = repo
        .claim_pending_session_for_project(project_id.as_str())
        .await
        .unwrap()
        .unwrap();

    // session_a was created first — it must be claimed first (FIFO)
    assert_eq!(first_claim.0, session_a.id.as_str());
    assert_eq!(first_claim.1, "Prompt A");

    let second_claim = repo
        .claim_pending_session_for_project(project_id.as_str())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(second_claim.0, session_b.id.as_str());
    assert_eq!(second_claim.1, "Prompt B");
}

#[tokio::test]
async fn test_list_projects_with_pending_sessions_empty() {
    let db = setup_test_db();
    let project_id = ProjectId::new();
    create_test_project(&db, &project_id, "Test Project", "/test/path");

    let repo = SqliteIdeationSessionRepository::new(db.new_connection());

    let result = repo.list_projects_with_pending_sessions().await.unwrap();
    assert!(result.is_empty());
}

#[tokio::test]
async fn test_list_projects_with_pending_sessions_returns_project() {
    let db = setup_test_db();
    let project_id = ProjectId::new();
    create_test_project(&db, &project_id, "Test Project", "/test/path");

    let repo = SqliteIdeationSessionRepository::new(db.new_connection());
    let session = create_test_session(&project_id, None);
    repo.create(session.clone()).await.unwrap();
    repo.set_pending_initial_prompt(session.id.as_str(), Some("pending".to_string()))
        .await
        .unwrap();

    let result = repo.list_projects_with_pending_sessions().await.unwrap();
    assert_eq!(result, vec![project_id.as_str().to_string()]);
}

#[tokio::test]
async fn test_list_projects_with_pending_sessions_deduplicates() {
    let db = setup_test_db();
    let project_id = ProjectId::new();
    create_test_project(&db, &project_id, "Test Project", "/test/path");

    let repo = SqliteIdeationSessionRepository::new(db.new_connection());

    // Two sessions in same project both have pending prompts
    let session_a = create_test_session(&project_id, Some("A"));
    let session_b = create_test_session(&project_id, Some("B"));
    repo.create(session_a.clone()).await.unwrap();
    repo.create(session_b.clone()).await.unwrap();
    repo.set_pending_initial_prompt(session_a.id.as_str(), Some("p1".to_string()))
        .await
        .unwrap();
    repo.set_pending_initial_prompt(session_b.id.as_str(), Some("p2".to_string()))
        .await
        .unwrap();

    let result = repo.list_projects_with_pending_sessions().await.unwrap();
    // DISTINCT — must appear exactly once
    assert_eq!(result.len(), 1);
    assert_eq!(result[0], project_id.as_str().to_string());
}

#[tokio::test]
async fn test_list_projects_with_pending_sessions_excludes_non_active() {
    let db = setup_test_db();
    let project_id = ProjectId::new();
    create_test_project(&db, &project_id, "Test Project", "/test/path");

    let repo = SqliteIdeationSessionRepository::new(db.new_connection());
    let session = create_test_session(&project_id, None);
    repo.create(session.clone()).await.unwrap();
    repo.set_pending_initial_prompt(session.id.as_str(), Some("archived".to_string()))
        .await
        .unwrap();
    repo.update_status(&session.id, IdeationSessionStatus::Archived)
        .await
        .unwrap();

    let result = repo.list_projects_with_pending_sessions().await.unwrap();
    assert!(
        result.is_empty(),
        "archived sessions must not appear in pending projects list"
    );
}

#[tokio::test]
async fn test_list_projects_with_pending_sessions_excludes_cleared_prompts() {
    let db = setup_test_db();
    let project_id = ProjectId::new();
    create_test_project(&db, &project_id, "Test Project", "/test/path");

    let repo = SqliteIdeationSessionRepository::new(db.new_connection());
    let session = create_test_session(&project_id, None);
    repo.create(session.clone()).await.unwrap();
    repo.set_pending_initial_prompt(session.id.as_str(), Some("temp".to_string()))
        .await
        .unwrap();
    repo.set_pending_initial_prompt(session.id.as_str(), None)
        .await
        .unwrap();

    let result = repo.list_projects_with_pending_sessions().await.unwrap();
    assert!(
        result.is_empty(),
        "cleared prompt must not appear in pending projects list"
    );
}

// ============================================================================
// count_pending_sessions_for_project tests
// ============================================================================

#[tokio::test]
async fn test_count_pending_sessions_for_project_basic() {
    let db = setup_test_db();
    let project_id = ProjectId::new();
    let other_project_id = ProjectId::new();
    create_test_project(&db, &project_id, "Project A", "/path/a");
    create_test_project(&db, &other_project_id, "Project B", "/path/b");

    let repo = SqliteIdeationSessionRepository::new(db.new_connection());

    // No pending sessions initially
    let count = repo
        .count_pending_sessions_for_project(&project_id)
        .await
        .unwrap();
    assert_eq!(count, 0, "no pending sessions initially");

    // Add a session with pending prompt
    let s1 = create_test_session(&project_id, Some("Session 1"));
    repo.create(s1.clone()).await.unwrap();
    repo.set_pending_initial_prompt(s1.id.as_str(), Some("hello".to_string()))
        .await
        .unwrap();

    let count = repo
        .count_pending_sessions_for_project(&project_id)
        .await
        .unwrap();
    assert_eq!(count, 1, "one pending session");

    // Add another pending session
    let s2 = create_test_session(&project_id, Some("Session 2"));
    repo.create(s2.clone()).await.unwrap();
    repo.set_pending_initial_prompt(s2.id.as_str(), Some("world".to_string()))
        .await
        .unwrap();

    let count = repo
        .count_pending_sessions_for_project(&project_id)
        .await
        .unwrap();
    assert_eq!(count, 2, "two pending sessions");

    // Other project should have 0
    let count_other = repo
        .count_pending_sessions_for_project(&other_project_id)
        .await
        .unwrap();
    assert_eq!(count_other, 0, "other project has no pending sessions");
}

#[tokio::test]
async fn test_count_pending_sessions_excludes_archived() {
    let db = setup_test_db();
    let project_id = ProjectId::new();
    create_test_project(&db, &project_id, "Project A", "/path/a");

    let repo = SqliteIdeationSessionRepository::new(db.new_connection());

    let session = create_test_session(&project_id, None);
    repo.create(session.clone()).await.unwrap();
    repo.set_pending_initial_prompt(session.id.as_str(), Some("queued".to_string()))
        .await
        .unwrap();
    // Archive the session — should be excluded from count
    repo.update_status(&session.id, IdeationSessionStatus::Archived)
        .await
        .unwrap();

    let count = repo
        .count_pending_sessions_for_project(&project_id)
        .await
        .unwrap();
    assert_eq!(count, 0, "archived sessions must not count as pending");
}

#[tokio::test]
async fn test_count_pending_sessions_excludes_cleared_prompt() {
    let db = setup_test_db();
    let project_id = ProjectId::new();
    create_test_project(&db, &project_id, "Project A", "/path/a");

    let repo = SqliteIdeationSessionRepository::new(db.new_connection());

    let session = create_test_session(&project_id, None);
    repo.create(session.clone()).await.unwrap();
    repo.set_pending_initial_prompt(session.id.as_str(), Some("temp".to_string()))
        .await
        .unwrap();
    // Clear the prompt
    repo.set_pending_initial_prompt(session.id.as_str(), None)
        .await
        .unwrap();

    let count = repo
        .count_pending_sessions_for_project(&project_id)
        .await
        .unwrap();
    assert_eq!(count, 0, "cleared prompt must not count as pending");
}

// ============================================================================
// set_pending_initial_prompt_if_unset tests (capacity-full guard)
// ============================================================================

#[tokio::test]
async fn test_set_pending_if_unset_sets_when_null() {
    let db = setup_test_db();
    let project_id = ProjectId::new();
    create_test_project(&db, &project_id, "Test Project", "/test/path");

    let repo = SqliteIdeationSessionRepository::new(db.new_connection());
    let session = create_test_session(&project_id, None);
    repo.create(session.clone()).await.unwrap();

    // First call: no existing prompt → should set and return true.
    let result = repo
        .set_pending_initial_prompt_if_unset(session.id.as_str(), "First message".to_string())
        .await
        .unwrap();
    assert!(result, "must return true when prompt was NULL");

    let fetched = repo.get_by_id(&session.id).await.unwrap().unwrap();
    assert_eq!(
        fetched.pending_initial_prompt.as_deref(),
        Some("First message")
    );
}

#[tokio::test]
async fn test_set_pending_if_unset_rejects_when_already_set() {
    let db = setup_test_db();
    let project_id = ProjectId::new();
    create_test_project(&db, &project_id, "Test Project", "/test/path");

    let repo = SqliteIdeationSessionRepository::new(db.new_connection());
    let session = create_test_session(&project_id, None);
    repo.create(session.clone()).await.unwrap();

    // Pre-set a prompt so the guard fires.
    repo.set_pending_initial_prompt(session.id.as_str(), Some("Existing prompt".to_string()))
        .await
        .unwrap();

    // Second call: prompt already set → must return false and NOT overwrite.
    let result = repo
        .set_pending_initial_prompt_if_unset(session.id.as_str(), "Overwrite attempt".to_string())
        .await
        .unwrap();
    assert!(!result, "must return false when prompt is already set");

    let fetched = repo.get_by_id(&session.id).await.unwrap().unwrap();
    assert_eq!(
        fetched.pending_initial_prompt.as_deref(),
        Some("Existing prompt"),
        "existing prompt must not be overwritten"
    );
}

#[tokio::test]
async fn test_set_pending_if_unset_returns_false_for_unknown_session() {
    let db = setup_test_db();
    let project_id = ProjectId::new();
    create_test_project(&db, &project_id, "Test Project", "/test/path");

    let repo = SqliteIdeationSessionRepository::new(db.new_connection());
    // Session does not exist — no rows matched → false.
    let result = repo
        .set_pending_initial_prompt_if_unset("nonexistent-id", "Hello".to_string())
        .await
        .unwrap();
    assert!(!result, "must return false when session does not exist");
}

// ==================== PENDING_INITIAL_PROMPT INVARIANT TESTS ====================

/// update_status(Accepted) must atomically clear pending_initial_prompt.
#[tokio::test]
async fn test_update_status_accepted_clears_pending_prompt() {
    let db = setup_test_db();
    let project_id = ProjectId::new();
    create_test_project(&db, &project_id, "Test Project", "/test/path");

    let repo = SqliteIdeationSessionRepository::new(db.new_connection());
    let session = create_test_session(&project_id, Some("Pending accepted"));
    repo.create(session.clone()).await.unwrap();

    repo.set_pending_initial_prompt(session.id.as_str(), Some("queued prompt".to_string()))
        .await
        .unwrap();

    // Verify prompt is set before transition
    let before = repo.get_by_id(&session.id).await.unwrap().unwrap();
    assert!(before.pending_initial_prompt.is_some());

    repo.update_status(&session.id, IdeationSessionStatus::Accepted)
        .await
        .unwrap();

    let after = repo.get_by_id(&session.id).await.unwrap().unwrap();
    assert_eq!(after.status, IdeationSessionStatus::Accepted);
    assert!(
        after.pending_initial_prompt.is_none(),
        "update_status(Accepted) must atomically clear pending_initial_prompt"
    );
}

/// update_status(Archived) must atomically clear pending_initial_prompt.
#[tokio::test]
async fn test_update_status_archived_clears_pending_prompt() {
    let db = setup_test_db();
    let project_id = ProjectId::new();
    create_test_project(&db, &project_id, "Test Project", "/test/path");

    let repo = SqliteIdeationSessionRepository::new(db.new_connection());
    let session = create_test_session(&project_id, Some("Pending archived"));
    repo.create(session.clone()).await.unwrap();

    repo.set_pending_initial_prompt(session.id.as_str(), Some("queued prompt".to_string()))
        .await
        .unwrap();

    let before = repo.get_by_id(&session.id).await.unwrap().unwrap();
    assert!(before.pending_initial_prompt.is_some());

    repo.update_status(&session.id, IdeationSessionStatus::Archived)
        .await
        .unwrap();

    let after = repo.get_by_id(&session.id).await.unwrap().unwrap();
    assert_eq!(after.status, IdeationSessionStatus::Archived);
    assert!(
        after.pending_initial_prompt.is_none(),
        "update_status(Archived) must atomically clear pending_initial_prompt"
    );
}

/// update_status(Active) must NOT clear pending_initial_prompt.
#[tokio::test]
async fn test_update_status_active_preserves_pending_prompt() {
    let db = setup_test_db();
    let project_id = ProjectId::new();
    create_test_project(&db, &project_id, "Test Project", "/test/path");

    let repo = SqliteIdeationSessionRepository::new(db.new_connection());
    // Start with an archived session that has a pending prompt
    let mut session = create_test_session(&project_id, Some("Reactivate with prompt"));
    session.archive();
    repo.create(session.clone()).await.unwrap();

    repo.set_pending_initial_prompt(session.id.as_str(), Some("queued prompt".to_string()))
        .await
        .unwrap();

    repo.update_status(&session.id, IdeationSessionStatus::Active)
        .await
        .unwrap();

    let after = repo.get_by_id(&session.id).await.unwrap().unwrap();
    assert_eq!(after.status, IdeationSessionStatus::Active);
    assert_eq!(
        after.pending_initial_prompt.as_deref(),
        Some("queued prompt"),
        "update_status(Active) must NOT clear pending_initial_prompt"
    );
}

/// Defense-in-depth: list_by_group returns has_pending_prompt=false for accepted sessions
/// even if pending_initial_prompt is stale in the DB (SQL guard: AND s.status = 'active').
#[tokio::test]
async fn test_list_has_pending_prompt_false_for_accepted() {
    let (_db, shared, repo) = setup_shared_test_db();
    let project_id = ProjectId::new();
    {
        let conn = shared.lock().await;
        insert_test_project(&conn, &project_id, "Test Project", "/test/path");
    }

    let session = create_test_session(&project_id, Some("Accepted stale prompt"));
    repo.create(session.clone()).await.unwrap();

    // Simulate stale state: set pending_initial_prompt and accepted status directly in SQL,
    // bypassing update_status() to prove the SQL guard is the last line of defense.
    {
        let conn = shared.lock().await;
        conn.execute(
            "UPDATE ideation_sessions SET status = 'accepted', converted_at = strftime('%Y-%m-%dT%H:%M:%S+00:00', 'now'), pending_initial_prompt = 'stale prompt' WHERE id = ?1",
            rusqlite::params![session.id.as_str()],
        )
        .unwrap();
    }

    let (sessions, _) = repo
        .list_by_group(&project_id, "accepted", 0, 20, None)
        .await
        .unwrap();

    let found = sessions
        .iter()
        .find(|s| s.session.id == session.id)
        .expect("session must appear in accepted group");

    assert!(
        !found.has_pending_prompt,
        "has_pending_prompt must be false for accepted sessions even if DB field is stale"
    );
}

/// list_by_group returns has_pending_prompt=true for active sessions with pending_initial_prompt set.
#[tokio::test]
async fn test_list_has_pending_prompt_true_for_active() {
    let (_db, shared, repo) = setup_shared_test_db();
    let project_id = ProjectId::new();
    {
        let conn = shared.lock().await;
        insert_test_project(&conn, &project_id, "Test Project", "/test/path");
    }

    let session = create_test_session(&project_id, Some("Active with prompt"));
    repo.create(session.clone()).await.unwrap();

    repo.set_pending_initial_prompt(session.id.as_str(), Some("waiting prompt".to_string()))
        .await
        .unwrap();

    let (sessions, _) = repo
        .list_by_group(&project_id, "drafts", 0, 20, None)
        .await
        .unwrap();

    let found = sessions
        .iter()
        .find(|s| s.session.id == session.id)
        .expect("session must appear in drafts group");

    assert!(
        found.has_pending_prompt,
        "has_pending_prompt must be true for active sessions with pending_initial_prompt set"
    );
}

/// Regression guard: follow-up provenance columns after blocker_fingerprint must not
/// corrupt parent_session_title/progress mapping in list_by_group.
#[tokio::test]
async fn test_list_by_group_with_blocker_fingerprint_keeps_parent_title_and_progress_aligned() {
    let (_db, shared, repo) = setup_shared_test_db();
    let project_id = ProjectId::new();
    {
        let conn = shared.lock().await;
        insert_test_project(&conn, &project_id, "Test Project", "/test/path");
    }

    let parent = create_test_session(&project_id, Some("Parent Session Title"));
    let parent_id = parent.id.clone();
    repo.create(parent).await.unwrap();

    let child = IdeationSession::builder()
        .project_id(project_id.clone())
        .title("Follow-up Session")
        .parent_session_id(parent_id.clone())
        .source_context_type("review".to_string())
        .source_context_id("rev-1".to_string())
        .spawn_reason("out_of_scope_failure".to_string())
        .blocker_fingerprint("ood:task-1:deadbeef".to_string())
        .status(IdeationSessionStatus::Accepted)
        .build();
    let child_id = child.id.clone();
    repo.create(child).await.unwrap();

    create_task_in_db(
        &shared,
        &IdeationSessionId::new().to_string(),
        project_id.as_str(),
        parent_id.as_str(),
        "merged",
    )
    .await;
    create_task_in_db(
        &shared,
        &IdeationSessionId::new().to_string(),
        project_id.as_str(),
        child_id.as_str(),
        "approved",
    )
    .await;

    let (sessions, _) = repo
        .list_by_group(&project_id, "done", 0, 20, None)
        .await
        .unwrap();

    let found = sessions
        .iter()
        .find(|s| s.session.id == child_id)
        .expect("child session must appear in done group");

    assert_eq!(
        found.parent_session_title.as_deref(),
        Some("Parent Session Title"),
        "parent_session_title must stay aligned after follow-up provenance columns"
    );
    let progress = found
        .progress
        .as_ref()
        .expect("done group should include progress");
    assert_eq!(progress.done, 1);
    assert_eq!(progress.total, 1);
    assert_eq!(progress.active, 0);
    assert_eq!(progress.idle, 0);
}

// ==================== SEARCH FILTERING TESTS ====================

#[tokio::test]
async fn test_get_group_counts_search_filters_by_title_substring() {
    let db = setup_test_db();
    let project_id = ProjectId::new();
    create_test_project(&db, &project_id, "Test Project", "/test/path");

    let repo = SqliteIdeationSessionRepository::new(db.new_connection());
    let s1 = create_test_session(&project_id, Some("Auth refactor"));
    let s2 = create_test_session(&project_id, Some("Dashboard redesign"));
    let s3 = create_test_session(&project_id, Some("Auth improvements"));
    repo.create(s1).await.unwrap();
    repo.create(s2).await.unwrap();
    repo.create(s3).await.unwrap();

    let counts = repo
        .get_group_counts(&project_id, Some("auth"))
        .await
        .unwrap();
    // Only the 2 "Auth*" sessions should be counted in drafts
    assert_eq!(counts.drafts, 2);

    let counts_dash = repo
        .get_group_counts(&project_id, Some("Dashboard"))
        .await
        .unwrap();
    assert_eq!(counts_dash.drafts, 1);
}

#[tokio::test]
async fn test_get_group_counts_search_case_insensitive() {
    let db = setup_test_db();
    let project_id = ProjectId::new();
    create_test_project(&db, &project_id, "Test Project", "/test/path");

    let repo = SqliteIdeationSessionRepository::new(db.new_connection());
    let s1 = create_test_session(&project_id, Some("Auth Refactor"));
    repo.create(s1).await.unwrap();

    // All variants of casing should match
    let counts_lower = repo
        .get_group_counts(&project_id, Some("auth"))
        .await
        .unwrap();
    let counts_upper = repo
        .get_group_counts(&project_id, Some("AUTH"))
        .await
        .unwrap();
    let counts_mixed = repo
        .get_group_counts(&project_id, Some("Auth"))
        .await
        .unwrap();

    assert_eq!(counts_lower.drafts, 1, "lowercase search should match");
    assert_eq!(counts_upper.drafts, 1, "uppercase search should match");
    assert_eq!(counts_mixed.drafts, 1, "mixed-case search should match");
}

#[tokio::test]
async fn test_get_group_counts_empty_search_returns_all() {
    let db = setup_test_db();
    let project_id = ProjectId::new();
    create_test_project(&db, &project_id, "Test Project", "/test/path");

    let repo = SqliteIdeationSessionRepository::new(db.new_connection());
    let s1 = create_test_session(&project_id, Some("Alpha"));
    let s2 = create_test_session(&project_id, Some("Beta"));
    repo.create(s1).await.unwrap();
    repo.create(s2).await.unwrap();

    // Empty string search = no filter = same as None
    let counts_empty = repo.get_group_counts(&project_id, Some("")).await.unwrap();
    let counts_none = repo.get_group_counts(&project_id, None).await.unwrap();

    assert_eq!(
        counts_empty.drafts, counts_none.drafts,
        "empty search = no filter"
    );
    assert_eq!(counts_empty.drafts, 2);
}

#[tokio::test]
async fn test_get_group_counts_search_no_match_returns_zeros() {
    let db = setup_test_db();
    let project_id = ProjectId::new();
    create_test_project(&db, &project_id, "Test Project", "/test/path");

    let repo = SqliteIdeationSessionRepository::new(db.new_connection());
    let s1 = create_test_session(&project_id, Some("Alpha"));
    repo.create(s1).await.unwrap();

    let counts = repo
        .get_group_counts(&project_id, Some("nonexistent"))
        .await
        .unwrap();
    assert_eq!(counts.drafts, 0);
    assert_eq!(counts.archived, 0);
}

#[tokio::test]
async fn test_list_by_group_search_filters_results() {
    let db = setup_test_db();
    let project_id = ProjectId::new();
    create_test_project(&db, &project_id, "Test Project", "/test/path");

    let repo = SqliteIdeationSessionRepository::new(db.new_connection());
    let s1 = create_test_session(&project_id, Some("Auth refactor"));
    let s2 = create_test_session(&project_id, Some("Dashboard redesign"));
    let s3 = create_test_session(&project_id, Some("Auth improvements"));
    repo.create(s1).await.unwrap();
    repo.create(s2).await.unwrap();
    repo.create(s3).await.unwrap();

    let (sessions, total) = repo
        .list_by_group(&project_id, "drafts", 0, 20, Some("auth"))
        .await
        .unwrap();

    assert_eq!(total, 2, "total should reflect filtered count");
    assert_eq!(sessions.len(), 2);
    for s in &sessions {
        let title = s.session.title.as_deref().unwrap_or("");
        assert!(
            title.to_lowercase().contains("auth"),
            "session title '{}' should contain 'auth'",
            title
        );
    }
}

#[tokio::test]
async fn test_list_by_group_search_case_insensitive() {
    let db = setup_test_db();
    let project_id = ProjectId::new();
    create_test_project(&db, &project_id, "Test Project", "/test/path");

    let repo = SqliteIdeationSessionRepository::new(db.new_connection());
    let s1 = create_test_session(&project_id, Some("UPPER CASE TITLE"));
    repo.create(s1).await.unwrap();

    let (sessions_lower, _) = repo
        .list_by_group(&project_id, "drafts", 0, 20, Some("upper case"))
        .await
        .unwrap();
    assert_eq!(
        sessions_lower.len(),
        1,
        "lowercase search should match uppercase title"
    );

    let (sessions_upper, _) = repo
        .list_by_group(&project_id, "drafts", 0, 20, Some("UPPER CASE"))
        .await
        .unwrap();
    assert_eq!(
        sessions_upper.len(),
        1,
        "uppercase search should match uppercase title"
    );
}

#[tokio::test]
async fn test_list_by_group_empty_search_returns_all() {
    let db = setup_test_db();
    let project_id = ProjectId::new();
    create_test_project(&db, &project_id, "Test Project", "/test/path");

    let repo = SqliteIdeationSessionRepository::new(db.new_connection());
    let s1 = create_test_session(&project_id, Some("Alpha"));
    let s2 = create_test_session(&project_id, Some("Beta"));
    repo.create(s1).await.unwrap();
    repo.create(s2).await.unwrap();

    let (sessions_empty, total_empty) = repo
        .list_by_group(&project_id, "drafts", 0, 20, Some(""))
        .await
        .unwrap();
    let (sessions_none, total_none) = repo
        .list_by_group(&project_id, "drafts", 0, 20, None)
        .await
        .unwrap();

    assert_eq!(total_empty, total_none, "empty search = no filter");
    assert_eq!(sessions_empty.len(), sessions_none.len());
    assert_eq!(total_empty, 2);
}

#[tokio::test]
async fn test_list_by_group_search_special_chars_treated_literally() {
    let db = setup_test_db();
    let project_id = ProjectId::new();
    create_test_project(&db, &project_id, "Test Project", "/test/path");

    let repo = SqliteIdeationSessionRepository::new(db.new_connection());
    // Create a session whose title contains a literal `%` character
    let s_percent = create_test_session(&project_id, Some("100% complete"));
    // Create a session whose title contains a literal `_` character
    let s_underscore = create_test_session(&project_id, Some("task_cleanup plan"));
    // A session that would match if `%` were treated as wildcard (any chars)
    let s_other = create_test_session(&project_id, Some("other session"));
    repo.create(s_percent).await.unwrap();
    repo.create(s_underscore).await.unwrap();
    repo.create(s_other).await.unwrap();

    // Search for literal `%` — should only match `100% complete`
    let (percent_results, percent_total) = repo
        .list_by_group(&project_id, "drafts", 0, 20, Some("%"))
        .await
        .unwrap();
    assert_eq!(
        percent_total, 1,
        "% should be treated literally, matching only '100% complete'"
    );
    assert_eq!(
        percent_results[0].session.title.as_deref(),
        Some("100% complete")
    );

    // Search for literal `_` — should only match `task_cleanup plan`
    let (underscore_results, underscore_total) = repo
        .list_by_group(&project_id, "drafts", 0, 20, Some("_"))
        .await
        .unwrap();
    assert_eq!(
        underscore_total, 1,
        "_ should be treated literally, matching only 'task_cleanup plan'"
    );
    assert_eq!(
        underscore_results[0].session.title.as_deref(),
        Some("task_cleanup plan")
    );
}

// ==================== GET LATEST VERIFICATION CHILD TESTS ====================

#[tokio::test]
async fn test_get_latest_verification_child_returns_none_when_no_children() {
    let db = setup_test_db();
    let project_id = ProjectId::new();
    create_test_project(&db, &project_id, "Test Project", "/test/path");

    let repo = SqliteIdeationSessionRepository::new(db.new_connection());
    let parent = create_test_session(&project_id, Some("Parent Session"));
    repo.create(parent.clone()).await.unwrap();

    let result = repo
        .get_latest_verification_child(&parent.id)
        .await
        .unwrap();
    assert!(
        result.is_none(),
        "should return None when parent has no verification children"
    );
}

#[tokio::test]
async fn test_get_latest_verification_child_returns_archived_child() {
    let db = setup_test_db();
    let project_id = ProjectId::new();
    create_test_project(&db, &project_id, "Test Project", "/test/path");

    let repo = SqliteIdeationSessionRepository::new(db.new_connection());

    let parent = create_test_session(&project_id, Some("Parent Session"));
    repo.create(parent.clone()).await.unwrap();

    // Create a verification child and then archive it
    let mut child = create_test_session(&project_id, Some("Verification Child"));
    child.parent_session_id = Some(parent.id.clone());
    child.session_purpose = SessionPurpose::Verification;
    repo.create(child.clone()).await.unwrap();

    // Archive the child
    repo.update_status(&child.id, IdeationSessionStatus::Archived)
        .await
        .unwrap();

    // get_latest_verification_child should return it even though archived
    let result = repo
        .get_latest_verification_child(&parent.id)
        .await
        .unwrap();
    assert!(result.is_some(), "should return archived child");
    assert_eq!(result.unwrap().id, child.id);
}

#[tokio::test]
async fn test_get_latest_verification_child_returns_most_recent() {
    let db = setup_test_db();
    let project_id = ProjectId::new();
    create_test_project(&db, &project_id, "Test Project", "/test/path");

    let repo = SqliteIdeationSessionRepository::new(db.new_connection());

    let parent = create_test_session(&project_id, Some("Parent Session"));
    repo.create(parent.clone()).await.unwrap();

    let mut child1 = create_test_session(&project_id, Some("First Verification Child"));
    child1.parent_session_id = Some(parent.id.clone());
    child1.session_purpose = SessionPurpose::Verification;
    repo.create(child1.clone()).await.unwrap();

    // Sleep briefly so created_at differs
    tokio::time::sleep(std::time::Duration::from_millis(5)).await;

    let mut child2 = create_test_session(&project_id, Some("Second Verification Child"));
    child2.parent_session_id = Some(parent.id.clone());
    child2.session_purpose = SessionPurpose::Verification;
    repo.create(child2.clone()).await.unwrap();

    // Should return the most recently created child
    let result = repo
        .get_latest_verification_child(&parent.id)
        .await
        .unwrap();
    assert!(result.is_some());
    assert_eq!(
        result.unwrap().id,
        child2.id,
        "should return the most recently created child"
    );
}

#[tokio::test]
async fn test_get_latest_verification_child_ignores_non_verification_children() {
    let db = setup_test_db();
    let project_id = ProjectId::new();
    create_test_project(&db, &project_id, "Test Project", "/test/path");

    let repo = SqliteIdeationSessionRepository::new(db.new_connection());

    let parent = create_test_session(&project_id, Some("Parent Session"));
    repo.create(parent.clone()).await.unwrap();

    // Create a general child (non-verification)
    let mut general_child = create_test_session(&project_id, Some("General Child"));
    general_child.parent_session_id = Some(parent.id.clone());
    // session_purpose defaults to General
    repo.create(general_child.clone()).await.unwrap();

    // No verification children exist — should return None
    let result = repo
        .get_latest_verification_child(&parent.id)
        .await
        .unwrap();
    assert!(result.is_none(), "should ignore non-verification children");
}
