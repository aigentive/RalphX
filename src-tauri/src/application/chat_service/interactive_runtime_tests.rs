use super::{
    agent_conversation_mode_for_send, conversation_spawn_harness_override,
    edit_mode_plan_handoff_runtime_message, get_agent_name,
    interactive_run_started_provider_session, persona_switch_requires_process_invalidation,
    plan_mode_runtime_message, provider_harness_switch_requires_fresh_session,
    registered_persona_metadata, resolve_agent_name_for_send,
    should_inherit_parent_harness_for_fresh_spawn, spawn_settings_require_task_metadata,
};
use crate::application::interactive_process_registry::InteractiveProcessMetadata;
use crate::application::persona_prompt::ResolvedPersona;
use crate::domain::agents::{AgentHarnessKind, ProviderSessionRef};
use crate::domain::entities::{
    AgentConversationWorkspace, AgentConversationWorkspaceMode, Artifact, ArtifactId, ArtifactType,
    ChatContextType, ChatConversation, ChatConversationId, IdeationAnalysisBaseRefKind,
    IdeationSessionId, PersonaId, ProjectId, TaskId,
};

fn resolved_persona(id: &str, content_hash: &str) -> ResolvedPersona {
    ResolvedPersona {
        id: PersonaId::from(id),
        slug: id.to_string(),
        version: 1,
        content_hash: content_hash.to_string(),
        block: String::new(),
    }
}
use crate::infrastructure::agents::claude::agent_names::{
    AGENT_AUTOMATION_SETUP, AGENT_CHAT_PROJECT, AGENT_GENERAL_EXPLORER, AGENT_GENERAL_WORKER,
    AGENT_ORCHESTRATOR_IDEATION, AGENT_PR_REVIEWER,
};

#[test]
fn interactive_run_started_provider_session_prefers_process_metadata_harness() {
    let conversation =
        ChatConversation::new_ideation(IdeationSessionId::from_string("session-1".to_string()));

    let (harness, provider_session_id) = interactive_run_started_provider_session(
        &conversation,
        Some(&InteractiveProcessMetadata {
            harness: Some(AgentHarnessKind::Codex),
            provider_session_id: None,
            persona_id: None,
            persona_content_hash: None,
        }),
    );

    assert_eq!(harness, AgentHarnessKind::Codex);
    assert_eq!(provider_session_id, None);
}

#[test]
fn interactive_run_started_provider_session_falls_back_to_conversation_session_ref() {
    let mut conversation =
        ChatConversation::new_task_execution(TaskId::from_string("task-1".to_string()));
    conversation.set_provider_session_ref(ProviderSessionRef {
        harness: AgentHarnessKind::Claude,
        provider_session_id: "claude-session-123".to_string(),
    });

    let (harness, provider_session_id) =
        interactive_run_started_provider_session(&conversation, None);

    assert_eq!(harness, AgentHarnessKind::Claude);
    assert_eq!(provider_session_id.as_deref(), Some("claude-session-123"));
}

#[test]
fn provider_harness_switch_requires_fresh_session_for_process_harness_mismatch() {
    let requires_fresh = provider_harness_switch_requires_fresh_session(
        Some(AgentHarnessKind::Codex),
        None,
        Some(&InteractiveProcessMetadata {
            harness: Some(AgentHarnessKind::Claude),
            provider_session_id: Some("claude-session-123".to_string()),
            persona_id: None,
            persona_content_hash: None,
        }),
    );

    assert!(requires_fresh);
}

#[test]
fn provider_harness_switch_uses_conversation_when_process_harness_missing() {
    let mut conversation =
        ChatConversation::new_task_execution(TaskId::from_string("task-2".to_string()));
    conversation.set_provider_session_ref(ProviderSessionRef {
        harness: AgentHarnessKind::Claude,
        provider_session_id: "claude-session-123".to_string(),
    });

    let requires_fresh = provider_harness_switch_requires_fresh_session(
        Some(AgentHarnessKind::Codex),
        Some(&conversation),
        Some(&InteractiveProcessMetadata {
            harness: None,
            provider_session_id: Some("legacy-session-123".to_string()),
            persona_id: None,
            persona_content_hash: None,
        }),
    );

    assert!(requires_fresh);
}

#[test]
fn persona_invalidation_for_content_hash_mismatch() {
    let resolved = resolved_persona("persona-a", "new-hash");
    let process = InteractiveProcessMetadata {
        harness: Some(AgentHarnessKind::Claude),
        provider_session_id: Some("claude-session-123".to_string()),
        persona_id: Some("persona-a".to_string()),
        persona_content_hash: Some("old-hash".to_string()),
    };

    assert!(persona_switch_requires_process_invalidation(
        Some(&resolved),
        Some(&process)
    ));
}

#[test]
fn persona_invalidation_for_persona_id_mismatch() {
    let resolved = resolved_persona("persona-b", "hash-b");
    let bound = InteractiveProcessMetadata {
        harness: None,
        provider_session_id: None,
        persona_id: Some("persona-a".to_string()),
        persona_content_hash: Some("hash-a".to_string()),
    };
    let unbound = InteractiveProcessMetadata::default();

    assert!(persona_switch_requires_process_invalidation(
        Some(&resolved),
        Some(&bound)
    ));
    assert!(persona_switch_requires_process_invalidation(
        None,
        Some(&bound)
    ));
    assert!(persona_switch_requires_process_invalidation(
        Some(&resolved),
        Some(&unbound)
    ));
}

#[test]
fn persona_invalidation_skipped_when_both_unbound() {
    assert!(!persona_switch_requires_process_invalidation(
        None,
        Some(&InteractiveProcessMetadata::default())
    ));
    assert!(!persona_switch_requires_process_invalidation(None, None));
}

#[test]
fn persona_invalidation_independent_of_harness_override() {
    let resolved = resolved_persona("persona-a", "hash-a");
    let process = InteractiveProcessMetadata {
        harness: Some(AgentHarnessKind::Codex),
        provider_session_id: Some("codex-session-123".to_string()),
        persona_id: Some("persona-a".to_string()),
        persona_content_hash: Some("stale-hash".to_string()),
    };

    assert!(persona_switch_requires_process_invalidation(
        Some(&resolved),
        Some(&process)
    ));
}

#[test]
fn injection_skipped_registers_persona_metadata_none() {
    let resolved = resolved_persona("persona-a", "hash-a");

    assert_eq!(
        registered_persona_metadata(Some(&resolved), true),
        (None, None)
    );
    assert_eq!(
        registered_persona_metadata(Some(&resolved), false),
        (Some("persona-a".to_string()), Some("hash-a".to_string()))
    );
}

#[test]
fn provider_harness_switch_keeps_same_provider_session() {
    let mut conversation =
        ChatConversation::new_task_execution(TaskId::from_string("task-3".to_string()));
    conversation.set_provider_session_ref(ProviderSessionRef {
        harness: AgentHarnessKind::Codex,
        provider_session_id: "codex-session-123".to_string(),
    });

    let requires_fresh = provider_harness_switch_requires_fresh_session(
        Some(AgentHarnessKind::Codex),
        Some(&conversation),
        None,
    );

    assert!(!requires_fresh);
}

#[test]
fn provider_harness_switch_requires_explicit_requested_provider() {
    let mut conversation =
        ChatConversation::new_task_execution(TaskId::from_string("task-4".to_string()));
    conversation.set_provider_session_ref(ProviderSessionRef {
        harness: AgentHarnessKind::Claude,
        provider_session_id: "claude-session-123".to_string(),
    });

    let requires_fresh =
        provider_harness_switch_requires_fresh_session(None, Some(&conversation), None);

    assert!(!requires_fresh);
}

#[test]
fn project_agent_send_uses_workspace_mode_agent_before_project_default() {
    let edit_agent = resolve_agent_name_for_send(
        &ChatContextType::Project,
        None,
        false,
        None,
        Some(AgentConversationWorkspaceMode::Edit),
    );
    let chat_agent = resolve_agent_name_for_send(
        &ChatContextType::Project,
        None,
        false,
        None,
        Some(AgentConversationWorkspaceMode::Chat),
    );
    let ideation_agent = resolve_agent_name_for_send(
        &ChatContextType::Project,
        None,
        false,
        None,
        Some(AgentConversationWorkspaceMode::Ideation),
    );
    let plan_agent = resolve_agent_name_for_send(
        &ChatContextType::Project,
        None,
        false,
        None,
        Some(AgentConversationWorkspaceMode::Plan),
    );
    let review_pr_agent = resolve_agent_name_for_send(
        &ChatContextType::Project,
        None,
        false,
        None,
        Some(AgentConversationWorkspaceMode::ReviewPr),
    );
    let automation_agent = resolve_agent_name_for_send(
        &ChatContextType::Project,
        None,
        false,
        None,
        Some(AgentConversationWorkspaceMode::Automation),
    );
    let default_project_agent =
        resolve_agent_name_for_send(&ChatContextType::Project, None, false, None, None);

    assert_eq!(edit_agent, AGENT_GENERAL_WORKER);
    assert_eq!(chat_agent, AGENT_GENERAL_EXPLORER);
    assert_eq!(plan_agent, AGENT_ORCHESTRATOR_IDEATION);
    assert_eq!(ideation_agent, AGENT_CHAT_PROJECT);
    assert_eq!(review_pr_agent, AGENT_PR_REVIEWER);
    assert_eq!(automation_agent, AGENT_AUTOMATION_SETUP);
    assert_eq!(default_project_agent, AGENT_CHAT_PROJECT);
}

#[test]
fn ideation_session_send_ignores_linked_workspace_mode_for_agent_selection() {
    // Regression: a genuine ideation session linked to an agent-conversation
    // workspace must resolve to the ideation orchestrator regardless of the
    // workspace's display mode. Previously an `Ideation`-mode workspace forced the
    // linked session onto `ralphx-chat-project`, which lacks the
    // proposal/plan/finalize tools, so the session produced no durable outputs.
    for workspace_mode in [
        AgentConversationWorkspaceMode::Chat,
        AgentConversationWorkspaceMode::Edit,
        AgentConversationWorkspaceMode::Ideation,
        AgentConversationWorkspaceMode::ReviewPr,
    ] {
        let mode =
            agent_conversation_mode_for_send(ChatContextType::Ideation, None, Some(workspace_mode));
        assert_eq!(
            mode, None,
            "ideation session must not inherit {workspace_mode:?} from a linked workspace"
        );

        let agent =
            resolve_agent_name_for_send(&ChatContextType::Ideation, None, false, None, mode);
        assert_eq!(
            agent, AGENT_ORCHESTRATOR_IDEATION,
            "ideation session linked to a {workspace_mode:?} workspace must use the orchestrator"
        );
    }
}

#[test]
fn ideation_session_send_preserves_plan_mode_profile() {
    // Plan mode's linked planning session keeps its constrained plan profile, so
    // its mode is still honored for the ideation context.
    let mode = agent_conversation_mode_for_send(
        ChatContextType::Ideation,
        None,
        Some(AgentConversationWorkspaceMode::Plan),
    );
    assert_eq!(mode, Some(AgentConversationWorkspaceMode::Plan));

    let agent = resolve_agent_name_for_send(&ChatContextType::Ideation, None, false, None, mode);
    assert_eq!(agent, AGENT_ORCHESTRATOR_IDEATION);
}

#[test]
fn project_workspace_conversation_send_keeps_mode_agent() {
    // Workspace conversations (Project context) still resolve by workspace mode:
    // `Ideation` mode intentionally stays on `ralphx-chat-project` + external v1_.
    let ideation_mode = agent_conversation_mode_for_send(
        ChatContextType::Project,
        None,
        Some(AgentConversationWorkspaceMode::Ideation),
    );
    assert_eq!(
        ideation_mode,
        Some(AgentConversationWorkspaceMode::Ideation)
    );
    let project_agent =
        resolve_agent_name_for_send(&ChatContextType::Project, None, false, None, ideation_mode);
    assert_eq!(project_agent, AGENT_CHAT_PROJECT);

    // A conversation-level agent_mode override still wins for Project context.
    let edit_mode = agent_conversation_mode_for_send(
        ChatContextType::Project,
        Some(AgentConversationWorkspaceMode::Edit),
        None,
    );
    assert_eq!(edit_mode, Some(AgentConversationWorkspaceMode::Edit));
}

#[test]
fn plan_mode_runtime_message_injects_linked_planning_session_context() {
    let conversation_id = ChatConversationId::from_string("conversation-plan-1".to_string());
    let mut workspace = AgentConversationWorkspace::new(
        conversation_id,
        ProjectId::from_string("project-plan-1".to_string()),
        AgentConversationWorkspaceMode::Plan,
        IdeationAnalysisBaseRefKind::CurrentBranch,
        "main".to_string(),
        None,
        None,
        "ralphx/project/agent-plan".to_string(),
        "/tmp/ralphx-plan-workspace".to_string(),
    );
    workspace.linked_ideation_session_id = Some(IdeationSessionId::from_string(
        "planning-session-1".to_string(),
    ));

    let message =
        plan_mode_runtime_message("draft the implementation".to_string(), Some(&workspace));

    assert!(message.contains("<plan_mode_context>"));
    assert!(message.contains("<planning_session_id>planning-session-1</planning_session_id>"));
    assert!(message.contains("Use this planning session for ask_user_question"));
    assert!(message.contains("<user_request>draft the implementation</user_request>"));
}

#[test]
fn edit_mode_plan_handoff_runtime_message_injects_linked_plan_context() {
    let conversation_id = ChatConversationId::from_string("conversation-plan-1".to_string());
    let mut workspace = AgentConversationWorkspace::new(
        conversation_id,
        ProjectId::from_string("project-plan-1".to_string()),
        AgentConversationWorkspaceMode::Edit,
        IdeationAnalysisBaseRefKind::CurrentBranch,
        "main".to_string(),
        None,
        None,
        "ralphx/project/agent-plan".to_string(),
        "/tmp/ralphx-plan-workspace".to_string(),
    );
    workspace.linked_ideation_session_id = Some(IdeationSessionId::from_string(
        "planning-session-1".to_string(),
    ));
    let mut artifact = Artifact::new_inline(
        "Plan Mode Implementation Plan",
        ArtifactType::Specification,
        "# Plan\n\nImplement the composer Plan mode.",
        "ralphx-ideation",
    );
    artifact.id = ArtifactId::from_string("plan-artifact-1".to_string());
    artifact.metadata.version = 3;

    let message = edit_mode_plan_handoff_runtime_message(
        "execute the plan".to_string(),
        Some(&workspace),
        Some(&artifact),
    );

    assert!(message.contains("<plan_execution_context>"));
    assert!(message.contains("<workspace_mode>edit</workspace_mode>"));
    assert!(message.contains("<planning_session_id>planning-session-1</planning_session_id>"));
    assert!(message.contains("<plan_artifact_reference kind=\"plan\""));
    assert!(message.contains("artifact_id=\"plan-artifact-1\""));
    assert!(message.contains("session_id=\"planning-session-1\""));
    assert!(message.contains("version=\"3\""));
    assert!(message.contains("Fetch the referenced plan artifact with get_artifact"));
    assert!(message.contains("edit the workspace branch directly"));
    assert!(message.contains("<user_request>execute the plan</user_request>"));
}

#[test]
fn edit_mode_plan_handoff_runtime_message_leaves_unlinked_edit_messages_unchanged() {
    let conversation_id = ChatConversationId::from_string("conversation-edit-1".to_string());
    let workspace = AgentConversationWorkspace::new(
        conversation_id,
        ProjectId::from_string("project-plan-1".to_string()),
        AgentConversationWorkspaceMode::Edit,
        IdeationAnalysisBaseRefKind::CurrentBranch,
        "main".to_string(),
        None,
        None,
        "ralphx/project/agent-edit".to_string(),
        "/tmp/ralphx-edit-workspace".to_string(),
    );

    let message = edit_mode_plan_handoff_runtime_message(
        "make a small edit".to_string(),
        Some(&workspace),
        None,
    );

    assert_eq!(message, "make a small edit");
}

#[test]
fn explicit_agent_override_wins_over_workspace_mode() {
    let agent = resolve_agent_name_for_send(
        &ChatContextType::Project,
        None,
        false,
        Some("custom-agent"),
        Some(AgentConversationWorkspaceMode::Edit),
    );

    assert_eq!(agent, "custom-agent");
}

#[test]
fn conversation_spawn_harness_override_falls_back_to_parent_conversation_for_recovery() {
    let task_id = TaskId::from_string("task-parent-1".to_string());
    let child = ChatConversation::new_task_execution(task_id.clone());
    let mut parent = ChatConversation::new_task_execution(task_id);
    parent.set_provider_session_ref(ProviderSessionRef {
        harness: AgentHarnessKind::Codex,
        provider_session_id: "codex-parent-session".to_string(),
    });

    let harness = conversation_spawn_harness_override(
        get_agent_name(&ChatContextType::TaskExecution),
        ChatContextType::TaskExecution,
        Some(r#"{"trigger_origin":"recovery"}"#),
        &child,
        Some(&parent),
    );

    assert_eq!(harness, Some(AgentHarnessKind::Codex));
}

#[test]
fn conversation_spawn_harness_override_skips_parent_for_retry() {
    let task_id = TaskId::from_string("task-parent-2".to_string());
    let child = ChatConversation::new_task_execution(task_id.clone());
    let mut parent = ChatConversation::new_task_execution(task_id);
    parent.set_provider_session_ref(ProviderSessionRef {
        harness: AgentHarnessKind::Codex,
        provider_session_id: "codex-parent-session".to_string(),
    });

    let harness = conversation_spawn_harness_override(
        get_agent_name(&ChatContextType::TaskExecution),
        ChatContextType::TaskExecution,
        Some(r#"{"trigger_origin":"retry"}"#),
        &child,
        Some(&parent),
    );

    assert_eq!(harness, None);
}

#[test]
fn conversation_spawn_harness_override_skips_parent_for_revision_reexecution() {
    let task_id = TaskId::from_string("task-parent-2b".to_string());
    let child = ChatConversation::new_task_execution(task_id.clone());
    let mut parent = ChatConversation::new_task_execution(task_id);
    parent.set_provider_session_ref(ProviderSessionRef {
        harness: AgentHarnessKind::Codex,
        provider_session_id: "codex-parent-session".to_string(),
    });

    let harness = conversation_spawn_harness_override(
        get_agent_name(&ChatContextType::TaskExecution),
        ChatContextType::TaskExecution,
        Some(r#"{"trigger_origin":"revision"}"#),
        &child,
        Some(&parent),
    );

    assert_eq!(harness, None);
}

#[test]
fn conversation_spawn_harness_override_skips_parent_without_continuation_metadata() {
    let task_id = TaskId::from_string("task-parent-3".to_string());
    let child = ChatConversation::new_task_execution(task_id.clone());
    let mut parent = ChatConversation::new_task_execution(task_id);
    parent.set_provider_session_ref(ProviderSessionRef {
        harness: AgentHarnessKind::Codex,
        provider_session_id: "codex-parent-session".to_string(),
    });

    let harness = conversation_spawn_harness_override(
        get_agent_name(&ChatContextType::TaskExecution),
        ChatContextType::TaskExecution,
        None,
        &child,
        Some(&parent),
    );

    assert_eq!(harness, None);
}

#[test]
fn conversation_spawn_harness_override_skips_parent_for_merge_new_attempt() {
    let task_id = TaskId::from_string("task-parent-4".to_string());
    let child = ChatConversation::new_merge(task_id.clone());
    let mut parent = ChatConversation::new_merge(task_id);
    parent.set_provider_session_ref(ProviderSessionRef {
        harness: AgentHarnessKind::Codex,
        provider_session_id: "codex-parent-session".to_string(),
    });

    let harness = conversation_spawn_harness_override(
        get_agent_name(&ChatContextType::Merge),
        ChatContextType::Merge,
        None,
        &child,
        Some(&parent),
    );

    assert_eq!(harness, None);
}

#[test]
fn conversation_spawn_harness_override_falls_back_to_parent_for_execution_startup_recovery() {
    let task_id = TaskId::from_string("task-parent-4a".to_string());
    let child = ChatConversation::new_task_execution(task_id.clone());
    let mut parent = ChatConversation::new_task_execution(task_id);
    parent.set_provider_session_ref(ProviderSessionRef {
        harness: AgentHarnessKind::Codex,
        provider_session_id: "codex-parent-session".to_string(),
    });

    let harness = conversation_spawn_harness_override(
        get_agent_name(&ChatContextType::TaskExecution),
        ChatContextType::TaskExecution,
        Some(r#"{"startup_recovery_attempts":1}"#),
        &child,
        Some(&parent),
    );

    assert_eq!(harness, Some(AgentHarnessKind::Codex));
}

#[test]
fn conversation_spawn_harness_override_falls_back_to_parent_for_merge_startup_recovery() {
    let task_id = TaskId::from_string("task-parent-4b".to_string());
    let child = ChatConversation::new_merge(task_id.clone());
    let mut parent = ChatConversation::new_merge(task_id);
    parent.set_provider_session_ref(ProviderSessionRef {
        harness: AgentHarnessKind::Codex,
        provider_session_id: "codex-parent-session".to_string(),
    });

    let harness = conversation_spawn_harness_override(
        get_agent_name(&ChatContextType::Merge),
        ChatContextType::Merge,
        Some(r#"{"startup_recovery_attempts":1}"#),
        &child,
        Some(&parent),
    );

    assert_eq!(harness, Some(AgentHarnessKind::Codex));
}

#[test]
fn conversation_spawn_harness_override_preserves_stored_review_harness_for_startup_recovery() {
    let task_id = TaskId::from_string("task-parent-5".to_string());
    let mut review = ChatConversation::new_review(task_id);
    review.set_provider_session_ref(ProviderSessionRef {
        harness: AgentHarnessKind::Codex,
        provider_session_id: "codex-review-session".to_string(),
    });

    let harness = conversation_spawn_harness_override(
        get_agent_name(&ChatContextType::Review),
        ChatContextType::Review,
        Some(r#"{"startup_recovery_attempts":1}"#),
        &review,
        None,
    );

    assert_eq!(harness, Some(AgentHarnessKind::Codex));
}

#[test]
fn conversation_spawn_harness_override_skips_stale_review_harness_for_fresh_cycle() {
    let task_id = TaskId::from_string("task-parent-6".to_string());
    let mut review = ChatConversation::new_review(task_id);
    review.set_provider_session_ref(ProviderSessionRef {
        harness: AgentHarnessKind::Codex,
        provider_session_id: "codex-review-session".to_string(),
    });

    let harness = conversation_spawn_harness_override(
        get_agent_name(&ChatContextType::Review),
        ChatContextType::Review,
        None,
        &review,
        None,
    );

    assert_eq!(harness, None);
}

#[test]
fn should_inherit_parent_harness_for_fresh_spawn_allows_startup_recovery() {
    assert!(should_inherit_parent_harness_for_fresh_spawn(
        ChatContextType::Merge,
        Some(r#"{"startup_recovery_attempts":1}"#),
    ));
}

#[test]
fn should_inherit_parent_harness_for_fresh_spawn_allows_resume() {
    assert!(should_inherit_parent_harness_for_fresh_spawn(
        ChatContextType::TaskExecution,
        Some(r#"{"trigger_origin":"resume"}"#),
    ));
}

#[test]
fn spawn_settings_require_task_metadata_includes_review() {
    assert!(spawn_settings_require_task_metadata(
        ChatContextType::TaskExecution
    ));
    assert!(spawn_settings_require_task_metadata(
        ChatContextType::Review
    ));
    assert!(spawn_settings_require_task_metadata(ChatContextType::Merge));
    assert!(!spawn_settings_require_task_metadata(
        ChatContextType::Ideation
    ));
}
