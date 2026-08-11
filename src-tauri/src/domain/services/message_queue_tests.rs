use super::*;

#[test]
fn test_queue_and_pop() {
    let queue = MessageQueue::new();

    // Queue two messages
    let _msg1 = queue.queue(
        ChatContextType::Ideation,
        "session-1",
        "First message".to_string(),
    );
    let _msg2 = queue.queue(
        ChatContextType::Ideation,
        "session-1",
        "Second message".to_string(),
    );

    // Pop should return in FIFO order
    let popped1 = queue.pop(ChatContextType::Ideation, "session-1");
    assert!(popped1.is_some());
    assert_eq!(popped1.unwrap().content, "First message");

    let popped2 = queue.pop(ChatContextType::Ideation, "session-1");
    assert!(popped2.is_some());
    assert_eq!(popped2.unwrap().content, "Second message");

    // Queue should be empty now
    let popped3 = queue.pop(ChatContextType::Ideation, "session-1");
    assert!(popped3.is_none());
}

#[test]
fn test_get_queued() {
    let queue = MessageQueue::new();

    // Initially empty
    assert_eq!(queue.get_queued(ChatContextType::Task, "task-1").len(), 0);

    // Queue two messages
    queue.queue(ChatContextType::Task, "task-1", "First".to_string());
    queue.queue(ChatContextType::Task, "task-1", "Second".to_string());

    // get_queued should return all messages without removing
    let queued = queue.get_queued(ChatContextType::Task, "task-1");
    assert_eq!(queued.len(), 2);
    assert_eq!(queued[0].content, "First");
    assert_eq!(queued[1].content, "Second");

    // Messages should still be in queue
    assert_eq!(queue.get_queued(ChatContextType::Task, "task-1").len(), 2);
}

#[test]
fn test_take_removes_selected_message_without_reordering_remaining() {
    let queue = MessageQueue::new();
    let first = queue.queue(ChatContextType::Task, "task-1", "First".to_string());
    let second = queue.queue(ChatContextType::Task, "task-1", "Second".to_string());
    let third = queue.queue(ChatContextType::Task, "task-1", "Third".to_string());

    let taken = queue
        .take(ChatContextType::Task, "task-1", &second.id)
        .expect("selected queued message should be removed");

    assert_eq!(taken.id, second.id);
    assert_eq!(taken.content, "Second");

    let remaining = queue.get_queued(ChatContextType::Task, "task-1");
    assert_eq!(
        remaining
            .iter()
            .map(|message| message.id.as_str())
            .collect::<Vec<_>>(),
        vec![first.id.as_str(), third.id.as_str()],
        "taking a selected queued message must preserve the order of the rest"
    );
}

#[test]
fn test_clear() {
    let queue = MessageQueue::new();

    queue.queue(ChatContextType::Project, "proj-1", "Message 1".to_string());
    queue.queue(ChatContextType::Project, "proj-1", "Message 2".to_string());

    assert_eq!(
        queue.get_queued(ChatContextType::Project, "proj-1").len(),
        2
    );

    queue.clear(ChatContextType::Project, "proj-1");

    assert_eq!(
        queue.get_queued(ChatContextType::Project, "proj-1").len(),
        0
    );
    assert!(queue.pop(ChatContextType::Project, "proj-1").is_none());
}

#[test]
fn test_list_keys_only_returns_non_empty_queues() {
    let queue = MessageQueue::new();

    queue.queue(ChatContextType::Ideation, "sess-1", "First".to_string());
    queue.queue(
        ChatContextType::TaskExecution,
        "task-1",
        "Second".to_string(),
    );
    queue.clear(ChatContextType::TaskExecution, "task-1");

    let mut keys = queue.list_keys();
    keys.sort_by(|a, b| a.context_id.cmp(&b.context_id));

    assert_eq!(keys.len(), 1);
    assert_eq!(keys[0].context_type, ChatContextType::Ideation);
    assert_eq!(keys[0].context_id, "sess-1");
}

#[test]
fn test_delete() {
    let queue = MessageQueue::new();

    let _msg1 = queue.queue(ChatContextType::Ideation, "sess-1", "First".to_string());
    let msg2 = queue.queue(ChatContextType::Ideation, "sess-1", "Second".to_string());
    let _msg3 = queue.queue(ChatContextType::Ideation, "sess-1", "Third".to_string());

    assert_eq!(
        queue.get_queued(ChatContextType::Ideation, "sess-1").len(),
        3
    );

    // Delete middle message
    let deleted = queue.delete(ChatContextType::Ideation, "sess-1", &msg2.id);
    assert!(deleted);

    let remaining = queue.get_queued(ChatContextType::Ideation, "sess-1");
    assert_eq!(remaining.len(), 2);
    assert_eq!(remaining[0].content, "First");
    assert_eq!(remaining[1].content, "Third");

    // Try deleting non-existent message
    let deleted = queue.delete(ChatContextType::Ideation, "sess-1", "non-existent-id");
    assert!(!deleted);
}

#[test]
fn test_different_contexts_isolated() {
    let queue = MessageQueue::new();

    // Queue messages for different context types
    queue.queue(
        ChatContextType::Ideation,
        "id-1",
        "Ideation message".to_string(),
    );
    queue.queue(ChatContextType::Task, "id-1", "Task message".to_string());
    queue.queue(
        ChatContextType::Project,
        "id-1",
        "Project message".to_string(),
    );
    queue.queue(
        ChatContextType::TaskExecution,
        "id-1",
        "Execution message".to_string(),
    );

    // Each context type has its own queue
    assert_eq!(queue.get_queued(ChatContextType::Ideation, "id-1").len(), 1);
    assert_eq!(queue.get_queued(ChatContextType::Task, "id-1").len(), 1);
    assert_eq!(queue.get_queued(ChatContextType::Project, "id-1").len(), 1);
    assert_eq!(
        queue
            .get_queued(ChatContextType::TaskExecution, "id-1")
            .len(),
        1
    );

    // Popping from one doesn't affect others
    let popped = queue.pop(ChatContextType::Ideation, "id-1");
    assert!(popped.is_some());
    assert_eq!(popped.unwrap().content, "Ideation message");

    assert_eq!(queue.get_queued(ChatContextType::Ideation, "id-1").len(), 0);
    assert_eq!(queue.get_queued(ChatContextType::Task, "id-1").len(), 1);
}

#[test]
fn test_different_context_ids_isolated() {
    let queue = MessageQueue::new();

    queue.queue(
        ChatContextType::Ideation,
        "session-1",
        "Session 1 message".to_string(),
    );
    queue.queue(
        ChatContextType::Ideation,
        "session-2",
        "Session 2 message".to_string(),
    );

    assert_eq!(
        queue
            .get_queued(ChatContextType::Ideation, "session-1")
            .len(),
        1
    );
    assert_eq!(
        queue
            .get_queued(ChatContextType::Ideation, "session-2")
            .len(),
        1
    );

    let popped = queue.pop(ChatContextType::Ideation, "session-1");
    assert!(popped.is_some());
    assert_eq!(popped.unwrap().content, "Session 1 message");

    assert_eq!(
        queue
            .get_queued(ChatContextType::Ideation, "session-1")
            .len(),
        0
    );
    assert_eq!(
        queue
            .get_queued(ChatContextType::Ideation, "session-2")
            .len(),
        1
    );
}

#[test]
fn test_backwards_compatible_task_methods() {
    let queue = MessageQueue::new();
    let task_id = TaskId::from_string("task-123".to_string());

    // Queue using backwards-compatible method
    let msg = queue.queue_for_task(task_id.clone(), "Task message".to_string());
    assert_eq!(msg.content, "Task message");

    // Should be accessible via both APIs
    assert_eq!(queue.get_queued_for_task(&task_id).len(), 1);
    assert_eq!(
        queue
            .get_queued(ChatContextType::TaskExecution, "task-123")
            .len(),
        1
    );

    // Pop using backwards-compatible method
    let popped = queue.pop_for_task(&task_id);
    assert!(popped.is_some());
    assert_eq!(popped.unwrap().content, "Task message");
}

#[test]
fn test_queue_key_convenience_methods() {
    let task_id = TaskId::from_string("task-1".to_string());

    let key1 = QueueKey::task_execution(&task_id);
    assert_eq!(key1.context_type, ChatContextType::TaskExecution);
    assert_eq!(key1.context_id, "task-1");

    let key2 = QueueKey::ideation("session-1");
    assert_eq!(key2.context_type, ChatContextType::Ideation);
    assert_eq!(key2.context_id, "session-1");

    let key3 = QueueKey::task("task-2");
    assert_eq!(key3.context_type, ChatContextType::Task);
    assert_eq!(key3.context_id, "task-2");

    let key4 = QueueKey::project("project-1");
    assert_eq!(key4.context_type, ChatContextType::Project);
    assert_eq!(key4.context_id, "project-1");
}

#[test]
fn test_queued_message_creation() {
    let msg = QueuedMessage::new("Test content".to_string());

    assert!(!msg.id.is_empty());
    assert_eq!(msg.content, "Test content");
    assert!(!msg.created_at.is_empty());
    assert!(!msg.is_editing);

    // Verify timestamp is valid RFC3339
    chrono::DateTime::parse_from_rfc3339(&msg.created_at).expect("Valid RFC3339 timestamp");
}

#[test]
fn test_clone_safety() {
    let queue1 = MessageQueue::new();
    let queue2 = queue1.clone();

    // Queue via queue1
    queue1.queue(
        ChatContextType::Ideation,
        "session-1",
        "Message".to_string(),
    );

    // Should be visible via queue2 (shared Arc)
    assert_eq!(
        queue2
            .get_queued(ChatContextType::Ideation, "session-1")
            .len(),
        1
    );

    // Pop via queue2
    let popped = queue2.pop(ChatContextType::Ideation, "session-1");
    assert!(popped.is_some());

    // Should be empty in both
    assert_eq!(
        queue1
            .get_queued(ChatContextType::Ideation, "session-1")
            .len(),
        0
    );
    assert_eq!(
        queue2
            .get_queued(ChatContextType::Ideation, "session-1")
            .len(),
        0
    );
}

#[test]
fn test_queue_front_inserts_before_existing() {
    let queue = MessageQueue::new();

    // Queue two regular messages
    queue.queue(
        ChatContextType::Ideation,
        "sess-1",
        "User msg 1".to_string(),
    );
    queue.queue(
        ChatContextType::Ideation,
        "sess-1",
        "User msg 2".to_string(),
    );

    // Insert priority message at front
    queue.queue_front(
        ChatContextType::Ideation,
        "sess-1",
        "Recovery context".to_string(),
    );

    // Pop should return the front-inserted message first
    let first = queue.pop(ChatContextType::Ideation, "sess-1").unwrap();
    assert_eq!(first.content, "Recovery context");

    let second = queue.pop(ChatContextType::Ideation, "sess-1").unwrap();
    assert_eq!(second.content, "User msg 1");

    let third = queue.pop(ChatContextType::Ideation, "sess-1").unwrap();
    assert_eq!(third.content, "User msg 2");

    assert!(queue.pop(ChatContextType::Ideation, "sess-1").is_none());
}

#[test]
fn test_queue_front_on_empty_queue() {
    let queue = MessageQueue::new();

    queue.queue_front(ChatContextType::Task, "task-1", "Priority msg".to_string());

    let queued = queue.get_queued(ChatContextType::Task, "task-1");
    assert_eq!(queued.len(), 1);
    assert_eq!(queued[0].content, "Priority msg");
}

#[test]
fn test_with_key_methods() {
    let queue = MessageQueue::new();
    let key = QueueKey::ideation("session-1");

    // Queue with key
    let msg = queue.queue_with_key(key.clone(), "Message 1".to_string());
    assert_eq!(msg.content, "Message 1");

    // Get with key
    let queued = queue.get_queued_with_key(&key);
    assert_eq!(queued.len(), 1);

    // Pop with key
    let popped = queue.pop_with_key(&key);
    assert!(popped.is_some());
    assert_eq!(popped.unwrap().content, "Message 1");

    // Should be empty
    assert!(queue.get_queued_with_key(&key).is_empty());
}

#[test]
fn test_remove_stale_drops_old_messages() {
    let queue = MessageQueue::new();

    // Manually construct a stale message (created 10 minutes ago)
    let stale_ts = (chrono::Utc::now() - chrono::Duration::seconds(600)).to_rfc3339();
    let fresh_ts = chrono::Utc::now().to_rfc3339();

    {
        let key = QueueKey::new(ChatContextType::Ideation, "sess-stale".to_string());
        let mut queues = queue.queues.lock().unwrap();
        let q = queues.entry(key).or_default();
        q.push(QueuedMessage {
            id: "stale-1".to_string(),
            content: "Old message".to_string(),
            created_at: stale_ts,
            is_editing: false,
            metadata_override: None,
            created_at_override: None,
            harness_override: None,
            model_override: None,
            logical_effort_override: None,
            service_tier_override: None,
            force_new_provider_session: false,
            composer_project_references: Vec::new(),
            composer_integration_references: Vec::new(),
            composer_artifact_references: Vec::new(),
            attachment_ids: Vec::new(),
        });
        q.push(QueuedMessage {
            id: "fresh-1".to_string(),
            content: "Fresh message".to_string(),
            created_at: fresh_ts,
            is_editing: false,
            metadata_override: None,
            created_at_override: None,
            harness_override: None,
            model_override: None,
            logical_effort_override: None,
            service_tier_override: None,
            force_new_provider_session: false,
            composer_project_references: Vec::new(),
            composer_integration_references: Vec::new(),
            composer_artifact_references: Vec::new(),
            attachment_ids: Vec::new(),
        });
    }

    // Threshold: 300s — stale-1 (600s old) should be dropped, fresh-1 kept
    let dropped = queue.remove_stale(ChatContextType::Ideation, "sess-stale", 300);
    assert_eq!(dropped.len(), 1);
    assert_eq!(dropped[0].id, "stale-1");

    let remaining = queue.get_queued(ChatContextType::Ideation, "sess-stale");
    assert_eq!(remaining.len(), 1);
    assert_eq!(remaining[0].id, "fresh-1");
}

#[test]
fn test_remove_stale_empty_queue() {
    let queue = MessageQueue::new();
    let dropped = queue.remove_stale(ChatContextType::Task, "nonexistent", 300);
    assert!(dropped.is_empty());
}

#[test]
fn test_remove_stale_all_fresh_messages_retained() {
    let queue = MessageQueue::new();

    // Fresh messages (created now)
    queue.queue(ChatContextType::Task, "task-fresh", "Msg 1".to_string());
    queue.queue(ChatContextType::Task, "task-fresh", "Msg 2".to_string());

    let dropped = queue.remove_stale(ChatContextType::Task, "task-fresh", 300);
    assert!(dropped.is_empty());

    let remaining = queue.get_queued(ChatContextType::Task, "task-fresh");
    assert_eq!(remaining.len(), 2);
}

#[test]
fn test_remove_stale_rehydration_messages_retained() {
    // queue_front messages are created with fresh timestamps — they must survive the staleness check
    let queue = MessageQueue::new();

    // Simulate a rehydration message injected by queue_front (freshly created)
    let rehydration = queue.queue_front(
        ChatContextType::Ideation,
        "sess-recover",
        "Rehydration prompt".to_string(),
    );

    let dropped = queue.remove_stale(ChatContextType::Ideation, "sess-recover", 300);
    assert!(
        dropped.is_empty(),
        "Fresh rehydration message should not be dropped"
    );

    let remaining = queue.get_queued(ChatContextType::Ideation, "sess-recover");
    assert_eq!(remaining.len(), 1);
    assert_eq!(remaining[0].id, rehydration.id);
}

#[test]
fn test_queue_with_overrides_preserves_metadata_and_timestamp() {
    let queue = MessageQueue::new();
    let metadata = r#"{"auto_verification":true}"#.to_string();
    let timestamp = "2026-03-11T10:00:00Z".to_string();
    let harness_override = crate::domain::agents::AgentHarnessKind::Codex;

    let queued = queue.queue_with_overrides(
        ChatContextType::Ideation,
        "sess-1",
        "AUTO-VERIFICATION MODE".to_string(),
        Some(metadata.clone()),
        Some(timestamp.clone()),
        Some(harness_override),
    );

    assert_eq!(queued.metadata_override, Some(metadata));
    assert_eq!(queued.created_at_override, Some(timestamp));
    assert_eq!(queued.harness_override, Some(harness_override));

    let popped = queue.pop(ChatContextType::Ideation, "sess-1").unwrap();
    assert_eq!(
        popped.metadata_override.as_deref(),
        Some(r#"{"auto_verification":true}"#)
    );
    assert_eq!(
        popped.created_at_override.as_deref(),
        Some("2026-03-11T10:00:00Z")
    );
    assert_eq!(popped.harness_override, Some(harness_override));
}

#[test]
fn test_queue_with_overrides_preserves_composer_project_references() {
    let queue = MessageQueue::new();
    let references = vec![ComposerProjectReference {
        path: "src/main.ts".to_string(),
        kind: Some(ComposerProjectReferenceKind::File),
    }];

    let queued = queue.queue_with_overrides_and_project_references(
        ChatContextType::Project,
        "project-1",
        "Read @src/main.ts".to_string(),
        None,
        None,
        None,
        references.clone(),
        Vec::new(),
        Vec::new(),
        Vec::new(),
    );

    assert_eq!(queued.composer_project_references, references);
    let popped = queue.pop(ChatContextType::Project, "project-1").unwrap();
    assert_eq!(popped.composer_project_references, references);
}

#[test]
fn test_queue_with_overrides_preserves_composer_integration_references() {
    let queue = MessageQueue::new();
    let references = vec![ComposerIntegrationReference {
        provider: "atlassian".to_string(),
        kind: "jira".to_string(),
        id: "RX-42".to_string(),
        key: Some("RX-42".to_string()),
        title: Some("Fix composer search".to_string()),
        url: Some("https://example.atlassian.net/browse/RX-42".to_string()),
        summary_excerpt: None,
        include_transcript: None,
        selected_excerpt: None,
        selected_source_path: None,
        selected_range_label: None,
    }];

    let queued = queue.queue_with_overrides_and_project_references(
        ChatContextType::Project,
        "project-1",
        "Read @jira:RX-42".to_string(),
        None,
        None,
        None,
        Vec::new(),
        references.clone(),
        Vec::new(),
        Vec::new(),
    );

    assert_eq!(queued.composer_integration_references, references);
    let popped = queue.pop(ChatContextType::Project, "project-1").unwrap();
    assert_eq!(popped.composer_integration_references, references);
}

#[test]
fn composer_integration_reference_deserializes_legacy_atlassian_metadata_defaults() {
    let reference: ComposerIntegrationReference = serde_json::from_str(
        r#"{"provider":"atlassian","kind":"jira","id":"RX-42","key":"RX-42"}"#,
    )
    .expect("legacy Atlassian reference should deserialize");

    assert_eq!(reference.provider, "atlassian");
    assert_eq!(reference.kind, "jira");
    assert_eq!(reference.key.as_deref(), Some("RX-42"));
    assert_eq!(reference.summary_excerpt, None);
    assert_eq!(reference.include_transcript, None);
}

#[test]
fn composer_integration_reference_serializes_granola_prompt_metadata() {
    let reference = ComposerIntegrationReference {
        provider: "granola".to_string(),
        kind: "note".to_string(),
        id: "not_1234567890ABCD".to_string(),
        key: None,
        title: Some("Planning note".to_string()),
        url: Some("https://granola.ai/notes/not_1234567890ABCD".to_string()),
        summary_excerpt: Some("Decision summary".to_string()),
        include_transcript: Some(true),
        selected_excerpt: None,
        selected_source_path: None,
        selected_range_label: None,
    };

    let value = serde_json::to_value(&reference).expect("serialize Granola reference");

    assert_eq!(value["provider"], "granola");
    assert_eq!(value["kind"], "note");
    assert_eq!(value["summaryExcerpt"], "Decision summary");
    assert_eq!(value["includeTranscript"], true);
    assert!(value.get("summary_excerpt").is_none());
    assert!(value.get("include_transcript").is_none());
}

#[test]
fn test_queue_with_overrides_preserves_composer_artifact_references() {
    let queue = MessageQueue::new();
    let references = vec![ComposerArtifactReference {
        artifact_id: "artifact-1".to_string(),
        kind: "plan".to_string(),
        title: Some("Implementation Plan".to_string()),
        session_id: Some("session-1".to_string()),
        version: Some(3),
        status: Some("approved".to_string()),
    }];

    let queued = queue.queue_with_overrides_and_project_references(
        ChatContextType::Project,
        "project-1",
        "Use @plan:artifact-1".to_string(),
        None,
        None,
        None,
        Vec::new(),
        Vec::new(),
        references.clone(),
        Vec::new(),
    );

    assert_eq!(queued.composer_artifact_references, references);
    let popped = queue.pop(ChatContextType::Project, "project-1").unwrap();
    assert_eq!(popped.composer_artifact_references, references);
}

#[test]
fn test_queue_with_overrides_preserves_attachment_ids() {
    use crate::domain::entities::ChatAttachmentId;

    let queue = MessageQueue::new();
    let attachment_ids = vec![ChatAttachmentId::new(), ChatAttachmentId::new()];

    let queued = queue.queue_with_overrides_and_project_references(
        ChatContextType::Project,
        "project-1",
        "Read the attached files".to_string(),
        None,
        None,
        None,
        Vec::new(),
        Vec::new(),
        Vec::new(),
        attachment_ids.clone(),
    );

    assert_eq!(queued.attachment_ids, attachment_ids);
    let popped = queue.pop(ChatContextType::Project, "project-1").unwrap();
    assert_eq!(popped.attachment_ids, attachment_ids);
}

#[test]
fn test_queue_standard_has_no_overrides() {
    let queue = MessageQueue::new();
    let queued = queue.queue(
        ChatContextType::Task,
        "task-1",
        "Normal message".to_string(),
    );
    assert_eq!(queued.metadata_override, None);
    assert_eq!(queued.created_at_override, None);
    assert_eq!(queued.harness_override, None);
    assert_eq!(queued.model_override, None);
    assert_eq!(queued.logical_effort_override, None);
    assert!(!queued.force_new_provider_session);
    assert!(queued.composer_project_references.is_empty());
    assert!(queued.composer_integration_references.is_empty());
    assert!(queued.attachment_ids.is_empty());
}

#[test]
fn test_queued_message_serialization_skips_default_fresh_session_flag() {
    let queued = QueuedMessage::new("Normal message".to_string());
    let value = serde_json::to_value(&queued).expect("queued message should serialize");
    assert_eq!(value.get("force_new_provider_session"), None);

    let mut fresh = QueuedMessage::new("Switch provider".to_string());
    fresh.force_new_provider_session = true;
    let value = serde_json::to_value(&fresh).expect("queued message should serialize");
    assert_eq!(value["force_new_provider_session"], true);
}

#[test]
fn test_queued_message_with_id_has_no_runtime_overrides() {
    let queued = QueuedMessage::with_id(
        "client-message-1".to_string(),
        "Client tracked message".to_string(),
    );

    assert_eq!(queued.id, "client-message-1");
    assert_eq!(queued.model_override, None);
    assert_eq!(queued.logical_effort_override, None);
    assert!(!queued.force_new_provider_session);
}

#[test]
fn test_queue_with_runtime_overrides_preserves_selection() {
    let queue = MessageQueue::new();

    let queued = queue.queue_with_runtime_overrides_and_project_references(
        ChatContextType::Project,
        "project-runtime",
        "switch provider".to_string(),
        Some(r#"{"source":"runtime-picker"}"#.to_string()),
        Some("2026-06-12T12:00:00Z".to_string()),
        Some(AgentHarnessKind::Codex),
        Some("gpt-5.5".to_string()),
        Some(LogicalEffort::XHigh),
        Some("fast".to_string()),
        true,
        Vec::new(),
        Vec::new(),
        Vec::new(),
        Vec::new(),
    );

    assert_eq!(queued.harness_override, Some(AgentHarnessKind::Codex));
    assert_eq!(queued.model_override.as_deref(), Some("gpt-5.5"));
    assert_eq!(queued.logical_effort_override, Some(LogicalEffort::XHigh));
    assert_eq!(queued.service_tier_override.as_deref(), Some("fast"));
    assert!(queued.force_new_provider_session);

    let stored = queue.get_queued(ChatContextType::Project, "project-runtime");
    assert_eq!(stored.len(), 1);
    assert_eq!(stored[0], queued);
}

#[test]
fn test_remove_stale_unparseable_timestamp_retained() {
    let queue = MessageQueue::new();

    {
        let key = QueueKey::new(ChatContextType::Task, "task-bad-ts".to_string());
        let mut queues = queue.queues.lock().unwrap();
        let q = queues.entry(key).or_default();
        q.push(QueuedMessage {
            id: "bad-ts-1".to_string(),
            content: "Unparseable timestamp".to_string(),
            created_at: "not-a-timestamp".to_string(),
            is_editing: false,
            metadata_override: None,
            created_at_override: None,
            harness_override: None,
            model_override: None,
            logical_effort_override: None,
            service_tier_override: None,
            force_new_provider_session: false,
            composer_project_references: Vec::new(),
            composer_integration_references: Vec::new(),
            composer_artifact_references: Vec::new(),
            attachment_ids: Vec::new(),
        });
    }

    // Messages with unparseable timestamps should be retained (safe default)
    let dropped = queue.remove_stale(ChatContextType::Task, "task-bad-ts", 300);
    assert!(
        dropped.is_empty(),
        "Unparseable timestamp should be retained"
    );

    let remaining = queue.get_queued(ChatContextType::Task, "task-bad-ts");
    assert_eq!(remaining.len(), 1);
}
