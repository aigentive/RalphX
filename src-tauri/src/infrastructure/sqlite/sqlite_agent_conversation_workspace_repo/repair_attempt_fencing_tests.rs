use chrono::{Duration, Utc};

use super::SqliteAgentConversationWorkspaceRepository;
use crate::domain::entities::{
    AgentConversationWorkspace, AgentConversationWorkspaceMode,
    AgentConversationWorkspacePublicationEvent, AgentWorkspaceRepairAttempt,
    AgentWorkspaceRepairContinuation, AgentWorkspaceRepairOutcome, AgentWorkspaceRepairPhase,
    AgentWorkspaceRepairSource, ChatConversationId, IdeationAnalysisBaseRefKind, ProjectId,
};
use crate::domain::repositories::{
    AgentConversationWorkspaceRepository, AgentWorkspaceRepairAttemptTransition,
    AgentWorkspaceRepairRepository, SettleAndStartAgentWorkspaceRepairSuccessor,
    SettleAndStartAgentWorkspaceRepairSuccessorOutcome, StartOrJoinAgentWorkspaceRepairAttempt,
    StartOrJoinAgentWorkspaceRepairAttemptOutcome,
};
use crate::testing::SqliteTestDb;

pub(super) fn setup_repo() -> (
    SqliteTestDb,
    SqliteAgentConversationWorkspaceRepository,
    ChatConversationId,
) {
    let db = SqliteTestDb::new("sqlite_repair_attempt_fencing_tests");
    let conversation_id = ChatConversationId::from_string("sqlite-repair-fencing");
    db.with_connection(|conn| {
        conn.execute(
            "INSERT INTO chat_conversations (id, context_type, context_id, created_at, updated_at)
             VALUES (?1, 'project', 'project-repair-fencing', ?2, ?2)",
            rusqlite::params![conversation_id.as_str(), Utc::now().to_rfc3339()],
        )
        .expect("seed conversation");
    });
    let repo = SqliteAgentConversationWorkspaceRepository::from_shared(db.shared_conn());
    (db, repo, conversation_id)
}

fn workspace(conversation_id: ChatConversationId) -> AgentConversationWorkspace {
    AgentConversationWorkspace::new(
        conversation_id,
        ProjectId::from_string("project-repair-fencing".to_string()),
        AgentConversationWorkspaceMode::Edit,
        IdeationAnalysisBaseRefKind::ProjectDefault,
        "main".to_string(),
        Some("main".to_string()),
        Some("base-1".to_string()),
        "ralphx/project-repair-fencing/agent".to_string(),
        "/tmp/ralphx/project-repair-fencing/agent".to_string(),
    )
}

pub(super) fn attempt(conversation_id: ChatConversationId) -> AgentWorkspaceRepairAttempt {
    AgentWorkspaceRepairAttempt::new(
        conversation_id,
        AgentWorkspaceRepairSource::BaseUpdate,
        AgentWorkspaceRepairContinuation::UpdateOnly,
        "origin/main",
        false,
        false,
        false,
        None,
        Utc::now(),
    )
}

pub(super) fn event(
    conversation_id: ChatConversationId,
    step: &str,
) -> AgentConversationWorkspacePublicationEvent {
    AgentConversationWorkspacePublicationEvent::new(
        conversation_id,
        step,
        "succeeded",
        format!("repair {step}"),
        Some("repair".to_string()),
    )
}

pub(super) async fn start_dispatching(
    repo: &SqliteAgentConversationWorkspaceRepository,
    conversation_id: ChatConversationId,
) -> AgentWorkspaceRepairAttempt {
    repo.create_or_update(workspace(conversation_id.clone()))
        .await
        .expect("persist workspace");
    let started = match repo
        .start_or_join_repair_attempt(StartOrJoinAgentWorkspaceRepairAttempt {
            attempt: attempt(conversation_id),
            reason: "base moved".to_string(),
            verified_newer_base: false,
            compatibility_projection: None,
            events: Vec::new(),
        })
        .await
        .expect("start attempt")
    {
        StartOrJoinAgentWorkspaceRepairAttemptOutcome::Started(attempt) => attempt,
        outcome => panic!("expected start, got {outcome:?}"),
    };
    let mut dispatching = started.clone();
    dispatching.phase = AgentWorkspaceRepairPhase::Dispatching;
    dispatching.updated_at += Duration::seconds(1);
    repo.transition_repair_attempt(AgentWorkspaceRepairAttemptTransition {
        attempt: dispatching.clone(),
        expected_phase: AgentWorkspaceRepairPhase::Requested,
        expected_updated_at: started.updated_at,
        next_phase: AgentWorkspaceRepairPhase::Dispatching,
        compatibility_projection: None,
        events: Vec::new(),
    })
    .await
    .expect("move to dispatching");
    dispatching
}

pub(super) async fn join_same_phase(
    repo: &SqliteAgentConversationWorkspaceRepository,
    conversation_id: ChatConversationId,
    current: &AgentWorkspaceRepairAttempt,
) -> AgentWorkspaceRepairAttempt {
    let mut join = attempt(conversation_id);
    join.updated_at = current.updated_at + Duration::seconds(1);
    match repo
        .start_or_join_repair_attempt(StartOrJoinAgentWorkspaceRepairAttempt {
            attempt: join,
            reason: "same-phase retry".to_string(),
            verified_newer_base: false,
            compatibility_projection: None,
            events: Vec::new(),
        })
        .await
        .expect("join same phase")
    {
        StartOrJoinAgentWorkspaceRepairAttemptOutcome::Joined(attempt) => attempt,
        outcome => panic!("expected join, got {outcome:?}"),
    }
}

#[tokio::test]
async fn successor_fencing_rejects_same_phase_stale_and_duplicate_settlement_without_side_effects()
{
    let (_db, repo, conversation_id) = setup_repo();
    let dispatching = start_dispatching(&repo, conversation_id.clone()).await;
    let current = join_same_phase(&repo, conversation_id.clone(), &dispatching).await;
    let before_workspace = repo
        .get_by_conversation_id(&conversation_id)
        .await
        .expect("load workspace")
        .expect("workspace exists");
    let stale_successor = attempt(conversation_id.clone());
    let stale_id = stale_successor.id.clone();
    let stale = repo
        .settle_and_start_repair_successor(SettleAndStartAgentWorkspaceRepairSuccessor {
            attempt_id: current.id.clone(),
            generation: current.generation,
            expected_phase: AgentWorkspaceRepairPhase::Dispatching,
            expected_updated_at: dispatching.updated_at,
            outcome: AgentWorkspaceRepairOutcome::Superseded,
            settled_at: current.updated_at + Duration::seconds(1),
            successor: StartOrJoinAgentWorkspaceRepairAttempt {
                attempt: stale_successor,
                reason: "stale successor".to_string(),
                verified_newer_base: false,
                compatibility_projection: None,
                events: vec![event(conversation_id.clone(), "stale-successor")],
            },
            compatibility_projection: Some(
                crate::domain::repositories::AgentWorkspaceRepairCompatibilityProjection {
                    publication_push_status: Some("must-not-project".to_string()),
                    pr_supervision_status: None,
                    pr_supervision_summary: None,
                    pr_supervision_updated_at: None,
                    pr_auto_merge_current: None,
                    base_commit: None,
                },
            ),
            events: vec![event(conversation_id.clone(), "stale-settlement")],
        })
        .await
        .expect("reject stale settlement");
    assert!(matches!(
        stale,
        SettleAndStartAgentWorkspaceRepairSuccessorOutcome::Stale(ref attempt)
            if attempt.updated_at == current.updated_at && attempt.settled_at.is_none()
    ));
    assert!(repo
        .get_repair_attempt(&stale_id)
        .await
        .expect("load stale successor")
        .is_none());
    assert_eq!(
        repo.get_by_conversation_id(&conversation_id)
            .await
            .expect("reload workspace"),
        Some(before_workspace.clone())
    );
    assert!(repo
        .list_publication_events(&conversation_id)
        .await
        .expect("list events")
        .is_empty());

    let accepted = match repo
        .settle_and_start_repair_successor(SettleAndStartAgentWorkspaceRepairSuccessor {
            attempt_id: current.id.clone(),
            generation: current.generation,
            expected_phase: AgentWorkspaceRepairPhase::Dispatching,
            expected_updated_at: current.updated_at,
            outcome: AgentWorkspaceRepairOutcome::Succeeded,
            settled_at: current.updated_at + Duration::seconds(2),
            successor: StartOrJoinAgentWorkspaceRepairAttempt {
                attempt: attempt(conversation_id.clone()),
                reason: "accepted successor".to_string(),
                verified_newer_base: false,
                compatibility_projection: None,
                events: Vec::new(),
            },
            compatibility_projection: None,
            events: Vec::new(),
        })
        .await
        .expect("settle once")
    {
        SettleAndStartAgentWorkspaceRepairSuccessorOutcome::Started(successor) => successor,
        outcome => panic!("expected successor, got {outcome:?}"),
    };
    let settled_parent = repo
        .get_repair_attempt(&current.id)
        .await
        .expect("load parent")
        .expect("parent exists");
    let duplicate = attempt(conversation_id.clone());
    let duplicate_id = duplicate.id.clone();
    let duplicate = repo
        .settle_and_start_repair_successor(SettleAndStartAgentWorkspaceRepairSuccessor {
            attempt_id: current.id.clone(),
            generation: current.generation,
            expected_phase: AgentWorkspaceRepairPhase::Dispatching,
            expected_updated_at: settled_parent.updated_at,
            outcome: AgentWorkspaceRepairOutcome::Succeeded,
            settled_at: settled_parent.updated_at + Duration::seconds(1),
            successor: StartOrJoinAgentWorkspaceRepairAttempt {
                attempt: duplicate,
                reason: "duplicate successor".to_string(),
                verified_newer_base: false,
                compatibility_projection: None,
                events: vec![event(conversation_id.clone(), "duplicate-successor")],
            },
            compatibility_projection: Some(
                crate::domain::repositories::AgentWorkspaceRepairCompatibilityProjection {
                    publication_push_status: Some("must-not-project".to_string()),
                    pr_supervision_status: None,
                    pr_supervision_summary: None,
                    pr_supervision_updated_at: None,
                    pr_auto_merge_current: None,
                    base_commit: None,
                },
            ),
            events: vec![event(conversation_id.clone(), "duplicate-settlement")],
        })
        .await
        .expect("reject duplicate settlement");
    assert!(matches!(
        duplicate,
        SettleAndStartAgentWorkspaceRepairSuccessorOutcome::Stale(_)
    ));
    assert!(repo
        .get_repair_attempt(&duplicate_id)
        .await
        .expect("load duplicate successor")
        .is_none());
    assert_eq!(
        repo.get_current_repair_attempt(&conversation_id)
            .await
            .expect("load active successor")
            .expect("one active successor")
            .id,
        accepted.id
    );
    assert_eq!(
        repo.get_by_conversation_id(&conversation_id)
            .await
            .expect("reload workspace after duplicate"),
        Some(before_workspace)
    );
    assert!(repo
        .list_publication_events(&conversation_id)
        .await
        .expect("list duplicate events")
        .is_empty());
}
