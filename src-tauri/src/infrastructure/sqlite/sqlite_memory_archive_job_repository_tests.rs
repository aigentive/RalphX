// Tests for SqliteMemoryArchiveJobRepository

use super::sqlite_memory_archive_job_repository::SqliteMemoryArchiveJobRepository;
use crate::domain::entities::{
    ArchiveJobPayload, ArchiveJobStatus, ArchiveJobType, MemoryArchiveJob, MemoryArchiveJobId,
    ProjectId,
};
use crate::domain::repositories::MemoryArchiveJobRepository;
use crate::testing::SqliteTestDb;

fn setup_test_db() -> SqliteTestDb {
    SqliteTestDb::new("sqlite_memory_archive_job_repository_tests")
}

fn create_test_project(db: &SqliteTestDb) -> ProjectId {
    let id = ProjectId::new();
    let working_dir = format!("/tmp/test/{}", id.as_str());
    db.with_connection(|conn| {
        conn.execute(
            "INSERT INTO projects (id, name, working_directory, git_mode, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, strftime('%Y-%m-%dT%H:%M:%S+00:00', 'now'), strftime('%Y-%m-%dT%H:%M:%S+00:00', 'now'))",
            rusqlite::params![id.as_str(), "Test Project", working_dir, "local"],
        )
        .unwrap();
    });
    id
}

fn make_memory_snapshot_job(project_id: ProjectId) -> MemoryArchiveJob {
    MemoryArchiveJob::new(
        project_id,
        ArchiveJobType::MemorySnapshot,
        ArchiveJobPayload::memory_snapshot("mem-123"),
    )
}

// ==================== CREATE TESTS ====================

#[tokio::test]
async fn test_create_returns_job() {
    let db = setup_test_db();
    let project_id = create_test_project(&db);
    let repo = SqliteMemoryArchiveJobRepository::from_shared(db.shared_conn());

    let job = make_memory_snapshot_job(project_id.clone());
    let job_id = job.id.clone();

    let result = repo.create(job).await;

    assert!(result.is_ok());
    let created = result.unwrap();
    assert_eq!(created.id, job_id);
    assert_eq!(created.project_id, project_id);
    assert_eq!(created.status, ArchiveJobStatus::Pending);
    assert_eq!(created.job_type, ArchiveJobType::MemorySnapshot);
}

#[tokio::test]
async fn test_create_rule_snapshot_job() {
    let db = setup_test_db();
    let project_id = create_test_project(&db);
    let repo = SqliteMemoryArchiveJobRepository::from_shared(db.shared_conn());

    let job = MemoryArchiveJob::new(
        project_id,
        ArchiveJobType::RuleSnapshot,
        ArchiveJobPayload::rule_snapshot("scope-key-1"),
    );
    let job_id = job.id.clone();

    let result = repo.create(job).await;

    assert!(result.is_ok());
    let created = result.unwrap();
    assert_eq!(created.id, job_id);
    assert_eq!(created.job_type, ArchiveJobType::RuleSnapshot);
}

#[tokio::test]
async fn test_create_full_rebuild_job() {
    let db = setup_test_db();
    let project_id = create_test_project(&db);
    let repo = SqliteMemoryArchiveJobRepository::from_shared(db.shared_conn());

    let job = MemoryArchiveJob::new(
        project_id,
        ArchiveJobType::FullRebuild,
        ArchiveJobPayload::full_rebuild(true),
    );
    let job_id = job.id.clone();

    let result = repo.create(job).await;

    assert!(result.is_ok());
    assert_eq!(result.unwrap().id, job_id);
}

#[tokio::test]
async fn test_create_preserves_null_timestamps() {
    let db = setup_test_db();
    let project_id = create_test_project(&db);
    let repo = SqliteMemoryArchiveJobRepository::from_shared(db.shared_conn());

    let job = make_memory_snapshot_job(project_id);
    let job_id = job.id.clone();
    repo.create(job).await.unwrap();

    let found = repo.get_by_id(&job_id).await.unwrap().unwrap();
    assert!(found.started_at.is_none());
    assert!(found.completed_at.is_none());
    assert!(found.error_message.is_none());
}

// ==================== GET BY ID TESTS ====================

#[tokio::test]
async fn test_get_by_id_returns_job() {
    let db = setup_test_db();
    let project_id = create_test_project(&db);
    let repo = SqliteMemoryArchiveJobRepository::from_shared(db.shared_conn());

    let job = make_memory_snapshot_job(project_id);
    let job_id = job.id.clone();
    repo.create(job).await.unwrap();

    let result = repo.get_by_id(&job_id).await;

    assert!(result.is_ok());
    let found = result.unwrap();
    assert!(found.is_some());
    assert_eq!(found.unwrap().id, job_id);
}

#[tokio::test]
async fn test_get_by_id_returns_none_for_nonexistent() {
    let db = setup_test_db();
    let repo = SqliteMemoryArchiveJobRepository::from_shared(db.shared_conn());

    let nonexistent_id = MemoryArchiveJobId("nonexistent-id".to_string());
    let result = repo.get_by_id(&nonexistent_id).await;

    assert!(result.is_ok());
    assert!(result.unwrap().is_none());
}

#[tokio::test]
async fn test_get_by_id_preserves_all_fields() {
    let db = setup_test_db();
    let project_id = create_test_project(&db);
    let repo = SqliteMemoryArchiveJobRepository::from_shared(db.shared_conn());

    let job = MemoryArchiveJob::new(
        project_id.clone(),
        ArchiveJobType::MemorySnapshot,
        ArchiveJobPayload::memory_snapshot("mem-abc"),
    );
    let job_id = job.id.clone();
    repo.create(job).await.unwrap();

    let found = repo.get_by_id(&job_id).await.unwrap().unwrap();
    assert_eq!(found.project_id, project_id);
    assert_eq!(found.job_type, ArchiveJobType::MemorySnapshot);
    assert_eq!(found.status, ArchiveJobStatus::Pending);
    assert!(found.error_message.is_none());
    assert!(found.started_at.is_none());
    assert!(found.completed_at.is_none());
}

// ==================== GET PENDING BY PROJECT TESTS ====================

#[tokio::test]
async fn test_get_pending_by_project_returns_only_pending() {
    let db = setup_test_db();
    let project_id = create_test_project(&db);
    let repo = SqliteMemoryArchiveJobRepository::from_shared(db.shared_conn());

    let pending1 = make_memory_snapshot_job(project_id.clone());
    let pending2 = MemoryArchiveJob::new(
        project_id.clone(),
        ArchiveJobType::FullRebuild,
        ArchiveJobPayload::full_rebuild(false),
    );
    let running = make_memory_snapshot_job(project_id.clone());
    let running_id = running.id.clone();

    repo.create(pending1).await.unwrap();
    repo.create(pending2).await.unwrap();
    repo.create(running).await.unwrap();

    repo.update_status(&running_id, ArchiveJobStatus::Running, None)
        .await
        .unwrap();

    let result = repo.get_pending_by_project(&project_id).await;

    assert!(result.is_ok());
    let pending_jobs = result.unwrap();
    assert_eq!(pending_jobs.len(), 2);
    assert!(pending_jobs
        .iter()
        .all(|j| j.status == ArchiveJobStatus::Pending));
}

#[tokio::test]
async fn test_get_pending_by_project_returns_empty_when_none_pending() {
    let db = setup_test_db();
    let project_id = create_test_project(&db);
    let repo = SqliteMemoryArchiveJobRepository::from_shared(db.shared_conn());

    let result = repo.get_pending_by_project(&project_id).await;

    assert!(result.is_ok());
    assert!(result.unwrap().is_empty());
}

#[tokio::test]
async fn test_get_pending_by_project_filters_by_project() {
    let db = setup_test_db();
    let project_id1 = create_test_project(&db);
    let project_id2 = create_test_project(&db);
    let repo = SqliteMemoryArchiveJobRepository::from_shared(db.shared_conn());

    let job1 = make_memory_snapshot_job(project_id1.clone());
    let job2 = make_memory_snapshot_job(project_id2.clone());

    repo.create(job1).await.unwrap();
    repo.create(job2).await.unwrap();

    let pending_p1 = repo.get_pending_by_project(&project_id1).await.unwrap();
    let pending_p2 = repo.get_pending_by_project(&project_id2).await.unwrap();

    assert_eq!(pending_p1.len(), 1);
    assert_eq!(pending_p2.len(), 1);
    assert_eq!(pending_p1[0].project_id, project_id1);
    assert_eq!(pending_p2[0].project_id, project_id2);
}

#[tokio::test]
async fn test_get_pending_excludes_done_and_failed() {
    let db = setup_test_db();
    let project_id = create_test_project(&db);
    let repo = SqliteMemoryArchiveJobRepository::from_shared(db.shared_conn());

    let done_job = make_memory_snapshot_job(project_id.clone());
    let failed_job = make_memory_snapshot_job(project_id.clone());
    let done_id = done_job.id.clone();
    let failed_id = failed_job.id.clone();

    repo.create(done_job).await.unwrap();
    repo.create(failed_job).await.unwrap();

    repo.update_status(&done_id, ArchiveJobStatus::Done, None)
        .await
        .unwrap();
    repo.update_status(&failed_id, ArchiveJobStatus::Failed, Some("error".to_string()))
        .await
        .unwrap();

    let pending = repo.get_pending_by_project(&project_id).await.unwrap();
    assert!(pending.is_empty());
}

// ==================== UPDATE STATUS TESTS ====================

#[tokio::test]
async fn test_update_status_to_running_sets_started_at() {
    let db = setup_test_db();
    let project_id = create_test_project(&db);
    let repo = SqliteMemoryArchiveJobRepository::from_shared(db.shared_conn());

    let job = make_memory_snapshot_job(project_id);
    let job_id = job.id.clone();
    repo.create(job).await.unwrap();

    repo.update_status(&job_id, ArchiveJobStatus::Running, None)
        .await
        .unwrap();

    let found = repo.get_by_id(&job_id).await.unwrap().unwrap();
    assert_eq!(found.status, ArchiveJobStatus::Running);
    assert!(found.started_at.is_some());
    // COALESCE: completed_at was None, stays None
    assert!(found.completed_at.is_none());
}

#[tokio::test]
async fn test_update_status_to_done_sets_completed_at_and_preserves_started_at() {
    let db = setup_test_db();
    let project_id = create_test_project(&db);
    let repo = SqliteMemoryArchiveJobRepository::from_shared(db.shared_conn());

    let job = make_memory_snapshot_job(project_id);
    let job_id = job.id.clone();
    repo.create(job).await.unwrap();

    repo.update_status(&job_id, ArchiveJobStatus::Running, None)
        .await
        .unwrap();

    repo.update_status(&job_id, ArchiveJobStatus::Done, None)
        .await
        .unwrap();

    let found = repo.get_by_id(&job_id).await.unwrap().unwrap();
    assert_eq!(found.status, ArchiveJobStatus::Done);
    // COALESCE: started_at was set during Running, preserved here
    assert!(found.started_at.is_some());
    assert!(found.completed_at.is_some());
}

#[tokio::test]
async fn test_update_status_to_failed_sets_error_message_and_completed_at() {
    let db = setup_test_db();
    let project_id = create_test_project(&db);
    let repo = SqliteMemoryArchiveJobRepository::from_shared(db.shared_conn());

    let job = make_memory_snapshot_job(project_id);
    let job_id = job.id.clone();
    repo.create(job).await.unwrap();

    repo.update_status(
        &job_id,
        ArchiveJobStatus::Failed,
        Some("Something went wrong".to_string()),
    )
    .await
    .unwrap();

    let found = repo.get_by_id(&job_id).await.unwrap().unwrap();
    assert_eq!(found.status, ArchiveJobStatus::Failed);
    assert_eq!(
        found.error_message,
        Some("Something went wrong".to_string())
    );
    assert!(found.completed_at.is_some());
}

#[tokio::test]
async fn test_update_status_pending_no_timestamp_set() {
    let db = setup_test_db();
    let project_id = create_test_project(&db);
    let repo = SqliteMemoryArchiveJobRepository::from_shared(db.shared_conn());

    let job = make_memory_snapshot_job(project_id);
    let job_id = job.id.clone();
    repo.create(job).await.unwrap();

    // Pending: both started_at and completed_at are None in the update
    // COALESCE(None, existing) → preserves null started_at
    repo.update_status(&job_id, ArchiveJobStatus::Pending, None)
        .await
        .unwrap();

    let found = repo.get_by_id(&job_id).await.unwrap().unwrap();
    assert_eq!(found.status, ArchiveJobStatus::Pending);
    assert!(found.started_at.is_none());
    assert!(found.completed_at.is_none());
}

#[tokio::test]
async fn test_update_status_for_nonexistent_id_returns_error() {
    let db = setup_test_db();
    let repo = SqliteMemoryArchiveJobRepository::from_shared(db.shared_conn());

    let nonexistent = MemoryArchiveJobId("does-not-exist".to_string());
    let result = repo
        .update_status(&nonexistent, ArchiveJobStatus::Running, None)
        .await;

    assert!(result.is_err());
}

// ==================== FROM SHARED TESTS ====================

#[tokio::test]
async fn test_from_shared_creates_and_retrieves() {
    let db = setup_test_db();
    let project_id = create_test_project(&db);
    let shared_conn = db.shared_conn();
    let repo = SqliteMemoryArchiveJobRepository::from_shared(shared_conn);

    let job = make_memory_snapshot_job(project_id);
    let job_id = job.id.clone();

    repo.create(job).await.unwrap();

    let found = repo.get_by_id(&job_id).await.unwrap();
    assert!(found.is_some());
}
