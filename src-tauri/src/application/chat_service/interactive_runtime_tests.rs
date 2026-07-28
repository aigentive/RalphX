use super::chat_service_context::noninteractive_agent_name;
use super::{
    agent_conversation_mode_for_send, canonical_parented_agent_binding,
    conversation_spawn_harness_override, get_agent_name, interactive_run_started_provider_session,
    persona_builder_runtime_message, persona_switch_requires_process_invalidation,
    plan_mode_runtime_message, preferred_agent_override,
    provider_harness_switch_requires_fresh_session, registered_persona_metadata,
    resolve_agent_name_for_send, should_inherit_parent_harness_for_fresh_spawn,
    spawn_settings_require_task_metadata, supervised_workspace_runtime_message,
};
use super::{ChatService, SendMessageOptions, SendQueuePolicy};
use crate::application::interactive_process_registry::{
    InteractiveProcessKey, InteractiveProcessMetadata,
    InteractiveProcessRetireAfterTurnDisposition, InteractiveProcessTurnCompleteDisposition,
};
use crate::application::persona_prompt::ResolvedPersona;
use crate::application::AppState;
use crate::domain::agents::{AgentHarnessKind, ProviderSessionRef};
use crate::domain::entities::{
    AgentConversationWorkspace, AgentConversationWorkspaceMode, ChatContextType, ChatConversation,
    ChatConversationId, IdeationAnalysisBaseRefKind, IdeationSessionId, Persona, PersonaId,
    PersonaStatus, ProjectId, TaskId,
};
use crate::infrastructure::agents::claude::agent_names::AGENT_WORKSPACE_REVIEWER;
use chrono::Utc;
use std::path::PathBuf;

#[cfg(unix)]
async fn interactive_test_stdin() -> (tokio::process::ChildStdin, tokio::process::Child) {
    let mut child = tokio::process::Command::new("cat")
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .expect("spawn stdin fixture");
    let stdin = child.stdin.take().expect("stdin fixture");
    (stdin, child)
}

#[cfg(unix)]
async fn capturing_interactive_test_stdin() -> (tokio::process::ChildStdin, tokio::process::Child) {
    let mut child = tokio::process::Command::new("cat")
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .spawn()
        .expect("spawn stdin capture fixture");
    let stdin = child.stdin.take().expect("stdin capture fixture");
    (stdin, child)
}

#[cfg(unix)]
async fn assert_retiring_owner_remains_armed_until_turn_complete(
    state: &AppState,
    key: &InteractiveProcessKey,
    token: crate::application::interactive_process_registry::InteractiveProcessToken,
    run_id: &str,
) {
    assert_eq!(
        state
            .interactive_process_registry
            .retire_after_turn_disposition_if_owner(key, token, run_id)
            .await,
        InteractiveProcessRetireAfterTurnDisposition::Active { is_armed: true },
        "a follow-up must not remove or disarm the exact retiring owner"
    );
    assert_eq!(
        state
            .interactive_process_registry
            .complete_turn_if_owner(key, token, run_id)
            .await,
        InteractiveProcessTurnCompleteDisposition::RetireAfterTurn,
        "the original TurnComplete must still retire its exact owner"
    );
}

#[cfg(unix)]
#[tokio::test]
async fn send_message_queues_follow_up_without_writing_or_disarming_retiring_owner() {
    let state = AppState::new_test();
    let context_id = "task-retiring-send";
    let conversation = ChatConversation::new_task(TaskId::from_string(context_id.to_string()));
    let conversation_id = conversation.id.as_str().to_string();
    state
        .chat_conversation_repo
        .create(conversation)
        .await
        .expect("persist retiring owner conversation");
    let key = InteractiveProcessKey::new("task", context_id);
    let (stdin, mut child) = capturing_interactive_test_stdin().await;
    let token = state
        .interactive_process_registry
        .register_with_metadata(
            key.clone(),
            stdin,
            InteractiveProcessMetadata {
                agent_run_id: Some("retiring-send-run".to_string()),
                ..Default::default()
            },
        )
        .await;
    state
        .interactive_process_registry
        .arm_retire_after_turn_if_owner(&key, token, "retiring-send-run")
        .await;
    state
        .running_agent_registry
        .register(
            crate::domain::services::RunningAgentKey::new("task", context_id),
            0,
            conversation_id,
            "retiring-send-run".to_string(),
            None,
            None,
        )
        .await;

    let result = state
        .build_chat_service()
        .send_message(
            ChatContextType::Task,
            context_id,
            "queue this after retirement",
            SendMessageOptions::default(),
        )
        .await
        .expect("retiring owner follow-up should queue");

    assert!(result.was_queued);
    assert_eq!(
        state
            .message_queue
            .get_queued(ChatContextType::Task, context_id)
            .len(),
        1,
        "send_message must queue the follow-up exactly once"
    );
    assert_retiring_owner_remains_armed_until_turn_complete(
        &state,
        &key,
        token,
        "retiring-send-run",
    )
    .await;

    let mut observed = Vec::new();
    use tokio::io::AsyncReadExt;
    child
        .stdout
        .take()
        .expect("capture stdout")
        .read_to_end(&mut observed)
        .await
        .expect("read captured stdin output");
    let _ = child.wait().await;
    assert!(
        observed.is_empty(),
        "a retiring owner must not receive the follow-up on its old stdin"
    );
}

#[cfg(unix)]
#[tokio::test]
async fn queue_message_queues_follow_up_without_writing_or_disarming_retiring_owner() {
    let state = AppState::new_test();
    let context_id = "task-retiring-queue";
    let conversation = ChatConversation::new_task(TaskId::from_string(context_id.to_string()));
    state
        .chat_conversation_repo
        .create(conversation)
        .await
        .expect("persist retiring owner conversation");
    let key = InteractiveProcessKey::new("task", context_id);
    let (stdin, mut child) = capturing_interactive_test_stdin().await;
    let token = state
        .interactive_process_registry
        .register_with_metadata(
            key.clone(),
            stdin,
            InteractiveProcessMetadata {
                agent_run_id: Some("retiring-queue-run".to_string()),
                ..Default::default()
            },
        )
        .await;
    state
        .interactive_process_registry
        .arm_retire_after_turn_if_owner(&key, token, "retiring-queue-run")
        .await;

    state
        .build_chat_service()
        .queue_message(
            ChatContextType::Task,
            context_id,
            "queue this after retirement",
            Some("retiring-queue-message"),
        )
        .await
        .expect("retiring owner follow-up should queue");

    let queued = state
        .message_queue
        .get_queued(ChatContextType::Task, context_id);
    assert_eq!(queued.len(), 1, "queue_message must enqueue exactly once");
    assert_eq!(queued[0].id, "retiring-queue-message");
    assert_retiring_owner_remains_armed_until_turn_complete(
        &state,
        &key,
        token,
        "retiring-queue-run",
    )
    .await;

    let mut observed = Vec::new();
    use tokio::io::AsyncReadExt;
    child
        .stdout
        .take()
        .expect("capture stdout")
        .read_to_end(&mut observed)
        .await
        .expect("read captured stdin output");
    let _ = child.wait().await;
    assert!(
        observed.is_empty(),
        "a retiring owner must not receive the queued follow-up on its old stdin"
    );
}

#[cfg(unix)]
#[tokio::test]
async fn active_interactive_process_cannot_strand_fresh_verification_in_queue() {
    let state = AppState::new_test();
    let project_id = ProjectId::new();
    let conversation = state
        .chat_conversation_repo
        .create(ChatConversation::new_project(project_id.clone()))
        .await
        .expect("conversation should persist");
    let key = InteractiveProcessKey::new("project", conversation.id.as_str());
    let (stdin, _child) = interactive_test_stdin().await;
    state
        .interactive_process_registry
        .register_with_metadata(
            key,
            stdin,
            InteractiveProcessMetadata {
                agent_run_id: Some("planning-run".to_string()),
                ..Default::default()
            },
        )
        .await;
    let service = state.build_chat_service();

    let error = service
        .send_message(
            ChatContextType::Project,
            project_id.as_str(),
            "Verify the plan",
            SendMessageOptions {
                conversation_id_override: Some(conversation.id),
                queue_policy: SendQueuePolicy::RequireImmediateStart,
                metadata: Some(
                    r#"{"ralphx_action_kind":"verify_plan","ralphx_action_context_id":"plan-session","ralphx_action_target_id":"plan-artifact"}"#
                        .to_string(),
                ),
                ..Default::default()
            },
        )
        .await
        .expect_err("active process must reject a fresh verifier without queueing it");

    assert!(error.to_string().contains("immediate start required"));
    let conversation_id = conversation.id.as_str();
    assert!(
        state
            .message_queue
            .get_queued(ChatContextType::Project, &conversation_id)
            .is_empty(),
        "a TurnComplete retry must not be blocked by a stranded Verify Plan queue row"
    );
}

fn resolved_persona(id: &str, content_hash: &str) -> ResolvedPersona {
    ResolvedPersona {
        id: PersonaId::from(id),
        slug: id.to_string(),
        version: 1,
        content_hash: content_hash.to_string(),
        block: String::new(),
        skipped_reason: None,
    }
}

#[test]
fn explicit_agent_override_precedes_persisted_child_binding() {
    assert_eq!(
        preferred_agent_override(
            Some("ralphx-workspace-repair"),
            Some("ralphx-workspace-reviewer"),
        ),
        Some("ralphx-workspace-repair")
    );
    assert_eq!(
        preferred_agent_override(None, Some("ralphx-workspace-reviewer")),
        Some("ralphx-workspace-reviewer")
    );
}

#[test]
fn noninteractive_specialist_override_drives_runtime_settings_resolution() {
    assert_eq!(
        noninteractive_agent_name(
            ChatContextType::Project,
            None,
            Some("ralphx-workspace-reviewer"),
        ),
        "ralphx-workspace-reviewer"
    );
    assert_eq!(
        noninteractive_agent_name(ChatContextType::Project, None, None),
        AGENT_CHAT_PROJECT
    );
}

#[test]
fn only_parented_conversations_bind_successfully_resolved_canonical_agents() {
    let plugin_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("src-tauri has a repository parent")
        .join("plugins/app");
    let mut child = ChatConversation::new_task(TaskId::from_string("task-child".to_string()));
    child.parent_conversation_id = Some("parent-conversation".to_string());

    assert_eq!(
        canonical_parented_agent_binding(&plugin_dir, &child, Some("ralphx-workspace-reviewer"),)
            .as_deref(),
        Some("ralphx-workspace-reviewer")
    );
    assert_eq!(
        canonical_parented_agent_binding(&plugin_dir, &child, Some("not-a-canonical-agent")),
        None
    );

    child.parent_conversation_id = None;
    assert_eq!(
        canonical_parented_agent_binding(&plugin_dir, &child, Some("ralphx-workspace-reviewer"),),
        None
    );
}

#[tokio::test]
async fn parented_specialist_override_persists_bound_agent_before_spawn() {
    let state = AppState::new_test();
    let project_id = ProjectId::from_string("project-bound-reviewer-send".to_string());
    let parent = state
        .chat_conversation_repo
        .create(ChatConversation::new_project(project_id.clone()))
        .await
        .expect("parent conversation should persist");
    let mut child = ChatConversation::new_project(project_id.clone());
    child.parent_conversation_id = Some(parent.id.as_str());
    let child_id = child.id;
    state
        .chat_conversation_repo
        .create(child)
        .await
        .expect("child conversation should persist");
    let service = state.build_chat_service();

    let (resolved, created) = service
        .get_or_create_conversation_for_send(
            ChatContextType::Project,
            project_id.as_str(),
            &SendMessageOptions {
                conversation_id_override: Some(child_id),
                agent_name_override: Some(AGENT_WORKSPACE_REVIEWER.to_string()),
                ..Default::default()
            },
        )
        .await
        .expect("conversation should resolve");

    assert!(!created);
    assert_eq!(
        resolved.bound_agent_name.as_deref(),
        Some("ralphx-workspace-reviewer")
    );
    let stored = state
        .chat_conversation_repo
        .get_by_id(&child_id)
        .await
        .expect("conversation should load")
        .expect("conversation should exist");
    assert_eq!(
        stored.bound_agent_name.as_deref(),
        Some("ralphx-workspace-reviewer")
    );
}
use crate::infrastructure::agents::claude::agent_names::{
    AGENT_AUTOMATION_SETUP, AGENT_CHAT_PROJECT, AGENT_GENERAL_EXPLORER, AGENT_GENERAL_WORKER,
    AGENT_ORCHESTRATOR_IDEATION, AGENT_PERSONA_EXTRACTOR, AGENT_PR_REVIEWER,
};

#[test]
fn interactive_run_started_provider_session_prefers_process_metadata_harness() {
    let conversation =
        ChatConversation::new_ideation(IdeationSessionId::from_string("session-1".to_string()));

    let (harness, provider_session_id) = interactive_run_started_provider_session(
        &conversation,
        Some(&InteractiveProcessMetadata {
            agent_run_id: None,
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
            agent_run_id: None,
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
            agent_run_id: None,
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
        agent_run_id: None,
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
        agent_run_id: None,
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
        agent_run_id: None,
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
        None,
        Some(AgentConversationWorkspaceMode::Edit),
    );
    let chat_agent = resolve_agent_name_for_send(
        &ChatContextType::Project,
        None,
        None,
        Some(AgentConversationWorkspaceMode::Chat),
    );
    let ideation_agent = resolve_agent_name_for_send(
        &ChatContextType::Project,
        None,
        None,
        Some(AgentConversationWorkspaceMode::Ideation),
    );
    let plan_agent = resolve_agent_name_for_send(
        &ChatContextType::Project,
        None,
        None,
        Some(AgentConversationWorkspaceMode::Plan),
    );
    let review_pr_agent = resolve_agent_name_for_send(
        &ChatContextType::Project,
        None,
        None,
        Some(AgentConversationWorkspaceMode::ReviewPr),
    );
    let automation_agent = resolve_agent_name_for_send(
        &ChatContextType::Project,
        None,
        None,
        Some(AgentConversationWorkspaceMode::Automation),
    );
    let persona_builder_agent = resolve_agent_name_for_send(
        &ChatContextType::Project,
        None,
        None,
        Some(AgentConversationWorkspaceMode::PersonaBuilder),
    );
    let default_project_agent =
        resolve_agent_name_for_send(&ChatContextType::Project, None, None, None);

    assert_eq!(edit_agent, AGENT_GENERAL_WORKER);
    assert_eq!(chat_agent, AGENT_GENERAL_EXPLORER);
    assert_eq!(plan_agent, AGENT_ORCHESTRATOR_IDEATION);
    assert_eq!(ideation_agent, AGENT_CHAT_PROJECT);
    assert_eq!(review_pr_agent, AGENT_PR_REVIEWER);
    assert_eq!(automation_agent, AGENT_AUTOMATION_SETUP);
    assert_eq!(persona_builder_agent, AGENT_PERSONA_EXTRACTOR);
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

        let agent = resolve_agent_name_for_send(&ChatContextType::Ideation, None, None, mode);
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

    let agent = resolve_agent_name_for_send(&ChatContextType::Ideation, None, None, mode);
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
        resolve_agent_name_for_send(&ChatContextType::Project, None, None, ideation_mode);
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
fn supervised_runtime_message_injects_autopilot_opt_in() {
    let workspace = AgentConversationWorkspace::new(
        ChatConversationId::from_string("conversation-autopilot-1"),
        ProjectId::from_string("project-autopilot-1".to_string()),
        AgentConversationWorkspaceMode::Autopilot,
        IdeationAnalysisBaseRefKind::CurrentBranch,
        "main".to_string(),
        None,
        None,
        "ralphx/project/agent-autopilot".to_string(),
        "/tmp/ralphx-autopilot-workspace".to_string(),
    );

    let message = supervised_workspace_runtime_message(
        "finish the change".to_string(),
        Some(&workspace),
        Some("message-autopilot-1"),
    );

    assert!(message.contains("<workspace_mode>autopilot</workspace_mode>"));
    assert!(message.contains("explicitly opted into Autopilot"));
    assert!(message.contains("<user_request>finish the change</user_request>"));
}

#[test]
fn supervised_runtime_message_injects_exact_tasks_source_identity() {
    let mut workspace = AgentConversationWorkspace::new(
        ChatConversationId::from_string("conversation-tasks-1"),
        ProjectId::from_string("project-tasks-1".to_string()),
        AgentConversationWorkspaceMode::Tasks,
        IdeationAnalysisBaseRefKind::CurrentBranch,
        "main".to_string(),
        None,
        None,
        "ralphx/project/agent-tasks".to_string(),
        "/tmp/ralphx-tasks-workspace".to_string(),
    );
    workspace.task_pipeline_session_id = Some(IdeationSessionId::from_string("pipeline-1"));

    let message = supervised_workspace_runtime_message(
        "please add this follow-up".to_string(),
        Some(&workspace),
        Some("message-tasks-1"),
    );

    assert!(message.contains("<workspace_mode>tasks</workspace_mode>"));
    assert!(message.contains("<task_pipeline_session_id>pipeline-1</task_pipeline_session_id>"));
    assert!(message.contains("<source_message_id>message-tasks-1</source_message_id>"));
    assert!(message.contains("explicit user request in this source message"));
}

#[test]
fn persona_builder_runtime_message_injects_bound_draft_without_leaking_content() {
    let mut conversation = ChatConversation::new_project(ProjectId::from_string(
        "persona-builder-project".to_string(),
    ));
    conversation.agent_mode = Some(AgentConversationWorkspaceMode::PersonaBuilder);
    conversation.builder_draft_id = Some("draft-1".to_string());
    let now = Utc::now();
    let draft = Persona {
        id: PersonaId::from("draft-1"),
        artifact_id: None,

        project_id: None,
        slug: "existing-reviewer".to_string(),
        name: "Existing Reviewer".to_string(),
        description: "Review carefully".to_string(),
        content: "SECRET PERSONA CONTENT".to_string(),
        status: PersonaStatus::Draft,
        version: 3,
        content_hash: "draft-hash-v3".to_string(),
        source_session_id: Some(conversation.id.as_str()),
        source_persona_id: Some(PersonaId::from("source-1")),
        source_content_hash: Some("source-hash".to_string()),
        source_json: "{}".to_string(),
        created_at: now,
        updated_at: now,
    };

    let message = persona_builder_runtime_message(
        "refine the persona".to_string(),
        Some(&conversation),
        Some(&draft),
    );

    assert!(message.contains("<persona_builder_context>"));
    assert!(message.contains("<builder_draft_id>draft-1</builder_draft_id>"));
    assert!(message.contains("<source_persona_id>source-1</source_persona_id>"));
    assert!(message.contains("<draft_version>3</draft_version>"));
    assert!(message.contains("save_persona_draft"));
    assert!(message.contains("<user_request>refine the persona</user_request>"));
    assert!(!message.contains("SECRET PERSONA CONTENT"));
}

#[test]
fn persona_builder_runtime_message_rejects_mode_only_identity_in_unsupported_context() {
    let mut conversation = ChatConversation::new_review(TaskId::new());
    conversation.agent_mode = Some(AgentConversationWorkspaceMode::PersonaBuilder);
    conversation.builder_draft_id = Some("invalid-context-draft".to_string());
    let now = Utc::now();
    let draft = Persona {
        id: PersonaId::from("invalid-context-draft"),
        artifact_id: None,
        project_id: None,
        slug: "invalid-context-draft".to_string(),
        name: "Invalid Context Draft".to_string(),
        description: "Must not be injected".to_string(),
        content: "MUST NOT APPEAR".to_string(),
        status: PersonaStatus::Draft,
        version: 1,
        content_hash: "invalid-context-hash".to_string(),
        source_session_id: Some(conversation.id.as_str()),
        source_persona_id: None,
        source_content_hash: None,
        source_json: "{}".to_string(),
        created_at: now,
        updated_at: now,
    };

    let message = persona_builder_runtime_message(
        "ordinary review request".to_string(),
        Some(&conversation),
        Some(&draft),
    );

    assert_eq!(message, "ordinary review request");
}

#[test]
fn explicit_agent_override_wins_over_workspace_mode() {
    let agent = resolve_agent_name_for_send(
        &ChatContextType::Project,
        None,
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
