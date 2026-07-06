use super::*;
use crate::domain::entities::{ReviewNote, ReviewOutcome, ReviewerType};
use crate::infrastructure::sqlite::SqliteReviewIssueRepository;
use crate::testing::SqliteTestDb;

struct TestContext {
    _db: SqliteTestDb,
    service: ReviewIssueService<SqliteReviewIssueRepository>,
    task_id: TaskId,
    review_note_id: ReviewNoteId,
}

fn create_service(db: &SqliteTestDb) -> ReviewIssueService<SqliteReviewIssueRepository> {
    let repo = SqliteReviewIssueRepository::new(db.new_connection());
    ReviewIssueService::new(Arc::new(repo))
}

fn create_test_review_note(db: &SqliteTestDb, task_id: &TaskId) -> ReviewNoteId {
    let mut review_note = ReviewNote::new(
        task_id.clone(),
        ReviewerType::Ai,
        ReviewOutcome::ChangesRequested,
    );
    review_note.summary = Some("Test summary".to_string());
    db.insert_review_note(review_note).id
}

fn setup_test_context() -> TestContext {
    let db = SqliteTestDb::new("review-issue-service");
    let project = db.seed_project("Test Project");
    let task = db.seed_task(project.id.clone(), "Test Task");
    let review_note_id = create_test_review_note(&db, &task.id);
    let service = create_service(&db);

    TestContext {
        _db: db,
        service,
        task_id: task.id,
        review_note_id,
    }
}

fn setup_empty_service() -> (SqliteTestDb, ReviewIssueService<SqliteReviewIssueRepository>) {
    let db = SqliteTestDb::new("review-issue-service-empty");
    let service = create_service(&db);
    (db, service)
}

fn create_test_input(title: &str) -> CreateIssueInput {
    CreateIssueInput {
        title: title.to_string(),
        description: Some("Test description".to_string()),
        severity: IssueSeverity::Major,
        category: Some(IssueCategory::Bug),
        step_id: None,
        no_step_reason: Some("Cross-cutting concern".to_string()),
        file_path: Some("src/main.rs".to_string()),
        line_number: Some(42),
        code_snippet: None,
    }
}

// ===== CreateIssueInput Validation Tests =====

#[test]
fn test_create_issue_input_validation_success() {
    let input = create_test_input("Test issue");
    assert!(input.validate().is_ok());
}

#[test]
fn test_create_issue_input_validation_with_step_id() {
    let input = CreateIssueInput {
        title: "Test issue".to_string(),
        description: None,
        severity: IssueSeverity::Minor,
        category: None,
        step_id: Some(TaskStepId::new()),
        no_step_reason: None,
        file_path: None,
        line_number: None,
        code_snippet: None,
    };
    assert!(input.validate().is_ok());
}

#[test]
fn test_create_issue_input_validation_missing_step_and_reason() {
    let input = CreateIssueInput {
        title: "Test issue".to_string(),
        description: None,
        severity: IssueSeverity::Minor,
        category: None,
        step_id: None,
        no_step_reason: None,
        file_path: None,
        line_number: None,
        code_snippet: None,
    };
    assert!(input.validate().is_err());
    assert!(input
        .validate()
        .unwrap_err()
        .contains("step_id or no_step_reason"));
}

#[test]
fn test_create_issue_input_validation_empty_title() {
    let mut input = create_test_input("");
    input.title = "   ".to_string();
    assert!(input.validate().is_err());
    assert!(input.validate().unwrap_err().contains("title"));
}

// ===== Service Tests =====

#[tokio::test]
async fn test_create_issues_from_review() {
    let ctx = setup_test_context();

    let inputs = vec![create_test_input("Issue 1"), create_test_input("Issue 2")];

    let issues = ctx
        .service
        .create_issues_from_review(ctx.review_note_id, ctx.task_id.clone(), inputs)
        .await
        .unwrap();

    assert_eq!(issues.len(), 2);
    assert_eq!(issues[0].title, "Issue 1");
    assert_eq!(issues[1].title, "Issue 2");
    assert!(issues.iter().all(|i| i.status == IssueStatus::Open));

    // Verify they're in the database
    let all_issues = ctx.service.get_issues_by_task(&ctx.task_id).await.unwrap();
    assert_eq!(all_issues.len(), 2);
}

#[tokio::test]
async fn test_create_issues_validation_failure() {
    let ctx = setup_test_context();

    // Second input is missing step_id and no_step_reason
    let inputs = vec![
        create_test_input("Valid issue"),
        CreateIssueInput {
            title: "Invalid issue".to_string(),
            description: None,
            severity: IssueSeverity::Minor,
            category: None,
            step_id: None,
            no_step_reason: None,
            file_path: None,
            line_number: None,
            code_snippet: None,
        },
    ];

    let result = ctx
        .service
        .create_issues_from_review(ctx.review_note_id, ctx.task_id, inputs)
        .await;

    assert!(result.is_err());
    let err = result.unwrap_err().to_string();
    assert!(err.contains("Issue 2"));
}

#[tokio::test]
async fn test_mark_issue_in_progress() {
    let ctx = setup_test_context();

    let inputs = vec![create_test_input("Test issue")];
    let issues = ctx
        .service
        .create_issues_from_review(ctx.review_note_id, ctx.task_id, inputs)
        .await
        .unwrap();

    let issue_id = &issues[0].id;
    let updated = ctx.service.mark_issue_in_progress(issue_id).await.unwrap();

    assert_eq!(updated.status, IssueStatus::InProgress);
}

#[tokio::test]
async fn test_mark_issue_in_progress_wrong_status() {
    let ctx = setup_test_context();

    let inputs = vec![create_test_input("Test issue")];
    let issues = ctx
        .service
        .create_issues_from_review(ctx.review_note_id, ctx.task_id, inputs)
        .await
        .unwrap();

    let issue_id = &issues[0].id;

    // Mark as addressed first
    ctx.service
        .mark_issue_addressed(issue_id, Some("Fixed".to_string()), 1)
        .await
        .unwrap();

    // Now try to mark as in_progress - should fail
    let result = ctx.service.mark_issue_in_progress(issue_id).await;
    assert!(result.is_err());
}

#[tokio::test]
async fn test_mark_issue_addressed() {
    let ctx = setup_test_context();

    let inputs = vec![create_test_input("Test issue")];
    let issues = ctx
        .service
        .create_issues_from_review(ctx.review_note_id, ctx.task_id, inputs)
        .await
        .unwrap();

    let issue_id = &issues[0].id;

    // Can mark addressed directly from open
    let updated = ctx
        .service
        .mark_issue_addressed(issue_id, Some("Fixed the bug".to_string()), 1)
        .await
        .unwrap();

    assert_eq!(updated.status, IssueStatus::Addressed);
    assert_eq!(updated.resolution_notes, Some("Fixed the bug".to_string()));
    assert_eq!(updated.addressed_in_attempt, Some(1));
}

#[tokio::test]
async fn test_mark_issue_addressed_from_in_progress() {
    let ctx = setup_test_context();

    let inputs = vec![create_test_input("Test issue")];
    let issues = ctx
        .service
        .create_issues_from_review(ctx.review_note_id, ctx.task_id, inputs)
        .await
        .unwrap();

    let issue_id = &issues[0].id;

    // First mark as in_progress
    ctx.service.mark_issue_in_progress(issue_id).await.unwrap();

    // Then mark as addressed
    let updated = ctx
        .service
        .mark_issue_addressed(issue_id, Some("Done".to_string()), 2)
        .await
        .unwrap();

    assert_eq!(updated.status, IssueStatus::Addressed);
    assert_eq!(updated.addressed_in_attempt, Some(2));
}

#[tokio::test]
async fn test_verify_issue() {
    let ctx = setup_test_context();
    let verifying_review_id = create_test_review_note(&ctx._db, &ctx.task_id);

    let inputs = vec![create_test_input("Test issue")];
    let issues = ctx
        .service
        .create_issues_from_review(ctx.review_note_id, ctx.task_id, inputs)
        .await
        .unwrap();

    let issue_id = &issues[0].id;

    // Mark as addressed first
    ctx.service
        .mark_issue_addressed(issue_id, Some("Fixed".to_string()), 1)
        .await
        .unwrap();

    // Verify it
    let verified = ctx
        .service
        .verify_issue(issue_id, verifying_review_id.clone())
        .await
        .unwrap();

    assert_eq!(verified.status, IssueStatus::Verified);
    assert_eq!(verified.verified_by_review_id, Some(verifying_review_id));
    assert!(verified.is_terminal());
}

#[tokio::test]
async fn test_verify_issue_wrong_status() {
    let ctx = setup_test_context();

    let inputs = vec![create_test_input("Test issue")];
    let issues = ctx
        .service
        .create_issues_from_review(ctx.review_note_id.clone(), ctx.task_id, inputs)
        .await
        .unwrap();

    let issue_id = &issues[0].id;

    // Try to verify without addressing first - should fail
    let result = ctx.service.verify_issue(issue_id, ctx.review_note_id).await;
    assert!(result.is_err());
}

#[tokio::test]
async fn test_reopen_issue() {
    let ctx = setup_test_context();

    let inputs = vec![create_test_input("Test issue")];
    let issues = ctx
        .service
        .create_issues_from_review(ctx.review_note_id, ctx.task_id, inputs)
        .await
        .unwrap();

    let issue_id = &issues[0].id;

    // Mark as addressed
    ctx.service
        .mark_issue_addressed(issue_id, Some("Fixed".to_string()), 1)
        .await
        .unwrap();

    // Reopen it
    let reopened = ctx
        .service
        .reopen_issue(issue_id, Some("Not actually fixed".to_string()))
        .await
        .unwrap();

    assert_eq!(reopened.status, IssueStatus::Open);
    assert!(reopened
        .resolution_notes
        .as_ref()
        .unwrap()
        .contains("Reopened"));
}

#[tokio::test]
async fn test_reopen_issue_wrong_status() {
    let ctx = setup_test_context();

    let inputs = vec![create_test_input("Test issue")];
    let issues = ctx
        .service
        .create_issues_from_review(ctx.review_note_id, ctx.task_id, inputs)
        .await
        .unwrap();

    let issue_id = &issues[0].id;

    // Try to reopen an open issue - should fail
    let result = ctx.service.reopen_issue(issue_id, None).await;
    assert!(result.is_err());
}

#[tokio::test]
async fn test_mark_issue_wontfix() {
    let ctx = setup_test_context();

    let inputs = vec![create_test_input("Test issue")];
    let issues = ctx
        .service
        .create_issues_from_review(ctx.review_note_id, ctx.task_id, inputs)
        .await
        .unwrap();

    let issue_id = &issues[0].id;

    let wontfix = ctx
        .service
        .mark_issue_wontfix(issue_id, "Not in scope".to_string())
        .await
        .unwrap();

    assert_eq!(wontfix.status, IssueStatus::WontFix);
    assert_eq!(wontfix.resolution_notes, Some("Not in scope".to_string()));
    assert!(wontfix.is_terminal());
}

#[tokio::test]
async fn test_mark_issue_wontfix_already_terminal() {
    let ctx = setup_test_context();
    let verifying_review = create_test_review_note(&ctx._db, &ctx.task_id);

    let inputs = vec![create_test_input("Test issue")];
    let issues = ctx
        .service
        .create_issues_from_review(ctx.review_note_id, ctx.task_id, inputs)
        .await
        .unwrap();

    let issue_id = &issues[0].id;

    // Mark as addressed and verify
    ctx.service
        .mark_issue_addressed(issue_id, Some("Fixed".to_string()), 1)
        .await
        .unwrap();
    ctx.service
        .verify_issue(issue_id, verifying_review)
        .await
        .unwrap();

    // Try to mark as wontfix - should fail
    let result = ctx
        .service
        .mark_issue_wontfix(issue_id, "Too late".to_string())
        .await;
    assert!(result.is_err());
}

#[tokio::test]
async fn test_get_issue_progress() {
    let ctx = setup_test_context();

    let inputs = vec![
        CreateIssueInput {
            title: "Critical issue".to_string(),
            severity: IssueSeverity::Critical,
            no_step_reason: Some("Cross-cutting".to_string()),
            ..create_test_input("Critical issue")
        },
        CreateIssueInput {
            title: "Major issue".to_string(),
            severity: IssueSeverity::Major,
            no_step_reason: Some("Cross-cutting".to_string()),
            ..create_test_input("Major issue")
        },
        CreateIssueInput {
            title: "Minor issue".to_string(),
            severity: IssueSeverity::Minor,
            no_step_reason: Some("Cross-cutting".to_string()),
            ..create_test_input("Minor issue")
        },
    ];

    let issues = ctx
        .service
        .create_issues_from_review(ctx.review_note_id, ctx.task_id.clone(), inputs)
        .await
        .unwrap();

    // Address one issue
    ctx.service
        .mark_issue_addressed(&issues[0].id, Some("Fixed".to_string()), 1)
        .await
        .unwrap();

    let progress = ctx.service.get_issue_progress(&ctx.task_id).await.unwrap();

    assert_eq!(progress.total, 3);
    assert_eq!(progress.open, 2);
    assert_eq!(progress.addressed, 1);
    assert_eq!(progress.by_severity.critical.total, 1);
    assert_eq!(progress.by_severity.major.total, 1);
    assert_eq!(progress.by_severity.minor.total, 1);
}

#[tokio::test]
async fn test_get_open_issues_by_task() {
    let ctx = setup_test_context();

    let inputs = vec![
        create_test_input("Issue 1"),
        create_test_input("Issue 2"),
        create_test_input("Issue 3"),
    ];

    let issues = ctx
        .service
        .create_issues_from_review(ctx.review_note_id, ctx.task_id.clone(), inputs)
        .await
        .unwrap();

    // Address one issue
    ctx.service
        .mark_issue_addressed(&issues[1].id, Some("Fixed".to_string()), 1)
        .await
        .unwrap();

    let open_issues = ctx.service.get_open_issues_by_task(&ctx.task_id).await.unwrap();
    assert_eq!(open_issues.len(), 2);
}

#[tokio::test]
async fn test_get_issue_not_found() {
    let (_db, service) = setup_empty_service();

    let nonexistent = ReviewIssueId::new();
    let result = service.mark_issue_in_progress(&nonexistent).await;

    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("not found"));
}

#[tokio::test]
async fn test_full_lifecycle() {
    let ctx = setup_test_context();
    let verifying_review = create_test_review_note(&ctx._db, &ctx.task_id);

    // Create issue
    let inputs = vec![create_test_input("Bug to fix")];
    let issues = ctx
        .service
        .create_issues_from_review(ctx.review_note_id, ctx.task_id.clone(), inputs)
        .await
        .unwrap();
    let issue_id = &issues[0].id;

    // Initial state
    let issue = ctx.service.get_issue(issue_id).await.unwrap().unwrap();
    assert_eq!(issue.status, IssueStatus::Open);

    // Start work
    let issue = ctx.service.mark_issue_in_progress(issue_id).await.unwrap();
    assert_eq!(issue.status, IssueStatus::InProgress);

    // Address it
    let issue = ctx
        .service
        .mark_issue_addressed(issue_id, Some("Fixed in commit abc123".to_string()), 1)
        .await
        .unwrap();
    assert_eq!(issue.status, IssueStatus::Addressed);

    // Reopen (found it wasn't actually fixed)
    let issue = ctx
        .service
        .reopen_issue(issue_id, Some("Tests still failing".to_string()))
        .await
        .unwrap();
    assert_eq!(issue.status, IssueStatus::Open);

    // Start work again
    let issue = ctx.service.mark_issue_in_progress(issue_id).await.unwrap();
    assert_eq!(issue.status, IssueStatus::InProgress);

    // Address again
    let issue = ctx
        .service
        .mark_issue_addressed(issue_id, Some("Actually fixed now".to_string()), 2)
        .await
        .unwrap();
    assert_eq!(issue.status, IssueStatus::Addressed);
    assert_eq!(issue.addressed_in_attempt, Some(2));

    // Verify
    let issue = ctx
        .service
        .verify_issue(issue_id, verifying_review.clone())
        .await
        .unwrap();
    assert_eq!(issue.status, IssueStatus::Verified);
    assert_eq!(issue.verified_by_review_id, Some(verifying_review));
    assert!(issue.is_terminal());

    // Progress should show fully resolved
    let progress = ctx.service.get_issue_progress(&ctx.task_id).await.unwrap();
    assert_eq!(progress.total, 1);
    assert_eq!(progress.verified, 1);
    assert_eq!(progress.percent_resolved, 100.0);
}
