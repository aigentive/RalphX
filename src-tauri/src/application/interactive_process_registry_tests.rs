use crate::application::interactive_process_registry::*;
use crate::domain::agents::AgentHarnessKind;
use std::sync::Arc;
use tokio::io::AsyncReadExt;
use tokio::process::ChildStdin;

#[tokio::test]
async fn successful_tracked_write_and_concurrent_removal_preserve_the_turn_exactly_once() {
    let registry = Arc::new(InteractiveProcessRegistry::new());
    let key = InteractiveProcessKey::new("project", "atomic-write-removal");
    let (stdin, _child) = create_test_stdin().await;
    let token = registry
        .register_with_metadata(
            key.clone(),
            stdin,
            InteractiveProcessMetadata {
                agent_run_id: Some("current-run".to_string()),
                ..Default::default()
            },
        )
        .await;
    let turn = PendingStdinTurn {
        persisted_message_id: "message-1".to_string(),
        content: "unanswered".to_string(),
        metadata_override: None,
        queued_at: "2026-07-30T10:00:00Z".to_string(),
    };

    let (write_result, removed) = tokio::join!(
        registry.write_message_if_owner_with_pending_turn(
            &key,
            token,
            "current-run",
            "user follow-up",
            turn.clone(),
        ),
        registry.remove_if_token(&key, token),
    );
    let mut removed = removed.expect("exact owner removal");
    assert!(
        write_result.is_ok(),
        "the first-polled write owns the registry lock"
    );
    assert_eq!(removed.take_pending_stdin_turns(), vec![turn]);
    assert!(removed.take_pending_stdin_turns().is_empty());
}

#[tokio::test]
async fn pending_stdin_turns_are_fifo_and_exact_owner_scoped() {
    let registry = InteractiveProcessRegistry::new();
    let key = InteractiveProcessKey::new("project", "pending-turns");
    let (stdin, _child) = create_test_stdin().await;
    let token = registry
        .register_with_metadata(key.clone(), stdin, InteractiveProcessMetadata::default())
        .await;
    let first = PendingStdinTurn {
        persisted_message_id: "message-1".to_string(),
        content: "first".to_string(),
        metadata_override: None,
        queued_at: "2026-07-30T10:00:00Z".to_string(),
    };
    let second = PendingStdinTurn {
        persisted_message_id: "message-2".to_string(),
        content: "second".to_string(),
        metadata_override: Some(r#"{\"source\":\"stdin\"}"#.to_string()),
        queued_at: "2026-07-30T10:00:01Z".to_string(),
    };

    assert!(registry.push_pending_turn(&key, token, first.clone()).await);
    assert!(
        registry
            .push_pending_turn(&key, token, second.clone())
            .await
    );
    assert_eq!(
        registry.settle_delivered_turns_if_owner(&key, token).await,
        vec![first, second]
    );
    assert!(registry.take_pending_turns(&key, token).await.is_empty());
}

#[tokio::test]
async fn pending_stdin_settlement_from_a_stale_token_is_a_no_op() {
    let registry = InteractiveProcessRegistry::new();
    let key = InteractiveProcessKey::new("project", "stale-settlement");
    let (stdin, _child) = create_test_stdin().await;
    let token = registry
        .register_with_metadata(key.clone(), stdin, InteractiveProcessMetadata::default())
        .await;
    let turn = PendingStdinTurn {
        persisted_message_id: "message-1".to_string(),
        content: "still unanswered".to_string(),
        metadata_override: None,
        queued_at: "2026-07-30T10:00:00Z".to_string(),
    };
    assert!(registry.push_pending_turn(&key, token, turn.clone()).await);
    let other_key = InteractiveProcessKey::new("project", "other-owner");
    let (other_stdin, _other_child) = create_test_stdin().await;
    let other_token = registry
        .register_with_metadata(
            other_key,
            other_stdin,
            InteractiveProcessMetadata::default(),
        )
        .await;

    assert!(registry
        .settle_delivered_turns_if_owner(&key, other_token)
        .await
        .is_empty());
    assert_eq!(registry.take_pending_turns(&key, token).await, vec![turn]);
}

#[tokio::test]
async fn pending_stdin_turns_do_not_cross_registration_owners() {
    let registry = InteractiveProcessRegistry::new();
    let key = InteractiveProcessKey::new("project", "pending-replacement");
    let (old_stdin, _old_child) = create_test_stdin().await;
    let old_token = registry
        .register_with_metadata(
            key.clone(),
            old_stdin,
            InteractiveProcessMetadata::default(),
        )
        .await;
    assert!(
        registry
            .push_pending_turn(
                &key,
                old_token,
                PendingStdinTurn {
                    persisted_message_id: "old-message".to_string(),
                    content: "old".to_string(),
                    metadata_override: None,
                    queued_at: "2026-07-30T10:00:00Z".to_string(),
                },
            )
            .await
    );

    let (new_stdin, _new_child) = create_test_stdin().await;
    let new_token = registry
        .register_with_metadata(
            key.clone(),
            new_stdin,
            InteractiveProcessMetadata::default(),
        )
        .await;

    assert!(registry
        .take_pending_turns(&key, old_token)
        .await
        .is_empty());
    assert!(registry
        .take_pending_turns(&key, new_token)
        .await
        .is_empty());
}

#[tokio::test]
async fn removed_entry_hands_back_pending_stdin_turns() {
    let registry = InteractiveProcessRegistry::new();
    let key = InteractiveProcessKey::new("project", "removed-pending-turns");
    let (stdin, _child) = create_test_stdin().await;
    let token = registry
        .register_with_metadata(key.clone(), stdin, InteractiveProcessMetadata::default())
        .await;
    let turn = PendingStdinTurn {
        persisted_message_id: "message-1".to_string(),
        content: "unanswered".to_string(),
        metadata_override: None,
        queued_at: "2026-07-30T10:00:00Z".to_string(),
    };
    assert!(registry.push_pending_turn(&key, token, turn.clone()).await);

    let mut removed = registry
        .remove_if_token(&key, token)
        .await
        .expect("owner must remove its own entry");
    assert_eq!(removed.take_pending_stdin_turns(), vec![turn]);
    assert!(removed.take_pending_stdin_turns().is_empty());
}

#[tokio::test]
async fn capture_owner_returns_current_token_run_id_and_cloned_metadata() {
    let registry = InteractiveProcessRegistry::new();
    let key = InteractiveProcessKey::new("project", "capture-owner");
    let (stdin, _child) = create_test_stdin().await;
    let metadata = InteractiveProcessMetadata {
        agent_run_id: Some("current-run".to_string()),
        harness: Some(AgentHarnessKind::Codex),
        provider_session_id: Some("thread-123".to_string()),
        persona_id: Some("planner".to_string()),
        persona_content_hash: Some("content-hash".to_string()),
        agent_name: Some("ralphx-ideation".to_string()),
        agent_profile: Some("plan".to_string()),
    };
    let token = registry
        .register_with_metadata(key.clone(), stdin, metadata.clone())
        .await;

    let owner = registry
        .capture_owner(&key)
        .await
        .expect("the current launch owner must be captured");

    assert_eq!(owner.token, token);
    assert_eq!(owner.agent_run_id, "current-run");
    assert_eq!(owner.metadata, metadata);
}

#[tokio::test]
async fn capture_owner_fails_closed_without_a_run_id() {
    let registry = InteractiveProcessRegistry::new();
    let key = InteractiveProcessKey::new("project", "capture-no-run-id");
    let (stdin, _child) = create_test_stdin().await;
    let token = registry
        .register_with_metadata(key.clone(), stdin, InteractiveProcessMetadata::default())
        .await;

    assert!(registry.capture_owner(&key).await.is_none());
    assert_eq!(
        registry
            .retire_after_turn_disposition_if_owner(&key, token, "")
            .await,
        InteractiveProcessRetireAfterTurnDisposition::Stale
    );
    assert_eq!(
        registry
            .retire_after_turn_disposition_if_owner(
                &InteractiveProcessKey::new("project", "missing-owner"),
                token,
                "current-run",
            )
            .await,
        InteractiveProcessRetireAfterTurnDisposition::Stale
    );
}

#[tokio::test]
async fn captured_owner_snapshot_is_invalidated_by_replacement() {
    let registry = InteractiveProcessRegistry::new();
    let key = InteractiveProcessKey::new("project", "capture-replacement");
    let (stale_stdin, _stale_child) = create_test_stdin().await;
    registry
        .register_with_metadata(
            key.clone(),
            stale_stdin,
            InteractiveProcessMetadata {
                agent_run_id: Some("stale-run".to_string()),
                ..Default::default()
            },
        )
        .await;
    let stale_owner = registry
        .capture_owner(&key)
        .await
        .expect("first owner must be captured");

    let (current_stdin, _current_child) = create_test_stdin().await;
    registry
        .register_with_metadata(
            key.clone(),
            current_stdin,
            InteractiveProcessMetadata {
                agent_run_id: Some("current-run".to_string()),
                ..Default::default()
            },
        )
        .await;

    let current_owner = registry
        .capture_owner(&key)
        .await
        .expect("replacement owner must be captured");

    assert_ne!(current_owner.token, stale_owner.token);
    assert_eq!(current_owner.agent_run_id, "current-run");
    assert_eq!(
        current_owner.metadata.agent_run_id.as_deref(),
        Some("current-run")
    );
}

#[tokio::test]
async fn exact_owner_write_rejects_replacement_without_touching_its_stdin() {
    let registry = InteractiveProcessRegistry::new();
    let key = InteractiveProcessKey::new("project", "write-replacement");
    let (stale_stdin, mut stale_child) = create_observable_test_stdin().await;
    registry
        .register_with_metadata(
            key.clone(),
            stale_stdin,
            InteractiveProcessMetadata {
                agent_run_id: Some("stale-run".to_string()),
                ..Default::default()
            },
        )
        .await;
    let stale_owner = registry
        .capture_owner(&key)
        .await
        .expect("capture stale owner");

    let (replacement_stdin, mut replacement_child) = create_observable_test_stdin().await;
    registry
        .register_with_metadata(
            key.clone(),
            replacement_stdin,
            InteractiveProcessMetadata {
                agent_run_id: Some("replacement-run".to_string()),
                ..Default::default()
            },
        )
        .await;

    assert!(matches!(
        registry
            .write_message_if_owner(
                &key,
                stale_owner.token,
                &stale_owner.agent_run_id,
                "must not reach replacement",
            )
            .await,
        Err(InteractiveProcessWriteError::Missing { .. })
    ));

    registry.remove(&key).await;
    let mut replacement_output = Vec::new();
    replacement_child
        .stdout
        .take()
        .expect("replacement stdout")
        .read_to_end(&mut replacement_output)
        .await
        .expect("read replacement stdout");
    let _ = replacement_child.wait().await;
    assert!(
        replacement_output.is_empty(),
        "a stale owner snapshot must not write to the replacement process"
    );
    let _ = stale_child.wait().await;
}

#[tokio::test]
async fn retire_after_turn_disposition_reports_armed_and_unarmed_active_and_idle_states() {
    let registry = InteractiveProcessRegistry::new();
    let key = InteractiveProcessKey::new("project", "retire-disposition");
    let (stdin, _child) = create_test_stdin().await;
    let token = registry
        .register_with_metadata(
            key.clone(),
            stdin,
            InteractiveProcessMetadata {
                agent_run_id: Some("current-run".to_string()),
                ..Default::default()
            },
        )
        .await;

    assert_eq!(
        registry
            .retire_after_turn_disposition_if_owner(&key, token, "current-run")
            .await,
        InteractiveProcessRetireAfterTurnDisposition::Active { is_armed: false }
    );
    assert_eq!(
        registry
            .arm_retire_after_turn_if_owner(&key, token, "current-run")
            .await,
        InteractiveProcessRetireArmDisposition::AwaitingTurn
    );
    assert_eq!(
        registry
            .retire_after_turn_disposition_if_owner(&key, token, "current-run")
            .await,
        InteractiveProcessRetireAfterTurnDisposition::Active { is_armed: true }
    );

    assert!(registry.mark_idle_if_token(&key, token).await);
    assert_eq!(
        registry
            .retire_after_turn_disposition_if_owner(&key, token, "current-run")
            .await,
        InteractiveProcessRetireAfterTurnDisposition::Idle { is_armed: true }
    );
    assert!(
        registry
            .disarm_retire_after_turn_if_owner(&key, token, "current-run")
            .await
    );
    assert_eq!(
        registry
            .retire_after_turn_disposition_if_owner(&key, token, "current-run")
            .await,
        InteractiveProcessRetireAfterTurnDisposition::Idle { is_armed: false }
    );
}

#[tokio::test]
async fn stale_retirement_disposition_query_preserves_the_replacement() {
    let registry = InteractiveProcessRegistry::new();
    let key = InteractiveProcessKey::new("project", "retire-query-replacement");
    let (stale_stdin, _stale_child) = create_test_stdin().await;
    let stale_token = registry
        .register_with_metadata(
            key.clone(),
            stale_stdin,
            InteractiveProcessMetadata {
                agent_run_id: Some("stale-run".to_string()),
                ..Default::default()
            },
        )
        .await;
    let (current_stdin, _current_child) = create_test_stdin().await;
    let current_token = registry
        .register_with_metadata(
            key.clone(),
            current_stdin,
            InteractiveProcessMetadata {
                agent_run_id: Some("current-run".to_string()),
                ..Default::default()
            },
        )
        .await;

    assert_eq!(
        registry
            .retire_after_turn_disposition_if_owner(&key, stale_token, "stale-run")
            .await,
        InteractiveProcessRetireAfterTurnDisposition::Stale
    );
    assert_eq!(
        registry
            .retire_after_turn_disposition_if_owner(&key, current_token, "current-run")
            .await,
        InteractiveProcessRetireAfterTurnDisposition::Active { is_armed: false }
    );
    assert_eq!(
        registry
            .capture_owner(&key)
            .await
            .expect("replacement must remain registered")
            .token,
        current_token
    );
}

#[tokio::test]
async fn retire_after_turn_requires_exact_token_and_run_owner() {
    let registry = InteractiveProcessRegistry::new();
    let key = InteractiveProcessKey::new("project", "retire-owner");
    let (stdin, _child) = create_test_stdin().await;
    let token = registry
        .register_with_metadata(
            key.clone(),
            stdin,
            InteractiveProcessMetadata {
                agent_run_id: Some("current-run".to_string()),
                ..Default::default()
            },
        )
        .await;

    assert_eq!(
        registry
            .arm_retire_after_turn_if_owner(&key, token, "current-run")
            .await,
        InteractiveProcessRetireArmDisposition::AwaitingTurn
    );
    assert!(matches!(
        registry
            .write_message(&key, "must not queue behind retirement")
            .await,
        Err(InteractiveProcessWriteError::Retiring { .. })
    ));
    assert_eq!(
        registry
            .complete_turn_if_owner(&key, token, "current-run")
            .await,
        InteractiveProcessTurnCompleteDisposition::RetireAfterTurn {
            pending_turns: Vec::new(),
        }
    );
    assert!(!registry.has_process(&key).await);
}

#[tokio::test]
async fn stale_retire_after_turn_owner_is_rejected_without_affecting_current_entry() {
    let registry = InteractiveProcessRegistry::new();
    let key = InteractiveProcessKey::new("project", "retire-stale");
    let (stdin_a, _child_a) = create_test_stdin().await;
    let stale_token = registry
        .register_with_metadata(
            key.clone(),
            stdin_a,
            InteractiveProcessMetadata {
                agent_run_id: Some("stale-run".to_string()),
                ..Default::default()
            },
        )
        .await;
    let (stdin_b, _child_b) = create_test_stdin().await;
    let current_token = registry
        .register_with_metadata(
            key.clone(),
            stdin_b,
            InteractiveProcessMetadata {
                agent_run_id: Some("current-run".to_string()),
                ..Default::default()
            },
        )
        .await;

    assert_eq!(
        registry
            .arm_retire_after_turn_if_owner(&key, stale_token, "stale-run")
            .await,
        InteractiveProcessRetireArmDisposition::Stale
    );
    assert_eq!(
        registry
            .arm_retire_after_turn_if_owner(&key, current_token, "stale-run")
            .await,
        InteractiveProcessRetireArmDisposition::Stale
    );
    assert_eq!(
        registry
            .complete_turn_if_owner(&key, stale_token, "stale-run")
            .await,
        InteractiveProcessTurnCompleteDisposition::Stale
    );
    assert_eq!(
        registry
            .complete_turn_if_owner(&key, current_token, "current-run")
            .await,
        InteractiveProcessTurnCompleteDisposition::KeepAlive
    );
    assert_eq!(
        registry.state_for_test(&key).await,
        Some(InteractiveProcessState::Idle)
    );
}

#[tokio::test]
async fn plan_to_edit_stale_direct_idle_retirement_preserves_the_current_owner() {
    let registry = InteractiveProcessRegistry::new();
    let key = InteractiveProcessKey::new("project", "direct-retire-stale");
    let (stdin, _child) = create_test_stdin().await;
    let token = registry
        .register_with_metadata(
            key.clone(),
            stdin,
            InteractiveProcessMetadata {
                agent_run_id: Some("current-run".to_string()),
                ..Default::default()
            },
        )
        .await;
    assert!(registry.mark_idle_if_token(&key, token).await);

    assert!(registry
        .retire_unarmed_idle_if_owner(&key, token, "stale-run")
        .await
        .is_none());
    assert_eq!(
        registry
            .capture_owner(&key)
            .await
            .expect("current owner must remain after stale retirement")
            .agent_run_id,
        "current-run"
    );
}

#[tokio::test]
async fn disarming_retire_after_turn_restores_writes_and_idle_disposition() {
    let registry = InteractiveProcessRegistry::new();
    let key = InteractiveProcessKey::new("project", "retire-disarm");
    let (stdin, _child) = create_test_stdin().await;
    let token = registry
        .register_with_metadata(
            key.clone(),
            stdin,
            InteractiveProcessMetadata {
                agent_run_id: Some("current-run".to_string()),
                ..Default::default()
            },
        )
        .await;

    assert_eq!(
        registry
            .arm_retire_after_turn_if_owner(&key, token, "current-run")
            .await,
        InteractiveProcessRetireArmDisposition::AwaitingTurn
    );
    assert!(
        registry
            .disarm_retire_after_turn_if_owner(&key, token, "current-run")
            .await
    );
    registry
        .write_message(&key, "continue normally")
        .await
        .unwrap();
    assert_eq!(
        registry
            .complete_turn_if_owner(&key, token, "current-run")
            .await,
        InteractiveProcessTurnCompleteDisposition::KeepAlive
    );
    assert!(registry.has_process(&key).await);
}

#[tokio::test]
async fn arming_an_idle_entry_stages_retirement_until_exact_owner_commits() {
    let registry = InteractiveProcessRegistry::new();
    let key = InteractiveProcessKey::new("project", "retire-idle");
    let (stdin, _child) = create_test_stdin().await;
    let token = registry
        .register_with_metadata(
            key.clone(),
            stdin,
            InteractiveProcessMetadata {
                agent_run_id: Some("current-run".to_string()),
                ..Default::default()
            },
        )
        .await;

    assert_eq!(
        registry
            .complete_turn_if_owner(&key, token, "current-run")
            .await,
        InteractiveProcessTurnCompleteDisposition::KeepAlive
    );
    assert_eq!(
        registry
            .arm_retire_after_turn_if_owner(&key, token, "current-run")
            .await,
        InteractiveProcessRetireArmDisposition::IdleReady
    );
    assert!(registry.has_process(&key).await);
    assert_eq!(
        registry.state_for_test(&key).await,
        Some(InteractiveProcessState::Idle)
    );
    assert!(registry.retire_if_idle(&key).await.is_none());
    assert!(registry.has_process(&key).await);
    assert!(matches!(
        registry
            .write_message(&key, "must not queue behind staged retirement")
            .await,
        Err(InteractiveProcessWriteError::Retiring { .. })
    ));
    assert!(
        registry
            .disarm_retire_after_turn_if_owner(&key, token, "current-run")
            .await
    );
    assert!(registry.has_process(&key).await);
    registry
        .write_message(&key, "resume after failed staging")
        .await
        .unwrap();
    assert_eq!(
        registry.state_for_test(&key).await,
        Some(InteractiveProcessState::Active)
    );
}

#[tokio::test]
async fn stale_retirement_cannot_remove_a_replacement_entry() {
    let registry = InteractiveProcessRegistry::new();
    let key = InteractiveProcessKey::new("project", "retire-replacement");
    let (stdin_a, _child_a) = create_test_stdin().await;
    let stale_token = registry
        .register_with_metadata(
            key.clone(),
            stdin_a,
            InteractiveProcessMetadata {
                agent_run_id: Some("stale-run".to_string()),
                ..Default::default()
            },
        )
        .await;
    let (stdin_b, _child_b) = create_test_stdin().await;
    let current_token = registry
        .register_with_metadata(
            key.clone(),
            stdin_b,
            InteractiveProcessMetadata {
                agent_run_id: Some("current-run".to_string()),
                ..Default::default()
            },
        )
        .await;

    assert_eq!(
        registry
            .arm_retire_after_turn_if_owner(&key, stale_token, "stale-run")
            .await,
        InteractiveProcessRetireArmDisposition::Stale
    );
    assert_eq!(
        registry
            .complete_turn_if_owner(&key, stale_token, "stale-run")
            .await,
        InteractiveProcessTurnCompleteDisposition::Stale
    );
    assert!(registry.has_process(&key).await);
    assert_eq!(
        registry
            .arm_retire_after_turn_if_owner(&key, current_token, "current-run")
            .await,
        InteractiveProcessRetireArmDisposition::AwaitingTurn
    );
    assert_eq!(
        registry
            .complete_turn_if_owner(&key, current_token, "current-run")
            .await,
        InteractiveProcessTurnCompleteDisposition::RetireAfterTurn {
            pending_turns: Vec::new(),
        }
    );
    assert!(!registry.has_process(&key).await);
}

#[tokio::test]
async fn exact_post_commit_idle_retirement_removes_only_the_armed_owner() {
    let registry = InteractiveProcessRegistry::new();
    let key = InteractiveProcessKey::new("project", "retire-idle-post-commit");
    let (stdin, _child) = create_test_stdin().await;
    let token = registry
        .register_with_metadata(
            key.clone(),
            stdin,
            InteractiveProcessMetadata {
                agent_run_id: Some("planning-run".to_string()),
                ..Default::default()
            },
        )
        .await;
    assert!(registry.mark_idle_if_token(&key, token).await);
    assert_eq!(
        registry
            .arm_retire_after_turn_if_owner(&key, token, "planning-run")
            .await,
        InteractiveProcessRetireArmDisposition::IdleReady
    );

    let retired = registry
        .retire_armed_idle_if_owner(&key, token, "planning-run")
        .await
        .expect("the exact armed idle entry must retire after commit");

    assert_eq!(
        retired.metadata.agent_run_id.as_deref(),
        Some("planning-run")
    );
    assert!(!registry.has_process(&key).await);
}

#[tokio::test]
async fn stale_post_commit_cleanup_preserves_a_replacement_entry() {
    let registry = InteractiveProcessRegistry::new();
    let key = InteractiveProcessKey::new("project", "retire-idle-replacement");
    let (stdin_a, _child_a) = create_test_stdin().await;
    let stale_token = registry
        .register_with_metadata(
            key.clone(),
            stdin_a,
            InteractiveProcessMetadata {
                agent_run_id: Some("stale-run".to_string()),
                ..Default::default()
            },
        )
        .await;
    let (stdin_b, _child_b) = create_test_stdin().await;
    let current_token = registry
        .register_with_metadata(
            key.clone(),
            stdin_b,
            InteractiveProcessMetadata {
                agent_run_id: Some("current-run".to_string()),
                ..Default::default()
            },
        )
        .await;
    assert!(registry.mark_idle_if_token(&key, current_token).await);
    assert_eq!(
        registry
            .arm_retire_after_turn_if_owner(&key, stale_token, "stale-run")
            .await,
        InteractiveProcessRetireArmDisposition::Stale
    );

    assert!(registry
        .retire_armed_idle_if_owner(&key, stale_token, "stale-run")
        .await
        .is_none());
    assert!(registry.has_process(&key).await);
    assert_eq!(
        registry.state_for_test(&key).await,
        Some(InteractiveProcessState::Idle)
    );
}

#[tokio::test]
async fn registration_is_active_and_token_scoped_idle_transition_rejects_stale_stream() {
    let registry = InteractiveProcessRegistry::new();
    let key = InteractiveProcessKey::new("project", "idle-token");
    let (stdin_a, _child_a) = create_test_stdin().await;
    let stale = registry
        .register_with_metadata(key.clone(), stdin_a, InteractiveProcessMetadata::default())
        .await;
    let (stdin_b, _child_b) = create_test_stdin().await;
    let current = registry
        .register_with_metadata(key.clone(), stdin_b, InteractiveProcessMetadata::default())
        .await;

    assert_eq!(
        registry.state_for_test(&key).await,
        Some(InteractiveProcessState::Active)
    );
    assert!(!registry.mark_idle_if_token(&key, stale).await);
    assert!(registry.mark_idle_if_token(&key, current).await);
    assert_eq!(
        registry.state_for_test(&key).await,
        Some(InteractiveProcessState::Idle)
    );
}

#[tokio::test]
async fn stdin_write_wins_over_idle_retirement_and_keeps_launch_run_owner() {
    let registry = InteractiveProcessRegistry::new();
    let key = InteractiveProcessKey::new("project", "write-race");
    let (stdin, _child) = create_test_stdin().await;
    let token = registry
        .register_with_metadata(
            key.clone(),
            stdin,
            InteractiveProcessMetadata {
                agent_run_id: Some("planning-run".to_string()),
                ..Default::default()
            },
        )
        .await;
    assert!(registry.mark_idle_if_token(&key, token).await);

    registry
        .write_message(&key, "user follow-up")
        .await
        .unwrap();

    assert_eq!(
        registry.state_for_test(&key).await,
        Some(InteractiveProcessState::Active)
    );
    assert!(registry.retire_if_idle(&key).await.is_none());
    assert!(registry.has_process(&key).await);
}

#[tokio::test]
async fn idle_retirement_returns_exact_launch_run_owner() {
    let registry = InteractiveProcessRegistry::new();
    let key = InteractiveProcessKey::new("project", "retire-idle");
    let (stdin, _child) = create_test_stdin().await;
    let token = registry
        .register_with_metadata(
            key.clone(),
            stdin,
            InteractiveProcessMetadata {
                agent_run_id: Some("planning-run".to_string()),
                ..Default::default()
            },
        )
        .await;
    assert!(registry.mark_idle_if_token(&key, token).await);

    let retired = registry.retire_if_idle(&key).await.expect("idle entry");

    assert_eq!(
        retired.metadata.agent_run_id.as_deref(),
        Some("planning-run")
    );
    assert!(!registry.has_process(&key).await);
}

#[tokio::test]
async fn test_register_and_has_process() {
    let registry = InteractiveProcessRegistry::new();
    let key = InteractiveProcessKey::new("ideation", "session-123");
    assert!(!registry.has_process(&key).await);
    assert_eq!(registry.count().await, 0);
}

#[tokio::test]
async fn test_remove_nonexistent_returns_none() {
    let registry = InteractiveProcessRegistry::new();
    let key = InteractiveProcessKey::new("ideation", "session-123");
    assert!(registry.remove(&key).await.is_none());
}

#[tokio::test]
async fn stale_stream_exit_does_not_remove_fresh_ipr_entry() {
    let registry = InteractiveProcessRegistry::new();
    let key = InteractiveProcessKey::new("project", "replacement-race");
    let (stdin_a, _child_a) = create_test_stdin().await;
    let token_a = registry
        .register_with_metadata(key.clone(), stdin_a, InteractiveProcessMetadata::default())
        .await;
    let (stdin_b, _child_b) = create_test_stdin().await;
    let token_b = registry
        .register_with_metadata(key.clone(), stdin_b, InteractiveProcessMetadata::default())
        .await;

    assert!(
        registry.remove_if_token(&key, token_a).await.is_none(),
        "old stream cleanup must not remove the replacement entry"
    );
    assert!(registry.has_process(&key).await);
    assert!(
        registry.remove_if_token(&key, token_b).await.is_some(),
        "the current stream cleanup must still remove its own entry"
    );
}

#[tokio::test]
async fn test_write_message_no_process_returns_error() {
    let registry = InteractiveProcessRegistry::new();
    let key = InteractiveProcessKey::new("ideation", "session-123");
    let result = registry.write_message(&key, "hello").await;
    assert!(matches!(
        result,
        Err(InteractiveProcessWriteError::Missing { .. })
    ));
}

#[cfg(unix)]
#[tokio::test]
async fn stdin_write_error_exposes_original_token_and_cannot_remove_replacement() {
    let registry = InteractiveProcessRegistry::new();
    let key = InteractiveProcessKey::new("project", "write-error-replacement");
    let (stdin, mut child) = create_test_stdin().await;
    let failed_token = registry
        .register_with_metadata(key.clone(), stdin, InteractiveProcessMetadata::default())
        .await;
    child.kill().await.expect("terminate stdin fixture");
    child.wait().await.expect("reap stdin fixture");

    let error = registry
        .write_message(&key, "must fail after process exit")
        .await
        .expect_err("closed stdin must report a write error");
    assert!(matches!(
        error,
        InteractiveProcessWriteError::StdinIo { token, .. } if token == failed_token
    ));

    let (replacement_stdin, _replacement_child) = create_test_stdin().await;
    let replacement_token = registry
        .register_with_metadata(
            key.clone(),
            replacement_stdin,
            InteractiveProcessMetadata {
                agent_run_id: Some("replacement-run".to_string()),
                ..Default::default()
            },
        )
        .await;
    assert!(
        registry.remove_if_token(&key, failed_token).await.is_none(),
        "I/O cleanup for the failed owner must not remove a replacement"
    );
    assert_eq!(
        registry
            .capture_owner(&key)
            .await
            .expect("replacement must remain registered")
            .token,
        replacement_token,
        "I/O cleanup for the failed owner must retain the replacement"
    );
    assert!(
        registry
            .remove_if_token(&key, replacement_token)
            .await
            .is_some(),
        "the current owner remains removable by its own token"
    );
}

#[tokio::test]
async fn test_register_returns_completion_signal() {
    let (stdin, _child) = create_test_stdin().await;
    let registry = InteractiveProcessRegistry::new();
    let key = InteractiveProcessKey::new("task", "task-789");

    let signal = registry.register(key.clone(), stdin).await;
    // Signal is live and shared with the entry
    let fetched = registry.get_completion_signal(&key).await.unwrap();
    assert!(Arc::ptr_eq(&signal, &fetched));
}

#[tokio::test]
async fn test_register_with_metadata_persists_harness_metadata() {
    let (stdin, _child) = create_test_stdin().await;
    let registry = InteractiveProcessRegistry::new();
    let key = InteractiveProcessKey::new("ideation", "session-xyz");

    registry
        .register_with_metadata(
            key.clone(),
            stdin,
            InteractiveProcessMetadata {
                agent_run_id: None,
                harness: Some(AgentHarnessKind::Codex),
                provider_session_id: Some("thread-123".to_string()),
                persona_id: None,
                persona_content_hash: None,
                agent_name: Some("ralphx-ideation".to_string()),
                agent_profile: Some("plan".to_string()),
            },
        )
        .await;

    let metadata = registry.get_metadata(&key).await.unwrap();
    assert_eq!(metadata.harness, Some(AgentHarnessKind::Codex));
    assert_eq!(metadata.provider_session_id.as_deref(), Some("thread-123"));
    assert_eq!(metadata.agent_name.as_deref(), Some("ralphx-ideation"));
    assert_eq!(metadata.agent_profile.as_deref(), Some("plan"));
}

#[tokio::test]
async fn test_get_completion_signal_none_if_not_registered() {
    let registry = InteractiveProcessRegistry::new();
    let key = InteractiveProcessKey::new("ideation", "session-999");
    assert!(registry.get_completion_signal(&key).await.is_none());
}

#[tokio::test]
async fn test_completion_signal_survives_remove() {
    // The Arc<Notify> should remain usable after the process is removed,
    // so any awaiter that cloned it before removal can still be notified.
    let (stdin, _child) = create_test_stdin().await;
    let registry = InteractiveProcessRegistry::new();
    let key = InteractiveProcessKey::new("merge", "merge-1");

    let signal = registry.register(key.clone(), stdin).await;
    let _removed = registry.remove(&key).await;

    // Notifying after removal should not panic
    signal.notify_waiters();
    // Signal for key is gone from registry
    assert!(registry.get_completion_signal(&key).await.is_none());
}

#[tokio::test]
async fn test_register_and_write_message() {
    // Create a real pipe to test write
    let (stdin, _child) = create_test_stdin().await;
    let registry = InteractiveProcessRegistry::new();
    let key = InteractiveProcessKey::new("task", "task-456");

    registry.register(key.clone(), stdin).await;
    assert!(registry.has_process(&key).await);
    assert_eq!(registry.count().await, 1);

    // Write should succeed
    let result = registry.write_message(&key, "test message").await;
    assert!(result.is_ok());

    // Remove
    let removed = registry.remove(&key).await;
    assert!(removed.is_some());
    assert!(!registry.has_process(&key).await);
}

#[tokio::test]
async fn test_dump_state_empty() {
    let registry = InteractiveProcessRegistry::new();
    let keys = registry.dump_state().await;
    assert!(keys.is_empty());
}

#[tokio::test]
async fn test_dump_state_returns_all_keys() {
    let (stdin1, _child1) = create_test_stdin().await;
    let (stdin2, _child2) = create_test_stdin().await;
    let registry = InteractiveProcessRegistry::new();

    let key1 = InteractiveProcessKey::new("ideation", "session-1");
    let key2 = InteractiveProcessKey::new("task_execution", "task-2");
    registry.register(key1.clone(), stdin1).await;
    registry.register(key2.clone(), stdin2).await;

    let keys = registry.dump_state().await;
    assert_eq!(keys.len(), 2);
    assert!(keys.contains(&key1));
    assert!(keys.contains(&key2));
}

#[tokio::test]
async fn test_clear_removes_all() {
    let (stdin1, _child1) = create_test_stdin().await;
    let (stdin2, _child2) = create_test_stdin().await;
    let registry = InteractiveProcessRegistry::new();

    registry
        .register(InteractiveProcessKey::new("a", "1"), stdin1)
        .await;
    registry
        .register(InteractiveProcessKey::new("b", "2"), stdin2)
        .await;
    assert_eq!(registry.count().await, 2);

    registry.clear().await;
    assert_eq!(registry.count().await, 0);
}

/// Helper: create a real stdin pipe via `cat` subprocess for testing writes.
/// Serializes fixture pipe creation + spawn so a concurrently forked `cat`
/// cannot inherit another test's pipe read end before CLOEXEC is applied.
/// Without this, closed-stdin write tests miss their EPIPE when enough
/// spawning tests run in parallel.
static SPAWN_GUARD: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

async fn create_test_stdin() -> (ChildStdin, tokio::process::Child) {
    let _guard = SPAWN_GUARD.lock().await;
    let mut child = tokio::process::Command::new("cat")
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .expect("failed to spawn cat");
    let stdin = child.stdin.take().expect("no stdin");
    (stdin, child)
}

async fn create_observable_test_stdin() -> (ChildStdin, tokio::process::Child) {
    let _guard = SPAWN_GUARD.lock().await;
    let mut child = tokio::process::Command::new("cat")
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .spawn()
        .expect("failed to spawn observable cat");
    let stdin = child.stdin.take().expect("no stdin");
    (stdin, child)
}
