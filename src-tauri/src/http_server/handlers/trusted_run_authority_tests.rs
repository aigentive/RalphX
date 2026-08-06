use std::sync::Arc;

use chrono::Duration;

use super::trusted_run_authority::{resolve_live_caller_run, TrustedRunRejection};
use crate::domain::entities::{AgentRun, AgentRunId, AgentRunStatus, ChatConversationId};
use crate::domain::repositories::AgentRunRepository;
use crate::infrastructure::memory::MemoryAgentRunRepository;
use crate::testing::GetByIdFailingAgentRunRepository;

fn repository() -> Arc<dyn AgentRunRepository> {
    Arc::new(MemoryAgentRunRepository::new())
}

async fn seed_run(
    repo: &Arc<dyn AgentRunRepository>,
    conversation_id: &ChatConversationId,
    status: AgentRunStatus,
) -> AgentRun {
    let run = repo
        .create(AgentRun::new(*conversation_id))
        .await
        .expect("seed run");
    if status != AgentRunStatus::Running {
        repo.update_status(&run.id, status)
            .await
            .expect("update seeded run status");
    }
    repo.get_by_id(&run.id)
        .await
        .expect("re-read seeded run")
        .expect("seeded run exists")
}

#[tokio::test]
async fn accepts_a_running_run_bound_to_the_caller_conversation() {
    let repo = repository();
    let conversation_id = ChatConversationId::new();
    let run = seed_run(&repo, &conversation_id, AgentRunStatus::Running).await;

    let resolved = resolve_live_caller_run(&repo, &conversation_id, &run.id)
        .await
        .expect("a live caller run is authorized");

    assert_eq!(resolved.id, run.id);
}

#[tokio::test]
async fn accepts_a_live_run_outranked_by_a_newer_running_sibling() {
    let repo = repository();
    let conversation_id = ChatConversationId::new();
    let caller = seed_run(&repo, &conversation_id, AgentRunStatus::Running).await;

    // An orphan left behind by a killed process: still `running`, and strictly newer than the
    // live caller. `started_at` is written explicitly rather than relying on wall-clock ordering
    // between two `Utc::now()` stamps, which can collide at coarse resolution.
    let mut orphan = AgentRun::new(conversation_id);
    orphan.started_at = caller.started_at + Duration::seconds(60);
    let orphan = repo
        .create(orphan)
        .await
        .expect("seed orphaned running run");

    assert!(
        orphan.started_at > caller.started_at,
        "the orphan must strictly outrank the caller for this regression to be meaningful"
    );
    // Deliberately not asserting on `get_active_for_conversation` here: the SQLite repo orders by
    // `started_at DESC` (so the orphan would win and veto the caller), but the in-memory double
    // returns the first `HashMap` match, which is arbitrary. Asserting recency disagreement
    // against the double would be nondeterministic.

    let resolved = resolve_live_caller_run(&repo, &conversation_id, &caller.id)
        .await
        .expect("liveness, not recency, decides caller authority");

    assert_eq!(resolved.id, caller.id);
}

#[tokio::test]
async fn rejects_terminal_runs_with_the_observed_status() {
    for status in [
        AgentRunStatus::Completed,
        AgentRunStatus::Failed,
        AgentRunStatus::Cancelled,
    ] {
        let repo = repository();
        let conversation_id = ChatConversationId::new();
        let run = seed_run(&repo, &conversation_id, status).await;

        let rejection = resolve_live_caller_run(&repo, &conversation_id, &run.id)
            .await
            .expect_err("a finished turn has no authority");

        match rejection {
            TrustedRunRejection::RunTerminal {
                status: observed_status,
            } => assert_eq!(observed_status, status),
            other => panic!("expected RunTerminal for {status:?}, got {other:?}"),
        }
    }
}

#[tokio::test]
async fn rejects_a_run_belonging_to_another_conversation() {
    let repo = repository();
    let caller_conversation_id = ChatConversationId::new();
    let other_conversation_id = ChatConversationId::new();
    let run = seed_run(&repo, &other_conversation_id, AgentRunStatus::Running).await;

    let rejection = resolve_live_caller_run(&repo, &caller_conversation_id, &run.id)
        .await
        .expect_err("run/conversation binding is never relaxed for nested delegates");

    assert!(matches!(
        rejection,
        TrustedRunRejection::ConversationMismatch
    ));
}

#[tokio::test]
async fn rejects_a_missing_run() {
    let repo = repository();
    let conversation_id = ChatConversationId::new();

    let rejection = resolve_live_caller_run(&repo, &conversation_id, &AgentRunId::new())
        .await
        .expect_err("an unknown run id carries no authority");

    assert!(matches!(rejection, TrustedRunRejection::RunNotFound));
}

#[tokio::test]
async fn repository_read_failures_fail_closed() {
    let inner = repository();
    let conversation_id = ChatConversationId::new();
    let run = seed_run(&inner, &conversation_id, AgentRunStatus::Running).await;
    let repo: Arc<dyn AgentRunRepository> =
        Arc::new(GetByIdFailingAgentRunRepository::new(inner.clone()));

    let rejection = resolve_live_caller_run(&repo, &conversation_id, &run.id)
        .await
        .expect_err("an unreadable run must never resolve to authorized");

    assert!(matches!(rejection, TrustedRunRejection::RepositoryError(_)));
}
