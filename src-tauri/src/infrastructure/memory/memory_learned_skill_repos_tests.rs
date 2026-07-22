use chrono::Utc;
use serde_json::json;

use super::MemoryTaskOutcomeRepository;
use crate::domain::entities::{ProjectId, TaskOutcome, TaskOutcomeId, TaskOutcomeStatus};
use crate::domain::repositories::{
    canonical_terminal_pr_source_ref_id, TaskOutcomeRepository, UpsertTaskOutcomeInput,
    AGENT_WORKSPACE_PR_OUTCOME_SOURCE, TERMINAL_PR_SOURCE_REF_KIND, WORKSPACE_PR_CLOSED_CLASS,
    WORKSPACE_PR_FAILED_CLASS, WORKSPACE_PR_MERGED_CLASS, WORKSPACE_PR_MERGED_CLEAN_CLASS,
    WORKSPACE_PR_MERGED_WITH_FOLLOWUPS_CLASS, WORKSPACE_PR_TERMINAL_CLASS,
};

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

async fn upsert(repo: &MemoryTaskOutcomeRepository, outcome: TaskOutcome) -> TaskOutcome {
    repo.upsert(UpsertTaskOutcomeInput { outcome })
        .await
        .expect("upsert task outcome")
}

#[tokio::test]
async fn canonical_terminal_lattice_preserves_identity_context_and_lower_winner() {
    let repo = MemoryTaskOutcomeRepository::new();
    let generic = upsert(
        &repo,
        terminal_outcome(WORKSPACE_PR_TERMINAL_CLASS, "generic"),
    )
    .await;
    assert_eq!(generic.status, TaskOutcomeStatus::Unknown);

    let found = repo
        .get_by_dedupe(
            &generic.project_id,
            AGENT_WORKSPACE_PR_OUTCOME_SOURCE,
            TERMINAL_PR_SOURCE_REF_KIND,
            &canonical_terminal_pr_source_ref_id("42"),
        )
        .await
        .expect("read by dedupe")
        .expect("terminal outcome exists");
    assert_eq!(found.id.as_str(), generic.id.as_str());

    let closed = upsert(&repo, terminal_outcome(WORKSPACE_PR_CLOSED_CLASS, "closed")).await;
    assert_eq!(closed.id.as_str(), generic.id.as_str());
    assert_eq!(closed.created_at, generic.created_at);
    assert_eq!(closed.status, TaskOutcomeStatus::Failed);

    let mut equal = terminal_outcome(WORKSPACE_PR_FAILED_CLASS, "failed detail");
    equal.conversation_id = None;
    equal.provider_harness = None;
    equal.provider_session_id = None;
    let equal = upsert(&repo, equal).await;
    assert_eq!(
        equal.outcome_class.as_deref(),
        Some(WORKSPACE_PR_FAILED_CLASS)
    );
    assert_eq!(equal.evidence_json, json!({ "summary": "failed detail" }));
    assert_eq!(equal.conversation_id.as_deref(), Some("conversation-1"));
    assert_eq!(equal.provider_harness.as_deref(), Some("codex"));

    let merged = upsert(&repo, terminal_outcome(WORKSPACE_PR_MERGED_CLASS, "merged")).await;
    assert_eq!(merged.status, TaskOutcomeStatus::Succeeded);
    let stale = upsert(
        &repo,
        terminal_outcome(WORKSPACE_PR_CLOSED_CLASS, "stale close"),
    )
    .await;
    assert_eq!(stale.id.as_str(), merged.id.as_str());
    assert_eq!(stale.outcome_class, merged.outcome_class);
    assert_eq!(stale.status, merged.status);
    assert_eq!(stale.evidence_json, merged.evidence_json);
    assert_eq!(stale.updated_at, merged.updated_at);

    let clean = upsert(
        &repo,
        terminal_outcome(WORKSPACE_PR_MERGED_CLEAN_CLASS, "clean"),
    )
    .await;
    let followups = upsert(
        &repo,
        terminal_outcome(WORKSPACE_PR_MERGED_WITH_FOLLOWUPS_CLASS, "followups"),
    )
    .await;
    assert_eq!(followups.id.as_str(), clean.id.as_str());
    assert_eq!(
        followups.outcome_class.as_deref(),
        Some(WORKSPACE_PR_MERGED_WITH_FOLLOWUPS_CLASS)
    );
    assert_eq!(followups.evidence_json, json!({ "summary": "followups" }));
}

#[tokio::test]
async fn noncanonical_outcomes_remain_last_write_wins_and_missing_dedupe_is_none() {
    let repo = MemoryTaskOutcomeRepository::new();
    let mut first = terminal_outcome("first", "first");
    first.source_ref_id = "42:terminal:legacy".to_string();
    first.status = TaskOutcomeStatus::Failed;
    upsert(&repo, first).await;

    let mut second = terminal_outcome("second", "second");
    second.source_ref_id = "42:terminal:legacy".to_string();
    second.status = TaskOutcomeStatus::Eligible;
    let saved = upsert(&repo, second).await;
    assert_eq!(saved.outcome_class.as_deref(), Some("second"));
    assert_eq!(saved.status, TaskOutcomeStatus::Eligible);

    let unknown = upsert(&repo, terminal_outcome("unrecognized", "unknown")).await;
    assert_eq!(unknown.status, TaskOutcomeStatus::Unknown);

    let mut mismatched = terminal_outcome(WORKSPACE_PR_MERGED_CLASS, "mismatched");
    mismatched.source_ref_id = canonical_terminal_pr_source_ref_id("99");
    mismatched.status = TaskOutcomeStatus::Eligible;
    let mismatched = upsert(&repo, mismatched).await;
    assert_eq!(mismatched.status, TaskOutcomeStatus::Eligible);
    assert!(repo
        .get_by_dedupe(
            &saved.project_id,
            AGENT_WORKSPACE_PR_OUTCOME_SOURCE,
            TERMINAL_PR_SOURCE_REF_KIND,
            "missing",
        )
        .await
        .expect("missing read")
        .is_none());
}
