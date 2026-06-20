use super::agent_issue_report_commands::{build_agent_issue_report, submit_agent_issue_report};
use crate::application::{AppState, BuildAgentIssueReportInput, SubmitAgentIssueReportInput};
use crate::domain::entities::{
    AgentConversationWorkspace, AgentConversationWorkspaceMode, ChatConversation,
    ChatConversationId, IdeationAnalysisBaseRefKind, Project, ProjectId,
};
use crate::domain::services::GithubServiceTrait;
use crate::tests::mock_github_service::MockGithubService;
use std::sync::Arc;
use tauri::test::{mock_builder, mock_context, noop_assets, MockRuntime};
use tauri::Manager;

async fn seeded_command_app() -> (
    tauri::App<MockRuntime>,
    ChatConversationId,
    ProjectId,
    tempfile::TempDir,
) {
    let state = AppState::new_test();
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let project_root = temp_dir.path().join("project");
    let workspace_root = temp_dir.path().join("workspace");
    std::fs::create_dir_all(&project_root).expect("project root");
    std::fs::create_dir_all(&workspace_root).expect("workspace root");

    let project = Project::new(
        "Command Report Project".to_string(),
        project_root.to_string_lossy().into_owned(),
    );
    let project_id = project.id.clone();
    state
        .project_repo
        .create(project)
        .await
        .expect("project should seed");

    let conversation = ChatConversation::new_project(project_id.clone());
    let conversation_id = conversation.id;
    state
        .chat_conversation_repo
        .create(conversation)
        .await
        .expect("conversation should seed");

    let workspace = AgentConversationWorkspace::new(
        conversation_id,
        project_id.clone(),
        AgentConversationWorkspaceMode::Edit,
        IdeationAnalysisBaseRefKind::ProjectDefault,
        "main".to_string(),
        Some("Project default (main)".to_string()),
        Some("abc123".to_string()),
        "ralphx/ralphx/agent-support".to_string(),
        workspace_root.to_string_lossy().into_owned(),
    );
    state
        .agent_conversation_workspace_repo
        .create_or_update(workspace)
        .await
        .expect("workspace should seed");

    let app = mock_builder()
        .manage(state)
        .build(mock_context(noop_assets()))
        .expect("mock app should build");

    (app, conversation_id, project_id, temp_dir)
}

#[tokio::test]
async fn build_agent_issue_report_command_injects_environment_and_delegates() {
    let (app, conversation_id, project_id, _temp_dir) = seeded_command_app().await;

    let draft = build_agent_issue_report(
        app.handle().clone(),
        app.state::<AppState>(),
        BuildAgentIssueReportInput {
            conversation_id: conversation_id.as_str(),
            project_id: Some(project_id.as_str().to_string()),
            include_logs: false,
            recent_errors_only: false,
            max_log_bytes: 24 * 1024,
        },
    )
    .await
    .expect("command should build draft");

    assert_eq!(draft.conversation_id, conversation_id.as_str());
    assert_eq!(draft.project_id, project_id.as_str());
    assert!(draft.markdown.contains("RalphX version:"));
    assert!(draft.markdown.contains("Architecture:"));
}

#[tokio::test]
async fn submit_agent_issue_report_command_delegates_success() {
    let mut state = AppState::new_test();
    let github = Arc::new(MockGithubService::new());
    github.will_create_issue("https://github.com/aigentive/ralphx.app/issues/123");
    state.github_service = Some(github.clone() as Arc<dyn GithubServiceTrait>);
    let app = mock_builder()
        .manage(state)
        .build(mock_context(noop_assets()))
        .expect("mock app should build");

    let response = submit_agent_issue_report(
        app.state::<AppState>(),
        SubmitAgentIssueReportInput {
            conversation_id: ChatConversationId::new().as_str(),
            repository: "aigentive/ralphx.app".to_string(),
            title: "Support report".to_string(),
            body_markdown: "Reviewed body".to_string(),
        },
    )
    .await
    .expect("command should submit");

    assert_eq!(response.repository, "aigentive/ralphx.app");
    assert_eq!(
        response.issue_url,
        "https://github.com/aigentive/ralphx.app/issues/123"
    );
    assert_eq!(github.state().create_issue_calls, 1);
}

#[tokio::test]
async fn submit_agent_issue_report_command_maps_validation_error_to_string() {
    let app = mock_builder()
        .manage(AppState::new_test())
        .build(mock_context(noop_assets()))
        .expect("mock app should build");

    let error = submit_agent_issue_report(
        app.state::<AppState>(),
        SubmitAgentIssueReportInput {
            conversation_id: "not-a-conversation-id".to_string(),
            repository: "aigentive/ralphx.app".to_string(),
            title: "Support report".to_string(),
            body_markdown: "Reviewed body".to_string(),
        },
    )
    .await
    .expect_err("invalid input should fail");

    assert!(error.contains("Invalid agent conversation ID"));
}
