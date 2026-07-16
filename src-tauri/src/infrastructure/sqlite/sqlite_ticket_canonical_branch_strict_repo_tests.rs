use chrono::Utc;

use crate::domain::entities::{
    ProjectId, TicketCanonicalBranch, TicketCanonicalBranchCycle, TicketCanonicalBranchCycleState,
    TicketCanonicalBranchPolicyKind, TicketGitConventionSnapshot,
};
use crate::domain::repositories::TicketCanonicalBranchRepository;
use crate::error::AppError;
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

fn strict_branch(issue_key: &str, branch_name: &str, task_title: &str) -> TicketCanonicalBranch {
    TicketCanonicalBranch::new_strict(
        ProjectId::from_string("project-1".to_string()),
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
async fn strict_binding_round_trips_frozen_policy_and_cycle_state() {
    let db = setup_test_db();
    let repo = SqliteTicketCanonicalBranchRepository::from_shared(db.shared_conn());
    let expected = strict_branch("CU-24", "cu-24_fix-login_ada", "Fix login");

    let stored = repo.create_if_absent(expected.clone()).await.unwrap();
    let by_branch = repo
        .get_by_branch_name(
            &ProjectId::from_string("project-1".to_string()),
            "cu-24_fix-login_ada",
        )
        .await
        .unwrap()
        .unwrap();

    assert_eq!(stored, expected);
    assert_eq!(by_branch, expected);
    assert_eq!(
        by_branch.policy_kind,
        TicketCanonicalBranchPolicyKind::StrictGitConvention
    );
    let policy = by_branch.strict_policy.unwrap();
    assert_eq!(policy.policy_version, 1);
    assert_eq!(policy.task_title, "Fix login");
    assert_eq!(policy.username.as_deref(), Some("Ada Lovelace"));
    assert_eq!(policy.commit_subject_rule, "CU-24 - Fix login: :summary:");
    assert_eq!(policy.pr_title, "CU-24 - Fix login");
    assert_eq!(
        by_branch.cycle.state,
        TicketCanonicalBranchCycleState::Preparing
    );
    assert_eq!(by_branch.cycle.generation, 1);
    assert_eq!(by_branch.cycle.base_commit.as_deref(), Some("abc123"));
    assert!(by_branch.cycle.started_at.is_some());
}

#[tokio::test]
async fn strict_create_if_absent_keeps_the_first_snapshot() {
    let db = setup_test_db();
    let repo = SqliteTicketCanonicalBranchRepository::from_shared(db.shared_conn());
    let first = strict_branch("CU-24", "cu-24_fix-login_ada", "Fix login");
    let competing = strict_branch("CU-24", "cu-24_renamed-task_grace", "Renamed task");

    let stored_first = repo.create_if_absent(first).await.unwrap();
    let stored_competing = repo.create_if_absent(competing).await.unwrap();

    assert_eq!(stored_competing, stored_first);
    assert_eq!(stored_competing.branch_name, "cu-24_fix-login_ada");
    assert_eq!(
        stored_competing.strict_policy.unwrap().task_title,
        "Fix login"
    );
    assert!(repo
        .get_by_branch_name(
            &ProjectId::from_string("project-1".to_string()),
            "cu-24_renamed-task_grace",
        )
        .await
        .unwrap()
        .is_none());
}

#[tokio::test]
async fn strict_create_if_absent_rejects_project_branch_collision() {
    let db = setup_test_db();
    let repo = SqliteTicketCanonicalBranchRepository::from_shared(db.shared_conn());
    let project_id = ProjectId::from_string("project-1".to_string());
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
        .get(&project_id, "clickup", "CU-25")
        .await
        .unwrap()
        .is_none());
    assert_eq!(
        repo.get_by_branch_name(&project_id, "cu-normalized-collision")
            .await
            .unwrap()
            .unwrap()
            .issue_key,
        "CU-24"
    );
}

#[tokio::test]
async fn strict_cycle_compare_and_swap_is_generation_and_state_guarded() {
    let db = setup_test_db();
    let repo = SqliteTicketCanonicalBranchRepository::from_shared(db.shared_conn());
    let project_id = ProjectId::from_string("project-1".to_string());
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

    let mut stale_generation_replacement = replacement.clone();
    stale_generation_replacement.generation = 2;
    assert!(!repo
        .compare_and_swap_cycle(
            &project_id,
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
            &project_id,
            "clickup",
            "CU-24",
            1,
            TicketCanonicalBranchCycleState::Merged,
            replacement.clone(),
        )
        .await
        .unwrap());
    assert_eq!(
        repo.get(&project_id, "clickup", "CU-24")
            .await
            .unwrap()
            .unwrap()
            .cycle,
        original_cycle
    );

    assert!(repo
        .compare_and_swap_cycle(
            &project_id,
            "clickup",
            "CU-24",
            1,
            TicketCanonicalBranchCycleState::Preparing,
            replacement.clone(),
        )
        .await
        .unwrap());
    assert_eq!(
        repo.get(&project_id, "clickup", "CU-24")
            .await
            .unwrap()
            .unwrap()
            .cycle,
        replacement
    );
}

#[tokio::test]
async fn legacy_rows_load_with_legacy_policy_defaults() {
    let db = setup_test_db();
    let repo = SqliteTicketCanonicalBranchRepository::from_shared(db.shared_conn());

    repo.upsert(branch("wise-24")).await.unwrap();
    let loaded = repo
        .get(
            &ProjectId::from_string("project-1".to_string()),
            "linear",
            "wise-24",
        )
        .await
        .unwrap()
        .unwrap();

    assert_eq!(
        loaded.policy_kind,
        TicketCanonicalBranchPolicyKind::LegacyCanonicalBase
    );
    assert!(loaded.strict_policy.is_none());
    assert_eq!(loaded.cycle.generation, 0);
    assert_eq!(loaded.cycle.state, TicketCanonicalBranchCycleState::Legacy);
}

#[tokio::test]
async fn legacy_mutators_reject_a_strict_binding_without_side_effects() {
    let db = setup_test_db();
    let repo = SqliteTicketCanonicalBranchRepository::from_shared(db.shared_conn());
    let project_id = ProjectId::from_string("project-1".to_string());
    let strict = repo
        .create_if_absent(strict_branch("CU-24", "cu-24_fix-login_ada", "Fix login"))
        .await
        .unwrap();

    let mut legacy = branch("CU-24");
    legacy.provider = "clickup".to_string();
    assert!(matches!(
        repo.upsert(legacy).await.unwrap_err(),
        AppError::Conflict(_)
    ));
    assert!(matches!(
        repo.mark_terminal(&project_id, "clickup", "CU-24")
            .await
            .unwrap_err(),
        AppError::Conflict(_)
    ));

    assert_eq!(
        repo.get(&project_id, "clickup", "CU-24")
            .await
            .unwrap()
            .unwrap(),
        strict
    );
}

#[tokio::test]
async fn corrupted_strict_cycle_state_fails_closed_on_read() {
    let db = setup_test_db();
    let repo = SqliteTicketCanonicalBranchRepository::from_shared(db.shared_conn());
    let project_id = ProjectId::from_string("project-1".to_string());
    repo.create_if_absent(strict_branch("CU-24", "cu-24_fix-login_ada", "Fix login"))
        .await
        .unwrap();

    let shared = db.shared_conn();
    shared
        .lock()
        .await
        .execute(
            "UPDATE ticket_canonical_branches
                SET cycle_generation = 0
              WHERE project_id = 'project-1' AND provider = 'clickup' AND issue_key = 'CU-24'",
            [],
        )
        .unwrap();

    let mutation_error = repo
        .mark_origin_pushed(&project_id, "clickup", "CU-24")
        .await
        .unwrap_err();
    assert!(matches!(mutation_error, AppError::Database(_)));
    let origin_pushed = shared
        .lock()
        .await
        .query_row(
            "SELECT origin_pushed FROM ticket_canonical_branches
              WHERE project_id = 'project-1' AND provider = 'clickup' AND issue_key = 'CU-24'",
            [],
            |row| row.get::<_, bool>(0),
        )
        .unwrap();
    assert!(
        !origin_pushed,
        "a corrupt row must not be partially mutated"
    );

    let error = repo.get(&project_id, "clickup", "CU-24").await.unwrap_err();
    assert!(matches!(error, AppError::Database(_)));
}
