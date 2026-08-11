use std::sync::Arc;

use ralphx_events::RecordingEventSink;
use ralphx_lib::application::chat_service::{ChatService, SendMessageOptions};
use ralphx_lib::application::interactive_process_registry::InteractiveProcessKey;
use ralphx_lib::application::AppState;
use ralphx_lib::commands::ExecutionState;
use ralphx_lib::domain::entities::{
    AgentConversationWorkspace, AgentConversationWorkspaceMode, ChatContextType, ChatConversation,
    IdeationAnalysisBaseRefKind, Project,
};
use tokio::io::AsyncReadExt;

#[tokio::test]
async fn standalone_gate1_rejects_override_for_another_conversation_before_stdin_write() {
    let state = AppState::new_test();
    let live_conversation = state
        .chat_conversation_repo
        .create(ChatConversation::new_standalone())
        .await
        .expect("persist live standalone conversation");
    let other_conversation = state
        .chat_conversation_repo
        .create(ChatConversation::new_standalone())
        .await
        .expect("persist unrelated standalone conversation");

    let mut child = tokio::process::Command::new("cat")
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .spawn()
        .expect("spawn stdin observer");
    let interactive_key = InteractiveProcessKey::new(
        ChatContextType::Standalone.to_string(),
        &live_conversation.context_id,
    );
    state
        .interactive_process_registry
        .register(
            interactive_key.clone(),
            child.stdin.take().expect("stdin observer pipe"),
        )
        .await;

    let service = state.build_chat_service_with_execution_state(Arc::new(ExecutionState::new()));
    let error = service
        .send_message(
            ChatContextType::Standalone,
            &live_conversation.context_id,
            "must not reach the live standalone process",
            SendMessageOptions {
                conversation_id_override: Some(other_conversation.id),
                ..Default::default()
            },
        )
        .await
        .expect_err("an override from another standalone conversation must fail closed");

    assert!(
        error
            .to_string()
            .contains("conversation context id mismatch"),
        "the persisted conversation identity mismatch must be explicit: {error}"
    );
    assert!(
        state
            .interactive_process_registry
            .has_process(&interactive_key)
            .await,
        "identity rejection must leave the live interactive process registered"
    );

    state
        .interactive_process_registry
        .remove(&interactive_key)
        .await;
    let mut observed_stdin = Vec::new();
    child
        .stdout
        .take()
        .expect("stdin observer stdout")
        .read_to_end(&mut observed_stdin)
        .await
        .expect("read stdin observer output");
    let _ = child.wait().await;
    assert!(
        observed_stdin.is_empty(),
        "identity rejection must happen before any Gate-1 stdin write"
    );
}

#[tokio::test]
async fn project_identity_mismatch_precedes_terminal_workspace_continuation_side_effects() {
    let events = RecordingEventSink::new();
    let mut state = AppState::new_test();
    state.events = Arc::new(events.clone());

    let project_a = state
        .project_repo
        .create(Project::new(
            "Project A".to_string(),
            "/tmp/ralphx-project-a".to_string(),
        ))
        .await
        .expect("persist caller project");
    let project_b = state
        .project_repo
        .create(Project::new(
            "Project B".to_string(),
            "/tmp/ralphx-project-b".to_string(),
        ))
        .await
        .expect("persist conversation project");
    let conversation_b = state
        .chat_conversation_repo
        .create(ChatConversation::new_project(project_b.id.clone()))
        .await
        .expect("persist Project B conversation");

    let mut workspace_b = AgentConversationWorkspace::new(
        conversation_b.id,
        project_b.id,
        AgentConversationWorkspaceMode::Edit,
        IdeationAnalysisBaseRefKind::ProjectDefault,
        "main".to_string(),
        None,
        None,
        "ralphx/project-b/terminal".to_string(),
        "/tmp/ralphx-project-b-terminal-worktree".to_string(),
    );
    workspace_b.publication_pr_status = Some("merged".to_string());
    let workspace_before = state
        .agent_conversation_workspace_repo
        .create_or_update(workspace_b)
        .await
        .expect("persist terminal Project B workspace");

    let mut child = tokio::process::Command::new("cat")
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .spawn()
        .expect("spawn stdin observer");
    let interactive_key = InteractiveProcessKey::new(
        ChatContextType::Project.to_string(),
        conversation_b.id.as_str(),
    );
    state
        .interactive_process_registry
        .register(
            interactive_key.clone(),
            child.stdin.take().expect("stdin observer pipe"),
        )
        .await;

    let service = state.build_chat_service_with_execution_state(Arc::new(ExecutionState::new()));
    let error = service
        .send_message(
            ChatContextType::Project,
            project_a.id.as_str(),
            "must not mutate Project B continuation state",
            SendMessageOptions {
                conversation_id_override: Some(conversation_b.id),
                ..Default::default()
            },
        )
        .await
        .expect_err("Project B conversation must be rejected from Project A context");

    assert!(
        error
            .to_string()
            .contains("conversation context id mismatch"),
        "the persisted conversation identity mismatch must be explicit: {error}"
    );
    assert!(
        state
            .interactive_process_registry
            .has_process(&interactive_key)
            .await,
        "identity rejection must preserve Project B's live interactive process"
    );
    let workspace_after = state
        .agent_conversation_workspace_repo
        .get_by_conversation_id(&conversation_b.id)
        .await
        .expect("reload Project B workspace")
        .expect("Project B workspace remains persisted");
    assert_eq!(
        workspace_after, workspace_before,
        "identity rejection must not roll over Project B's terminal workspace"
    );
    assert!(
        events.events().is_empty(),
        "identity rejection must happen before workspace-change events"
    );

    state
        .interactive_process_registry
        .remove(&interactive_key)
        .await;
    let mut observed_stdin = Vec::new();
    child
        .stdout
        .take()
        .expect("stdin observer stdout")
        .read_to_end(&mut observed_stdin)
        .await
        .expect("read stdin observer output");
    let _ = child.wait().await;
    assert!(
        observed_stdin.is_empty(),
        "identity rejection must happen before any Project B stdin write"
    );
}
