#[test]
fn merged_suite_requires_nextest() {
    if std::env::var_os("NEXTEST").is_none() {
        panic!(
            "merged integration suites must be run with cargo nextest; see .claude/rules/rust-test-execution.md"
        );
    }
}

mod tauri_events_tests {
    use std::sync::{Arc, Mutex};
    use std::time::Duration;

    use ralphx_events::catalog::AGENT_RUN_COMPLETED;
    use ralphx_lib::shell::agent_completion_event_runtime::create_agent_completion_event_runtime;
    use ralphx_lib::testing::create_mock_app;
    use serde_json::json;
    use tauri::Listener;

    /// Anti-fail-open gate: proves that the production event sink delivers to both a real Tauri
    /// listener and the InternalEventBus. The test fails if the Tauri half is absent (a noop
    /// sink would still deliver to the bus but would not fire the listener).
    #[tokio::test]
    async fn tauri_events_wiring_gate() {
        let app = create_mock_app();

        let received_tauri = Arc::new(Mutex::new(false));
        let flag = Arc::clone(&received_tauri);
        app.handle().listen_any(AGENT_RUN_COMPLETED, move |_| {
            *flag.lock().expect("tauri flag lock") = true;
        });

        let runtime = create_agent_completion_event_runtime(app.handle().clone());
        let mut bus_sub = runtime.bus.subscribe();

        let payload = json!({
            "conversation_id": "11111111-1111-1111-1111-111111111111",
            "context_type": "project",
            "context_id": "project-1",
            "run_id": "22222222-2222-2222-2222-222222222222"
        });
        runtime.sink.emit(AGENT_RUN_COMPLETED, payload.clone());

        for _ in 0..40u8 {
            if *received_tauri.lock().expect("tauri flag lock") {
                break;
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }

        assert!(
            *received_tauri.lock().expect("tauri flag lock"),
            "Tauri listener must receive events emitted through the production sink — \
             the sink composition must contain TauriEventSink (shell/event_sink.rs); \
             this gate fails if the Tauri half is absent or replaced with a noop"
        );

        let envelope = tokio::time::timeout(Duration::from_millis(200), bus_sub.recv())
            .await
            .expect("InternalEventBus delivery must not time out")
            .expect("bus recv must succeed");
        assert_eq!(
            envelope.name, AGENT_RUN_COMPLETED,
            "InternalEventBus must carry the same event as the Tauri listener"
        );
    }

    /// Dual-AppState singleton sharing: build_http_app_state must share ALL coordinating Arcs
    /// with the Tauri AppState graph. A missing share causes silent state splits between
    /// MCP/HTTP handlers and Tauri commands.
    ///
    /// Live field count derived from shell/runtime_wiring.rs::build_http_app_state.
    #[tokio::test]
    async fn build_http_app_state_shares_all_arcs_with_tauri_state() {
        use ralphx_events::EventEnvelope;
        use ralphx_lib::application::AppState;
        use ralphx_lib::shell::runtime_wiring::build_http_app_state;
        use serde_json::json;

        let app = create_mock_app();
        let tauri_state = AppState::new_sqlite_test();
        let http_state = build_http_app_state(&tauri_state, app.handle().clone())
            .expect("build_http_app_state must succeed");

        // 1. Shared SQLite connection
        assert!(
            Arc::ptr_eq(tauri_state.db.inner(), http_state.db.inner()),
            "db: shared SQLite connection must be the same Arc"
        );
        // 2. Event sink
        assert!(
            Arc::ptr_eq(&tauri_state.events, &http_state.events),
            "events: production event sink must be shared"
        );
        // 3. Window focus state
        assert!(
            Arc::ptr_eq(&tauri_state.window_focus_state, &http_state.window_focus_state),
            "window_focus_state"
        );
        // 4. Question state
        assert!(
            Arc::ptr_eq(&tauri_state.question_state, &http_state.question_state),
            "question_state"
        );
        // 5. Permission state
        assert!(
            Arc::ptr_eq(&tauri_state.permission_state, &http_state.permission_state),
            "permission_state"
        );
        // 6. Message queue
        assert!(
            Arc::ptr_eq(&tauri_state.message_queue, &http_state.message_queue),
            "message_queue"
        );
        // 7. Queued message repo
        assert!(
            Arc::ptr_eq(&tauri_state.queued_message_repo, &http_state.queued_message_repo),
            "queued_message_repo"
        );
        // 8. Interactive process registry
        assert!(
            Arc::ptr_eq(
                &tauri_state.interactive_process_registry,
                &http_state.interactive_process_registry
            ),
            "interactive_process_registry"
        );
        // 9. GitHub service — None in test factory; both must agree
        assert!(
            tauri_state.github_service.is_none(),
            "github_service must be None in new_sqlite_test"
        );
        assert!(
            http_state.github_service.is_none(),
            "http github_service must be None (shared from tauri state)"
        );
        // 10. PR poller registry
        assert!(
            Arc::ptr_eq(&tauri_state.pr_poller_registry, &http_state.pr_poller_registry),
            "pr_poller_registry"
        );
        // 11. Streaming state cache — uses arc_ptr accessor (inner Arc<Mutex<...>>)
        assert_eq!(
            tauri_state.streaming_state_cache.arc_ptr(),
            http_state.streaming_state_cache.arc_ptr(),
            "streaming_state_cache: inner allocation must be shared"
        );
        // 12. Webhook publisher — None in test factory; both must agree
        assert!(
            tauri_state.webhook_publisher.is_none(),
            "webhook_publisher must be None in new_sqlite_test"
        );
        assert!(
            http_state.webhook_publisher.is_none(),
            "http webhook_publisher must be None"
        );
        // 13. Session merge locks
        assert!(
            Arc::ptr_eq(&tauri_state.session_merge_locks, &http_state.session_merge_locks),
            "session_merge_locks"
        );
        // 14. Notification service cache (pub(crate) — accessed via arc_ptr accessor)
        assert_eq!(
            tauri_state.notification_service_cache_arc_ptr(),
            http_state.notification_service_cache_arc_ptr(),
            "notification_service_cache: Arc must be shared"
        );
        // 15. Agent capability gate
        assert!(
            Arc::ptr_eq(&tauri_state.agent_capability_gate, &http_state.agent_capability_gate),
            "agent_capability_gate"
        );
        // 16. Delegation park repo
        assert!(
            Arc::ptr_eq(&tauri_state.delegation_park_repo, &http_state.delegation_park_repo),
            "delegation_park_repo"
        );
        // 17. Managed team
        assert!(
            Arc::ptr_eq(&tauri_state.managed_team, &http_state.managed_team),
            "managed_team"
        );
        // 18. Repair publish continuation (pub(crate) — accessed via arc_ptr accessor)
        assert_eq!(
            tauri_state.repair_publish_continuation_arc_ptr(),
            http_state.repair_publish_continuation_arc_ptr(),
            "agent_workspace_repair_publish_continuation: Arc must be shared"
        );
        // 19. PR-fix review publish resumer (pub(crate) — accessed via arc_ptr accessor)
        assert_eq!(
            tauri_state.pr_fix_review_publish_resumer_arc_ptr(),
            http_state.pr_fix_review_publish_resumer_arc_ptr(),
            "agent_workspace_pr_fix_review_publish_resumer: Arc must be shared"
        );
        // 20. Startup coordinator
        assert!(
            Arc::ptr_eq(&tauri_state.startup_coordinator, &http_state.startup_coordinator),
            "startup_coordinator"
        );
        // 21. Plan verification locks
        assert!(
            Arc::ptr_eq(
                &tauri_state.plan_verification_locks,
                &http_state.plan_verification_locks
            ),
            "plan_verification_locks"
        );
        // 22. Plan verification admissions
        assert!(
            Arc::ptr_eq(
                &tauri_state.plan_verification_admissions,
                &http_state.plan_verification_admissions
            ),
            "plan_verification_admissions"
        );
        // 23. InternalEventBus: shared broadcast channel — verify via publish/subscribe
        let mut bus_sub = http_state.internal_event_bus.subscribe();
        let probe = EventEnvelope::new("test:sharing_probe", json!({}));
        tauri_state
            .internal_event_bus
            .publish(probe)
            .expect("publish to shared bus must succeed (at least one subscriber)");
        bus_sub
            .try_recv()
            .expect("subscriber on http_state bus must receive event published via tauri_state bus");
        // 24. AppPaths: value type (no Arc::ptr_eq); verify value equality
        assert_eq!(
            format!("{:?}", tauri_state.app_paths),
            format!("{:?}", http_state.app_paths),
            "app_paths value must be equal between the two AppState graphs"
        );
    }
}
