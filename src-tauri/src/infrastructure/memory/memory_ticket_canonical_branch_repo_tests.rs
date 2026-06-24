use chrono::Utc;

use crate::domain::entities::{ProjectId, TicketCanonicalBranch};
use crate::domain::repositories::TicketCanonicalBranchRepository;
use crate::infrastructure::memory::MemoryTicketCanonicalBranchRepository;

fn project() -> ProjectId {
    ProjectId::from_string("project-1".to_string())
}

fn branch(issue_key: &str) -> TicketCanonicalBranch {
    TicketCanonicalBranch::new(
        project(),
        "linear",
        issue_key.to_string(),
        format!("ralphx/ticket/linear-{issue_key}"),
        "main".to_string(),
        Some("abc123".to_string()),
        Utc::now(),
    )
}

#[tokio::test]
async fn upsert_then_get_round_trips() {
    let repo = MemoryTicketCanonicalBranchRepository::new();

    let saved = repo.upsert(branch("wise-24")).await.unwrap();
    assert_eq!(saved.branch_name, "ralphx/ticket/linear-wise-24");

    let loaded = repo
        .get(&project(), "linear", "wise-24")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(loaded.branch_name, "ralphx/ticket/linear-wise-24");
    assert!(!loaded.origin_pushed);
    assert!(!loaded.terminal);
}

#[tokio::test]
async fn get_returns_none_for_unknown_key() {
    let repo = MemoryTicketCanonicalBranchRepository::new();

    assert!(repo
        .get(&project(), "linear", "missing")
        .await
        .unwrap()
        .is_none());
}

#[tokio::test]
async fn upsert_preserves_created_at_on_conflict() {
    let repo = MemoryTicketCanonicalBranchRepository::new();
    let first = repo.upsert(branch("wise-24")).await.unwrap();

    let mut updated = branch("wise-24");
    updated.created_at = Utc::now();
    updated.base_commit = Some("def456".to_string());
    let second = repo.upsert(updated).await.unwrap();

    assert_eq!(second.created_at, first.created_at);
    assert_eq!(second.base_commit.as_deref(), Some("def456"));
}

#[tokio::test]
async fn mark_origin_pushed_sets_flag() {
    let repo = MemoryTicketCanonicalBranchRepository::new();
    repo.upsert(branch("wise-24")).await.unwrap();

    repo.mark_origin_pushed(&project(), "linear", "wise-24")
        .await
        .unwrap();

    let loaded = repo
        .get(&project(), "linear", "wise-24")
        .await
        .unwrap()
        .unwrap();
    assert!(loaded.origin_pushed);
}

#[tokio::test]
async fn mark_terminal_sets_flag() {
    let repo = MemoryTicketCanonicalBranchRepository::new();
    repo.upsert(branch("wise-24")).await.unwrap();

    repo.mark_terminal(&project(), "linear", "wise-24")
        .await
        .unwrap();

    let loaded = repo
        .get(&project(), "linear", "wise-24")
        .await
        .unwrap()
        .unwrap();
    assert!(loaded.terminal);
}

#[tokio::test]
async fn mark_helpers_are_noops_for_unknown_key() {
    let repo = MemoryTicketCanonicalBranchRepository::new();

    repo.mark_origin_pushed(&project(), "linear", "missing")
        .await
        .unwrap();
    repo.mark_terminal(&project(), "linear", "missing")
        .await
        .unwrap();

    assert!(repo
        .get(&project(), "linear", "missing")
        .await
        .unwrap()
        .is_none());
}
