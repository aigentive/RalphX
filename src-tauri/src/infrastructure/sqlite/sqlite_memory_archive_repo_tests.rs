use super::*;
use crate::domain::entities::ArchiveJobPayload;
use crate::domain::entities::{Project, ProjectId};
use crate::testing::SqliteTestDb;

struct MemoryArchiveRepoFixture {
    db: SqliteTestDb,
    repo: SqliteMemoryArchiveRepository,
}

impl std::ops::Deref for MemoryArchiveRepoFixture {
    type Target = SqliteMemoryArchiveRepository;

    fn deref(&self) -> &Self::Target {
        &self.repo
    }
}

impl MemoryArchiveRepoFixture {
    fn db(&self) -> &SqliteTestDb {
        &self.db
    }
}

fn insert_test_project(db: &SqliteTestDb, project_id: &str, working_directory: &str) {
    let mut project = Project::new("Test Project".to_string(), working_directory.to_string());
    project.id = ProjectId::from_string(project_id.to_string());
    db.insert_project(project);
}

fn setup_test_repo() -> MemoryArchiveRepoFixture {
    let db = SqliteTestDb::new("sqlite-memory-archive-repo");
    insert_test_project(&db, "test-project", "/test/path");
    let repo = SqliteMemoryArchiveRepository::from_shared(db.shared_conn());
    MemoryArchiveRepoFixture { db, repo }
}

#[tokio::test]
async fn test_create_and_get_job() {
    let repo = setup_test_repo();
    let project_id = ProjectId::from_string("test-project".to_string());
    let payload = ArchiveJobPayload::memory_snapshot("mem_123");
    let job = MemoryArchiveJob::new(project_id.clone(), ArchiveJobType::MemorySnapshot, payload);

    let created_job = repo.create(job.clone()).await.unwrap();
    assert_eq!(created_job.id, job.id);

    let retrieved_job = repo.get_by_id(&job.id).await.unwrap();
    assert!(retrieved_job.is_some());
    assert_eq!(retrieved_job.unwrap().id, job.id);
}

#[tokio::test]
async fn test_claim_next() {
    let repo = setup_test_repo();
    let project_id = ProjectId::from_string("test-project".to_string());

    // Create two jobs
    let job1 = MemoryArchiveJob::new(
        project_id.clone(),
        ArchiveJobType::MemorySnapshot,
        ArchiveJobPayload::memory_snapshot("mem_1"),
    );
    let job2 = MemoryArchiveJob::new(
        project_id.clone(),
        ArchiveJobType::RuleSnapshot,
        ArchiveJobPayload::rule_snapshot("rule_1"),
    );

    repo.create(job1.clone()).await.unwrap();
    // Add a small delay to ensure different timestamps
    tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;
    repo.create(job2.clone()).await.unwrap();

    // Claim the first job (oldest)
    let claimed = repo.claim_next().await.unwrap();
    assert!(claimed.is_some());
    let claimed_job = claimed.unwrap();
    assert_eq!(claimed_job.id, job1.id);
    assert_eq!(claimed_job.status, ArchiveJobStatus::Running);

    // Claim the second job
    let claimed2 = repo.claim_next().await.unwrap();
    assert!(claimed2.is_some());
    assert_eq!(claimed2.unwrap().id, job2.id);

    // No more claimable jobs
    let claimed3 = repo.claim_next().await.unwrap();
    assert!(claimed3.is_none());
}

#[tokio::test]
async fn test_update_job_status() {
    let repo = setup_test_repo();
    let project_id = ProjectId::from_string("test-project".to_string());
    let mut job = MemoryArchiveJob::new(
        project_id,
        ArchiveJobType::MemorySnapshot,
        ArchiveJobPayload::memory_snapshot("mem_123"),
    );

    repo.create(job.clone()).await.unwrap();

    job.start();
    repo.update(&job).await.unwrap();

    let updated = repo.get_by_id(&job.id).await.unwrap().unwrap();
    assert_eq!(updated.status, ArchiveJobStatus::Running);
    assert!(updated.started_at.is_some());

    job.complete();
    repo.update(&job).await.unwrap();

    let completed = repo.get_by_id(&job.id).await.unwrap().unwrap();
    assert_eq!(completed.status, ArchiveJobStatus::Done);
    assert!(completed.completed_at.is_some());
}

#[tokio::test]
async fn test_get_by_status() {
    let repo = setup_test_repo();
    let project_id = ProjectId::from_string("test-project".to_string());

    let mut job1 = MemoryArchiveJob::new(
        project_id.clone(),
        ArchiveJobType::MemorySnapshot,
        ArchiveJobPayload::memory_snapshot("mem_1"),
    );
    let job2 = MemoryArchiveJob::new(
        project_id,
        ArchiveJobType::RuleSnapshot,
        ArchiveJobPayload::rule_snapshot("rule_1"),
    );

    repo.create(job1.clone()).await.unwrap();
    repo.create(job2).await.unwrap();

    // Mark job1 as running
    job1.start();
    repo.update(&job1).await.unwrap();

    let pending_jobs = repo.get_by_status(ArchiveJobStatus::Pending).await.unwrap();
    assert_eq!(pending_jobs.len(), 1);

    let running_jobs = repo.get_by_status(ArchiveJobStatus::Running).await.unwrap();
    assert_eq!(running_jobs.len(), 1);
}

#[tokio::test]
async fn test_count_claimable() {
    let repo = setup_test_repo();
    let project_id = ProjectId::from_string("test-project".to_string());

    assert_eq!(repo.count_claimable().await.unwrap(), 0);

    let job1 = MemoryArchiveJob::new(
        project_id.clone(),
        ArchiveJobType::MemorySnapshot,
        ArchiveJobPayload::memory_snapshot("mem_1"),
    );
    let mut job2 = MemoryArchiveJob::new(
        project_id,
        ArchiveJobType::RuleSnapshot,
        ArchiveJobPayload::rule_snapshot("rule_1"),
    );

    repo.create(job1).await.unwrap();
    repo.create(job2.clone()).await.unwrap();

    assert_eq!(repo.count_claimable().await.unwrap(), 2);

    // Mark job2 as running
    job2.start();
    repo.update(&job2).await.unwrap();

    assert_eq!(repo.count_claimable().await.unwrap(), 1);

    // Fail job2 - should be claimable again
    job2.fail("Test error");
    repo.update(&job2).await.unwrap();

    assert_eq!(repo.count_claimable().await.unwrap(), 2);
}

// ─── helpers ─────────────────────────────────────────────────────────────────

fn pid() -> ProjectId {
    ProjectId::from_string("test-project".to_string())
}

fn pid2() -> ProjectId {
    ProjectId::from_string("test-project-2".to_string())
}

// ─── get_by_id not found ─────────────────────────────────────────────────────

#[tokio::test]
async fn test_get_by_id_not_found() {
    let repo = setup_test_repo();
    let fake_id = MemoryArchiveJobId::from("nonexistent-id".to_string());
    assert!(repo.get_by_id(&fake_id).await.unwrap().is_none());
}

// ─── delete ──────────────────────────────────────────────────────────────────

#[tokio::test]
async fn test_delete() {
    let repo = setup_test_repo();
    let job = MemoryArchiveJob::new(pid(), ArchiveJobType::MemorySnapshot, ArchiveJobPayload::memory_snapshot("mem_1"));
    let job_id = job.id.clone();

    repo.create(job).await.unwrap();
    assert!(repo.get_by_id(&job_id).await.unwrap().is_some());

    repo.delete(&job_id).await.unwrap();
    assert!(repo.get_by_id(&job_id).await.unwrap().is_none());
}

// ─── get_by_project ──────────────────────────────────────────────────────────

#[tokio::test]
async fn test_get_by_project() {
    let repo = setup_test_repo();
    insert_test_project(repo.db(), "test-project-2", "/test/path2");

    repo.create(MemoryArchiveJob::new(pid(), ArchiveJobType::MemorySnapshot, ArchiveJobPayload::memory_snapshot("m1"))).await.unwrap();
    repo.create(MemoryArchiveJob::new(pid(), ArchiveJobType::RuleSnapshot, ArchiveJobPayload::rule_snapshot("r1"))).await.unwrap();
    repo.create(MemoryArchiveJob::new(pid2(), ArchiveJobType::MemorySnapshot, ArchiveJobPayload::memory_snapshot("m2"))).await.unwrap();

    assert_eq!(repo.get_by_project(&pid()).await.unwrap().len(), 2);
    assert_eq!(repo.get_by_project(&pid2()).await.unwrap().len(), 1);
}

#[tokio::test]
async fn test_get_by_project_empty() {
    let repo = setup_test_repo();
    let other = ProjectId::from_string("no-such-project".to_string());
    assert!(repo.get_by_project(&other).await.unwrap().is_empty());
}

// ─── get_by_project_and_status ───────────────────────────────────────────────

#[tokio::test]
async fn test_get_by_project_and_status() {
    let repo = setup_test_repo();
    let mut job1 = MemoryArchiveJob::new(pid(), ArchiveJobType::MemorySnapshot, ArchiveJobPayload::memory_snapshot("m1"));
    let job2 = MemoryArchiveJob::new(pid(), ArchiveJobType::RuleSnapshot, ArchiveJobPayload::rule_snapshot("r1"));

    repo.create(job1.clone()).await.unwrap();
    repo.create(job2).await.unwrap();

    job1.start();
    repo.update(&job1).await.unwrap();

    let pending = repo.get_by_project_and_status(&pid(), ArchiveJobStatus::Pending).await.unwrap();
    assert_eq!(pending.len(), 1);

    let running = repo.get_by_project_and_status(&pid(), ArchiveJobStatus::Running).await.unwrap();
    assert_eq!(running.len(), 1);
    assert_eq!(running[0].id, job1.id);

    let done = repo.get_by_project_and_status(&pid(), ArchiveJobStatus::Done).await.unwrap();
    assert!(done.is_empty());
}

// ─── get_by_project_and_type ─────────────────────────────────────────────────

#[tokio::test]
async fn test_get_by_project_and_type() {
    let repo = setup_test_repo();
    repo.create(MemoryArchiveJob::new(pid(), ArchiveJobType::MemorySnapshot, ArchiveJobPayload::memory_snapshot("m1"))).await.unwrap();
    repo.create(MemoryArchiveJob::new(pid(), ArchiveJobType::MemorySnapshot, ArchiveJobPayload::memory_snapshot("m2"))).await.unwrap();
    repo.create(MemoryArchiveJob::new(pid(), ArchiveJobType::RuleSnapshot, ArchiveJobPayload::rule_snapshot("r1"))).await.unwrap();

    let mem = repo.get_by_project_and_type(&pid(), ArchiveJobType::MemorySnapshot).await.unwrap();
    assert_eq!(mem.len(), 2);

    let rule = repo.get_by_project_and_type(&pid(), ArchiveJobType::RuleSnapshot).await.unwrap();
    assert_eq!(rule.len(), 1);
}

// ─── claim_next_for_project ──────────────────────────────────────────────────

#[tokio::test]
async fn test_claim_next_for_project_returns_oldest_pending() {
    let repo = setup_test_repo();
    insert_test_project(repo.db(), "test-project-2", "/test/path2");

    let job1 = MemoryArchiveJob::new(pid(), ArchiveJobType::MemorySnapshot, ArchiveJobPayload::memory_snapshot("m1"));
    let job2 = MemoryArchiveJob::new(pid2(), ArchiveJobType::RuleSnapshot, ArchiveJobPayload::rule_snapshot("r1"));

    repo.create(job1.clone()).await.unwrap();
    tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;
    repo.create(job2.clone()).await.unwrap();

    // Claiming for pid2 should return job2 (not job1 from pid)
    let claimed = repo.claim_next_for_project(&pid2()).await.unwrap();
    assert!(claimed.is_some());
    let c = claimed.unwrap();
    assert_eq!(c.id, job2.id);
    assert_eq!(c.status, ArchiveJobStatus::Running);

    // pid still has job1
    let still_claimable = repo.claim_next_for_project(&pid()).await.unwrap();
    assert!(still_claimable.is_some());
    assert_eq!(still_claimable.unwrap().id, job1.id);
}

#[tokio::test]
async fn test_claim_next_for_project_returns_none_when_empty() {
    let repo = setup_test_repo();
    let result = repo.claim_next_for_project(&pid()).await.unwrap();
    assert!(result.is_none());
}

#[tokio::test]
async fn test_claim_next_for_project_picks_failed_jobs() {
    let repo = setup_test_repo();
    let mut job = MemoryArchiveJob::new(pid(), ArchiveJobType::MemorySnapshot, ArchiveJobPayload::memory_snapshot("m1"));
    repo.create(job.clone()).await.unwrap();

    job.start();
    repo.update(&job).await.unwrap();
    job.fail("transient error");
    repo.update(&job).await.unwrap();

    // Failed job should be claimable
    let claimed = repo.claim_next_for_project(&pid()).await.unwrap();
    assert!(claimed.is_some());
    assert_eq!(claimed.unwrap().status, ArchiveJobStatus::Running);
}

// ─── count_by_status ─────────────────────────────────────────────────────────

#[tokio::test]
async fn test_count_by_status() {
    let repo = setup_test_repo();

    assert_eq!(repo.count_by_status(ArchiveJobStatus::Pending).await.unwrap(), 0);

    let j1 = MemoryArchiveJob::new(pid(), ArchiveJobType::MemorySnapshot, ArchiveJobPayload::memory_snapshot("m1"));
    let mut j2 = MemoryArchiveJob::new(pid(), ArchiveJobType::RuleSnapshot, ArchiveJobPayload::rule_snapshot("r1"));
    repo.create(j1).await.unwrap();
    repo.create(j2.clone()).await.unwrap();

    assert_eq!(repo.count_by_status(ArchiveJobStatus::Pending).await.unwrap(), 2);

    j2.start();
    repo.update(&j2).await.unwrap();

    assert_eq!(repo.count_by_status(ArchiveJobStatus::Pending).await.unwrap(), 1);
    assert_eq!(repo.count_by_status(ArchiveJobStatus::Running).await.unwrap(), 1);
    assert_eq!(repo.count_by_status(ArchiveJobStatus::Done).await.unwrap(), 0);
}

// ─── count_claimable_for_project ─────────────────────────────────────────────

#[tokio::test]
async fn test_count_claimable_for_project() {
    let repo = setup_test_repo();
    insert_test_project(repo.db(), "test-project-2", "/test/path2");

    repo.create(MemoryArchiveJob::new(pid(), ArchiveJobType::MemorySnapshot, ArchiveJobPayload::memory_snapshot("m1"))).await.unwrap();
    repo.create(MemoryArchiveJob::new(pid(), ArchiveJobType::RuleSnapshot, ArchiveJobPayload::rule_snapshot("r1"))).await.unwrap();
    repo.create(MemoryArchiveJob::new(pid2(), ArchiveJobType::MemorySnapshot, ArchiveJobPayload::memory_snapshot("m2"))).await.unwrap();

    assert_eq!(repo.count_claimable_for_project(&pid()).await.unwrap(), 2);
    assert_eq!(repo.count_claimable_for_project(&pid2()).await.unwrap(), 1);

    // Claim pid2's job — count should drop to 0
    repo.claim_next_for_project(&pid2()).await.unwrap();
    assert_eq!(repo.count_claimable_for_project(&pid2()).await.unwrap(), 0);
}

// ─── delete_completed_older_than ─────────────────────────────────────────────

#[tokio::test]
async fn test_delete_completed_older_than_removes_old_done_jobs() {
    let repo = setup_test_repo();
    let mut job = MemoryArchiveJob::new(pid(), ArchiveJobType::MemorySnapshot, ArchiveJobPayload::memory_snapshot("m1"));
    repo.create(job.clone()).await.unwrap();

    job.start();
    job.complete();
    // Backdate completed_at to 35 days ago
    job.completed_at = Some(Utc::now() - chrono::Duration::days(35));
    repo.update(&job).await.unwrap();

    let deleted = repo.delete_completed_older_than(30).await.unwrap();
    assert_eq!(deleted, 1);
    assert!(repo.get_by_id(&job.id).await.unwrap().is_none());
}

#[tokio::test]
async fn test_delete_completed_older_than_keeps_recent_jobs() {
    let repo = setup_test_repo();
    let mut job = MemoryArchiveJob::new(pid(), ArchiveJobType::MemorySnapshot, ArchiveJobPayload::memory_snapshot("m1"));
    repo.create(job.clone()).await.unwrap();

    job.start();
    job.complete();
    // completed_at is recent (now)
    repo.update(&job).await.unwrap();

    let deleted = repo.delete_completed_older_than(30).await.unwrap();
    assert_eq!(deleted, 0);
    assert!(repo.get_by_id(&job.id).await.unwrap().is_some());
}

#[tokio::test]
async fn test_delete_completed_older_than_ignores_non_done_jobs() {
    let repo = setup_test_repo();
    // Pending job with old created_at — should NOT be deleted (only 'done' status)
    let job = MemoryArchiveJob::new(pid(), ArchiveJobType::MemorySnapshot, ArchiveJobPayload::memory_snapshot("m1"));
    let job_id = job.id.clone();
    repo.create(job).await.unwrap();

    let deleted = repo.delete_completed_older_than(0).await.unwrap();
    assert_eq!(deleted, 0);
    assert!(repo.get_by_id(&job_id).await.unwrap().is_some());
}

// ─── delete_by_project ───────────────────────────────────────────────────────

#[tokio::test]
async fn test_delete_by_project() {
    let repo = setup_test_repo();
    insert_test_project(repo.db(), "test-project-2", "/test/path2");

    repo.create(MemoryArchiveJob::new(pid(), ArchiveJobType::MemorySnapshot, ArchiveJobPayload::memory_snapshot("m1"))).await.unwrap();
    repo.create(MemoryArchiveJob::new(pid(), ArchiveJobType::RuleSnapshot, ArchiveJobPayload::rule_snapshot("r1"))).await.unwrap();
    let j2 = MemoryArchiveJob::new(pid2(), ArchiveJobType::MemorySnapshot, ArchiveJobPayload::memory_snapshot("m2"));
    let j2_id = j2.id.clone();
    repo.create(j2).await.unwrap();

    repo.delete_by_project(&pid()).await.unwrap();

    assert!(repo.get_by_project(&pid()).await.unwrap().is_empty());
    assert!(repo.get_by_id(&j2_id).await.unwrap().is_some());
}
