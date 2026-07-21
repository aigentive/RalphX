use super::*;

fn pending_plan(plan_id: &str, context_id: &str, created_at: DateTime<Utc>) -> PendingTeamPlan {
    PendingTeamPlan {
        plan_id: plan_id.to_string(),
        context_type: "ideation".to_string(),
        context_id: context_id.to_string(),
        process: "Implement the plan".to_string(),
        teammates: Vec::new(),
        created_at,
        team_name: "test-team".to_string(),
        lead_session_id: None,
    }
}

#[tokio::test]
async fn store_pending_plan_reports_superseded_and_garbage_collected_authority() {
    let tracker = TeamStateTracker::new();
    tracker
        .store_pending_plan(pending_plan("plan-old", "context-a", Utc::now()))
        .await;
    tracker
        .store_pending_plan(pending_plan(
            "plan-stale",
            "context-b",
            Utc::now() - chrono::Duration::minutes(31),
        ))
        .await;

    let removed = tracker
        .store_pending_plan(pending_plan("plan-new", "context-a", Utc::now()))
        .await;

    assert!(removed.iter().any(|entry| {
        entry.plan.plan_id == "plan-old" && entry.reason == PendingTeamPlanRemovalReason::Superseded
    }));
    assert!(removed.iter().any(|entry| {
        entry.plan.plan_id == "plan-stale"
            && entry.reason == PendingTeamPlanRemovalReason::GarbageCollected
    }));
    assert!(tracker
        .get_pending_plan_for_context("context-a")
        .await
        .is_some_and(|plan| plan.plan_id == "plan-new"));
}

#[tokio::test]
async fn expire_pending_plan_removes_authority_and_channel_together() {
    let tracker = TeamStateTracker::new();
    tracker
        .store_pending_plan(pending_plan("plan-timeout", "context-a", Utc::now()))
        .await;
    tracker.register_plan_channel("plan-timeout").await;

    let expired = tracker.expire_pending_plan("plan-timeout").await;

    assert!(expired.is_some_and(|plan| plan.plan_id == "plan-timeout"));
    assert!(tracker
        .get_pending_plan_for_context("context-a")
        .await
        .is_none());
    assert!(!tracker.plan_channel_exists("plan-timeout").await);
}

#[tokio::test]
async fn expiry_preserves_the_channel_after_approval_claims_authority() {
    let tracker = TeamStateTracker::new();
    tracker
        .store_pending_plan(pending_plan("plan-claimed", "context-a", Utc::now()))
        .await;
    tracker.register_plan_channel("plan-claimed").await;
    let claimed = tracker
        .take_pending_plan("plan-claimed")
        .await
        .expect("approval should claim the pending plan");

    assert!(tracker.expire_pending_plan("plan-claimed").await.is_none());
    assert!(tracker.plan_channel_exists("plan-claimed").await);
    assert_eq!(claimed.plan_id, "plan-claimed");
}

#[tokio::test]
async fn test_create_team() {
    let tracker = TeamStateTracker::new();
    tracker
        .create_team("ideation-team", "session-123", "ideation")
        .await
        .unwrap();

    assert!(tracker.team_exists("ideation-team").await);
    assert!(!tracker.team_exists("nonexistent").await);
}

#[tokio::test]
async fn test_create_duplicate_team_fails() {
    let tracker = TeamStateTracker::new();
    tracker
        .create_team("team1", "ctx-1", "ideation")
        .await
        .unwrap();

    let result = tracker.create_team("team1", "ctx-2", "ideation").await;
    assert!(matches!(
        result,
        Err(TeamTrackerError::TeamAlreadyExists(_))
    ));
}

#[tokio::test]
async fn test_add_teammate() {
    let tracker = TeamStateTracker::new();
    tracker
        .create_team("team1", "ctx-1", "ideation")
        .await
        .unwrap();

    tracker
        .add_teammate("team1", "researcher", "#ff6b35", "opus", "explore")
        .await
        .unwrap();

    let status = tracker.get_team_status("team1").await.unwrap();
    assert_eq!(status.teammates.len(), 1);
    assert_eq!(status.teammates[0].name, "researcher");
    assert_eq!(status.teammates[0].status, TeammateStatus::Spawning);
    assert_eq!(status.lead_name, Some("researcher".to_string()));
    assert_eq!(status.phase, TeamPhase::Active);
}

#[tokio::test]
async fn test_add_duplicate_teammate_fails() {
    let tracker = TeamStateTracker::new();
    tracker
        .create_team("team1", "ctx-1", "ideation")
        .await
        .unwrap();
    tracker
        .add_teammate("team1", "researcher", "#ff6b35", "opus", "explore")
        .await
        .unwrap();

    let result = tracker
        .add_teammate("team1", "researcher", "#00ff00", "sonnet", "plan")
        .await;
    assert!(matches!(
        result,
        Err(TeamTrackerError::TeammateAlreadyExists(_))
    ));
}

#[tokio::test]
async fn test_update_teammate_status() {
    let tracker = TeamStateTracker::new();
    tracker
        .create_team("team1", "ctx-1", "ideation")
        .await
        .unwrap();
    tracker
        .add_teammate("team1", "worker", "#00ff00", "sonnet", "code")
        .await
        .unwrap();

    tracker
        .update_teammate_status("team1", "worker", TeammateStatus::Running)
        .await
        .unwrap();

    let status = tracker.get_team_status("team1").await.unwrap();
    assert_eq!(status.teammates[0].status, TeammateStatus::Running);
}

#[tokio::test]
async fn test_update_status_nonexistent_team_fails() {
    let tracker = TeamStateTracker::new();
    let result = tracker
        .update_teammate_status("nonexistent", "worker", TeammateStatus::Running)
        .await;
    assert!(matches!(result, Err(TeamTrackerError::TeamNotFound(_))));
}

#[tokio::test]
async fn test_update_status_nonexistent_teammate_fails() {
    let tracker = TeamStateTracker::new();
    tracker
        .create_team("team1", "ctx-1", "ideation")
        .await
        .unwrap();

    let result = tracker
        .update_teammate_status("team1", "ghost", TeammateStatus::Running)
        .await;
    assert!(matches!(result, Err(TeamTrackerError::TeammateNotFound(_))));
}

#[tokio::test]
async fn test_update_teammate_cost() {
    let tracker = TeamStateTracker::new();
    tracker
        .create_team("team1", "ctx-1", "ideation")
        .await
        .unwrap();
    tracker
        .add_teammate("team1", "researcher", "#ff6b35", "opus", "explore")
        .await
        .unwrap();

    let cost = TeammateCost {
        input_tokens: 1000,
        output_tokens: 500,
        cache_creation_tokens: 200,
        cache_read_tokens: 100,
        estimated_usd: 0.05,
    };
    tracker
        .update_teammate_cost("team1", "researcher", cost)
        .await
        .unwrap();

    let cost_response = tracker
        .get_teammate_cost("team1", "researcher")
        .await
        .unwrap();
    assert_eq!(cost_response.input_tokens, 1000);
    assert_eq!(cost_response.output_tokens, 500);
    assert_eq!(cost_response.estimated_usd, 0.05);
}

#[tokio::test]
async fn test_send_user_message() {
    let tracker = TeamStateTracker::new();
    tracker
        .create_team("team1", "ctx-1", "ideation")
        .await
        .unwrap();

    let msg = tracker
        .send_user_message("team1", "Hello team!")
        .await
        .unwrap();
    assert_eq!(msg.sender, "user");
    assert_eq!(msg.content, "Hello team!");
    assert_eq!(msg.message_type, TeamMessageType::UserMessage);
}

#[tokio::test]
async fn test_add_teammate_message() {
    let tracker = TeamStateTracker::new();
    tracker
        .create_team("team1", "ctx-1", "ideation")
        .await
        .unwrap();

    let msg = tracker
        .add_teammate_message(
            "team1",
            "researcher",
            Some("planner"),
            "Found some results",
            TeamMessageType::TeammateMessage,
        )
        .await
        .unwrap();
    assert_eq!(msg.sender, "researcher");
    assert_eq!(msg.recipient, Some("planner".to_string()));
}

#[tokio::test]
async fn test_get_team_messages_with_limit() {
    let tracker = TeamStateTracker::new();
    tracker
        .create_team("team1", "ctx-1", "ideation")
        .await
        .unwrap();

    for i in 0..5 {
        tracker
            .send_user_message("team1", &format!("Message {}", i))
            .await
            .unwrap();
    }

    // Get all messages
    let all = tracker.get_team_messages("team1", None).await.unwrap();
    assert_eq!(all.len(), 5);

    // Get limited messages (most recent first)
    let limited = tracker.get_team_messages("team1", Some(2)).await.unwrap();
    assert_eq!(limited.len(), 2);
}

#[tokio::test]
async fn test_stop_team() {
    let tracker = TeamStateTracker::new();
    tracker
        .create_team("team1", "ctx-1", "ideation")
        .await
        .unwrap();
    tracker
        .add_teammate("team1", "worker1", "#ff0000", "sonnet", "code")
        .await
        .unwrap();
    tracker
        .add_teammate("team1", "worker2", "#00ff00", "sonnet", "code")
        .await
        .unwrap();

    tracker.stop_team("team1").await.unwrap();

    let status = tracker.get_team_status("team1").await.unwrap();
    assert_eq!(status.phase, TeamPhase::Winding);
    for t in &status.teammates {
        assert_eq!(t.status, TeammateStatus::Shutdown);
    }
}

#[tokio::test]
async fn test_disband_team() {
    let tracker = TeamStateTracker::new();
    tracker
        .create_team("team1", "ctx-1", "ideation")
        .await
        .unwrap();
    tracker
        .add_teammate("team1", "worker", "#ff0000", "sonnet", "code")
        .await
        .unwrap();

    tracker.disband_team("team1").await.unwrap();

    let status = tracker.get_team_status("team1").await.unwrap();
    assert_eq!(status.phase, TeamPhase::Disbanded);
}

#[tokio::test]
async fn test_list_teams() {
    let tracker = TeamStateTracker::new();
    tracker
        .create_team("alpha", "ctx-1", "ideation")
        .await
        .unwrap();
    tracker
        .create_team("beta", "ctx-2", "ideation")
        .await
        .unwrap();

    let teams = tracker.list_teams().await;
    assert_eq!(teams.len(), 2);
    assert!(teams.contains(&"alpha".to_string()));
    assert!(teams.contains(&"beta".to_string()));
}

#[tokio::test]
async fn test_remove_teammate() {
    let tracker = TeamStateTracker::new();
    tracker
        .create_team("team1", "ctx-1", "ideation")
        .await
        .unwrap();
    tracker
        .add_teammate("team1", "worker", "#ff0000", "sonnet", "code")
        .await
        .unwrap();

    tracker.remove_teammate("team1", "worker").await.unwrap();

    let status = tracker.get_team_status("team1").await.unwrap();
    assert_eq!(status.teammates.len(), 0);
}

#[tokio::test]
async fn test_thread_safety() {
    let tracker = TeamStateTracker::new();
    tracker
        .create_team("shared", "ctx-1", "ideation")
        .await
        .unwrap();

    let mut handles = vec![];
    for i in 0..10 {
        let t = tracker.clone();
        handles.push(tokio::spawn(async move {
            t.add_teammate(
                "shared",
                &format!("worker-{}", i),
                "#ffffff",
                "sonnet",
                "code",
            )
            .await
            .unwrap();
        }));
    }

    for h in handles {
        h.await.unwrap();
    }

    let status = tracker.get_team_status("shared").await.unwrap();
    assert_eq!(status.teammates.len(), 10);
}

#[tokio::test]
async fn test_default_creates_new_tracker() {
    let tracker = TeamStateTracker::default();
    let teams = tracker.list_teams().await;
    assert!(teams.is_empty());
}

#[tokio::test]
async fn test_teammate_status_idle_to_running_cycle() {
    // Models the TurnComplete → Idle → (activity) → Running cycle
    // from team_stream_processor.rs: Fix B
    let tracker = TeamStateTracker::new();
    tracker
        .create_team("team1", "ctx-1", "ideation")
        .await
        .unwrap();
    tracker
        .add_teammate("team1", "worker", "#ff6b35", "opus", "code")
        .await
        .unwrap();

    // Spawning → Running (first text chunk)
    tracker
        .update_teammate_status("team1", "worker", TeammateStatus::Running)
        .await
        .unwrap();
    let status = tracker.get_team_status("team1").await.unwrap();
    assert_eq!(status.teammates[0].status, TeammateStatus::Running);

    // Running → Idle (TurnComplete received)
    tracker
        .update_teammate_status("team1", "worker", TeammateStatus::Idle)
        .await
        .unwrap();
    let status = tracker.get_team_status("team1").await.unwrap();
    assert_eq!(status.teammates[0].status, TeammateStatus::Idle);

    // Idle → Running (next activity after TurnComplete)
    tracker
        .update_teammate_status("team1", "worker", TeammateStatus::Running)
        .await
        .unwrap();
    let status = tracker.get_team_status("team1").await.unwrap();
    assert_eq!(status.teammates[0].status, TeammateStatus::Running);
}

#[tokio::test]
async fn test_multiple_idle_running_cycles() {
    // Verifies the cycle can repeat (multiple turns from same teammate)
    let tracker = TeamStateTracker::new();
    tracker
        .create_team("team1", "ctx-1", "ideation")
        .await
        .unwrap();
    tracker
        .add_teammate("team1", "agent", "#ff6b35", "sonnet", "code")
        .await
        .unwrap();

    for _ in 0..3 {
        tracker
            .update_teammate_status("team1", "agent", TeammateStatus::Running)
            .await
            .unwrap();
        let s = tracker.get_team_status("team1").await.unwrap();
        assert_eq!(s.teammates[0].status, TeammateStatus::Running);

        tracker
            .update_teammate_status("team1", "agent", TeammateStatus::Idle)
            .await
            .unwrap();
        let s = tracker.get_team_status("team1").await.unwrap();
        assert_eq!(s.teammates[0].status, TeammateStatus::Idle);
    }
}
