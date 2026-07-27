use super::*;
use crate::domain::entities::{AgentRun, AgentRunActionKind, ChatConversationId};
use crate::domain::entities::{VerificationGap, VerificationRoundSnapshot};
use crate::domain::repositories::AgentRunRepository;
use crate::infrastructure::sqlite::SqliteAgentRunRepository;
use crate::testing::SqliteTestDb;

fn verification_gap(severity: &str, description: &str) -> VerificationGap {
    VerificationGap {
        severity: severity.to_string(),
        category: "regression".to_string(),
        description: description.to_string(),
        why_it_matters: Some("The verification summary must remain authoritative.".to_string()),
        source: Some("coverage".to_string()),
    }
}

#[tokio::test]
async fn save_verification_run_snapshot_updates_snapshot_rows_and_session_summary() {
    let db = SqliteTestDb::new("sqlite-ideation-session-verification-snapshot-lib");
    let project = db.seed_project("Verification Snapshot Coverage");
    let session = db.seed_ideation_session(project.id);
    let repo = SqliteIdeationSessionRepository::from_shared(db.shared_conn());

    let snapshot = VerificationRunSnapshot {
        generation: 0,
        status: VerificationStatus::NeedsRevision,
        in_progress: false,
        current_round: 2,
        max_rounds: 5,
        best_round_index: Some(1),
        convergence_reason: Some("max_rounds".to_string()),
        current_gaps: vec![verification_gap(
            "high",
            "Queued verifier completion was not reconciled.",
        )],
        rounds: vec![VerificationRoundSnapshot {
            round: 2,
            gap_score: 3,
            fingerprints: vec!["queued-verifier-reconciliation".to_string()],
            gaps: vec![verification_gap(
                "high",
                "Queued verifier completion was not reconciled.",
            )],
            parse_failed: false,
        }],
    };

    repo.save_verification_run_snapshot(&session.id, &snapshot)
        .await
        .expect("verification snapshot should save through the SQLite repository");

    let found = repo
        .get_verification_run_snapshot(&session.id, snapshot.generation)
        .await
        .expect("verification snapshot lookup should succeed")
        .expect("verification snapshot should exist");
    assert_eq!(found, snapshot);

    let updated = repo
        .get_by_id(&session.id)
        .await
        .expect("session lookup should succeed")
        .expect("session should exist");
    assert_eq!(
        updated.verification_status,
        VerificationStatus::NeedsRevision
    );
    assert!(!updated.verification_in_progress);
    assert_eq!(updated.verification_current_round, Some(2));
    assert_eq!(updated.verification_max_rounds, Some(5));
    assert_eq!(updated.verification_gap_count, 1);
    assert_eq!(updated.verification_gap_score, Some(3));
    assert_eq!(
        updated.verification_convergence_reason.as_deref(),
        Some("max_rounds")
    );
}

#[tokio::test]
async fn complete_plan_verification_requires_live_exact_action_authority() {
    let db = SqliteTestDb::new("sqlite-ideation-session-model-native-verification");
    let project = db.seed_project("Model-native verification proof");
    let session = db.seed_ideation_session(project.id);
    let conversation_id = ChatConversationId::new();
    db.with_connection(|conn| {
        conn.execute(
            "INSERT INTO artifacts (
                id, type, name, content_type, content_text, created_by, version, created_at
             ) VALUES (
                'artifact-current', 'specification', 'Plan', 'inline', '# Plan',
                'orchestrator', 1, '2026-07-15T00:00:00Z'
             )",
            [],
        )
        .unwrap();
        conn.execute(
            "UPDATE ideation_sessions
             SET plan_artifact_id = 'artifact-current', plan_contract_version = 1
             WHERE id = ?1",
            [session.id.as_str()],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO chat_conversations (
                id, context_type, context_id, title, message_count, created_at, updated_at
             ) VALUES (?1, 'ideation', ?2, 'Plan', 0, ?3, ?3)",
            rusqlite::params![
                conversation_id.as_str(),
                session.id.as_str(),
                "2026-07-15T00:00:00Z"
            ],
        )
        .unwrap();
    });

    let run_repo = SqliteAgentRunRepository::from_shared(db.shared_conn());
    let session_repo = SqliteIdeationSessionRepository::from_shared(db.shared_conn());

    let ordinary = run_repo
        .create(AgentRun::new(conversation_id))
        .await
        .unwrap();
    assert!(!session_repo
        .complete_plan_verification(&session.id, &ordinary.id.as_str(), "artifact-current")
        .await
        .unwrap());

    let mut wrong_target = AgentRun::new(conversation_id);
    wrong_target.action_kind = Some(AgentRunActionKind::VerifyPlan);
    wrong_target.action_context_id = Some(session.id.as_str().to_string());
    wrong_target.action_target_id = Some("artifact-stale".to_string());
    let wrong_target = run_repo.create(wrong_target).await.unwrap();
    assert!(!session_repo
        .complete_plan_verification(&session.id, &wrong_target.id.as_str(), "artifact-current",)
        .await
        .unwrap());

    let mut valid = AgentRun::new(conversation_id);
    valid.action_kind = Some(AgentRunActionKind::VerifyPlan);
    valid.action_context_id = Some(session.id.as_str().to_string());
    valid.action_target_id = Some("artifact-current".to_string());
    let valid = run_repo.create(valid).await.unwrap();
    assert!(session_repo
        .complete_plan_verification(&session.id, &valid.id.as_str(), "artifact-current")
        .await
        .unwrap());
    assert!(!session_repo
        .complete_plan_verification(&session.id, &valid.id.as_str(), "artifact-current")
        .await
        .unwrap());

    let verified = session_repo.get_by_id(&session.id).await.unwrap().unwrap();
    assert_eq!(
        verified
            .verified_plan_artifact_id
            .as_ref()
            .map(|id| id.as_str()),
        Some("artifact-current")
    );
    let valid_run_id = valid.id.as_str();
    assert_eq!(
        verified.verified_plan_agent_run_id.as_deref(),
        Some(valid_run_id.as_str())
    );
}
