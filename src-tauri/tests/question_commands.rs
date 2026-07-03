use ralphx_lib::application::agent_conversation_workspace::{
    prepare_agent_conversation_workspace, AgentConversationWorkspaceBaseSelection,
};
use ralphx_lib::application::{
    AppState, QuestionAnswer, QuestionOption, QuestionState, TeamService, TeamStateTracker,
};
use ralphx_lib::commands::question_commands::{
    resolve_user_question, ResolveQuestionArgs, ResolveQuestionResponse,
};
use ralphx_lib::commands::ExecutionState;
use ralphx_lib::domain::entities::{
    AgentConversationWorkspaceMode, ChatContextType, ChatConversation, IdeationAnalysisBaseRefKind,
    IdeationSessionFlow, Project,
};
use serde_json::json;
use std::path::Path;
use std::process::Command;
use std::sync::Arc;
use tauri::test::{mock_builder, mock_context, noop_assets};
use tauri::Manager;

#[test]
fn test_resolve_question_args_deserialize() {
    let json = r#"{"requestId": "abc-123", "selectedOptions": ["opt1", "opt2"], "customResponse": "Custom answer"}"#;
    let args: ResolveQuestionArgs = serde_json::from_str(json).unwrap();
    assert_eq!(args.request_id, "abc-123");
    assert_eq!(args.selected_options, vec!["opt1", "opt2"]);
    assert_eq!(args.custom_response, Some("Custom answer".to_string()));
    assert!(!args.skipped);
}

#[test]
fn test_resolve_question_args_without_custom_response() {
    let json = r#"{"requestId": "abc-123", "selectedOptions": ["opt1"]}"#;
    let args: ResolveQuestionArgs = serde_json::from_str(json).unwrap();
    assert_eq!(args.request_id, "abc-123");
    assert_eq!(args.selected_options, vec!["opt1"]);
    assert!(args.custom_response.is_none());
    assert!(!args.skipped);
}

#[test]
fn test_resolve_question_args_with_skipped() {
    let json = r#"{"requestId": "abc-123", "selectedOptions": [], "skipped": true}"#;
    let args: ResolveQuestionArgs = serde_json::from_str(json).unwrap();
    assert_eq!(args.request_id, "abc-123");
    assert!(args.selected_options.is_empty());
    assert!(args.custom_response.is_none());
    assert!(args.skipped);
}

#[test]
fn test_resolve_question_response_serialize() {
    let response = ResolveQuestionResponse {
        success: true,
        message: Some("Resolved".to_string()),
        delivered_to_waiting_agent: true,
        plan_mode_proposal_handled: false,
    };
    let json = serde_json::to_string(&response).unwrap();
    assert!(json.contains("\"success\":true"));
    assert!(json.contains("\"message\":\"Resolved\""));
    assert!(json.contains("\"deliveredToWaitingAgent\":true"));
    assert!(json.contains("\"planModeProposalHandled\":false"));
}

/// Verify that resolve() returns (true, Some(session_id)) for a known question,
/// which is the condition that gates event emission in resolve_user_question.
#[tokio::test]
async fn test_resolve_returns_true_with_session_id_when_question_exists() {
    let state = QuestionState::new();
    state
        .register(
            "req-abc".to_string(),
            "session-xyz".to_string(),
            "Which option?".to_string(),
            None,
            vec![QuestionOption {
                value: "a".to_string(),
                label: "Option A".to_string(),
                description: None,
            }],
            false,
        )
        .await;

    let answer = QuestionAnswer {
        selected_options: vec!["a".to_string()],
        text: None,
        skipped: false,
    };
    let result = state.resolve("req-abc", answer).await;

    // emit path should be taken: resolved == true and session_id.is_some()
    assert!(
        result.resolved,
        "resolve should return true for a known request_id"
    );
    assert_eq!(
        result.session_id,
        Some("session-xyz".to_string()),
        "session_id should match the registered session"
    );
    assert!(result.delivered_to_waiting_agent);
}

/// Verify that resolve() returns (false, None) for an unknown question,
/// which means the event emission path is NOT taken.
#[tokio::test]
async fn test_resolve_returns_false_when_question_not_found() {
    let state = QuestionState::new();

    let answer = QuestionAnswer {
        selected_options: vec!["a".to_string()],
        text: None,
        skipped: false,
    };
    let result = state.resolve("nonexistent-req", answer).await;

    // emit path should NOT be taken: resolved == false
    assert!(
        !result.resolved,
        "resolve should return false for an unknown request_id"
    );
    assert!(
        result.session_id.is_none(),
        "session_id should be None when not resolved"
    );
    assert!(!result.delivered_to_waiting_agent);
}

fn git(repo: &Path, args: &[&str]) {
    let output = Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(args)
        .output()
        .expect("git command should run");
    assert!(
        output.status.success(),
        "git {:?} failed: {}{}",
        args,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn setup_repo(root: &Path) {
    std::fs::create_dir_all(root).expect("repo root should be created");
    git(root, &["init", "-b", "main"]);
    git(root, &["config", "user.email", "test@example.com"]);
    git(root, &["config", "user.name", "Test User"]);
    std::fs::write(root.join("README.md"), "hello\n").expect("fixture file should be written");
    git(root, &["add", "README.md"]);
    git(root, &["commit", "-m", "initial"]);
}

fn build_question_command_app(state: AppState) -> tauri::App<tauri::test::MockRuntime> {
    mock_builder()
        .manage(state)
        .manage(Arc::new(ExecutionState::new()))
        .manage(Arc::new(TeamService::new_without_events(Arc::new(
            TeamStateTracker::new(),
        ))))
        .build(mock_context(noop_assets()))
        .expect("mock app should build")
}

#[tokio::test]
async fn accepted_plan_mode_proposal_links_planning_session_before_hidden_continuation() {
    let temp = tempfile::tempdir().expect("tempdir should be created");
    let repo_path = temp.path().join("repo");
    let worktree_parent = temp.path().join("worktrees");
    setup_repo(&repo_path);

    let state = AppState::new_test();
    let mut project = Project::new(
        "Plan Proposal".to_string(),
        repo_path.to_string_lossy().to_string(),
    );
    project.base_branch = Some("main".to_string());
    project.worktree_parent_directory = Some(worktree_parent.to_string_lossy().to_string());
    let project = state
        .project_repo
        .create(project)
        .await
        .expect("project should persist");

    let mut conversation = ChatConversation::new_project(project.id.clone());
    conversation.agent_mode = Some(AgentConversationWorkspaceMode::Edit);
    let conversation = state
        .chat_conversation_repo
        .create(conversation)
        .await
        .expect("conversation should persist");
    let conversation_id = conversation.id;
    let conversation_id_string = conversation_id.as_str();

    let workspace = prepare_agent_conversation_workspace(
        &project,
        &conversation_id,
        AgentConversationWorkspaceMode::Edit,
        AgentConversationWorkspaceBaseSelection {
            kind: Some(IdeationAnalysisBaseRefKind::ProjectDefault),
                branch_mode: None,
            base_ref: Some("main".to_string()),
            display_name: None,
            source_pull_request: None,
        },
    )
    .await
    .expect("edit workspace should be prepared");
    state
        .agent_conversation_workspace_repo
        .create_or_update(workspace)
        .await
        .expect("workspace should persist");

    state
        .question_state
        .register_with_metadata(
            "req-plan".to_string(),
            conversation_id_string.clone(),
            "Switch to Plan mode?".to_string(),
            None,
            vec![QuestionOption {
                value: "switch_to_plan".to_string(),
                label: "Switch to Plan".to_string(),
                description: None,
            }],
            false,
            true,
            None,
            None,
            Some(json!({
                "kind": "plan_mode_proposal",
                "conversation_id": conversation_id_string.clone(),
                "reason": "Draft the implementation plan first"
            })),
        )
        .await;

    let app = build_question_command_app(state);
    let response = resolve_user_question(
        app.state::<AppState>(),
        app.state::<Arc<ExecutionState>>(),
        app.state::<Arc<TeamService>>(),
        app.handle().clone(),
        ResolveQuestionArgs {
            request_id: "req-plan".to_string(),
            selected_options: vec!["switch_to_plan".to_string()],
            custom_response: None,
            skipped: false,
        },
    )
    .await
    .expect("question should resolve");

    assert!(response.success);
    assert!(response.delivered_to_waiting_agent);
    assert!(response.plan_mode_proposal_handled);

    let state = app.state::<AppState>();
    let conversation = state
        .chat_conversation_repo
        .get_by_id(&conversation_id)
        .await
        .expect("conversation lookup should succeed")
        .expect("conversation should exist");
    assert_eq!(
        conversation.agent_mode,
        Some(AgentConversationWorkspaceMode::Plan)
    );

    let workspace = state
        .agent_conversation_workspace_repo
        .get_by_conversation_id(&conversation_id)
        .await
        .expect("workspace lookup should succeed")
        .expect("workspace should exist");
    assert_eq!(workspace.mode, AgentConversationWorkspaceMode::Plan);
    let planning_session_id = workspace
        .linked_ideation_session_id
        .clone()
        .expect("plan workspace should link to a planning session");
    assert!(
        workspace.linked_plan_branch_id.is_none(),
        "Plan-mode handoff should start with a planning session, not a plan branch"
    );

    let session = state
        .ideation_session_repo
        .get_by_id(&planning_session_id)
        .await
        .expect("planning session lookup should succeed")
        .expect("planning session should exist");
    assert_eq!(session.session_flow, IdeationSessionFlow::Planning);
    assert_eq!(
        session.source_context_type.as_deref(),
        Some("agent_conversation")
    );
    assert_eq!(
        session.source_context_id.as_deref(),
        Some(conversation_id_string.as_str())
    );
    assert_eq!(session.spawn_reason.as_deref(), Some("agent_plan_mode"));

    let queued = state
        .message_queue
        .get_queued(ChatContextType::Project, conversation_id_string.as_str());
    assert_eq!(queued.len(), 1);
    assert!(queued[0]
        .metadata_override
        .as_deref()
        .expect("queued continuation should carry metadata")
        .contains("\"source\":\"accepted_plan_mode_proposal\""));
    assert!(queued[0]
        .metadata_override
        .as_deref()
        .expect("queued continuation should carry metadata")
        .contains("\"resume_in_place\":true"));
    let metadata: serde_json::Value = serde_json::from_str(
        queued[0]
            .metadata_override
            .as_deref()
            .expect("queued continuation should carry metadata"),
    )
    .expect("queued continuation metadata should be valid json");
    let outcome = metadata
        .get("plan_mode_verdict_outcome")
        .expect("accepted Plan-mode proposal should capture compact outcome metadata");
    assert_eq!(
        outcome
            .get("outcome_class")
            .and_then(|value| value.as_str()),
        Some("plan_mode_accepted")
    );
    assert_eq!(
        outcome
            .get("refs")
            .and_then(|value| value.get("planning_session_id"))
            .and_then(|value| value.as_str()),
        Some(planning_session_id.0.as_str())
    );
    assert_eq!(
        outcome
            .get("mutates_accepted_session")
            .and_then(|value| value.as_bool()),
        Some(false)
    );
}
