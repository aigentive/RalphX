use super::*;
use crate::agents::AgentHarnessKind;

#[test]
fn test_conversation_id_creation() {
    let id1 = ChatConversationId::new();
    let id2 = ChatConversationId::new();
    assert_ne!(id1, id2);
}

#[test]
fn test_conversation_id_from_string() {
    let id = ChatConversationId::new();
    let str_id = id.to_string();
    let parsed_id: ChatConversationId = str_id.parse().unwrap();
    assert_eq!(id, parsed_id);
}

#[test]
fn test_context_type_serialization() {
    assert_eq!(ChatContextType::Ideation.to_string(), "ideation");
    assert_eq!(ChatContextType::Task.to_string(), "task");
    assert_eq!(ChatContextType::Project.to_string(), "project");
    assert_eq!(ChatContextType::Standalone.to_string(), "standalone");
    assert_eq!(ChatContextType::TaskExecution.to_string(), "task_execution");
    assert_eq!(ChatContextType::Review.to_string(), "review");
    assert_eq!(ChatContextType::BranchUpdate.to_string(), "branch_update");
}

#[test]
fn test_context_type_parsing() {
    assert_eq!(
        "ideation".parse::<ChatContextType>().unwrap(),
        ChatContextType::Ideation
    );
    assert_eq!(
        "task".parse::<ChatContextType>().unwrap(),
        ChatContextType::Task
    );
    assert_eq!(
        "project".parse::<ChatContextType>().unwrap(),
        ChatContextType::Project
    );
    assert_eq!(
        "standalone".parse::<ChatContextType>().unwrap(),
        ChatContextType::Standalone
    );
    assert_eq!(
        "task_execution".parse::<ChatContextType>().unwrap(),
        ChatContextType::TaskExecution
    );
    assert_eq!(
        "review".parse::<ChatContextType>().unwrap(),
        ChatContextType::Review
    );
    assert_eq!(
        "branch_update".parse::<ChatContextType>().unwrap(),
        ChatContextType::BranchUpdate
    );
    assert!("invalid".parse::<ChatContextType>().is_err());
}

#[test]
fn test_new_branch_update_conversation() {
    let task_id = TaskId::from_string("branch-update-task".to_string());
    let conversation = ChatConversation::new_branch_update(task_id);
    assert_eq!(conversation.context_type, ChatContextType::BranchUpdate);
    assert_eq!(conversation.context_id, "branch-update-task");
    assert_eq!(conversation.coordination_mode, CoordinationMode::Solo);
}

#[test]
fn test_new_standalone_conversation_is_self_keyed() {
    let conversation = ChatConversation::new_standalone();
    assert_eq!(conversation.context_type, ChatContextType::Standalone);
    assert_eq!(conversation.context_id, conversation.id.as_str());
    assert_eq!(conversation.coordination_mode, CoordinationMode::Solo);
    assert!(conversation.is_valid_standalone_self_key());
}

#[test]
fn test_new_standalone_conversations_have_distinct_ids() {
    let first = ChatConversation::new_standalone();
    let second = ChatConversation::new_standalone();
    assert_ne!(first.id, second.id);
    assert_ne!(first.context_id, second.context_id);
}

#[test]
fn persona_builder_identity_requires_supported_context_and_mode() {
    for context_type in [ChatContextType::Project, ChatContextType::Standalone] {
        assert!(ChatConversation::is_persona_builder_identity(
            context_type,
            Some(AgentConversationWorkspaceMode::PersonaBuilder),
        ));
        assert!(!ChatConversation::is_persona_builder_identity(
            context_type,
            Some(AgentConversationWorkspaceMode::Chat),
        ));
    }

    for context_type in [
        ChatContextType::Ideation,
        ChatContextType::Delegation,
        ChatContextType::Task,
        ChatContextType::TaskExecution,
        ChatContextType::Review,
        ChatContextType::Merge,
        ChatContextType::BranchUpdate,
    ] {
        assert!(!ChatConversation::is_persona_builder_identity(
            context_type,
            Some(AgentConversationWorkspaceMode::PersonaBuilder),
        ));
    }
}

#[test]
fn test_is_valid_standalone_self_key_rejects_mismatched_context_id() {
    let mut conversation = ChatConversation::new_standalone();
    conversation.context_id = "not-my-own-id".to_string();
    assert!(!conversation.is_valid_standalone_self_key());
}

#[test]
fn test_is_valid_standalone_self_key_rejects_non_standalone_context_type() {
    let conversation = ChatConversation::new_project(ProjectId::from_string("proj-1".to_string()));
    // Even in the (impossible in production) case where context_id happened to
    // equal the conversation id, the context_type gate must still reject it.
    let mut conversation = conversation;
    conversation.context_id = conversation.id.as_str();
    assert!(!conversation.is_valid_standalone_self_key());
}

#[test]
fn test_new_ideation_conversation() {
    let session_id = IdeationSessionId::new();
    let expected_context_id = session_id.as_str().to_string();
    let conv = ChatConversation::new_ideation(session_id);

    assert_eq!(conv.context_type, ChatContextType::Ideation);
    assert_eq!(conv.context_id, expected_context_id);
    assert_eq!(conv.claude_session_id, None);
    assert_eq!(conv.provider_session_id, None);
    assert_eq!(conv.provider_harness, None);
    assert_eq!(conv.message_count, 0);
    assert!(!conv.has_claude_session());
}

#[test]
fn test_set_claude_session_id() {
    let session_id = IdeationSessionId::new();
    let mut conv = ChatConversation::new_ideation(session_id);

    conv.set_claude_session_id("550e8400-e29b-41d4-a716-446655440000");
    assert!(conv.has_claude_session());
    assert_eq!(
        conv.claude_session_id,
        Some("550e8400-e29b-41d4-a716-446655440000".to_string())
    );
    assert_eq!(
        conv.provider_session_id,
        Some("550e8400-e29b-41d4-a716-446655440000".to_string())
    );
    assert_eq!(conv.provider_harness, Some(AgentHarnessKind::Claude));
}

#[test]
fn test_set_provider_session_ref_for_codex() {
    let session_id = IdeationSessionId::new();
    let mut conv = ChatConversation::new_ideation(session_id);

    conv.set_provider_session_ref(crate::agents::ProviderSessionRef {
        harness: AgentHarnessKind::Codex,
        provider_session_id: "codex-session-123".to_string(),
    });

    assert_eq!(conv.provider_harness, Some(AgentHarnessKind::Codex));
    assert_eq!(
        conv.provider_session_id,
        Some("codex-session-123".to_string())
    );
    assert_eq!(conv.claude_session_id, None);
    assert!(!conv.has_claude_session());
}

#[test]
fn test_provider_session_ref_falls_back_to_legacy_claude_field() {
    let session_id = IdeationSessionId::new();
    let mut conv = ChatConversation::new_ideation(session_id);
    conv.claude_session_id = Some("legacy-session".to_string());

    let session_ref = conv.provider_session_ref().expect("provider session");
    assert_eq!(session_ref.harness, AgentHarnessKind::Claude);
    assert_eq!(session_ref.provider_session_id, "legacy-session");
}

#[test]
fn test_legacy_claude_session_alias_only_applies_to_claude() {
    assert_eq!(
        legacy_claude_session_alias(Some(AgentHarnessKind::Claude), Some("session-123")),
        Some("session-123".to_string())
    );
    assert_eq!(
        legacy_claude_session_alias(Some(AgentHarnessKind::Codex), Some("session-123")),
        None
    );
}

#[test]
fn test_compatible_provider_session_fields_from_provider_ref_restores_legacy_alias_for_claude() {
    let (claude_session_id, provider_session_id, provider_harness) =
        compatible_provider_session_fields_from_provider_ref(
            Some(AgentHarnessKind::Claude),
            Some("session-123".to_string()),
        );

    assert_eq!(claude_session_id.as_deref(), Some("session-123"));
    assert_eq!(provider_session_id.as_deref(), Some("session-123"));
    assert_eq!(provider_harness, Some(AgentHarnessKind::Claude));
}

#[test]
fn test_compatible_provider_session_fields_from_provider_ref_keeps_codex_without_legacy_alias() {
    let (claude_session_id, provider_session_id, provider_harness) =
        compatible_provider_session_fields_from_provider_ref(
            Some(AgentHarnessKind::Codex),
            Some("thread-123".to_string()),
        );

    assert_eq!(claude_session_id, None);
    assert_eq!(provider_session_id.as_deref(), Some("thread-123"));
    assert_eq!(provider_harness, Some(AgentHarnessKind::Codex));
}

#[test]
fn test_normalize_provider_session_compatibility_from_legacy_claude_field() {
    let (claude_session_id, provider_session_id, provider_harness) =
        normalize_provider_session_compatibility(Some("legacy-session".to_string()), None, None);

    assert_eq!(claude_session_id.as_deref(), Some("legacy-session"));
    assert_eq!(provider_session_id.as_deref(), Some("legacy-session"));
    assert_eq!(provider_harness, Some(AgentHarnessKind::Claude));
}

#[test]
fn test_normalize_provider_session_compatibility_restores_legacy_alias_for_claude() {
    let (claude_session_id, provider_session_id, provider_harness) =
        normalize_provider_session_compatibility(
            None,
            Some("provider-session".to_string()),
            Some(AgentHarnessKind::Claude),
        );

    assert_eq!(claude_session_id.as_deref(), Some("provider-session"));
    assert_eq!(provider_session_id.as_deref(), Some("provider-session"));
    assert_eq!(provider_harness, Some(AgentHarnessKind::Claude));
}

#[test]
fn test_normalize_provider_session_compatibility_keeps_codex_without_legacy_alias() {
    let (claude_session_id, provider_session_id, provider_harness) =
        normalize_provider_session_compatibility(
            None,
            Some("codex-session".to_string()),
            Some(AgentHarnessKind::Codex),
        );

    assert_eq!(claude_session_id, None);
    assert_eq!(provider_session_id.as_deref(), Some("codex-session"));
    assert_eq!(provider_harness, Some(AgentHarnessKind::Codex));
}

#[test]
fn test_compatible_provider_session_fields_restore_legacy_claude_alias() {
    let session_id = IdeationSessionId::new();
    let mut conv = ChatConversation::new_ideation(session_id);
    conv.provider_harness = Some(AgentHarnessKind::Claude);
    conv.provider_session_id = Some("claude-session-456".to_string());

    let (claude_session_id, provider_session_id, provider_harness) =
        conv.compatible_provider_session_fields();

    assert_eq!(claude_session_id.as_deref(), Some("claude-session-456"));
    assert_eq!(provider_session_id.as_deref(), Some("claude-session-456"));
    assert_eq!(provider_harness, Some(AgentHarnessKind::Claude));
}

#[test]
fn test_compatible_provider_session_fields_derive_provider_metadata_from_legacy_alias() {
    let session_id = IdeationSessionId::new();
    let mut conv = ChatConversation::new_ideation(session_id);
    conv.claude_session_id = Some("legacy-session".to_string());

    let (claude_session_id, provider_session_id, provider_harness) =
        conv.compatible_provider_session_fields();

    assert_eq!(claude_session_id.as_deref(), Some("legacy-session"));
    assert_eq!(provider_session_id.as_deref(), Some("legacy-session"));
    assert_eq!(provider_harness, Some(AgentHarnessKind::Claude));
}

#[test]
fn test_normalize_provider_session_fields_in_place_from_legacy_alias() {
    let session_id = IdeationSessionId::new();
    let mut conv = ChatConversation::new_ideation(session_id);
    conv.claude_session_id = Some("legacy-session".to_string());

    conv.normalize_provider_session_fields();

    assert_eq!(conv.claude_session_id.as_deref(), Some("legacy-session"));
    assert_eq!(conv.provider_session_id.as_deref(), Some("legacy-session"));
    assert_eq!(conv.provider_harness, Some(AgentHarnessKind::Claude));
}

#[test]
fn test_normalize_provider_session_fields_in_place_restores_legacy_claude_alias() {
    let session_id = IdeationSessionId::new();
    let mut conv = ChatConversation::new_ideation(session_id);
    conv.provider_harness = Some(AgentHarnessKind::Claude);
    conv.provider_session_id = Some("claude-session-456".to_string());

    conv.normalize_provider_session_fields();

    assert_eq!(
        conv.claude_session_id.as_deref(),
        Some("claude-session-456")
    );
    assert_eq!(
        conv.provider_session_id.as_deref(),
        Some("claude-session-456")
    );
    assert_eq!(conv.provider_harness, Some(AgentHarnessKind::Claude));
}

#[test]
fn test_set_title() {
    let session_id = IdeationSessionId::new();
    let mut conv = ChatConversation::new_ideation(session_id);

    conv.set_title("Dark mode implementation");
    assert_eq!(conv.display_title(), "Dark mode implementation");
}

#[test]
fn test_display_title_default() {
    let session_id = IdeationSessionId::new();
    let conv = ChatConversation::new_ideation(session_id);
    assert_eq!(conv.display_title(), "Untitled conversation");
}

#[test]
fn test_new_task_execution_conversation() {
    let task_id = TaskId::new();
    let expected_context_id = task_id.as_str().to_string();
    let conv = ChatConversation::new_task_execution(task_id);

    assert_eq!(conv.context_type, ChatContextType::TaskExecution);
    assert_eq!(conv.context_id, expected_context_id);
    assert_eq!(conv.claude_session_id, None);
    assert_eq!(conv.provider_session_id, None);
    assert_eq!(conv.message_count, 0);
    assert!(!conv.has_claude_session());
}

#[test]
fn test_new_review_conversation() {
    let task_id = TaskId::new();
    let expected_context_id = task_id.as_str().to_string();
    let conv = ChatConversation::new_review(task_id);

    assert_eq!(conv.context_type, ChatContextType::Review);
    assert_eq!(conv.context_id, expected_context_id);
    assert_eq!(conv.claude_session_id, None);
    assert_eq!(conv.provider_session_id, None);
    assert_eq!(conv.message_count, 0);
    assert!(!conv.has_claude_session());
}
