use chrono::Utc;

use crate::domain::entities::{ProjectId, TicketCanonicalBranch};
use crate::domain::repositories::TicketCanonicalBranchRepository;
use crate::infrastructure::sqlite::SqliteTicketCanonicalBranchRepository;
use crate::testing::SqliteTestDb;

fn setup_test_db() -> SqliteTestDb {
    SqliteTestDb::new("sqlite_ticket_canonical_branch_repo_tests")
}

fn branch(issue_key: &str) -> TicketCanonicalBranch {
    TicketCanonicalBranch::new(
        ProjectId::from_string("project-1".to_string()),
        "linear",
        issue_key.to_string(),
        format!("ralphx/ticket/linear-{issue_key}"),
        "main".to_string(),
        Some("abc123".to_string()),
        Utc::now(),
    )
}

#[tokio::test]
async fn upsert_round_trips_all_fields() {
    let db = setup_test_db();
    let repo = SqliteTicketCanonicalBranchRepository::from_shared(db.shared_conn());

    let saved = repo.upsert(branch("wise-24")).await.unwrap();
    assert_eq!(saved.branch_name, "ralphx/ticket/linear-wise-24");
    assert_eq!(saved.base_branch, "main");
    assert_eq!(saved.base_commit.as_deref(), Some("abc123"));
    assert!(!saved.origin_pushed);
    assert!(!saved.terminal);

    let project_id = ProjectId::from_string("project-1".to_string());
    let loaded = repo
        .get(&project_id, "linear", "wise-24")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(loaded.branch_name, "ralphx/ticket/linear-wise-24");
    assert_eq!(loaded.base_commit.as_deref(), Some("abc123"));
    assert!(!loaded.origin_pushed);
    assert!(!loaded.terminal);
}

#[tokio::test]
async fn get_returns_none_for_unknown_key() {
    let db = setup_test_db();
    let repo = SqliteTicketCanonicalBranchRepository::from_shared(db.shared_conn());
    let project_id = ProjectId::from_string("project-1".to_string());

    let result = repo.get(&project_id, "linear", "missing").await.unwrap();

    assert!(result.is_none());
}

#[tokio::test]
async fn upsert_updates_existing_row_on_primary_key_conflict() {
    let db = setup_test_db();
    let repo = SqliteTicketCanonicalBranchRepository::from_shared(db.shared_conn());

    repo.upsert(branch("wise-24")).await.unwrap();

    let mut updated = branch("wise-24");
    updated.origin_pushed = true;
    updated.base_commit = Some("def456".to_string());
    let saved = repo.upsert(updated).await.unwrap();

    assert!(saved.origin_pushed);
    assert_eq!(saved.base_commit.as_deref(), Some("def456"));

    let project_id = ProjectId::from_string("project-1".to_string());
    let loaded = repo
        .get(&project_id, "linear", "wise-24")
        .await
        .unwrap()
        .unwrap();
    assert!(loaded.origin_pushed);
    assert_eq!(loaded.base_commit.as_deref(), Some("def456"));
}

#[tokio::test]
async fn mark_origin_pushed_sets_flag() {
    let db = setup_test_db();
    let repo = SqliteTicketCanonicalBranchRepository::from_shared(db.shared_conn());
    let project_id = ProjectId::from_string("project-1".to_string());

    repo.upsert(branch("wise-24")).await.unwrap();
    repo.mark_origin_pushed(&project_id, "linear", "wise-24")
        .await
        .unwrap();

    let loaded = repo
        .get(&project_id, "linear", "wise-24")
        .await
        .unwrap()
        .unwrap();
    assert!(loaded.origin_pushed);
    assert!(!loaded.terminal);
}

#[tokio::test]
async fn mark_terminal_sets_flag() {
    let db = setup_test_db();
    let repo = SqliteTicketCanonicalBranchRepository::from_shared(db.shared_conn());
    let project_id = ProjectId::from_string("project-1".to_string());

    repo.upsert(branch("wise-24")).await.unwrap();
    repo.mark_terminal(&project_id, "linear", "wise-24")
        .await
        .unwrap();

    let loaded = repo
        .get(&project_id, "linear", "wise-24")
        .await
        .unwrap()
        .unwrap();
    assert!(loaded.terminal);
}

#[tokio::test]
async fn rows_are_scoped_by_project_provider_and_issue_key() {
    let db = setup_test_db();
    let repo = SqliteTicketCanonicalBranchRepository::from_shared(db.shared_conn());

    repo.upsert(branch("wise-24")).await.unwrap();

    let mut other_project = branch("wise-24");
    other_project.project_id = ProjectId::from_string("project-2".to_string());
    other_project.branch_name = "ralphx/ticket/linear-wise-24-p2".to_string();
    repo.upsert(other_project).await.unwrap();

    let mut other_provider = branch("wise-24");
    other_provider.provider = "jira".to_string();
    other_provider.branch_name = "ralphx/ticket/jira-wise-24".to_string();
    repo.upsert(other_provider).await.unwrap();

    let p1 = ProjectId::from_string("project-1".to_string());
    let p2 = ProjectId::from_string("project-2".to_string());
    assert_eq!(
        repo.get(&p1, "linear", "wise-24")
            .await
            .unwrap()
            .unwrap()
            .branch_name,
        "ralphx/ticket/linear-wise-24"
    );
    assert_eq!(
        repo.get(&p2, "linear", "wise-24")
            .await
            .unwrap()
            .unwrap()
            .branch_name,
        "ralphx/ticket/linear-wise-24-p2"
    );
    assert_eq!(
        repo.get(&p1, "jira", "wise-24")
            .await
            .unwrap()
            .unwrap()
            .branch_name,
        "ralphx/ticket/jira-wise-24"
    );
}
