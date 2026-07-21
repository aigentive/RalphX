use super::*;

#[test]
fn conversation_launch_security_class_is_exhaustive_and_preserves_builder_compatibility() {
    assert_eq!(
        conversation_launch_security_class(
            ChatContextType::Standalone,
            Some(AgentConversationWorkspaceMode::Chat),
        ),
        ConversationLaunchSecurityClass::StandaloneContainedChat,
    );
    assert_eq!(
        conversation_launch_security_class(
            ChatContextType::Standalone,
            Some(AgentConversationWorkspaceMode::PersonaBuilder),
        ),
        ConversationLaunchSecurityClass::ConfiguredMcp,
    );

    for context_type in [
        ChatContextType::Ideation,
        ChatContextType::Delegation,
        ChatContextType::Task,
        ChatContextType::Project,
        ChatContextType::TaskExecution,
        ChatContextType::Review,
        ChatContextType::Merge,
        ChatContextType::BranchUpdate,
    ] {
        assert_eq!(
            conversation_launch_security_class(
                context_type,
                Some(AgentConversationWorkspaceMode::Chat),
            ),
            ConversationLaunchSecurityClass::ConfiguredMcp,
            "{context_type:?} must retain the configured MCP launch contract",
        );
    }
}

#[test]
fn conversation_launch_identity_rejects_context_downgrade_before_building() {
    let conversation = ChatConversation::new_standalone();
    let conversation_id = conversation.id.as_str();

    validate_conversation_launch_identity(
        &conversation,
        conversation_id.as_str(),
        conversation.context_type,
        conversation.context_id.as_str(),
    )
    .expect("matching persisted conversation identity should be accepted");

    let error = validate_conversation_launch_identity(
        &conversation,
        conversation_id.as_str(),
        ChatContextType::Project,
        conversation.context_id.as_str(),
    )
    .expect_err("a caller must not relabel persisted Standalone state as Project");

    assert!(error.contains("context type"), "unexpected error: {error}");
    assert!(error.contains("standalone"), "unexpected error: {error}");
    assert!(error.contains("project"), "unexpected error: {error}");
}
