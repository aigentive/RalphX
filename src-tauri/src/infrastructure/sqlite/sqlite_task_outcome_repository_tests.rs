use std::sync::Arc;

use chrono::Utc;
use serde_json::json;

use super::SqliteTaskOutcomeRepository;
use crate::domain::entities::{ProjectId, TaskOutcome, TaskOutcomeId, TaskOutcomeStatus};
use crate::domain::repositories::{
    canonical_terminal_pr_source_ref_id, TaskOutcomeRepository, UpsertTaskOutcomeInput,
    AGENT_WORKSPACE_PR_OUTCOME_SOURCE, TERMINAL_PR_SOURCE_REF_KIND, WORKSPACE_PR_CLOSED_CLASS,
    WORKSPACE_PR_MERGED_CLASS, WORKSPACE_PR_MERGED_CLEAN_CLASS,
};
use crate::testing::SqliteTestDb;

async fn setup(name: &str) -> (SqliteTestDb, Arc<SqliteTaskOutcomeRepository>) {
    let db = SqliteTestDb::new(name);
    db.shared_conn()
        .lock()
        .await
        .execute(
            "INSERT INTO projects (id, name, working_directory)
             VALUES ('project-1', 'Project 1', '/tmp/project-1')",
            [],
        )
        .expect("insert project fixture");
    let repo = Arc::new(SqliteTaskOutcomeRepository::from_shared(db.shared_conn()));
    (db, repo)
}

fn terminal_outcome(outcome_class: &str, evidence: &str) -> TaskOutcome {
    let now = Utc::now();
    TaskOutcome {
        id: TaskOutcomeId::new(),
        project_id: ProjectId::from_string("project-1".to_string()),
        source: AGENT_WORKSPACE_PR_OUTCOME_SOURCE.to_string(),
        source_ref_kind: TERMINAL_PR_SOURCE_REF_KIND.to_string(),
        source_ref_id: canonical_terminal_pr_source_ref_id("42"),
        task_id: None,
        conversation_id: Some("conversation-1".to_string()),
        agent_run_id: None,
        pull_request_id: Some("42".to_string()),
        proposal_id: None,
        verification_id: None,
        review_id: None,
        outcome_class: Some(outcome_class.to_string()),
        status: TaskOutcomeStatus::Eligible,
        evidence_json: json!({ "summary": evidence }),
        provider_harness: Some("codex".to_string()),
        provider_session_id: Some("session-1".to_string()),
        created_at: now,
        updated_at: now,
    }
}

#[tokio::test]
async fn sqlite_terminal_lattice_and_dedupe_getter_match_memory_semantics() {
    let (db, repo) = setup("sqlite-terminal-outcome-lattice").await;
    let first = repo
        .upsert(UpsertTaskOutcomeInput {
            outcome: terminal_outcome(WORKSPACE_PR_CLOSED_CLASS, "closed"),
        })
        .await
        .expect("insert closed outcome");
    assert_eq!(first.status, TaskOutcomeStatus::Failed);
    {
        let conn = db.shared_conn();
        let conn = conn.lock().await;
        conn.execute(
            "INSERT INTO project_skills (
                id, project_id, title, bucket, stage, status, compact_guidance,
                body_markdown, predicted_effect
             ) VALUES (
                'skill-1', 'project-1', 'Skill', 'execution', 'execution', 'approved',
                'guidance', 'body', 'effect'
             )",
            [],
        )
        .expect("insert skill fixture");
        conn.execute(
            "INSERT INTO skill_usage_events (
                id, project_id, project_skill_id, injection_kind, outcome_id
             ) VALUES ('usage-1', 'project-1', 'skill-1', 'compact_index', ?1)",
            [first.id.as_str()],
        )
        .expect("insert linked usage fixture");
    }

    let merged = repo
        .upsert(UpsertTaskOutcomeInput {
            outcome: terminal_outcome(WORKSPACE_PR_MERGED_CLASS, "merged"),
        })
        .await
        .expect("upgrade merged outcome");
    assert_eq!(merged.id.as_str(), first.id.as_str());
    assert_eq!(merged.created_at, first.created_at);
    assert_eq!(merged.status, TaskOutcomeStatus::Succeeded);
    let linked_outcome_id: String = db
        .shared_conn()
        .lock()
        .await
        .query_row(
            "SELECT outcome_id FROM skill_usage_events WHERE id = 'usage-1'",
            [],
            |row| row.get(0),
        )
        .expect("read linked usage outcome");
    assert_eq!(linked_outcome_id, first.id.as_str());

    let stale = repo
        .upsert(UpsertTaskOutcomeInput {
            outcome: terminal_outcome(WORKSPACE_PR_CLOSED_CLASS, "stale"),
        })
        .await
        .expect("ignore stale outcome");
    assert_eq!(stale.id.as_str(), merged.id.as_str());
    assert_eq!(stale.outcome_class, merged.outcome_class);
    assert_eq!(stale.status, merged.status);
    assert_eq!(stale.evidence_json, merged.evidence_json);
    assert_eq!(stale.updated_at, merged.updated_at);

    let found = repo
        .get_by_dedupe(
            &merged.project_id,
            AGENT_WORKSPACE_PR_OUTCOME_SOURCE,
            TERMINAL_PR_SOURCE_REF_KIND,
            &canonical_terminal_pr_source_ref_id("42"),
        )
        .await
        .expect("read terminal outcome")
        .expect("terminal outcome exists");
    assert_eq!(found.id.as_str(), merged.id.as_str());
    assert_eq!(found.outcome_class, merged.outcome_class);
    assert_eq!(found.status, merged.status);
    assert_eq!(found.evidence_json, merged.evidence_json);
    assert_eq!(found.updated_at, merged.updated_at);
    assert!(repo
        .get_by_dedupe(
            &merged.project_id,
            AGENT_WORKSPACE_PR_OUTCOME_SOURCE,
            TERMINAL_PR_SOURCE_REF_KIND,
            "missing",
        )
        .await
        .expect("read missing outcome")
        .is_none());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn sqlite_competing_terminal_writes_cannot_leave_lower_rank_winner() {
    let (_db, repo) = setup("sqlite-terminal-outcome-race").await;
    repo.upsert(UpsertTaskOutcomeInput {
        outcome: terminal_outcome(WORKSPACE_PR_CLOSED_CLASS, "seed"),
    })
    .await
    .expect("seed terminal outcome");

    let lower_repo = Arc::clone(&repo);
    let higher_repo = Arc::clone(&repo);
    let (lower, higher) = tokio::join!(
        lower_repo.upsert(UpsertTaskOutcomeInput {
            outcome: terminal_outcome(WORKSPACE_PR_CLOSED_CLASS, "concurrent close"),
        }),
        higher_repo.upsert(UpsertTaskOutcomeInput {
            outcome: terminal_outcome(WORKSPACE_PR_MERGED_CLEAN_CLASS, "concurrent merge"),
        })
    );
    lower.expect("lower-rank write completes");
    higher.expect("higher-rank write completes");

    let winner = repo
        .get_by_dedupe(
            &ProjectId::from_string("project-1".to_string()),
            AGENT_WORKSPACE_PR_OUTCOME_SOURCE,
            TERMINAL_PR_SOURCE_REF_KIND,
            &canonical_terminal_pr_source_ref_id("42"),
        )
        .await
        .expect("read winner")
        .expect("winner exists");
    assert_eq!(
        winner.outcome_class.as_deref(),
        Some(WORKSPACE_PR_MERGED_CLEAN_CLASS)
    );
    assert_eq!(winner.status, TaskOutcomeStatus::Succeeded);
    assert_eq!(
        winner.evidence_json,
        json!({ "summary": "concurrent merge" })
    );
}
