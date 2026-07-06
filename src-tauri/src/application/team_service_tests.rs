use super::*;
use crate::domain::repositories::{TeamMessageRepository, TeamSessionRepository};
use crate::infrastructure::memory::{MemoryTeamMessageRepository, MemoryTeamSessionRepository};

fn test_service() -> TeamService {
    TeamService::new_without_events(Arc::new(TeamStateTracker::new()))
}

/// Helper: creates a TeamService backed by in-memory repos for persistence tests.
/// Returns (service, session_repo, message_repo) so tests can query the DB layer.
fn test_service_with_repos() -> (
    TeamService,
    Arc<dyn TeamSessionRepository>,
    Arc<dyn TeamMessageRepository>,
) {
    let tracker = Arc::new(TeamStateTracker::new());
    let session_repo: Arc<dyn TeamSessionRepository> =
        Arc::new(MemoryTeamSessionRepository::new());
    let message_repo: Arc<dyn TeamMessageRepository> =
        Arc::new(MemoryTeamMessageRepository::new());
    let svc = TeamService::new_with_repos_for_testing(
        tracker,
        Arc::clone(&session_repo),
        Arc::clone(&message_repo),
    );
    (svc, session_repo, message_repo)
}

#[tokio::test]
async fn test_create_team() {
    let svc = test_service();
    svc.create_team("alpha", "session-1", "ideation")
        .await
        .unwrap();

    assert!(svc.team_exists("alpha").await);
}

#[tokio::test]
async fn test_create_duplicate_team_fails() {
    let svc = test_service();
    svc.create_team("alpha", "s-1", "ideation").await.unwrap();

    let err = svc.create_team("alpha", "s-2", "ideation").await;
    assert!(matches!(err, Err(TeamTrackerError::TeamAlreadyExists(_))));
}

#[tokio::test]
async fn test_add_teammate_and_status() {
    let svc = test_service();
    svc.create_team("t1", "ctx-1", "ideation").await.unwrap();
    svc.add_teammate("t1", "researcher", "#ff6b35", "opus", "explore")
        .await
        .unwrap();

    let status = svc.get_team_status("t1").await.unwrap();
    assert_eq!(status.teammates.len(), 1);
    assert_eq!(status.teammates[0].name, "researcher");
    assert_eq!(status.teammates[0].status, TeammateStatus::Spawning);
    assert_eq!(status.context_id, "ctx-1");
    assert_eq!(status.context_type, "ideation");
}

#[tokio::test]
async fn test_update_teammate_status() {
    let svc = test_service();
    svc.create_team("t1", "ctx-1", "ideation").await.unwrap();
    svc.add_teammate("t1", "worker", "#00ff00", "sonnet", "code")
        .await
        .unwrap();

    svc.update_teammate_status("t1", "worker", TeammateStatus::Running)
        .await
        .unwrap();

    let status = svc.get_team_status("t1").await.unwrap();
    assert_eq!(status.teammates[0].status, TeammateStatus::Running);
}

#[tokio::test]
async fn test_update_teammate_cost() {
    let svc = test_service();
    svc.create_team("t1", "ctx-1", "ideation").await.unwrap();
    svc.add_teammate("t1", "r", "#ff6b35", "opus", "explore")
        .await
        .unwrap();

    let cost = TeammateCost {
        input_tokens: 1000,
        output_tokens: 500,
        cache_creation_tokens: 200,
        cache_read_tokens: 100,
        estimated_usd: 0.05,
    };
    svc.update_teammate_cost("t1", "r", cost).await.unwrap();

    let resp = svc.get_teammate_cost("t1", "r").await.unwrap();
    assert_eq!(resp.input_tokens, 1000);
    assert_eq!(resp.estimated_usd, 0.05);
}

#[tokio::test]
async fn test_send_user_message() {
    let svc = test_service();
    svc.create_team("t1", "ctx-1", "ideation").await.unwrap();

    let msg = svc.send_user_message("t1", "Hello").await.unwrap();
    assert_eq!(msg.sender, "user");
    assert_eq!(msg.content, "Hello");
}

#[tokio::test]
async fn test_add_teammate_message() {
    let svc = test_service();
    svc.create_team("t1", "ctx-1", "ideation").await.unwrap();

    let msg = svc
        .add_teammate_message(
            "t1",
            "researcher",
            Some("planner"),
            "Found results",
            TeamMessageType::TeammateMessage,
        )
        .await
        .unwrap();
    assert_eq!(msg.sender, "researcher");
    assert_eq!(msg.recipient, Some("planner".to_string()));
}

#[tokio::test]
async fn test_stop_teammate() {
    let svc = test_service();
    svc.create_team("t1", "ctx-1", "ideation").await.unwrap();
    svc.add_teammate("t1", "w", "#ff0000", "sonnet", "code")
        .await
        .unwrap();

    svc.stop_teammate("t1", "w").await.unwrap();

    let status = svc.get_team_status("t1").await.unwrap();
    assert_eq!(status.teammates[0].status, TeammateStatus::Shutdown);
}

#[tokio::test]
async fn test_stop_team() {
    let svc = test_service();
    svc.create_team("t1", "ctx-1", "ideation").await.unwrap();
    svc.add_teammate("t1", "w1", "#ff0000", "sonnet", "code")
        .await
        .unwrap();
    svc.add_teammate("t1", "w2", "#00ff00", "sonnet", "code")
        .await
        .unwrap();

    svc.stop_team("t1").await.unwrap();

    let status = svc.get_team_status("t1").await.unwrap();
    assert_eq!(
        status.phase,
        super::super::team_state_tracker::TeamPhase::Winding
    );
    for t in &status.teammates {
        assert_eq!(t.status, TeammateStatus::Shutdown);
    }
}

#[tokio::test]
async fn test_disband_team() {
    let svc = test_service();
    svc.create_team("t1", "ctx-1", "ideation").await.unwrap();
    svc.add_teammate("t1", "w", "#ff0000", "sonnet", "code")
        .await
        .unwrap();

    svc.disband_team("t1").await.unwrap();

    let status = svc.get_team_status("t1").await.unwrap();
    assert_eq!(
        status.phase,
        super::super::team_state_tracker::TeamPhase::Disbanded
    );
}

#[tokio::test]
async fn test_get_messages_with_limit() {
    let svc = test_service();
    svc.create_team("t1", "ctx-1", "ideation").await.unwrap();

    for i in 0..5 {
        svc.send_user_message("t1", &format!("Msg {}", i))
            .await
            .unwrap();
    }

    let all = svc.get_team_messages("t1", None).await.unwrap();
    assert_eq!(all.len(), 5);

    let limited = svc.get_team_messages("t1", Some(2)).await.unwrap();
    assert_eq!(limited.len(), 2);
}

#[tokio::test]
async fn test_list_teams() {
    let svc = test_service();
    svc.create_team("a", "ctx-1", "ideation").await.unwrap();
    svc.create_team("b", "ctx-2", "task").await.unwrap();

    let teams = svc.list_teams().await;
    assert_eq!(teams.len(), 2);
}

#[tokio::test]
async fn test_remove_teammate() {
    let svc = test_service();
    svc.create_team("t1", "ctx-1", "ideation").await.unwrap();
    svc.add_teammate("t1", "w", "#ff0000", "sonnet", "code")
        .await
        .unwrap();

    svc.remove_teammate("t1", "w").await.unwrap();

    let status = svc.get_team_status("t1").await.unwrap();
    assert_eq!(status.teammates.len(), 0);
}

#[tokio::test]
async fn test_teammate_count() {
    let svc = test_service();
    svc.create_team("t1", "ctx-1", "ideation").await.unwrap();
    svc.add_teammate("t1", "a", "#ff0000", "sonnet", "code")
        .await
        .unwrap();
    svc.add_teammate("t1", "b", "#00ff00", "opus", "explore")
        .await
        .unwrap();

    assert_eq!(svc.get_teammate_count("t1").await.unwrap(), 2);
}

#[tokio::test]
async fn test_cleanup_stale_teams_for_context() {
    let svc = test_service();
    // Create two teams: one for the target context, one for a different context
    svc.create_team("team-a", "ctx-target", "task_execution")
        .await
        .unwrap();
    svc.create_team("team-b", "ctx-other", "ideation")
        .await
        .unwrap();

    // Cleanup should only disband the team belonging to ctx-target
    svc.cleanup_stale_teams_for_context("ctx-target").await;

    // team-a should now be disbanded
    let status_a = svc.get_team_status("team-a").await.unwrap();
    assert_eq!(
        status_a.phase,
        super::super::team_state_tracker::TeamPhase::Disbanded
    );

    // team-b should still be active (different context)
    let status_b = svc.get_team_status("team-b").await.unwrap();
    assert_ne!(
        status_b.phase,
        super::super::team_state_tracker::TeamPhase::Disbanded
    );
}

#[tokio::test]
async fn test_cleanup_stale_teams_no_match() {
    let svc = test_service();
    svc.create_team("team-x", "ctx-1", "ideation")
        .await
        .unwrap();

    // Cleanup for a non-existent context should not panic or affect other teams
    svc.cleanup_stale_teams_for_context("ctx-nonexistent").await;

    // team-x should remain unaffected
    let status = svc.get_team_status("team-x").await.unwrap();
    assert_ne!(
        status.phase,
        super::super::team_state_tracker::TeamPhase::Disbanded
    );
}

#[tokio::test]
async fn test_disband_sets_all_teammates_shutdown() {
    // Verifies disband_team transitions every teammate to Shutdown before marking Disbanded.
    // This mirrors the per-teammate emit_teammate_shutdown loop in team_service.rs.
    let svc = test_service();
    svc.create_team("t1", "ctx-1", "ideation").await.unwrap();
    svc.add_teammate("t1", "worker1", "#ff0000", "sonnet", "code")
        .await
        .unwrap();
    svc.add_teammate("t1", "worker2", "#00ff00", "sonnet", "code")
        .await
        .unwrap();
    svc.add_teammate("t1", "worker3", "#0000ff", "opus", "explore")
        .await
        .unwrap();

    svc.disband_team("t1").await.unwrap();

    let status = svc.get_team_status("t1").await.unwrap();
    assert_eq!(
        status.phase,
        super::super::team_state_tracker::TeamPhase::Disbanded,
        "team phase must be Disbanded"
    );
    // Every teammate must be Shutdown (per-teammate events precede team:disbanded)
    for t in &status.teammates {
        assert_eq!(
            t.status,
            TeammateStatus::Shutdown,
            "teammate {} must be Shutdown after disband",
            t.name
        );
    }
}

/// Regression: persist_message must not silently drop messages when the calling
/// TeamService instance has an empty session_id_cache because create_team was
/// called on a DIFFERENT TeamService sharing the same tracker + repos.
///
/// Production scenario:
///  - Tauri service calls create_team via chat_service_streaming → populates its cache
///  - HTTP service checks team_exists → true → skips create_team → empty cache
///  - HTTP service calls add_teammate_message → must fall back to DB for session ID
#[tokio::test]
async fn test_persist_message_db_fallback_when_cache_empty() {
    let tracker = Arc::new(TeamStateTracker::new());
    let session_repo: Arc<dyn crate::domain::repositories::TeamSessionRepository> =
        Arc::new(crate::infrastructure::memory::MemoryTeamSessionRepository::new());
    let message_repo: Arc<dyn crate::domain::repositories::TeamMessageRepository> =
        Arc::new(MemoryTeamMessageRepository::new());

    // Service A: simulates the Tauri service that creates the team and populates its cache
    let svc_a = TeamService::new_with_repos_for_testing(
        tracker.clone(),
        Arc::clone(&session_repo),
        Arc::clone(&message_repo),
    );
    svc_a.create_team("team-x", "ctx-99", "ideation").await.unwrap();

    // Service B: simulates the HTTP service — same tracker + repos, but fresh empty cache
    let svc_b = TeamService::new_with_repos_for_testing(
        tracker.clone(),
        Arc::clone(&session_repo),
        Arc::clone(&message_repo),
    );

    // svc_b must find the session via DB fallback and persist the message
    let msg = svc_b
        .add_teammate_message(
            "team-x",
            "researcher-1",
            None,
            "Here are the results",
            TeamMessageType::TeammateMessage,
        )
        .await
        .unwrap();
    assert_eq!(msg.sender, "researcher-1");
    assert_eq!(msg.content, "Here are the results");

    // Verify message was actually persisted to the DB (not just in-memory tracker)
    let sessions = session_repo.get_by_context("ideation", "ctx-99").await.unwrap();
    let sid = &sessions[0].id;
    let persisted = message_repo.get_by_session(sid).await.unwrap();
    assert_eq!(persisted.len(), 1);
    assert_eq!(persisted[0].sender, "researcher-1");
    assert_eq!(persisted[0].content, "Here are the results");
}

#[tokio::test]
async fn test_disband_empty_team_succeeds() {
    // Disband a team with no teammates — no panic, phase → Disbanded
    let svc = test_service();
    svc.create_team("empty-team", "ctx-1", "ideation")
        .await
        .unwrap();

    svc.disband_team("empty-team").await.unwrap();

    let status = svc.get_team_status("empty-team").await.unwrap();
    assert_eq!(
        status.phase,
        super::super::team_state_tracker::TeamPhase::Disbanded
    );
    assert!(status.teammates.is_empty());
}

// ============================================================================
// Persistence integration tests — verify round-trip through DB layer
// ============================================================================

/// conversation_id round-trip: set on tracker → persist → read back from DB snapshot.
#[tokio::test]
async fn test_persist_conversation_id_round_trip() {
    let (svc, session_repo, _msg_repo) = test_service_with_repos();

    svc.create_team("t-conv", "ctx-1", "ideation").await.unwrap();
    svc.add_teammate("t-conv", "worker", "#ff6b35", "sonnet", "code")
        .await
        .unwrap();

    // Set conversation_id on the in-memory tracker
    svc.tracker()
        .set_teammate_conversation_id("t-conv", "worker", "conv-abc-123".to_string())
        .await
        .unwrap();

    // Trigger a persist (update_teammate_status calls persist_teammates internally)
    svc.update_teammate_status("t-conv", "worker", TeammateStatus::Running)
        .await
        .unwrap();

    // Read back from DB
    let sessions = session_repo.get_by_context("ideation", "ctx-1").await.unwrap();
    assert_eq!(sessions.len(), 1);
    let snap = &sessions[0].teammates;
    assert_eq!(snap.len(), 1);
    assert_eq!(snap[0].name, "worker");
    assert_eq!(
        snap[0].conversation_id.as_deref(),
        Some("conv-abc-123"),
        "conversation_id must survive persist round-trip"
    );
}

/// Shutdown status persistence: stop_team persists AFTER setting shutdown status.
#[tokio::test]
async fn test_stop_team_persists_shutdown_status() {
    let (svc, session_repo, _msg_repo) = test_service_with_repos();

    svc.create_team("t-stop", "ctx-2", "ideation").await.unwrap();
    svc.add_teammate("t-stop", "w1", "#ff0000", "sonnet", "code")
        .await
        .unwrap();
    svc.add_teammate("t-stop", "w2", "#00ff00", "opus", "explore")
        .await
        .unwrap();

    // Move to running so we know they're not stuck at initial status
    svc.update_teammate_status("t-stop", "w1", TeammateStatus::Running)
        .await
        .unwrap();
    svc.update_teammate_status("t-stop", "w2", TeammateStatus::Idle)
        .await
        .unwrap();

    // Stop team — should persist final "shutdown" status, not stale "running"/"idle"
    svc.stop_team("t-stop").await.unwrap();

    let sessions = session_repo.get_by_context("ideation", "ctx-2").await.unwrap();
    assert_eq!(sessions.len(), 1);
    for snap in &sessions[0].teammates {
        assert_eq!(
            snap.status, "shutdown",
            "teammate {} must be 'shutdown' in DB after stop_team, got '{}'",
            snap.name, snap.status
        );
    }
}

/// Disband persistence: all teammates shutdown + disbanded_at set in DB.
#[tokio::test]
async fn test_disband_persists_shutdown_and_disbanded_at() {
    let (svc, session_repo, _msg_repo) = test_service_with_repos();

    svc.create_team("t-disband", "ctx-3", "ideation").await.unwrap();
    svc.add_teammate("t-disband", "alpha", "#ff6b35", "opus", "explore")
        .await
        .unwrap();
    svc.add_teammate("t-disband", "beta", "#00ff00", "sonnet", "code")
        .await
        .unwrap();

    // Set conversation_ids to verify they survive disband
    svc.tracker()
        .set_teammate_conversation_id("t-disband", "alpha", "conv-alpha".to_string())
        .await
        .unwrap();
    svc.tracker()
        .set_teammate_conversation_id("t-disband", "beta", "conv-beta".to_string())
        .await
        .unwrap();

    // Move to running before disband
    svc.update_teammate_status("t-disband", "alpha", TeammateStatus::Running)
        .await
        .unwrap();
    svc.update_teammate_status("t-disband", "beta", TeammateStatus::Idle)
        .await
        .unwrap();

    svc.disband_team("t-disband").await.unwrap();

    let sessions = session_repo.get_by_context("ideation", "ctx-3").await.unwrap();
    assert_eq!(sessions.len(), 1);
    let session = &sessions[0];

    // disbanded_at must be set
    assert!(
        session.disbanded_at.is_some(),
        "disbanded_at must be set after disband_team"
    );

    // All teammates must be shutdown
    assert_eq!(session.teammates.len(), 2);
    for snap in &session.teammates {
        assert_eq!(
            snap.status, "shutdown",
            "teammate {} must be 'shutdown' after disband, got '{}'",
            snap.name, snap.status
        );
    }

    // conversation_ids must survive disband persist
    let alpha_snap = session.teammates.iter().find(|t| t.name == "alpha").unwrap();
    assert_eq!(alpha_snap.conversation_id.as_deref(), Some("conv-alpha"));
    let beta_snap = session.teammates.iter().find(|t| t.name == "beta").unwrap();
    assert_eq!(beta_snap.conversation_id.as_deref(), Some("conv-beta"));
}

/// Backward compatibility: existing teammate_json without conversation_id deserializes as None.
#[tokio::test]
async fn test_backward_compat_teammate_json_without_conversation_id() {
    use crate::domain::entities::team::TeammateSnapshot;

    // Simulate a legacy JSON blob from the DB that was written before conversation_id existed
    let legacy_json = r##"[{
        "name": "old-worker",
        "color": "#ff6b35",
        "model": "sonnet",
        "role": "code",
        "status": "idle",
        "cost": {"input_tokens":500,"output_tokens":200,"cache_creation_tokens":0,"cache_read_tokens":0,"estimated_usd":0.01},
        "spawned_at": "2024-01-01T00:00:00Z",
        "last_activity_at": "2024-01-01T00:05:00Z"
    }]"##;

    let snapshots: Vec<TeammateSnapshot> = serde_json::from_str(legacy_json).unwrap();
    assert_eq!(snapshots.len(), 1);
    assert_eq!(snapshots[0].name, "old-worker");
    assert!(
        snapshots[0].conversation_id.is_none(),
        "legacy JSON without conversation_id must deserialize as None"
    );

    // Also verify that new JSON WITH conversation_id round-trips correctly
    let new_json = r##"[{
        "name": "new-worker",
        "color": "#00ff00",
        "model": "opus",
        "role": "explore",
        "status": "shutdown",
        "cost": {"input_tokens":1000,"output_tokens":400,"cache_creation_tokens":0,"cache_read_tokens":0,"estimated_usd":0.03},
        "spawned_at": "2024-06-01T00:00:00Z",
        "last_activity_at": "2024-06-01T00:10:00Z",
        "conversation_id": "conv-new-123"
    }]"##;

    let snapshots: Vec<TeammateSnapshot> = serde_json::from_str(new_json).unwrap();
    assert_eq!(snapshots[0].conversation_id.as_deref(), Some("conv-new-123"));
}
