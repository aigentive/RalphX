use chrono::Utc;

use crate::domain::entities::{
    ProjectId, TicketCanonicalBranch, TicketCanonicalBranchCycle, TicketCanonicalBranchCycleState,
    TicketCanonicalBranchPolicyKind, TicketGitConventionSnapshot,
};
use crate::domain::repositories::TicketCanonicalBranchRepository;
use crate::error::AppError;
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

fn strict_branch(issue_key: &str, branch_name: &str, task_title: &str) -> TicketCanonicalBranch {
    TicketCanonicalBranch::new_strict(
        project(),
        "clickup",
        issue_key,
        branch_name,
        "main",
        Some("abc123".to_string()),
        TicketGitConventionSnapshot {
            policy_version: 1,
            task_title: task_title.to_string(),
            username: Some("Ada Lovelace".to_string()),
            commit_subject_rule: format!("{issue_key} - {task_title}: :summary:"),
            pr_title: format!("{issue_key} - {task_title}"),
        },
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

#[tokio::test]
async fn strict_create_if_absent_keeps_the_first_immutable_binding() {
    let repo = MemoryTicketCanonicalBranchRepository::new();
    let first = strict_branch("CU-24", "cu-24_fix-login_ada", "Fix login");
    let competing = strict_branch("CU-24", "cu-24_renamed-task_grace", "Renamed task");

    let stored_first = repo.create_if_absent(first).await.unwrap();
    let stored_competing = repo.create_if_absent(competing).await.unwrap();

    assert_eq!(stored_competing, stored_first);
    assert_eq!(
        stored_competing.branch_name, "cu-24_fix-login_ada",
        "the first rendered branch remains authoritative"
    );
    assert_eq!(
        stored_competing
            .strict_policy
            .as_ref()
            .map(|policy| policy.task_title.as_str()),
        Some("Fix login"),
        "task/template drift cannot rewrite the frozen snapshot"
    );
    assert!(repo
        .get_by_branch_name(&project(), "cu-24_renamed-task_grace")
        .await
        .unwrap()
        .is_none());
}

#[tokio::test]
async fn strict_create_if_absent_rejects_a_project_branch_collision() {
    let repo = MemoryTicketCanonicalBranchRepository::new();
    repo.create_if_absent(strict_branch(
        "CU-24",
        "cu-normalized-collision",
        "First task",
    ))
    .await
    .unwrap();

    let error = repo
        .create_if_absent(strict_branch(
            "CU-25",
            "cu-normalized-collision",
            "Second task",
        ))
        .await
        .unwrap_err();

    assert!(matches!(error, AppError::Conflict(_)));
    assert!(repo
        .get(&project(), "clickup", "CU-25")
        .await
        .unwrap()
        .is_none());
    assert_eq!(
        repo.get_by_branch_name(&project(), "cu-normalized-collision")
            .await
            .unwrap()
            .unwrap()
            .issue_key,
        "CU-24"
    );
}

#[tokio::test]
async fn strict_cycle_compare_and_swap_rejects_stale_state_without_mutation() {
    let repo = MemoryTicketCanonicalBranchRepository::new();
    let stored = repo
        .create_if_absent(strict_branch("CU-24", "cu-24_fix-login_ada", "Fix login"))
        .await
        .unwrap();
    let original_cycle = stored.cycle.clone();
    let replacement = TicketCanonicalBranchCycle {
        generation: 1,
        state: TicketCanonicalBranchCycleState::Active,
        base_commit: Some("abc123".to_string()),
        effective_merge_base: Some("abc123".to_string()),
        started_at: original_cycle.started_at,
        terminal_at: None,
    };

    let mut skipped_generation = replacement.clone();
    skipped_generation.generation = 3;
    let invalid_error = repo
        .compare_and_swap_cycle(
            &project(),
            "clickup",
            "CU-24",
            1,
            TicketCanonicalBranchCycleState::Preparing,
            skipped_generation,
        )
        .await
        .unwrap_err();
    assert!(matches!(invalid_error, AppError::Validation(_)));
    assert_eq!(
        repo.get(&project(), "clickup", "CU-24")
            .await
            .unwrap()
            .unwrap()
            .cycle,
        original_cycle,
        "an invalid generation jump must not mutate cycle state"
    );

    let mut stale_generation_replacement = replacement.clone();
    stale_generation_replacement.generation = 2;
    assert!(!repo
        .compare_and_swap_cycle(
            &project(),
            "clickup",
            "CU-24",
            2,
            TicketCanonicalBranchCycleState::Preparing,
            stale_generation_replacement,
        )
        .await
        .unwrap());
    assert!(!repo
        .compare_and_swap_cycle(
            &project(),
            "clickup",
            "CU-24",
            1,
            TicketCanonicalBranchCycleState::Merged,
            replacement.clone(),
        )
        .await
        .unwrap());
    assert_eq!(
        repo.get(&project(), "clickup", "CU-24")
            .await
            .unwrap()
            .unwrap()
            .cycle,
        original_cycle,
        "stale CAS attempts must not partially update cycle state"
    );

    assert!(repo
        .compare_and_swap_cycle(
            &project(),
            "clickup",
            "CU-24",
            1,
            TicketCanonicalBranchCycleState::Preparing,
            replacement.clone(),
        )
        .await
        .unwrap());
    assert_eq!(
        repo.get(&project(), "clickup", "CU-24")
            .await
            .unwrap()
            .unwrap()
            .cycle,
        replacement
    );
}

#[tokio::test]
async fn legacy_upsert_and_terminal_helpers_cannot_mutate_a_strict_binding() {
    let repo = MemoryTicketCanonicalBranchRepository::new();
    let strict = repo
        .create_if_absent(strict_branch("CU-24", "cu-24_fix-login_ada", "Fix login"))
        .await
        .unwrap();

    let mut legacy = branch("CU-24");
    legacy.provider = "clickup".to_string();
    let upsert_error = repo.upsert(legacy).await.unwrap_err();
    assert!(matches!(upsert_error, AppError::Conflict(_)));

    let terminal_error = repo
        .mark_terminal(&project(), "clickup", "CU-24")
        .await
        .unwrap_err();
    assert!(matches!(terminal_error, AppError::Conflict(_)));

    let loaded = repo
        .get(&project(), "clickup", "CU-24")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(loaded, strict);
    assert_eq!(
        loaded.policy_kind,
        TicketCanonicalBranchPolicyKind::StrictGitConvention
    );
    assert!(!loaded.terminal);
    assert_eq!(
        loaded.cycle.state,
        TicketCanonicalBranchCycleState::Preparing
    );
}
