use super::*;

#[test]
fn merge_recovery_metadata_new_creates_empty() {
    let meta = MergeRecoveryMetadata::new();
    assert_eq!(meta.version, 1);
    assert!(meta.events.is_empty());
    assert_eq!(meta.last_state, MergeRecoveryState::Succeeded);
}

#[test]
fn merge_recovery_metadata_default_works() {
    let meta = MergeRecoveryMetadata::default();
    assert_eq!(meta.version, 1);
    assert!(meta.events.is_empty());
}

#[test]
fn review_scope_metadata_roundtrip_through_task_metadata() {
    let meta = ReviewScopeMetadata::new(
        vec!["src-tauri/src/http_server".to_string()],
        vec!["ralphx.yaml".to_string()],
        Some("plan_correction".to_string()),
        Some("Needed a small adjacent boundary expansion".to_string()),
    );

    let json = meta
        .update_task_metadata(Some(r#"{"other":"value"}"#))
        .unwrap();
    let restored = ReviewScopeMetadata::from_task_metadata(Some(&json))
        .unwrap()
        .expect("review_scope should exist");

    assert_eq!(restored.planned_paths, vec!["src-tauri/src/http_server"]);
    assert_eq!(restored.reviewed_out_of_scope_files, vec!["ralphx.yaml"]);
    assert_eq!(
        restored.drift_classification.as_deref(),
        Some("plan_correction")
    );
    assert_eq!(
        restored.drift_notes.as_deref(),
        Some("Needed a small adjacent boundary expansion")
    );
}

#[test]
fn review_scope_metadata_clear_removes_only_review_scope_key() {
    let meta = ReviewScopeMetadata::new(
        vec!["src".to_string()],
        vec!["other/file.rs".to_string()],
        Some("adjacent_scope_expansion".to_string()),
        None,
    );
    let json = meta
        .update_task_metadata(Some(r#"{"keep":"value"}"#))
        .unwrap();

    let cleared = ReviewScopeMetadata::clear_from_task_metadata(Some(&json))
        .unwrap()
        .expect("metadata should remain");
    let value: serde_json::Value = serde_json::from_str(&cleared).unwrap();

    assert_eq!(value["keep"], "value");
    assert!(value.get("review_scope").is_none());
}

#[test]
fn merge_recovery_metadata_max_events_constant() {
    assert_eq!(MergeRecoveryMetadata::MAX_EVENTS, 50);
}

#[test]
fn merge_recovery_event_new_sets_defaults() {
    let event = MergeRecoveryEvent::new(
        MergeRecoveryEventKind::Deferred,
        MergeRecoverySource::System,
        MergeRecoveryReasonCode::TargetBranchBusy,
        "Merge deferred",
    );

    assert_eq!(event.kind, MergeRecoveryEventKind::Deferred);
    assert_eq!(event.source, MergeRecoverySource::System);
    assert_eq!(event.reason_code, MergeRecoveryReasonCode::TargetBranchBusy);
    assert_eq!(event.message, "Merge deferred");
    assert!(event.target_branch.is_none());
    assert!(event.source_branch.is_none());
    assert!(event.blocking_task_id.is_none());
    assert!(event.attempt.is_none());
}

#[test]
fn merge_recovery_event_builder_methods() {
    let task_id = TaskId::new();
    let event = MergeRecoveryEvent::new(
        MergeRecoveryEventKind::AutoRetryTriggered,
        MergeRecoverySource::Auto,
        MergeRecoveryReasonCode::TargetBranchBusy,
        "Auto retry",
    )
    .with_target_branch("main")
    .with_source_branch("task-branch")
    .with_blocking_task(task_id.clone())
    .with_attempt(2);

    assert_eq!(event.target_branch, Some("main".to_string()));
    assert_eq!(event.source_branch, Some("task-branch".to_string()));
    assert_eq!(event.blocking_task_id, Some(task_id));
    assert_eq!(event.attempt, Some(2));
}

#[test]
fn merge_recovery_event_serializes_to_json() {
    let event = MergeRecoveryEvent::new(
        MergeRecoveryEventKind::Deferred,
        MergeRecoverySource::System,
        MergeRecoveryReasonCode::TargetBranchBusy,
        "Merge deferred",
    )
    .with_target_branch("main")
    .with_attempt(1);

    let json = serde_json::to_string(&event).unwrap();
    assert!(json.contains("\"kind\":\"deferred\""));
    assert!(json.contains("\"source\":\"system\""));
    assert!(json.contains("\"reason_code\":\"target_branch_busy\""));
    assert!(json.contains("\"target_branch\":\"main\""));
    assert!(json.contains("\"attempt\":1"));
}

#[test]
fn merge_recovery_event_deserializes_from_json() {
    let json = r#"{
        "at": "2026-02-11T10:00:00Z",
        "kind": "deferred",
        "source": "system",
        "reason_code": "target_branch_busy",
        "message": "Merge deferred",
        "target_branch": "main",
        "attempt": 1
    }"#;

    let event: MergeRecoveryEvent = serde_json::from_str(json).unwrap();
    assert_eq!(event.kind, MergeRecoveryEventKind::Deferred);
    assert_eq!(event.source, MergeRecoverySource::System);
    assert_eq!(event.reason_code, MergeRecoveryReasonCode::TargetBranchBusy);
    assert_eq!(event.message, "Merge deferred");
    assert_eq!(event.target_branch, Some("main".to_string()));
    assert_eq!(event.attempt, Some(1));
}

#[test]
fn merge_recovery_event_skips_serializing_none_fields() {
    let event = MergeRecoveryEvent::new(
        MergeRecoveryEventKind::Deferred,
        MergeRecoverySource::System,
        MergeRecoveryReasonCode::TargetBranchBusy,
        "Merge deferred",
    );

    let json = serde_json::to_string(&event).unwrap();
    assert!(!json.contains("\"target_branch\""));
    assert!(!json.contains("\"source_branch\""));
    assert!(!json.contains("\"blocking_task_id\""));
    assert!(!json.contains("\"attempt\""));
}

#[test]
fn merge_recovery_metadata_serializes_to_json() {
    let mut meta = MergeRecoveryMetadata::new();
    meta.events.push(MergeRecoveryEvent::new(
        MergeRecoveryEventKind::Deferred,
        MergeRecoverySource::System,
        MergeRecoveryReasonCode::TargetBranchBusy,
        "Deferred",
    ));
    meta.last_state = MergeRecoveryState::Deferred;

    let json = serde_json::to_string(&meta).unwrap();
    assert!(json.contains("\"version\":1"));
    assert!(json.contains("\"events\":["));
    assert!(json.contains("\"last_state\":\"deferred\""));
}

#[test]
fn merge_recovery_metadata_deserializes_from_json() {
    let json = r#"{
        "version": 1,
        "events": [
            {
                "at": "2026-02-11T10:00:00Z",
                "kind": "deferred",
                "source": "system",
                "reason_code": "target_branch_busy",
                "message": "Deferred"
            }
        ],
        "last_state": "deferred"
    }"#;

    let meta: MergeRecoveryMetadata = serde_json::from_str(json).unwrap();
    assert_eq!(meta.version, 1);
    assert_eq!(meta.events.len(), 1);
    assert_eq!(meta.last_state, MergeRecoveryState::Deferred);
}

#[test]
fn merge_recovery_metadata_roundtrip() {
    let mut meta = MergeRecoveryMetadata::new();
    meta.events.push(
        MergeRecoveryEvent::new(
            MergeRecoveryEventKind::Deferred,
            MergeRecoverySource::System,
            MergeRecoveryReasonCode::TargetBranchBusy,
            "Deferred",
        )
        .with_target_branch("main")
        .with_attempt(1),
    );
    meta.last_state = MergeRecoveryState::Deferred;

    let json = serde_json::to_string(&meta).unwrap();
    let restored: MergeRecoveryMetadata = serde_json::from_str(&json).unwrap();

    assert_eq!(meta, restored);
}

#[test]
fn merge_recovery_event_kind_serialization() {
    let kinds = [
        (MergeRecoveryEventKind::Deferred, "deferred"),
        (
            MergeRecoveryEventKind::AutoRetryTriggered,
            "auto_retry_triggered",
        ),
        (MergeRecoveryEventKind::AttemptStarted, "attempt_started"),
        (MergeRecoveryEventKind::AttemptFailed, "attempt_failed"),
        (
            MergeRecoveryEventKind::AttemptSucceeded,
            "attempt_succeeded",
        ),
        (MergeRecoveryEventKind::ManualRetry, "manual_retry"),
        (
            MergeRecoveryEventKind::MainMergeDeferred,
            "main_merge_deferred",
        ),
        (MergeRecoveryEventKind::MainMergeRetry, "main_merge_retry"),
    ];

    for (kind, expected) in &kinds {
        let json = serde_json::to_string(kind).unwrap();
        assert_eq!(json, format!("\"{}\"", expected));
    }
}

#[test]
fn merge_recovery_source_serialization() {
    let sources = [
        (MergeRecoverySource::System, "system"),
        (MergeRecoverySource::Auto, "auto"),
        (MergeRecoverySource::User, "user"),
    ];

    for (source, expected) in &sources {
        let json = serde_json::to_string(source).unwrap();
        assert_eq!(json, format!("\"{}\"", expected));
    }
}

#[test]
fn merge_recovery_reason_code_serialization() {
    let codes = [
        (
            MergeRecoveryReasonCode::TargetBranchBusy,
            "target_branch_busy",
        ),
        (MergeRecoveryReasonCode::GitError, "git_error"),
        (
            MergeRecoveryReasonCode::ValidationFailed,
            "validation_failed",
        ),
        (MergeRecoveryReasonCode::BranchNotFound, "branch_not_found"),
        (MergeRecoveryReasonCode::AgentsRunning, "agents_running"),
        (MergeRecoveryReasonCode::DeferredTimeout, "deferred_timeout"),
        (
            MergeRecoveryReasonCode::ProviderRateLimited,
            "provider_rate_limited",
        ),
        (MergeRecoveryReasonCode::Unknown, "unknown"),
    ];

    for (code, expected) in &codes {
        let json = serde_json::to_string(code).unwrap();
        assert_eq!(json, format!("\"{}\"", expected));
    }
}

#[test]
fn merge_recovery_state_serialization() {
    let states = [
        (MergeRecoveryState::Deferred, "deferred"),
        (MergeRecoveryState::Retrying, "retrying"),
        (MergeRecoveryState::Failed, "failed"),
        (MergeRecoveryState::Succeeded, "succeeded"),
        (MergeRecoveryState::RateLimited, "rate_limited"),
    ];

    for (state, expected) in &states {
        let json = serde_json::to_string(state).unwrap();
        assert_eq!(json, format!("\"{}\"", expected));
    }
}

// ===== Helper Function Tests =====

#[test]
fn append_event_adds_to_log() {
    let mut meta = MergeRecoveryMetadata::new();
    let event = MergeRecoveryEvent::new(
        MergeRecoveryEventKind::Deferred,
        MergeRecoverySource::System,
        MergeRecoveryReasonCode::TargetBranchBusy,
        "Deferred",
    );

    meta.append_event(event.clone());

    assert_eq!(meta.events.len(), 1);
    assert_eq!(meta.events[0].message, "Deferred");
}

#[test]
fn append_event_with_state_updates_both() {
    let mut meta = MergeRecoveryMetadata::new();
    let event = MergeRecoveryEvent::new(
        MergeRecoveryEventKind::Deferred,
        MergeRecoverySource::System,
        MergeRecoveryReasonCode::TargetBranchBusy,
        "Deferred",
    );

    meta.append_event_with_state(event, MergeRecoveryState::Deferred);

    assert_eq!(meta.events.len(), 1);
    assert_eq!(meta.last_state, MergeRecoveryState::Deferred);
}

#[test]
fn append_event_trims_when_exceeds_max() {
    let mut meta = MergeRecoveryMetadata::new();

    // Add MAX_EVENTS + 5 events
    for i in 0..(MergeRecoveryMetadata::MAX_EVENTS + 5) {
        let event = MergeRecoveryEvent::new(
            MergeRecoveryEventKind::Deferred,
            MergeRecoverySource::System,
            MergeRecoveryReasonCode::TargetBranchBusy,
            format!("Event {}", i),
        );
        meta.append_event(event);
    }

    // Should only keep MAX_EVENTS
    assert_eq!(meta.events.len(), MergeRecoveryMetadata::MAX_EVENTS);

    // Oldest events should be trimmed, newest kept
    // First event should now be event #5 (0-4 trimmed)
    assert_eq!(meta.events[0].message, "Event 5");
    assert_eq!(
        meta.events[MergeRecoveryMetadata::MAX_EVENTS - 1].message,
        format!("Event {}", MergeRecoveryMetadata::MAX_EVENTS + 4)
    );
}

#[test]
fn append_event_preserves_chronological_order() {
    let mut meta = MergeRecoveryMetadata::new();

    for i in 0..10 {
        let event = MergeRecoveryEvent::new(
            MergeRecoveryEventKind::Deferred,
            MergeRecoverySource::System,
            MergeRecoveryReasonCode::TargetBranchBusy,
            format!("Event {}", i),
        );
        meta.append_event(event);
    }

    // Events should be in order
    for i in 0..10 {
        assert_eq!(meta.events[i].message, format!("Event {}", i));
    }
}

#[test]
fn from_task_metadata_with_no_metadata() {
    let result = MergeRecoveryMetadata::from_task_metadata(None).unwrap();
    assert!(result.is_none());
}

#[test]
fn from_task_metadata_with_empty_json() {
    let result = MergeRecoveryMetadata::from_task_metadata(Some("{}")).unwrap();
    assert!(result.is_none());
}

#[test]
fn from_task_metadata_with_other_keys_only() {
    let json = r#"{"error": "some error", "source_branch": "task-branch"}"#;
    let result = MergeRecoveryMetadata::from_task_metadata(Some(json)).unwrap();
    assert!(result.is_none());
}

#[test]
fn from_task_metadata_with_valid_recovery_data() {
    let json = r#"{
        "error": "some error",
        "merge_recovery": {
            "version": 1,
            "events": [
                {
                    "at": "2026-02-11T10:00:00Z",
                    "kind": "deferred",
                    "source": "system",
                    "reason_code": "target_branch_busy",
                    "message": "Deferred"
                }
            ],
            "last_state": "deferred"
        }
    }"#;

    let result = MergeRecoveryMetadata::from_task_metadata(Some(json)).unwrap();
    assert!(result.is_some());

    let meta = result.unwrap();
    assert_eq!(meta.version, 1);
    assert_eq!(meta.events.len(), 1);
    assert_eq!(meta.last_state, MergeRecoveryState::Deferred);
    assert_eq!(meta.events[0].message, "Deferred");
}

#[test]
fn from_task_metadata_with_invalid_json() {
    let result = MergeRecoveryMetadata::from_task_metadata(Some("not json"));
    assert!(result.is_err());
}

#[test]
fn from_task_metadata_with_invalid_recovery_structure() {
    let json = r#"{"merge_recovery": "not an object"}"#;
    let result = MergeRecoveryMetadata::from_task_metadata(Some(json));
    assert!(result.is_err());
}

#[test]
fn update_task_metadata_creates_new_object() {
    let mut meta = MergeRecoveryMetadata::new();
    meta.append_event_with_state(
        MergeRecoveryEvent::new(
            MergeRecoveryEventKind::Deferred,
            MergeRecoverySource::System,
            MergeRecoveryReasonCode::TargetBranchBusy,
            "Deferred",
        ),
        MergeRecoveryState::Deferred,
    );

    let result = meta.update_task_metadata(None).unwrap();

    let value: serde_json::Value = serde_json::from_str(&result).unwrap();
    assert!(value.get("merge_recovery").is_some());
    assert_eq!(value["merge_recovery"]["version"], 1);
    assert_eq!(
        value["merge_recovery"]["events"].as_array().unwrap().len(),
        1
    );
    assert_eq!(value["merge_recovery"]["last_state"], "deferred");
}

#[test]
fn update_task_metadata_preserves_existing_keys() {
    let mut meta = MergeRecoveryMetadata::new();
    meta.append_event_with_state(
        MergeRecoveryEvent::new(
            MergeRecoveryEventKind::Deferred,
            MergeRecoverySource::System,
            MergeRecoveryReasonCode::TargetBranchBusy,
            "Deferred",
        ),
        MergeRecoveryState::Deferred,
    );

    let existing = r#"{"error": "some error", "source_branch": "task-branch"}"#;
    let result = meta.update_task_metadata(Some(existing)).unwrap();

    let value: serde_json::Value = serde_json::from_str(&result).unwrap();
    assert_eq!(value["error"], "some error");
    assert_eq!(value["source_branch"], "task-branch");
    assert!(value.get("merge_recovery").is_some());
}

#[test]
fn update_task_metadata_overwrites_existing_recovery() {
    let mut meta = MergeRecoveryMetadata::new();
    meta.append_event_with_state(
        MergeRecoveryEvent::new(
            MergeRecoveryEventKind::AutoRetryTriggered,
            MergeRecoverySource::Auto,
            MergeRecoveryReasonCode::TargetBranchBusy,
            "Retry",
        ),
        MergeRecoveryState::Retrying,
    );

    let existing = r#"{
        "merge_recovery": {
            "version": 1,
            "events": [],
            "last_state": "succeeded"
        }
    }"#;

    let result = meta.update_task_metadata(Some(existing)).unwrap();

    let value: serde_json::Value = serde_json::from_str(&result).unwrap();
    assert_eq!(
        value["merge_recovery"]["events"].as_array().unwrap().len(),
        1
    );
    assert_eq!(value["merge_recovery"]["last_state"], "retrying");
}

#[test]
fn update_task_metadata_handles_malformed_existing() {
    let mut meta = MergeRecoveryMetadata::new();
    meta.append_event_with_state(
        MergeRecoveryEvent::new(
            MergeRecoveryEventKind::Deferred,
            MergeRecoverySource::System,
            MergeRecoveryReasonCode::TargetBranchBusy,
            "Deferred",
        ),
        MergeRecoveryState::Deferred,
    );

    // Malformed JSON should be replaced with empty object
    let result = meta.update_task_metadata(Some("not json")).unwrap();

    let value: serde_json::Value = serde_json::from_str(&result).unwrap();
    assert!(value.get("merge_recovery").is_some());
    // Only merge_recovery key should exist (malformed JSON was discarded)
    assert_eq!(value.as_object().unwrap().len(), 1);
}

#[test]
fn roundtrip_from_and_update_task_metadata() {
    // Create metadata with events
    let mut original = MergeRecoveryMetadata::new();
    original.append_event_with_state(
        MergeRecoveryEvent::new(
            MergeRecoveryEventKind::Deferred,
            MergeRecoverySource::System,
            MergeRecoveryReasonCode::TargetBranchBusy,
            "Deferred",
        )
        .with_target_branch("main")
        .with_attempt(1),
        MergeRecoveryState::Deferred,
    );

    // Serialize to task metadata JSON
    let json_str = original.update_task_metadata(None).unwrap();

    // Parse back from task metadata JSON
    let parsed = MergeRecoveryMetadata::from_task_metadata(Some(&json_str))
        .unwrap()
        .unwrap();

    // Should match original
    assert_eq!(parsed.version, original.version);
    assert_eq!(parsed.events.len(), original.events.len());
    assert_eq!(parsed.last_state, original.last_state);
    assert_eq!(parsed.events[0].message, "Deferred");
    assert_eq!(parsed.events[0].target_branch, Some("main".to_string()));
    assert_eq!(parsed.events[0].attempt, Some(1));
}

#[test]
fn merge_recovery_reason_code_branch_not_found_serializes() {
    let code = MergeRecoveryReasonCode::BranchNotFound;
    let json = serde_json::to_string(&code).unwrap();
    assert_eq!(json, "\"branch_not_found\"");

    let parsed: MergeRecoveryReasonCode = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, MergeRecoveryReasonCode::BranchNotFound);
}

// ===== Rate limit metadata tests =====

#[test]
fn provider_rate_limited_reason_code_serializes() {
    let code = MergeRecoveryReasonCode::ProviderRateLimited;
    let json = serde_json::to_string(&code).unwrap();
    assert_eq!(json, "\"provider_rate_limited\"");

    let parsed: MergeRecoveryReasonCode = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, MergeRecoveryReasonCode::ProviderRateLimited);
}

#[test]
fn rate_limited_state_serializes() {
    let state = MergeRecoveryState::RateLimited;
    let json = serde_json::to_string(&state).unwrap();
    assert_eq!(json, "\"rate_limited\"");

    let parsed: MergeRecoveryState = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, MergeRecoveryState::RateLimited);
}

#[test]
fn rate_limit_retry_after_stored_in_metadata() {
    let mut meta = MergeRecoveryMetadata::new();
    meta.rate_limit_retry_after = Some("2026-02-20T15:00:00+00:00".to_string());
    meta.append_event_with_state(
        MergeRecoveryEvent::new(
            MergeRecoveryEventKind::AttemptFailed,
            MergeRecoverySource::System,
            MergeRecoveryReasonCode::ProviderRateLimited,
            "Rate limit hit during merge",
        ),
        MergeRecoveryState::RateLimited,
    );

    let json = meta.update_task_metadata(None).unwrap();
    let restored = MergeRecoveryMetadata::from_task_metadata(Some(&json))
        .unwrap()
        .unwrap();

    assert_eq!(
        restored.rate_limit_retry_after,
        Some("2026-02-20T15:00:00+00:00".to_string())
    );
    assert_eq!(restored.last_state, MergeRecoveryState::RateLimited);
    assert_eq!(
        restored.events[0].reason_code,
        MergeRecoveryReasonCode::ProviderRateLimited
    );
}

#[test]
fn rate_limit_retry_after_none_not_serialized() {
    let meta = MergeRecoveryMetadata::new();
    let json = serde_json::to_string(&meta).unwrap();
    assert!(
        !json.contains("rate_limit_retry_after"),
        "None rate_limit_retry_after should be skipped in serialization"
    );
}

#[test]
fn rate_limit_retry_after_backward_compat_deserialize() {
    // Old metadata without rate_limit_retry_after field should deserialize fine
    let json = r#"{
        "version": 1,
        "events": [],
        "last_state": "succeeded"
    }"#;
    let meta: MergeRecoveryMetadata = serde_json::from_str(json).unwrap();
    assert_eq!(meta.rate_limit_retry_after, None);
}

#[test]
fn rate_limit_retry_after_roundtrip_through_task_metadata() {
    let mut meta = MergeRecoveryMetadata::new();
    meta.rate_limit_retry_after = Some("2026-02-20T15:00:00+00:00".to_string());

    // Write to task metadata with existing keys
    let existing = r#"{"error": "some error"}"#;
    let json = meta.update_task_metadata(Some(existing)).unwrap();

    // Read back
    let restored = MergeRecoveryMetadata::from_task_metadata(Some(&json))
        .unwrap()
        .unwrap();
    assert_eq!(
        restored.rate_limit_retry_after,
        Some("2026-02-20T15:00:00+00:00".to_string())
    );

    // Verify other keys preserved
    let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed["error"], "some error");
}

#[test]
fn rate_limit_cleared_after_expiry() {
    let mut meta = MergeRecoveryMetadata::new();
    meta.rate_limit_retry_after = Some("2026-02-20T15:00:00+00:00".to_string());
    meta.last_state = MergeRecoveryState::RateLimited;

    // Simulate clearing after expiry
    meta.rate_limit_retry_after = None;
    meta.last_state = MergeRecoveryState::Retrying;

    let json = meta.update_task_metadata(None).unwrap();
    let restored = MergeRecoveryMetadata::from_task_metadata(Some(&json))
        .unwrap()
        .unwrap();
    assert_eq!(restored.rate_limit_retry_after, None);
    assert_eq!(restored.last_state, MergeRecoveryState::Retrying);
}

// --- ExecutionRecoveryMetadata tests ---

#[test]
fn execution_recovery_metadata_new_creates_empty() {
    let meta = ExecutionRecoveryMetadata::new();
    assert_eq!(meta.version, 1);
    assert!(meta.events.is_empty());
    assert_eq!(meta.last_state, ExecutionRecoveryState::Retrying);
    assert!(!meta.stop_retrying);
}

#[test]
fn execution_recovery_metadata_default_works() {
    let meta = ExecutionRecoveryMetadata::default();
    assert_eq!(meta.version, 1);
    assert!(meta.events.is_empty());
    assert!(!meta.stop_retrying);
}

#[test]
fn execution_recovery_metadata_max_events_constant() {
    assert_eq!(ExecutionRecoveryMetadata::MAX_EVENTS, 50);
}

#[test]
fn execution_recovery_event_new_sets_defaults() {
    let event = ExecutionRecoveryEvent::new(
        ExecutionRecoveryEventKind::Failed,
        ExecutionRecoverySource::System,
        ExecutionRecoveryReasonCode::Timeout,
        "Timed out",
    );

    assert_eq!(event.kind, ExecutionRecoveryEventKind::Failed);
    assert_eq!(event.source, ExecutionRecoverySource::System);
    assert_eq!(event.reason_code, ExecutionRecoveryReasonCode::Timeout);
    assert_eq!(event.message, "Timed out");
    assert!(event.attempt.is_none());
    assert!(event.failure_source.is_none());
}

#[test]
fn execution_recovery_event_builder_methods() {
    let event = ExecutionRecoveryEvent::new(
        ExecutionRecoveryEventKind::AutoRetryTriggered,
        ExecutionRecoverySource::Auto,
        ExecutionRecoveryReasonCode::Timeout,
        "Auto retry",
    )
    .with_attempt(2)
    .with_failure_source(ExecutionFailureSource::TransientTimeout);

    assert_eq!(event.attempt, Some(2));
    assert_eq!(
        event.failure_source,
        Some(ExecutionFailureSource::TransientTimeout)
    );
}

#[test]
fn execution_recovery_event_serializes_to_json() {
    let event = ExecutionRecoveryEvent::new(
        ExecutionRecoveryEventKind::AutoRetryTriggered,
        ExecutionRecoverySource::Auto,
        ExecutionRecoveryReasonCode::Timeout,
        "Auto retry",
    )
    .with_attempt(1)
    .with_failure_source(ExecutionFailureSource::TransientTimeout);

    let json = serde_json::to_string(&event).unwrap();
    assert!(json.contains("\"kind\":\"auto_retry_triggered\""));
    assert!(json.contains("\"source\":\"auto\""));
    assert!(json.contains("\"reason_code\":\"timeout\""));
    assert!(json.contains("\"attempt\":1"));
    assert!(json.contains("\"failure_source\":\"transient_timeout\""));
}

#[test]
fn execution_recovery_event_skips_serializing_none_fields() {
    let event = ExecutionRecoveryEvent::new(
        ExecutionRecoveryEventKind::Failed,
        ExecutionRecoverySource::System,
        ExecutionRecoveryReasonCode::Timeout,
        "Failed",
    );

    let json = serde_json::to_string(&event).unwrap();
    assert!(!json.contains("\"attempt\""));
    assert!(!json.contains("\"failure_source\""));
}

#[test]
fn execution_recovery_event_kind_serialization() {
    let kinds = [
        (ExecutionRecoveryEventKind::Failed, "failed"),
        (
            ExecutionRecoveryEventKind::AutoRetryTriggered,
            "auto_retry_triggered",
        ),
        (
            ExecutionRecoveryEventKind::AttemptStarted,
            "attempt_started",
        ),
        (
            ExecutionRecoveryEventKind::AttemptSucceeded,
            "attempt_succeeded",
        ),
        (ExecutionRecoveryEventKind::ManualRetry, "manual_retry"),
        (ExecutionRecoveryEventKind::StopRetrying, "stop_retrying"),
    ];

    for (kind, expected) in &kinds {
        let json = serde_json::to_string(kind).unwrap();
        assert_eq!(json, format!("\"{}\"", expected));
    }
}

#[test]
fn execution_recovery_source_serialization() {
    let sources = [
        (ExecutionRecoverySource::System, "system"),
        (ExecutionRecoverySource::Auto, "auto"),
        (ExecutionRecoverySource::User, "user"),
    ];

    for (source, expected) in &sources {
        let json = serde_json::to_string(source).unwrap();
        assert_eq!(json, format!("\"{}\"", expected));
    }
}

#[test]
fn execution_recovery_reason_code_serialization() {
    let codes = [
        (ExecutionRecoveryReasonCode::Timeout, "timeout"),
        (ExecutionRecoveryReasonCode::ParseStall, "parse_stall"),
        (ExecutionRecoveryReasonCode::AgentExit, "agent_exit"),
        (ExecutionRecoveryReasonCode::ProviderError, "provider_error"),
        (
            ExecutionRecoveryReasonCode::WallClockExceeded,
            "wall_clock_exceeded",
        ),
        (
            ExecutionRecoveryReasonCode::MaxRetriesExceeded,
            "max_retries_exceeded",
        ),
        (ExecutionRecoveryReasonCode::UserStopped, "user_stopped"),
        (
            ExecutionRecoveryReasonCode::IncompleteSteps,
            "incomplete_steps",
        ),
        (
            ExecutionRecoveryReasonCode::ValidationFailed,
            "validation_failed",
        ),
        (
            ExecutionRecoveryReasonCode::LocalToolFailed,
            "local_tool_failed",
        ),
        (ExecutionRecoveryReasonCode::Unknown, "unknown"),
    ];

    for (code, expected) in &codes {
        let json = serde_json::to_string(code).unwrap();
        assert_eq!(json, format!("\"{}\"", expected));
    }
}

#[test]
fn execution_recovery_state_serialization() {
    let states = [
        (ExecutionRecoveryState::Retrying, "retrying"),
        (ExecutionRecoveryState::Failed, "failed"),
        (ExecutionRecoveryState::Succeeded, "succeeded"),
    ];

    for (state, expected) in &states {
        let json = serde_json::to_string(state).unwrap();
        assert_eq!(json, format!("\"{}\"", expected));
    }
}

#[test]
fn execution_failure_source_serialization() {
    let sources = [
        (
            ExecutionFailureSource::TransientTimeout,
            "transient_timeout",
        ),
        (ExecutionFailureSource::ParseStall, "parse_stall"),
        (ExecutionFailureSource::AgentCrash, "agent_crash"),
        (ExecutionFailureSource::ProviderError, "provider_error"),
        (
            ExecutionFailureSource::WallClockTimeout,
            "wall_clock_timeout",
        ),
        (ExecutionFailureSource::AgentIncomplete, "agent_incomplete"),
        (ExecutionFailureSource::LocalToolFailed, "local_tool_failed"),
        (
            ExecutionFailureSource::ValidationFailed,
            "validation_failed",
        ),
        (ExecutionFailureSource::Unknown, "unknown"),
    ];

    for (source, expected) in &sources {
        let json = serde_json::to_string(source).unwrap();
        assert_eq!(json, format!("\"{}\"", expected));
    }
}

#[test]
fn execution_failure_source_is_transient_for_retryable_variants() {
    assert!(ExecutionFailureSource::TransientTimeout.is_transient());
    assert!(ExecutionFailureSource::ParseStall.is_transient());
    assert!(ExecutionFailureSource::AgentCrash.is_transient());
}

#[test]
fn execution_failure_source_not_transient_for_non_retryable_variants() {
    assert!(!ExecutionFailureSource::ProviderError.is_transient());
    assert!(!ExecutionFailureSource::WallClockTimeout.is_transient());
    assert!(!ExecutionFailureSource::AgentIncomplete.is_transient());
    assert!(!ExecutionFailureSource::LocalToolFailed.is_transient());
    assert!(!ExecutionFailureSource::ValidationFailed.is_transient());
    assert!(!ExecutionFailureSource::Unknown.is_transient());
}

#[test]
fn execution_recovery_metadata_append_event_trims_when_exceeds_max() {
    let mut meta = ExecutionRecoveryMetadata::new();

    for i in 0..(ExecutionRecoveryMetadata::MAX_EVENTS + 5) {
        let event = ExecutionRecoveryEvent::new(
            ExecutionRecoveryEventKind::Failed,
            ExecutionRecoverySource::System,
            ExecutionRecoveryReasonCode::Timeout,
            format!("Event {}", i),
        );
        meta.append_event(event);
    }

    assert_eq!(meta.events.len(), ExecutionRecoveryMetadata::MAX_EVENTS);
    // Oldest events trimmed — first remaining is event #5
    assert_eq!(meta.events[0].message, "Event 5");
    assert_eq!(
        meta.events[ExecutionRecoveryMetadata::MAX_EVENTS - 1].message,
        format!("Event {}", ExecutionRecoveryMetadata::MAX_EVENTS + 4)
    );
}

#[test]
fn execution_recovery_metadata_append_event_with_state_updates_both() {
    let mut meta = ExecutionRecoveryMetadata::new();
    let event = ExecutionRecoveryEvent::new(
        ExecutionRecoveryEventKind::AutoRetryTriggered,
        ExecutionRecoverySource::Auto,
        ExecutionRecoveryReasonCode::Timeout,
        "Retry",
    );

    meta.append_event_with_state(event, ExecutionRecoveryState::Retrying);

    assert_eq!(meta.events.len(), 1);
    assert_eq!(meta.last_state, ExecutionRecoveryState::Retrying);
}

#[test]
fn execution_recovery_metadata_from_task_metadata_with_no_metadata() {
    let result = ExecutionRecoveryMetadata::from_task_metadata(None).unwrap();
    assert!(result.is_none());
}

#[test]
fn execution_recovery_metadata_from_task_metadata_with_empty_json() {
    let result = ExecutionRecoveryMetadata::from_task_metadata(Some("{}")).unwrap();
    assert!(result.is_none());
}

#[test]
fn execution_recovery_metadata_from_task_metadata_with_other_keys_only() {
    let json = r#"{"is_timeout": true, "failure_error": "Agent timed out"}"#;
    let result = ExecutionRecoveryMetadata::from_task_metadata(Some(json)).unwrap();
    assert!(result.is_none());
}

#[test]
fn execution_recovery_metadata_from_task_metadata_with_valid_data() {
    let json = r#"{
        "is_timeout": true,
        "execution_recovery": {
            "version": 1,
            "events": [
                {
                    "at": "2026-03-06T10:00:00Z",
                    "kind": "failed",
                    "source": "system",
                    "reason_code": "timeout",
                    "message": "Timed out",
                    "failure_source": "transient_timeout"
                }
            ],
            "last_state": "retrying",
            "stop_retrying": false
        }
    }"#;

    let result = ExecutionRecoveryMetadata::from_task_metadata(Some(json)).unwrap();
    assert!(result.is_some());

    let meta = result.unwrap();
    assert_eq!(meta.version, 1);
    assert_eq!(meta.events.len(), 1);
    assert_eq!(meta.last_state, ExecutionRecoveryState::Retrying);
    assert!(!meta.stop_retrying);
    assert_eq!(
        meta.events[0].failure_source,
        Some(ExecutionFailureSource::TransientTimeout)
    );
}

#[test]
fn execution_recovery_metadata_from_task_metadata_with_invalid_json() {
    let result = ExecutionRecoveryMetadata::from_task_metadata(Some("not json"));
    assert!(result.is_err());
}

#[test]
fn execution_recovery_metadata_update_task_metadata_creates_new_object() {
    let mut meta = ExecutionRecoveryMetadata::new();
    meta.append_event_with_state(
        ExecutionRecoveryEvent::new(
            ExecutionRecoveryEventKind::Failed,
            ExecutionRecoverySource::System,
            ExecutionRecoveryReasonCode::Timeout,
            "Timed out",
        ),
        ExecutionRecoveryState::Retrying,
    );

    let result = meta.update_task_metadata(None).unwrap();

    let value: serde_json::Value = serde_json::from_str(&result).unwrap();
    assert!(value.get("execution_recovery").is_some());
    assert_eq!(value["execution_recovery"]["version"], 1);
    assert_eq!(
        value["execution_recovery"]["events"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    assert_eq!(value["execution_recovery"]["last_state"], "retrying");
}

#[test]
fn execution_recovery_metadata_update_task_metadata_preserves_existing_keys() {
    let meta = ExecutionRecoveryMetadata::new();
    let existing = r#"{"is_timeout": true, "failure_error": "Agent timed out"}"#;
    let result = meta.update_task_metadata(Some(existing)).unwrap();

    let value: serde_json::Value = serde_json::from_str(&result).unwrap();
    assert_eq!(value["is_timeout"], true);
    assert_eq!(value["failure_error"], "Agent timed out");
    assert!(value.get("execution_recovery").is_some());
}

#[test]
fn execution_recovery_metadata_update_task_metadata_overwrites_existing_recovery() {
    let mut meta = ExecutionRecoveryMetadata::new();
    meta.stop_retrying = true;
    meta.last_state = ExecutionRecoveryState::Failed;

    let existing = r#"{
        "execution_recovery": {
            "version": 1,
            "events": [],
            "last_state": "retrying",
            "stop_retrying": false
        }
    }"#;

    let result = meta.update_task_metadata(Some(existing)).unwrap();
    let value: serde_json::Value = serde_json::from_str(&result).unwrap();
    assert_eq!(value["execution_recovery"]["last_state"], "failed");
    assert_eq!(value["execution_recovery"]["stop_retrying"], true);
}

#[test]
fn roundtrip_from_and_update_execution_task_metadata() {
    let mut original = ExecutionRecoveryMetadata::new();
    original.append_event_with_state(
        ExecutionRecoveryEvent::new(
            ExecutionRecoveryEventKind::AutoRetryTriggered,
            ExecutionRecoverySource::Auto,
            ExecutionRecoveryReasonCode::Timeout,
            "Auto retry attempt 1",
        )
        .with_attempt(1)
        .with_failure_source(ExecutionFailureSource::AgentCrash),
        ExecutionRecoveryState::Retrying,
    );

    let json_str = original.update_task_metadata(None).unwrap();
    let parsed = ExecutionRecoveryMetadata::from_task_metadata(Some(&json_str))
        .unwrap()
        .unwrap();

    assert_eq!(parsed.version, original.version);
    assert_eq!(parsed.events.len(), 1);
    assert_eq!(parsed.last_state, ExecutionRecoveryState::Retrying);
    assert_eq!(parsed.events[0].message, "Auto retry attempt 1");
    assert_eq!(parsed.events[0].attempt, Some(1));
    assert_eq!(
        parsed.events[0].failure_source,
        Some(ExecutionFailureSource::AgentCrash)
    );
}

fn make_failed_event(source: ExecutionFailureSource) -> ExecutionRecoveryEvent {
    ExecutionRecoveryEvent::new(
        ExecutionRecoveryEventKind::Failed,
        ExecutionRecoverySource::System,
        ExecutionRecoveryReasonCode::Timeout,
        "failed",
    )
    .with_failure_source(source)
}

fn make_succeeded_event() -> ExecutionRecoveryEvent {
    ExecutionRecoveryEvent::new(
        ExecutionRecoveryEventKind::AttemptSucceeded,
        ExecutionRecoverySource::System,
        ExecutionRecoveryReasonCode::Unknown,
        "succeeded",
    )
}

#[test]
fn last_failure_is_transient_empty_returns_false() {
    let meta = ExecutionRecoveryMetadata::new();
    assert!(!meta.last_failure_is_transient());
}

#[test]
fn last_failure_is_transient_only_checks_last_event() {
    // Old event was transient; last event has no failure_source (succeeded)
    // The guard must return false so auto-commit is NOT skipped after a successful retry
    let mut meta = ExecutionRecoveryMetadata::new();
    meta.append_event(make_failed_event(ExecutionFailureSource::TransientTimeout));
    meta.append_event(make_succeeded_event()); // last event — no failure_source
    assert!(
        !meta.last_failure_is_transient(),
        "must not return true when last event has no failure_source"
    );
}

#[test]
fn last_failure_is_transient_true_when_last_event_is_transient() {
    let mut meta = ExecutionRecoveryMetadata::new();
    meta.append_event(make_failed_event(ExecutionFailureSource::TransientTimeout));
    assert!(meta.last_failure_is_transient());
}

#[test]
fn last_failure_is_transient_false_when_last_event_is_non_transient() {
    let mut meta = ExecutionRecoveryMetadata::new();
    meta.append_event(make_failed_event(ExecutionFailureSource::TransientTimeout));
    meta.append_event(make_failed_event(ExecutionFailureSource::Unknown)); // last event
    assert!(!meta.last_failure_is_transient());
}

#[test]
fn execution_recovery_metadata_roundtrip() {
    let mut meta = ExecutionRecoveryMetadata::new();
    meta.append_event_with_state(
        make_failed_event(ExecutionFailureSource::AgentCrash),
        ExecutionRecoveryState::Retrying,
    );
    let json = meta.update_task_metadata(None).unwrap();
    let restored = ExecutionRecoveryMetadata::from_task_metadata(Some(&json))
        .unwrap()
        .unwrap();
    assert_eq!(restored.last_state, ExecutionRecoveryState::Retrying);
    assert_eq!(restored.events.len(), 1);
    assert!(restored.last_failure_is_transient());
}

// ===== MergeFailureSource extended variants tests =====

#[test]
fn test_merge_failure_source_serde_roundtrip() {
    let variants = [
        MergeFailureSource::TransientGit,
        MergeFailureSource::AgentReported,
        MergeFailureSource::SystemDetected,
        MergeFailureSource::ValidationFailed,
        MergeFailureSource::WorktreeMissing,
        MergeFailureSource::SpawnFailure,
        MergeFailureSource::LockContention,
        MergeFailureSource::RateLimited,
        MergeFailureSource::CleanupTimeout,
        MergeFailureSource::TeardownRace,
        MergeFailureSource::PipelineActiveExpired,
        MergeFailureSource::HookEnvironment,
        MergeFailureSource::RepeatedHookFailure,
        MergeFailureSource::AuthFailure,
        MergeFailureSource::DiskFull,
        MergeFailureSource::DeterministicInfra,
    ];
    for variant in &variants {
        let json = serde_json::to_string(variant).unwrap();
        let deserialized: MergeFailureSource = serde_json::from_str(&json).unwrap();
        assert_eq!(*variant, deserialized);
    }
}

#[test]
fn test_merge_failure_source_unknown_fallback() {
    // cleanup_timeout now maps to the CleanupTimeout variant (no longer Unknown)
    let result: MergeFailureSource = serde_json::from_str("\"cleanup_timeout\"").unwrap();
    assert_eq!(result, MergeFailureSource::CleanupTimeout);

    // Truly unrecognized strings still deserialize as Unknown
    let result: MergeFailureSource = serde_json::from_str("\"BranchFreshnessTimeout\"").unwrap();
    assert_eq!(result, MergeFailureSource::Unknown);
}

#[test]
fn test_merge_recovery_metadata_circuit_breaker_serde_compat() {
    // Old metadata without circuit_breaker fields should deserialize correctly
    let old_json = r#"{"version":1,"events":[],"last_state":"succeeded"}"#;
    let meta: MergeRecoveryMetadata = serde_json::from_str(old_json).unwrap();
    assert!(!meta.circuit_breaker_active);
    assert!(meta.circuit_breaker_reason.is_none());
}

#[test]
fn test_circuit_breaker_false_not_serialized() {
    let meta = MergeRecoveryMetadata::new();
    let json = serde_json::to_string(&meta).unwrap();
    // circuit_breaker_active=false should be omitted from JSON
    assert!(
        !json.contains("circuit_breaker_active"),
        "circuit_breaker_active=false should be skipped in serialization"
    );
    assert!(
        !json.contains("circuit_breaker_reason"),
        "circuit_breaker_reason=None should be skipped in serialization"
    );
}

#[test]
fn test_circuit_breaker_true_serialized() {
    let mut meta = MergeRecoveryMetadata::new();
    meta.circuit_breaker_active = true;
    meta.circuit_breaker_reason = Some("too many failures".to_string());
    let json = serde_json::to_string(&meta).unwrap();
    assert!(json.contains("circuit_breaker_active"));
    assert!(json.contains("circuit_breaker_reason"));

    // Roundtrip
    let restored: MergeRecoveryMetadata = serde_json::from_str(&json).unwrap();
    assert!(restored.circuit_breaker_active);
    assert_eq!(
        restored.circuit_breaker_reason,
        Some("too many failures".to_string())
    );
}

#[test]
fn test_retry_strategy() {
    let cases = [
        (MergeFailureSource::TransientGit, RetryStrategy::AutoRetry),
        (MergeFailureSource::SystemDetected, RetryStrategy::AutoRetry),
        (
            MergeFailureSource::WorktreeMissing,
            RetryStrategy::AutoRetry,
        ),
        (MergeFailureSource::SpawnFailure, RetryStrategy::AutoRetry),
        (MergeFailureSource::LockContention, RetryStrategy::AutoRetry),
        (MergeFailureSource::RateLimited, RetryStrategy::AutoRetry),
        (MergeFailureSource::CleanupTimeout, RetryStrategy::AutoRetry),
        (MergeFailureSource::TeardownRace, RetryStrategy::AutoRetry),
        (
            MergeFailureSource::PipelineActiveExpired,
            RetryStrategy::AutoRetry,
        ),
        (
            MergeFailureSource::TargetBranchBusy,
            RetryStrategy::AutoRetryNoCB,
        ),
        (
            MergeFailureSource::AgentReported,
            RetryStrategy::NoAutomaticRetry,
        ),
        (
            MergeFailureSource::ValidationFailed,
            RetryStrategy::NoAutomaticRetry,
        ),
        (
            MergeFailureSource::HookEnvironment,
            RetryStrategy::NoAutomaticRetry,
        ),
        (
            MergeFailureSource::RepeatedHookFailure,
            RetryStrategy::NoAutomaticRetry,
        ),
        (
            MergeFailureSource::AuthFailure,
            RetryStrategy::NoAutomaticRetry,
        ),
        (
            MergeFailureSource::DiskFull,
            RetryStrategy::NoAutomaticRetry,
        ),
        (
            MergeFailureSource::DeterministicInfra,
            RetryStrategy::NoAutomaticRetry,
        ),
        (MergeFailureSource::Unknown, RetryStrategy::NoAutomaticRetry),
    ];

    for (source, expected) in cases {
        assert_eq!(
            source.retry_strategy(),
            expected,
            "{source:?} retry strategy drifted"
        );
    }
}

// ── GitIsolation tests ────────────────────────────────────────────────────────

#[test]
fn git_isolation_error_prefix_constant_is_correct() {
    assert_eq!(GIT_ISOLATION_ERROR_PREFIX, "Git isolation failed");
}

#[test]
fn execution_failure_source_git_isolation_is_transient() {
    assert!(
        ExecutionFailureSource::GitIsolation.is_transient(),
        "GitIsolation should be transient (safe to auto-retry)"
    );
}

#[test]
fn execution_failure_source_all_variants_is_transient_coverage() {
    // Non-transient variants
    assert!(!ExecutionFailureSource::ProviderError.is_transient());
    assert!(!ExecutionFailureSource::WallClockTimeout.is_transient());
    assert!(!ExecutionFailureSource::Unknown.is_transient());
    // Transient variants
    assert!(ExecutionFailureSource::TransientTimeout.is_transient());
    assert!(ExecutionFailureSource::ParseStall.is_transient());
    assert!(ExecutionFailureSource::AgentCrash.is_transient());
    assert!(ExecutionFailureSource::GitIsolation.is_transient());
}

#[test]
fn execution_failure_source_git_isolation_serde_round_trip() {
    let source = ExecutionFailureSource::GitIsolation;
    let json = serde_json::to_string(&source).expect("serialize");
    assert_eq!(json, "\"git_isolation\"");
    let deserialized: ExecutionFailureSource = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(deserialized, ExecutionFailureSource::GitIsolation);
}

#[test]
fn execution_recovery_reason_code_git_isolation_failed_serde_round_trip() {
    let code = ExecutionRecoveryReasonCode::GitIsolationFailed;
    let json = serde_json::to_string(&code).expect("serialize");
    assert_eq!(json, "\"git_isolation_failed\"");
    let deserialized: ExecutionRecoveryReasonCode =
        serde_json::from_str(&json).expect("deserialize");
    assert_eq!(
        deserialized,
        ExecutionRecoveryReasonCode::GitIsolationFailed
    );
}

#[test]
fn execution_recovery_metadata_backward_compat_deserializes_without_new_fields() {
    // Old metadata JSON that does NOT contain git_isolation events — should still parse.
    let old_json = r#"{
        "execution_recovery": {
            "version": 1,
            "events": [
                {
                    "at": "2026-01-01T00:00:00Z",
                    "kind": "auto_retry_triggered",
                    "source": "auto",
                    "reason_code": "timeout",
                    "message": "retry 1",
                    "failure_source": "transient_timeout"
                }
            ],
            "last_state": "retrying",
            "stop_retrying": false
        }
    }"#;
    let recovery = ExecutionRecoveryMetadata::from_json(old_json)
        .expect("parse")
        .expect("some");
    assert_eq!(recovery.events.len(), 1);
    assert_eq!(
        recovery.events[0].failure_source,
        Some(ExecutionFailureSource::TransientTimeout)
    );
}

#[test]
fn auto_retry_count_for_source_counts_only_matching_source() {
    let mut recovery = ExecutionRecoveryMetadata::new();

    // Add 2 GitIsolation retries
    for i in 1..=2u32 {
        let ev = ExecutionRecoveryEvent::new(
            ExecutionRecoveryEventKind::AutoRetryTriggered,
            ExecutionRecoverySource::Auto,
            ExecutionRecoveryReasonCode::GitIsolationFailed,
            format!("git retry {i}"),
        )
        .with_failure_source(ExecutionFailureSource::GitIsolation);
        recovery.append_event(ev);
    }
    // Add 1 TransientTimeout retry
    let timeout_ev = ExecutionRecoveryEvent::new(
        ExecutionRecoveryEventKind::AutoRetryTriggered,
        ExecutionRecoverySource::Auto,
        ExecutionRecoveryReasonCode::Timeout,
        "timeout retry",
    )
    .with_failure_source(ExecutionFailureSource::TransientTimeout);
    recovery.append_event(timeout_ev);

    assert_eq!(
        recovery.auto_retry_count_for_source(ExecutionFailureSource::GitIsolation),
        2,
        "should count only GitIsolation retries"
    );
    assert_eq!(
        recovery.auto_retry_count_for_source(ExecutionFailureSource::TransientTimeout),
        1,
        "should count only TransientTimeout retries"
    );
}

#[test]
fn auto_retry_count_for_source_ignores_non_auto_retry_events() {
    let mut recovery = ExecutionRecoveryMetadata::new();
    // Add a Failed event with GitIsolation source — should NOT be counted
    let failed_ev = ExecutionRecoveryEvent::new(
        ExecutionRecoveryEventKind::Failed,
        ExecutionRecoverySource::System,
        ExecutionRecoveryReasonCode::GitIsolationFailed,
        "failed",
    )
    .with_failure_source(ExecutionFailureSource::GitIsolation);
    recovery.append_event(failed_ev);

    assert_eq!(
        recovery.auto_retry_count_for_source(ExecutionFailureSource::GitIsolation),
        0,
        "Failed events should not be counted as retries"
    );
}

// ============================================================
// Tests for StopRetryingReason enum
// ============================================================

#[test]
fn test_stop_retrying_reason_serde_roundtrip() {
    let variants = [
        StopRetryingReason::MaxRetriesExceeded,
        StopRetryingReason::GitBranchLost,
        StopRetryingReason::ManualStop,
    ];
    for variant in &variants {
        let serialized = serde_json::to_string(variant).unwrap();
        let deserialized: StopRetryingReason = serde_json::from_str(&serialized).unwrap();
        assert_eq!(*variant, deserialized, "Roundtrip failed for {:?}", variant);
    }
}

#[test]
fn test_stop_retrying_reason_snake_case_serialization() {
    assert_eq!(
        serde_json::to_string(&StopRetryingReason::MaxRetriesExceeded).unwrap(),
        "\"max_retries_exceeded\""
    );
    assert_eq!(
        serde_json::to_string(&StopRetryingReason::GitBranchLost).unwrap(),
        "\"git_branch_lost\""
    );
    assert_eq!(
        serde_json::to_string(&StopRetryingReason::ManualStop).unwrap(),
        "\"manual_stop\""
    );
}

// ============================================================
// Tests for auto_recovery_count backward compatibility
// ============================================================

#[test]
fn test_execution_recovery_metadata_backward_compat_auto_recovery_count() {
    // Old metadata JSON (from before auto_recovery_count was added) should deserialize
    // with auto_recovery_count defaulting to 0 due to #[serde(default)].
    let old_json = r#"{
        "execution_recovery": {
            "version": 1,
            "events": [],
            "last_state": "retrying",
            "stop_retrying": false
        }
    }"#;

    let recovery = ExecutionRecoveryMetadata::from_json(old_json)
        .expect("Should parse without error")
        .expect("Should contain execution_recovery key");

    assert_eq!(
        recovery.auto_recovery_count, 0,
        "auto_recovery_count should default to 0 when absent from JSON"
    );
    assert_eq!(
        recovery.unrecoverable_reason, None,
        "unrecoverable_reason should default to None when absent from JSON"
    );
}

#[test]
fn test_execution_recovery_metadata_auto_recovery_count_persists() {
    // Create metadata with auto_recovery_count = 1 and verify it survives serde roundtrip.
    let mut metadata = ExecutionRecoveryMetadata::new();
    metadata.auto_recovery_count = 1;
    metadata.unrecoverable_reason = Some(StopRetryingReason::GitBranchLost);

    // Serialize into task metadata JSON
    let task_metadata = metadata
        .update_task_metadata(None)
        .expect("Should serialize successfully");

    // Deserialize back
    let recovered = ExecutionRecoveryMetadata::from_json(&task_metadata)
        .expect("Should parse without error")
        .expect("Should contain execution_recovery key");

    assert_eq!(recovered.auto_recovery_count, 1);
    assert_eq!(
        recovered.unrecoverable_reason,
        Some(StopRetryingReason::GitBranchLost)
    );
}

// ============================================================
// Test: GitBranchLost budget isolation
// ============================================================

#[test]
fn test_git_branch_lost_sets_distinct_reason_code() {
    // Verify GitBranchLost has its own distinct reason (not MaxRetriesExceeded).
    // This documents the per-source budget isolation guarantee: git branch failures
    // use a different reason code than normal retry exhaustion, so a future
    // "Reset to Ready" admin action can distinguish the two cases.
    let git_lost = StopRetryingReason::GitBranchLost;
    let max_retries = StopRetryingReason::MaxRetriesExceeded;
    assert_ne!(
        git_lost, max_retries,
        "GitBranchLost must be distinct from MaxRetriesExceeded"
    );

    // Also verify serialization differs (used for UI display and DB storage)
    let git_json = serde_json::to_string(&git_lost).unwrap();
    let max_json = serde_json::to_string(&max_retries).unwrap();
    assert_ne!(git_json, max_json);
    assert_eq!(git_json, "\"git_branch_lost\"");
    assert_eq!(max_json, "\"max_retries_exceeded\"");
}

#[test]
fn test_auto_recovery_count_independent_of_events() {
    // auto_recovery_count is a separate counter from the event log.
    // Clearing events (as done in auto_recover_task) does NOT reset auto_recovery_count.
    let mut metadata = ExecutionRecoveryMetadata::new();

    // Simulate first auto-recovery: increment counter, then clear events
    metadata.auto_recovery_count = 1;
    metadata.events.clear();
    metadata.last_state = ExecutionRecoveryState::Retrying;
    metadata.stop_retrying = false;

    // After clearing events (fresh start), auto_recovery_count still reflects recovery history
    assert_eq!(
        metadata.auto_recovery_count, 1,
        "auto_recovery_count must survive event log clear"
    );

    // Simulate second auto-recovery: MAX is 2
    metadata.auto_recovery_count = 2;
    assert!(
        metadata.auto_recovery_count >= 2,
        "After 2 recoveries, auto_recovery_count >= MAX_AUTO_RECOVERIES (2)"
    );
}

// =============================================================================
// ValidationCacheMetadata tests
// =============================================================================

#[test]
fn validation_cache_metadata_from_task_metadata_none_when_null() {
    let result = ValidationCacheMetadata::from_task_metadata(None);
    assert!(result.is_ok());
    assert!(result.unwrap().is_none());
}

#[test]
fn validation_cache_metadata_from_task_metadata_none_when_key_missing() {
    let json = r#"{"merge_recovery": {"version": 1, "events": [], "last_state": "succeeded", "circuit_breaker_active": false}}"#;
    let result = ValidationCacheMetadata::from_task_metadata(Some(json));
    assert!(result.is_ok());
    assert!(result.unwrap().is_none());
}

#[test]
fn validation_cache_metadata_from_task_metadata_parses_valid_entry() {
    let json = r#"{
        "validation_cache": {
            "version": 1,
            "commit_sha": "abc123def456",
            "tests_ran": true,
            "tests_passed": true,
            "test_summary": "42 passed, 0 failed",
            "captured_at": "2026-03-28T00:00:00Z",
            "captured_by": "execution_complete"
        }
    }"#;
    let result = ValidationCacheMetadata::from_task_metadata(Some(json));
    assert!(result.is_ok());
    let cache = result.unwrap().expect("should have validation_cache");
    assert_eq!(cache.version, 1);
    assert_eq!(cache.commit_sha, "abc123def456");
    assert!(cache.tests_ran);
    assert!(cache.tests_passed);
    assert_eq!(cache.test_summary.as_deref(), Some("42 passed, 0 failed"));
    assert_eq!(cache.captured_by, "execution_complete");
}

#[test]
fn validation_cache_metadata_from_task_metadata_tests_ran_false() {
    let json = r#"{
        "validation_cache": {
            "version": 1,
            "commit_sha": "abc123",
            "tests_ran": false,
            "tests_passed": false,
            "captured_at": "2026-03-28T00:00:00Z",
            "captured_by": "execution_complete"
        }
    }"#;
    let result = ValidationCacheMetadata::from_task_metadata(Some(json));
    let cache = result.unwrap().expect("should have validation_cache");
    assert!(!cache.tests_ran);
    assert!(!cache.tests_passed);
    assert!(cache.test_summary.is_none());
}

#[test]
fn validation_cache_metadata_update_task_metadata_preserves_other_keys() {
    use chrono::Utc;

    let existing = r#"{"merge_recovery": {"version": 1, "events": [], "last_state": "succeeded", "circuit_breaker_active": false}, "execution_recovery": {"version": 1, "events": [], "last_state": "succeeded", "stop_retrying": false, "auto_recovery_count": 0}}"#;

    let cache = ValidationCacheMetadata {
        version: 1,
        commit_sha: "deadbeef".to_string(),
        tests_ran: true,
        tests_passed: true,
        test_summary: Some("10 passed".to_string()),
        captured_at: Utc::now(),
        captured_by: "execution_complete".to_string(),
    };

    let result = cache.update_task_metadata(Some(existing));
    assert!(result.is_ok());
    let updated_json = result.unwrap();
    let value: serde_json::Value = serde_json::from_str(&updated_json).unwrap();

    // All three keys must be present
    assert!(
        value.get("merge_recovery").is_some(),
        "merge_recovery must be preserved"
    );
    assert!(
        value.get("execution_recovery").is_some(),
        "execution_recovery must be preserved"
    );
    assert!(
        value.get("validation_cache").is_some(),
        "validation_cache must be added"
    );

    let vc = value.get("validation_cache").unwrap();
    assert_eq!(vc["commit_sha"], "deadbeef");
    assert_eq!(vc["tests_ran"], true);
    assert_eq!(vc["tests_passed"], true);
}

#[test]
fn validation_cache_metadata_update_task_metadata_on_empty() {
    use chrono::Utc;

    let cache = ValidationCacheMetadata {
        version: 1,
        commit_sha: "cafe1234".to_string(),
        tests_ran: false,
        tests_passed: false,
        test_summary: None,
        captured_at: Utc::now(),
        captured_by: "execution_complete".to_string(),
    };

    let result = cache.update_task_metadata(None);
    assert!(result.is_ok());
    let value: serde_json::Value = serde_json::from_str(&result.unwrap()).unwrap();
    assert!(value.get("validation_cache").is_some());
    assert_eq!(value["validation_cache"]["commit_sha"], "cafe1234");
    // test_summary should be omitted when None (skip_serializing_if)
    assert!(value["validation_cache"].get("test_summary").is_none());
}

#[test]
fn validation_cache_metadata_round_trip_serialization() {
    use chrono::Utc;

    let original = ValidationCacheMetadata {
        version: 1,
        commit_sha: "f00dbadc0de".to_string(),
        tests_ran: true,
        tests_passed: false,
        test_summary: Some("3 passed, 2 failed".to_string()),
        captured_at: Utc::now(),
        captured_by: "execution_complete".to_string(),
    };

    let json = serde_json::to_string(&original).unwrap();
    let parsed: ValidationCacheMetadata = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed.commit_sha, original.commit_sha);
    assert_eq!(parsed.tests_ran, original.tests_ran);
    assert_eq!(parsed.tests_passed, original.tests_passed);
    assert_eq!(parsed.test_summary, original.test_summary);
    assert_eq!(parsed.captured_by, original.captured_by);
    assert_eq!(parsed.version, original.version);
}

#[test]
fn validation_cache_metadata_update_task_metadata_preserves_auto_retry_count_keys() {
    use chrono::Utc;

    // auto_retry_count_* are top-level keys written by the reconciler.
    // update_task_metadata must NOT clobber them.
    let existing = r#"{
        "auto_retry_count_executing": 2,
        "auto_retry_count_reviewing": 1,
        "merge_recovery": {"version": 1, "events": [], "last_state": "succeeded", "circuit_breaker_active": false}
    }"#;

    let cache = ValidationCacheMetadata {
        version: 1,
        commit_sha: "abc123".to_string(),
        tests_ran: true,
        tests_passed: true,
        test_summary: None,
        captured_at: Utc::now(),
        captured_by: "execution_complete".to_string(),
    };

    let result = cache.update_task_metadata(Some(existing));
    assert!(result.is_ok());
    let value: serde_json::Value = serde_json::from_str(&result.unwrap()).unwrap();

    assert_eq!(
        value["auto_retry_count_executing"], 2,
        "auto_retry_count_executing must be preserved"
    );
    assert_eq!(
        value["auto_retry_count_reviewing"], 1,
        "auto_retry_count_reviewing must be preserved"
    );
    assert!(
        value.get("merge_recovery").is_some(),
        "merge_recovery must be preserved"
    );
    assert!(
        value.get("validation_cache").is_some(),
        "validation_cache must be added"
    );
    assert_eq!(value["validation_cache"]["commit_sha"], "abc123");
}

#[test]
fn validation_cache_metadata_version_is_one() {
    use chrono::Utc;

    let cache = ValidationCacheMetadata {
        version: 1,
        commit_sha: "sha123".to_string(),
        tests_ran: true,
        tests_passed: true,
        test_summary: None,
        captured_at: Utc::now(),
        captured_by: "execution_complete".to_string(),
    };
    assert_eq!(cache.version, 1);

    // Verify version survives JSON round-trip
    let json_str = serde_json::to_string(&cache).unwrap();
    let parsed: ValidationCacheMetadata = serde_json::from_str(&json_str).unwrap();
    assert_eq!(parsed.version, 1);
}

#[test]
fn stop_retrying_reason_serde_round_trip_new_variants() {
    // StructuralGitError round-trips
    let json = serde_json::to_string(&StopRetryingReason::StructuralGitError).unwrap();
    assert_eq!(json, "\"structural_git_error\"");
    let parsed: StopRetryingReason = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, StopRetryingReason::StructuralGitError);

    // GitIsolationExhausted round-trips
    let json = serde_json::to_string(&StopRetryingReason::GitIsolationExhausted).unwrap();
    assert_eq!(json, "\"git_isolation_exhausted\"");
    let parsed: StopRetryingReason = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, StopRetryingReason::GitIsolationExhausted);

    // AgentCommandInvalid round-trips
    let json = serde_json::to_string(&StopRetryingReason::AgentCommandInvalid).unwrap();
    assert_eq!(json, "\"agent_command_invalid\"");
    let parsed: StopRetryingReason = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, StopRetryingReason::AgentCommandInvalid);
}

#[test]
fn stop_retrying_reason_unknown_fallback_for_future_variants() {
    // An unrecognised variant from newer code must deserialise to Unknown
    let parsed: StopRetryingReason = serde_json::from_str("\"some_future_variant\"").unwrap();
    assert_eq!(parsed, StopRetryingReason::Unknown);
}
