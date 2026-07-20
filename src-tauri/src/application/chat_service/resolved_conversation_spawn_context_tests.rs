use super::*;
use chat_service_context::ResolvedConversationSpawnContext;

fn canonical_enforcement(
    context_type: ChatContextType,
    effective_mode: Option<AgentConversationWorkspaceMode>,
    working_directory: &Path,
) -> bool {
    chat_service_context::build_mcp_runtime_context(
        context_type,
        "context-id",
        None,
        "conversation-id",
        None,
        working_directory,
        None,
        None,
        &[],
        None,
        None,
        effective_mode,
    )
    .enforce_filesystem_roots
}

#[test]
fn fallback_spawn_context_uses_authoritative_enforcement_and_keeps_roots_empty() {
    let working_directory = Path::new("/fallback-workspace");

    for (context_type, effective_mode, expected) in [
        (
            ChatContextType::Project,
            Some(AgentConversationWorkspaceMode::PersonaBuilder),
            true,
        ),
        (ChatContextType::Standalone, None, true),
        (ChatContextType::Project, None, false),
    ] {
        let fallback = ResolvedConversationSpawnContext::without_app_state(
            context_type,
            effective_mode,
            working_directory,
        );

        assert_eq!(
            fallback.enforce_filesystem_roots,
            canonical_enforcement(context_type, effective_mode, working_directory)
        );
        assert_eq!(fallback.enforce_filesystem_roots, expected);
        assert!(fallback.folder_roots.is_empty());
        assert!(fallback.folder_refs_block.is_none());
        assert_eq!(fallback.workspace_root, working_directory);
    }
}

#[test]
fn ordinary_project_fallback_shape_remains_byte_for_byte_unchanged() {
    let working_directory = Path::new("/ordinary-project-workspace");
    let fallback = ResolvedConversationSpawnContext::without_app_state(
        ChatContextType::Project,
        None,
        working_directory,
    );

    assert_eq!(
        fallback,
        ResolvedConversationSpawnContext {
            folder_refs_block: None,
            folder_roots: Vec::new(),
            workspace_root: working_directory.to_path_buf(),
            enforce_filesystem_roots: false,
        }
    );
}
