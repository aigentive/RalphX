use super::*;
use crate::domain::entities::{
    IssueCategory, IssueSeverity, ReviewNote, ReviewOutcome, ReviewerType, Task,
};
use crate::testing::SqliteTestDb;

fn setup_repo() -> (SqliteTestDb, SqliteReviewIssueRepository) {
    let db = SqliteTestDb::new("sqlite-review-issue-repo");
    let repo = SqliteReviewIssueRepository::new(db.new_connection());
    (db, repo)
}

fn create_test_review_note(db: &SqliteTestDb, task_id: TaskId) -> ReviewNote {
    let mut review_note = ReviewNote::new(task_id, ReviewerType::Ai, ReviewOutcome::ChangesRequested);
    review_note.summary = Some("Test summary".to_string());
    db.insert_review_note(review_note)
}

fn seed_task_graph(db: &SqliteTestDb) -> (Task, ReviewNote) {
    let project = db.seed_project("Test Project");
    let task = db.seed_task(project.id, "Test Task");
    let review_note = create_test_review_note(db, task.id.clone());
    (task, review_note)
}

fn create_test_issue(review_note: &ReviewNote, task_id: &TaskId) -> ReviewIssue {
    ReviewIssue::new(
        review_note.id.clone(),
        task_id.clone(),
        "Test issue".to_string(),
        IssueSeverity::Major,
    )
}

#[tokio::test]
async fn test_create_and_get_by_id() {
    let (db, repo) = setup_repo();
    let (task, review_note) = seed_task_graph(&db);

    let mut issue = create_test_issue(&review_note, &task.id);
    issue.description = Some("Test description".to_string());
    issue.category = Some(IssueCategory::Bug);
    issue.file_path = Some("src/main.rs".to_string());
    issue.line_number = Some(42);
    let issue_id = issue.id.clone();

    let created = repo.create(issue).await.unwrap();
    assert_eq!(created.title, "Test issue");
    assert_eq!(created.severity, IssueSeverity::Major);
    assert_eq!(created.status, IssueStatus::Open);

    let fetched = repo.get_by_id(&issue_id).await.unwrap();
    assert!(fetched.is_some());
    let fetched = fetched.unwrap();
    assert_eq!(fetched.title, "Test issue");
    assert_eq!(fetched.description, Some("Test description".to_string()));
    assert_eq!(fetched.category, Some(IssueCategory::Bug));
    assert_eq!(fetched.file_path, Some("src/main.rs".to_string()));
    assert_eq!(fetched.line_number, Some(42));
}

#[tokio::test]
async fn test_get_by_id_not_found() {
    let (_db, repo) = setup_repo();

    let issue_id = ReviewIssueId::new();
    let result = repo.get_by_id(&issue_id).await.unwrap();
    assert!(result.is_none());
}

#[tokio::test]
async fn test_get_by_task_id() {
    let (db, repo) = setup_repo();
    let (task, review_note) = seed_task_graph(&db);

    let issue1 = create_test_issue(&review_note, &task.id);
    let mut issue2 = create_test_issue(&review_note, &task.id);
    issue2.title = "Second issue".to_string();

    repo.create(issue1).await.unwrap();
    repo.create(issue2).await.unwrap();

    let issues = repo.get_by_task_id(&task.id).await.unwrap();
    assert_eq!(issues.len(), 2);
}

#[tokio::test]
async fn test_get_open_by_task_id() {
    let (db, repo) = setup_repo();
    let (task, review_note) = seed_task_graph(&db);

    let issue1 = create_test_issue(&review_note, &task.id);
    let mut issue2 = create_test_issue(&review_note, &task.id);
    issue2.title = "Addressed issue".to_string();
    issue2.status = IssueStatus::Addressed;

    let issue1_id = issue1.id.clone();
    repo.create(issue1).await.unwrap();
    repo.create(issue2).await.unwrap();

    let open_issues = repo.get_open_by_task_id(&task.id).await.unwrap();
    assert_eq!(open_issues.len(), 1);
    assert_eq!(open_issues[0].id, issue1_id);
}

#[tokio::test]
async fn test_update_status() {
    let (db, repo) = setup_repo();
    let (task, review_note) = seed_task_graph(&db);

    let issue = create_test_issue(&review_note, &task.id);
    let issue_id = issue.id.clone();

    repo.create(issue).await.unwrap();

    let updated = repo
        .update_status(
            &issue_id,
            IssueStatus::Addressed,
            Some("Fixed the bug".to_string()),
        )
        .await
        .unwrap();

    assert_eq!(updated.status, IssueStatus::Addressed);
    assert_eq!(updated.resolution_notes, Some("Fixed the bug".to_string()));
}

#[tokio::test]
async fn test_update_full_issue() {
    let (db, repo) = setup_repo();
    let (task, review_note) = seed_task_graph(&db);

    let mut issue = create_test_issue(&review_note, &task.id);
    let issue_id = issue.id.clone();
    repo.create(issue.clone()).await.unwrap();

    issue.start_work();
    issue.mark_addressed(Some("Fixed".to_string()), 2);
    repo.update(&issue).await.unwrap();

    let fetched = repo.get_by_id(&issue_id).await.unwrap().unwrap();
    assert_eq!(fetched.status, IssueStatus::Addressed);
    assert_eq!(fetched.resolution_notes, Some("Fixed".to_string()));
    assert_eq!(fetched.addressed_in_attempt, Some(2));
}

#[tokio::test]
async fn test_bulk_create() {
    let (db, repo) = setup_repo();
    let (task, review_note) = seed_task_graph(&db);

    let issues = vec![
        {
            let mut issue = create_test_issue(&review_note, &task.id);
            issue.title = "Issue 1".to_string();
            issue.severity = IssueSeverity::Critical;
            issue
        },
        {
            let mut issue = create_test_issue(&review_note, &task.id);
            issue.title = "Issue 2".to_string();
            issue.severity = IssueSeverity::Minor;
            issue
        },
        {
            let mut issue = create_test_issue(&review_note, &task.id);
            issue.title = "Issue 3".to_string();
            issue.severity = IssueSeverity::Suggestion;
            issue
        },
    ];

    let created = repo.bulk_create(issues).await.unwrap();
    assert_eq!(created.len(), 3);

    let fetched = repo.get_by_task_id(&task.id).await.unwrap();
    assert_eq!(fetched.len(), 3);
}

#[tokio::test]
async fn test_bulk_create_rollback_on_error() {
    let (db, repo) = setup_repo();
    let (task, review_note) = seed_task_graph(&db);

    let issue = create_test_issue(&review_note, &task.id);
    let issue_id = issue.id.clone();

    repo.create(issue.clone()).await.unwrap();

    let issues = vec![
        issue.clone(),
        {
            let mut new_issue = create_test_issue(&review_note, &task.id);
            new_issue.title = "New issue".to_string();
            new_issue
        },
    ];

    let result = repo.bulk_create(issues).await;
    assert!(result.is_err());

    let fetched = repo.get_by_task_id(&task.id).await.unwrap();
    assert_eq!(fetched.len(), 1);
    assert_eq!(fetched[0].id, issue_id);
}

#[tokio::test]
async fn test_get_summary() {
    let (db, repo) = setup_repo();
    let (task, review_note) = seed_task_graph(&db);

    let issues = vec![
        {
            let mut issue = create_test_issue(&review_note, &task.id);
            issue.severity = IssueSeverity::Critical;
            issue.status = IssueStatus::Open;
            issue
        },
        {
            let mut issue = create_test_issue(&review_note, &task.id);
            issue.severity = IssueSeverity::Major;
            issue.status = IssueStatus::InProgress;
            issue
        },
        {
            let mut issue = create_test_issue(&review_note, &task.id);
            issue.severity = IssueSeverity::Minor;
            issue.status = IssueStatus::Addressed;
            issue
        },
        {
            let mut issue = create_test_issue(&review_note, &task.id);
            issue.severity = IssueSeverity::Suggestion;
            issue.status = IssueStatus::Verified;
            issue
        },
    ];

    repo.bulk_create(issues).await.unwrap();

    let summary = repo.get_summary(&task.id).await.unwrap();
    assert_eq!(summary.total, 4);
    assert_eq!(summary.open, 1);
    assert_eq!(summary.in_progress, 1);
    assert_eq!(summary.addressed, 1);
    assert_eq!(summary.verified, 1);
    assert_eq!(summary.wontfix, 0);
    assert_eq!(summary.percent_resolved, 50.0);
    assert_eq!(summary.by_severity.critical.total, 1);
    assert_eq!(summary.by_severity.critical.open, 1);
    assert_eq!(summary.by_severity.major.total, 1);
    assert_eq!(summary.by_severity.minor.total, 1);
    assert_eq!(summary.by_severity.suggestion.total, 1);
}

#[tokio::test]
async fn test_get_summary_empty() {
    let (db, repo) = setup_repo();
    let project = db.seed_project("Test Project");
    let task = db.seed_task(project.id, "Test Task");

    let summary = repo.get_summary(&task.id).await.unwrap();
    assert_eq!(summary.total, 0);
    assert_eq!(summary.open, 0);
    assert_eq!(summary.percent_resolved, 0.0);
}

#[tokio::test]
async fn test_issue_with_all_optional_fields() {
    let (db, repo) = setup_repo();
    let (task, review_note) = seed_task_graph(&db);
    let verifying_review = create_test_review_note(&db, task.id.clone());

    let mut issue = create_test_issue(&review_note, &task.id);
    issue.description = Some("Full description".to_string());
    issue.category = Some(IssueCategory::Quality);
    issue.file_path = Some("/path/to/file.rs".to_string());
    issue.line_number = Some(100);
    issue.code_snippet = Some("fn buggy_code() {}".to_string());
    issue.no_step_reason = Some("Cross-cutting concern".to_string());
    issue.resolution_notes = Some("Fixed by refactoring".to_string());
    issue.addressed_in_attempt = Some(3);
    issue.verified_by_review_id = Some(verifying_review.id.clone());
    issue.status = IssueStatus::Verified;

    let issue_id = issue.id.clone();
    repo.create(issue).await.unwrap();

    let fetched = repo.get_by_id(&issue_id).await.unwrap().unwrap();
    assert_eq!(fetched.description, Some("Full description".to_string()));
    assert_eq!(fetched.category, Some(IssueCategory::Quality));
    assert_eq!(fetched.file_path, Some("/path/to/file.rs".to_string()));
    assert_eq!(fetched.line_number, Some(100));
    assert_eq!(fetched.code_snippet, Some("fn buggy_code() {}".to_string()));
    assert_eq!(
        fetched.no_step_reason,
        Some("Cross-cutting concern".to_string())
    );
    assert_eq!(
        fetched.resolution_notes,
        Some("Fixed by refactoring".to_string())
    );
    assert_eq!(fetched.addressed_in_attempt, Some(3));
    assert_eq!(fetched.verified_by_review_id, Some(verifying_review.id));
    assert_eq!(fetched.status, IssueStatus::Verified);
}
