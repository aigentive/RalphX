// Tests for SqliteExecutionPlanRepository

use crate::domain::entities::{
    ExecutionPlan, ExecutionPlanHaltMode, ExecutionPlanId, ExecutionPlanStatus, IdeationSessionId,
};
use crate::domain::repositories::ExecutionPlanRepository;
use crate::infrastructure::sqlite::SqliteExecutionPlanRepository;
use crate::testing::SqliteTestDb;

fn setup_repo_with_session(
    fixture_name: &str,
    session_id: &str,
) -> (SqliteTestDb, SqliteExecutionPlanRepository) {
    let db = SqliteTestDb::new(fixture_name);
    db.with_connection(|conn| {
        conn.execute(
            "INSERT INTO projects (id, name, working_directory, created_at, updated_at)
             VALUES (?1, 'Test Project', '/test', strftime('%Y-%m-%dT%H:%M:%S+00:00', 'now'), strftime('%Y-%m-%dT%H:%M:%S+00:00', 'now'))",
            ["proj-test-123"],
        )
        .unwrap();

        conn.execute(
            "INSERT INTO ideation_sessions (id, project_id, status, created_at, updated_at)
             VALUES (?1, 'proj-test-123', 'accepted', strftime('%Y-%m-%dT%H:%M:%S+00:00', 'now'), strftime('%Y-%m-%dT%H:%M:%S+00:00', 'now'))",
            [session_id],
        )
        .unwrap();
    });

    let repo = SqliteExecutionPlanRepository::from_shared(db.shared_conn());
    (db, repo)
}

#[tokio::test]
async fn test_create_execution_plan() {
    let session_id = IdeationSessionId::from_string("session-test-1");
    let (_db, repo) = setup_repo_with_session(
        "sqlite_execution_plan_repo_tests-create",
        session_id.as_str(),
    );

    let plan = ExecutionPlan::new(session_id.clone());
    let created = repo.create(plan.clone()).await.unwrap();

    assert_eq!(created.id, plan.id);
    assert_eq!(created.session_id, session_id);
    assert_eq!(created.status, ExecutionPlanStatus::Active);
    assert_eq!(created.halt_mode, ExecutionPlanHaltMode::Running);
}

#[tokio::test]
async fn test_get_by_id() {
    let session_id = IdeationSessionId::from_string("session-test-2");
    let (_db, repo) = setup_repo_with_session(
        "sqlite_execution_plan_repo_tests-get-by-id",
        session_id.as_str(),
    );

    let plan = ExecutionPlan::new(session_id);
    let created = repo.create(plan.clone()).await.unwrap();

    let found = repo.get_by_id(&created.id).await.unwrap();
    assert!(found.is_some());
    let found = found.unwrap();
    assert_eq!(found.id, created.id);
    assert_eq!(found.halt_mode, ExecutionPlanHaltMode::Running);
}

#[tokio::test]
async fn test_get_by_id_not_found() {
    let session_id = IdeationSessionId::from_string("session-test-3");
    let (_db, repo) = setup_repo_with_session(
        "sqlite_execution_plan_repo_tests-get-by-id-miss",
        session_id.as_str(),
    );
    let fake_id = ExecutionPlanId::new();

    let result = repo.get_by_id(&fake_id).await.unwrap();
    assert!(result.is_none());
}

#[tokio::test]
async fn test_get_by_session() {
    let session_id = IdeationSessionId::from_string("session-test-4");
    let (_db, repo) = setup_repo_with_session(
        "sqlite_execution_plan_repo_tests-get-by-session",
        session_id.as_str(),
    );

    let plan1 = ExecutionPlan::new(session_id.clone());
    repo.create(plan1).await.unwrap();

    // Create another plan for a different session (need separate repo with different session)
    let other_session = IdeationSessionId::from_string("session-test-other");
    let (_other_db, other_repo) = setup_repo_with_session(
        "sqlite_execution_plan_repo_tests-get-by-session-other",
        other_session.as_str(),
    );
    let plan2 = ExecutionPlan::new(other_session);
    other_repo.create(plan2).await.unwrap();

    let plans = repo.get_by_session(&session_id).await.unwrap();
    assert_eq!(plans.len(), 1);
    assert_eq!(plans[0].session_id, session_id);
}

#[tokio::test]
async fn test_get_active_for_session() {
    let session_id = IdeationSessionId::from_string("session-test-5");
    let (_db, repo) = setup_repo_with_session(
        "sqlite_execution_plan_repo_tests-get-active",
        session_id.as_str(),
    );

    // Create first plan (will be superseded)
    let plan1 = ExecutionPlan::new(session_id.clone());
    let created1 = repo.create(plan1.clone()).await.unwrap();
    repo.mark_superseded(&created1.id).await.unwrap();

    // Create second plan (active)
    let plan2 = ExecutionPlan::new(session_id.clone());
    repo.create(plan2).await.unwrap();

    let active = repo.get_active_for_session(&session_id).await.unwrap();
    assert!(active.is_some());
    assert_eq!(active.unwrap().status, ExecutionPlanStatus::Active);
}

#[tokio::test]
async fn test_mark_superseded() {
    let session_id = IdeationSessionId::from_string("session-test-6");
    let (_db, repo) = setup_repo_with_session(
        "sqlite_execution_plan_repo_tests-mark-superseded",
        session_id.as_str(),
    );

    let plan = ExecutionPlan::new(session_id);
    let created = repo.create(plan.clone()).await.unwrap();

    repo.mark_superseded(&created.id).await.unwrap();

    let updated = repo.get_by_id(&created.id).await.unwrap().unwrap();
    assert_eq!(updated.status, ExecutionPlanStatus::Superseded);
    assert_eq!(updated.halt_mode, ExecutionPlanHaltMode::Running);
}

#[tokio::test]
async fn test_set_halt_mode_updates_execution_plan_without_changing_status() {
    let session_id = IdeationSessionId::from_string("session-test-halt");
    let (_db, repo) = setup_repo_with_session(
        "sqlite_execution_plan_repo_tests-set-halt-mode",
        session_id.as_str(),
    );

    let created = repo.create(ExecutionPlan::new(session_id)).await.unwrap();

    repo.set_halt_mode(&created.id, ExecutionPlanHaltMode::Paused)
        .await
        .unwrap();

    let paused = repo.get_by_id(&created.id).await.unwrap().unwrap();
    assert_eq!(paused.status, ExecutionPlanStatus::Active);
    assert_eq!(paused.halt_mode, ExecutionPlanHaltMode::Paused);

    repo.set_halt_mode(&created.id, ExecutionPlanHaltMode::Stopped)
        .await
        .unwrap();
    let stopped = repo.get_by_id(&created.id).await.unwrap().unwrap();
    assert_eq!(stopped.status, ExecutionPlanStatus::Active);
    assert_eq!(stopped.halt_mode, ExecutionPlanHaltMode::Stopped);
}

#[tokio::test]
async fn test_set_halt_mode_not_found() {
    let session_id = IdeationSessionId::from_string("session-test-halt-miss");
    let (_db, repo) = setup_repo_with_session(
        "sqlite_execution_plan_repo_tests-set-halt-mode-miss",
        session_id.as_str(),
    );
    let fake_id = ExecutionPlanId::new();

    let result = repo
        .set_halt_mode(&fake_id, ExecutionPlanHaltMode::Paused)
        .await;

    assert!(result.is_err());
}

#[tokio::test]
async fn test_mark_superseded_not_found() {
    let session_id = IdeationSessionId::from_string("session-test-7");
    let (_db, repo) = setup_repo_with_session(
        "sqlite_execution_plan_repo_tests-mark-superseded-miss",
        session_id.as_str(),
    );
    let fake_id = ExecutionPlanId::new();

    let result = repo.mark_superseded(&fake_id).await;
    assert!(result.is_err());
}

#[tokio::test]
async fn test_delete() {
    let session_id = IdeationSessionId::from_string("session-test-8");
    let (_db, repo) = setup_repo_with_session(
        "sqlite_execution_plan_repo_tests-delete",
        session_id.as_str(),
    );

    let plan = ExecutionPlan::new(session_id);
    let created = repo.create(plan.clone()).await.unwrap();

    repo.delete(&created.id).await.unwrap();

    let found = repo.get_by_id(&created.id).await.unwrap();
    assert!(found.is_none());
}

#[tokio::test]
async fn test_delete_not_found() {
    let session_id = IdeationSessionId::from_string("session-test-9");
    let (_db, repo) = setup_repo_with_session(
        "sqlite_execution_plan_repo_tests-delete-miss",
        session_id.as_str(),
    );
    let fake_id = ExecutionPlanId::new();

    let result = repo.delete(&fake_id).await;
    assert!(result.is_err());
}

// ---------------------------------------------------------------------------
// Additional coverage: gap tests
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_create_duplicate_id_fails() {
    let session_id = IdeationSessionId::from_string("session-dup-1");
    let (_db, repo) = setup_repo_with_session(
        "sqlite_execution_plan_repo_tests-duplicate",
        session_id.as_str(),
    );

    let plan = ExecutionPlan::new(session_id);
    repo.create(plan.clone()).await.unwrap();

    // Attempt to create again with the same ID
    let result = repo.create(plan).await;
    assert!(
        result.is_err(),
        "Creating an execution plan with duplicate ID should fail"
    );
}

#[tokio::test]
async fn test_get_by_session_multiple_plans() {
    let session_id = IdeationSessionId::from_string("session-multi-1");
    let (_db, repo) = setup_repo_with_session(
        "sqlite_execution_plan_repo_tests-multi",
        session_id.as_str(),
    );

    // Create first plan
    let plan1 = ExecutionPlan::new(session_id.clone());
    let created1 = repo.create(plan1).await.unwrap();

    // Supersede it
    repo.mark_superseded(&created1.id).await.unwrap();

    // Create second plan for same session
    let plan2 = ExecutionPlan::new(session_id.clone());
    repo.create(plan2).await.unwrap();

    let plans = repo.get_by_session(&session_id).await.unwrap();
    assert_eq!(plans.len(), 2, "Should return both plans for the session");

    // Verify ordering: DESC by created_at (newest first)
    assert_eq!(plans[0].status, ExecutionPlanStatus::Active);
    assert_eq!(plans[1].status, ExecutionPlanStatus::Superseded);
}

#[tokio::test]
async fn test_get_by_session_none() {
    let session_id = IdeationSessionId::from_string("session-none-1");
    let (_db, repo) =
        setup_repo_with_session("sqlite_execution_plan_repo_tests-none", session_id.as_str());

    let plans = repo.get_by_session(&session_id).await.unwrap();
    assert!(
        plans.is_empty(),
        "Should return empty vec when no plans exist for session"
    );
}

#[tokio::test]
async fn test_get_active_for_session_none_when_all_superseded() {
    let session_id = IdeationSessionId::from_string("session-all-super");
    let (_db, repo) = setup_repo_with_session(
        "sqlite_execution_plan_repo_tests-all-superseded",
        session_id.as_str(),
    );

    let plan = ExecutionPlan::new(session_id.clone());
    let created = repo.create(plan).await.unwrap();
    repo.mark_superseded(&created.id).await.unwrap();

    let active = repo.get_active_for_session(&session_id).await.unwrap();
    assert!(
        active.is_none(),
        "Should return None when all plans are superseded"
    );
}

#[tokio::test]
async fn test_get_active_for_session_none_when_no_plans() {
    let session_id = IdeationSessionId::from_string("session-no-plans");
    let (_db, repo) = setup_repo_with_session(
        "sqlite_execution_plan_repo_tests-no-plans",
        session_id.as_str(),
    );

    let active = repo.get_active_for_session(&session_id).await.unwrap();
    assert!(active.is_none(), "Should return None when no plans exist");
}

#[tokio::test]
async fn test_mark_superseded_already_superseded() {
    let session_id = IdeationSessionId::from_string("session-double-super");
    let (_db, repo) = setup_repo_with_session(
        "sqlite_execution_plan_repo_tests-double-supersede",
        session_id.as_str(),
    );

    let plan = ExecutionPlan::new(session_id);
    let created = repo.create(plan).await.unwrap();

    // First supersede
    repo.mark_superseded(&created.id).await.unwrap();

    // Second supersede — should succeed (idempotent update)
    let result = repo.mark_superseded(&created.id).await;
    assert!(
        result.is_ok(),
        "mark_superseded on already-superseded plan should succeed"
    );

    let updated = repo.get_by_id(&created.id).await.unwrap().unwrap();
    assert_eq!(updated.status, ExecutionPlanStatus::Superseded);
}

#[tokio::test]
async fn test_re_accept_flow_supersedes_old_creates_new() {
    let session_id = IdeationSessionId::from_string("session-re-accept");
    let (_db, repo) = setup_repo_with_session(
        "sqlite_execution_plan_repo_tests-reaccept",
        session_id.as_str(),
    );

    // First accept: create active plan
    let plan1 = ExecutionPlan::new(session_id.clone());
    let created1 = repo.create(plan1).await.unwrap();
    assert_eq!(created1.status, ExecutionPlanStatus::Active);

    // Re-accept: supersede old, create new
    repo.mark_superseded(&created1.id).await.unwrap();
    let plan2 = ExecutionPlan::new(session_id.clone());
    let created2 = repo.create(plan2).await.unwrap();

    // Old plan is superseded
    let old = repo.get_by_id(&created1.id).await.unwrap().unwrap();
    assert_eq!(old.status, ExecutionPlanStatus::Superseded);

    // New plan is active
    let new = repo.get_by_id(&created2.id).await.unwrap().unwrap();
    assert_eq!(new.status, ExecutionPlanStatus::Active);

    // get_active_for_session returns only the new one
    let active = repo
        .get_active_for_session(&session_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(active.id, created2.id);

    // Total: 2 plans for this session
    let all = repo.get_by_session(&session_id).await.unwrap();
    assert_eq!(all.len(), 2);
}

// ---------------------------------------------------------------------------
// Entity tests
// ---------------------------------------------------------------------------

#[test]
fn test_execution_plan_new_has_active_status() {
    let session_id = IdeationSessionId::from_string("session-entity-1");
    let plan = ExecutionPlan::new(session_id.clone());

    assert_eq!(plan.status, ExecutionPlanStatus::Active);
    assert_eq!(plan.halt_mode, ExecutionPlanHaltMode::Running);
    assert_eq!(plan.session_id, session_id);
    assert!(!plan.id.as_str().is_empty());
}

#[test]
fn test_execution_plan_status_to_db_string() {
    assert_eq!(ExecutionPlanStatus::Active.to_db_string(), "active");
    assert_eq!(ExecutionPlanStatus::Superseded.to_db_string(), "superseded");
}

#[test]
fn test_execution_plan_status_from_db_string() {
    assert_eq!(
        ExecutionPlanStatus::from_db_string("active").unwrap(),
        ExecutionPlanStatus::Active
    );
    assert_eq!(
        ExecutionPlanStatus::from_db_string("superseded").unwrap(),
        ExecutionPlanStatus::Superseded
    );
}

#[test]
fn test_execution_plan_status_from_db_string_invalid() {
    let result = ExecutionPlanStatus::from_db_string("invalid");
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(
        err.to_string().contains("invalid"),
        "Error should mention the invalid value"
    );
}

#[test]
fn test_execution_plan_status_display() {
    assert_eq!(format!("{}", ExecutionPlanStatus::Active), "active");
    assert_eq!(format!("{}", ExecutionPlanStatus::Superseded), "superseded");
}

#[test]
fn test_execution_plan_halt_mode_to_db_string() {
    assert_eq!(ExecutionPlanHaltMode::Running.to_db_string(), "running");
    assert_eq!(ExecutionPlanHaltMode::Paused.to_db_string(), "paused");
    assert_eq!(ExecutionPlanHaltMode::Stopped.to_db_string(), "stopped");
}

#[test]
fn test_execution_plan_halt_mode_from_db_string() {
    assert_eq!(
        ExecutionPlanHaltMode::from_db_string("running").unwrap(),
        ExecutionPlanHaltMode::Running
    );
    assert_eq!(
        ExecutionPlanHaltMode::from_db_string("paused").unwrap(),
        ExecutionPlanHaltMode::Paused
    );
    assert_eq!(
        ExecutionPlanHaltMode::from_db_string("stopped").unwrap(),
        ExecutionPlanHaltMode::Stopped
    );
}

#[test]
fn test_execution_plan_halt_mode_from_db_string_invalid() {
    let result = ExecutionPlanHaltMode::from_db_string("invalid");
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(
        err.to_string().contains("invalid"),
        "Error should mention the invalid value"
    );
}

#[test]
fn test_execution_plan_halt_mode_display() {
    assert_eq!(format!("{}", ExecutionPlanHaltMode::Running), "running");
    assert_eq!(format!("{}", ExecutionPlanHaltMode::Paused), "paused");
    assert_eq!(format!("{}", ExecutionPlanHaltMode::Stopped), "stopped");
}
