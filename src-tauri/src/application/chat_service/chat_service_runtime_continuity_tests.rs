use super::{ChatService, ChatServiceError, SendMessageOptions};
use crate::application::agent_conversation_workspace::resolve_agent_conversation_workspace_path;
use crate::application::AppState;
use crate::domain::agents::{
    AgentHarnessKind, AgentProviderSettings, LogicalEffort, ProviderSessionRef,
};
use crate::domain::entities::{
    AgentConversationWorkspace, AgentConversationWorkspaceMode, AgentRun, AgentRunStatus,
    ChatContextType, ChatConversation, IdeationAnalysisBaseRefKind, Project,
};
use std::path::Path;

fn write_codex_fixture(path: &Path) {
    std::fs::write(
        path,
        r#"#!/bin/sh
if [ "$1" = "--version" ]; then
  printf 'codex-cli 0.116.0\n'
elif [ "$1" = "--help" ]; then
  printf '%s\n' 'Codex CLI' 'Commands:' '  exec' '  resume' '  mcp' 'Options:' '  -c, --config <key=value>' '  -m, --model <MODEL>' '  -s, --sandbox <SANDBOX>' '      --add-dir <DIR>'
elif [ "$1" = "exec" ] && [ "$2" = "--help" ]; then
  printf '%s\n' 'Run Codex non-interactively' 'Options:' '  -c, --config <key=value>' '  -m, --model <MODEL>' '  -s, --sandbox <SANDBOX>' '      --add-dir <DIR>' '      --json'
else
  printf '%s\n' '{"type":"thread.started","thread_id":"codex-session"}' '{"type":"item.completed","item":{"id":"answer","type":"agent_message","text":"verified"}}' '{"type":"turn.completed","usage":{"input_tokens":1,"cached_input_tokens":0,"output_tokens":1}}'
fi
"#,
    )
    .expect("write Codex fixture");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755))
            .expect("make Codex fixture executable");
    }
}

async fn enable_codex_fixture(state: &AppState, cli_path: &Path) {
    let mut settings = AgentProviderSettings::disabled_defaults(AgentHarnessKind::Codex);
    settings.enabled = true;
    settings.custom_binary_enabled = true;
    settings.custom_binary_path = Some(cli_path.to_string_lossy().into_owned());
    state
        .agent_provider_settings_repo
        .upsert(&settings)
        .await
        .expect("enable Codex fixture");
}

#[tokio::test]
async fn project_plan_action_resumes_exact_successful_codex_runtime() {
    let state = AppState::new_sqlite_test();
    let project_dir = tempfile::tempdir().expect("project directory");
    let cli_path = project_dir.path().join("codex-fixture");
    write_codex_fixture(&cli_path);
    enable_codex_fixture(&state, &cli_path).await;

    let mut project = Project::new(
        "Plan runtime continuity".to_string(),
        project_dir.path().to_string_lossy().into_owned(),
    );
    project.worktree_parent_directory = Some(
        project_dir
            .path()
            .join("agent-worktrees")
            .to_string_lossy()
            .into_owned(),
    );
    state
        .project_repo
        .create(project.clone())
        .await
        .expect("persist project");

    let mut conversation = ChatConversation::new_project(project.id.clone());
    conversation.set_agent_mode(Some(AgentConversationWorkspaceMode::Plan));
    conversation.set_provider_session_ref(ProviderSessionRef {
        harness: AgentHarnessKind::Codex,
        provider_session_id: "codex-session".to_string(),
    });
    let conversation_id = conversation.id;
    state
        .chat_conversation_repo
        .create(conversation)
        .await
        .expect("persist Plan conversation");

    let workspace_path = resolve_agent_conversation_workspace_path(&project, &conversation_id)
        .expect("resolve workspace path");
    std::fs::create_dir_all(workspace_path.join(".git")).expect("create workspace fixture");
    state
        .agent_conversation_workspace_repo
        .create_or_update(AgentConversationWorkspace::new(
            conversation_id,
            project.id.clone(),
            AgentConversationWorkspaceMode::Plan,
            IdeationAnalysisBaseRefKind::ProjectDefault,
            "main".to_string(),
            None,
            None,
            "ralphx/test/plan-runtime".to_string(),
            workspace_path.to_string_lossy().into_owned(),
        ))
        .await
        .expect("persist Plan workspace");

    let mut successful = AgentRun::new(conversation_id);
    successful.complete();
    successful.harness = Some(AgentHarnessKind::Codex);
    successful.provider_session_id = Some("codex-session".to_string());
    successful.logical_model = Some("gpt-5.6-sol".to_string());
    successful.effective_model_id = Some("gpt-5.6-sol".to_string());
    successful.logical_effort = Some(LogicalEffort::High);
    successful.effective_effort = Some("high".to_string());
    successful.approval_policy = Some("never".to_string());
    successful.sandbox_mode = Some("danger-full-access".to_string());
    state
        .agent_run_repo
        .create(successful)
        .await
        .expect("persist successful continuation authority");
    let mut failed = AgentRun::new(conversation_id);
    failed.status = AgentRunStatus::Failed;
    failed.harness = Some(AgentHarnessKind::Codex);
    failed.effective_model_id = Some("opus".to_string());
    state
        .agent_run_repo
        .create(failed)
        .await
        .expect("persist newer failed action history");

    let service = state
        .build_chat_service()
        .with_working_directory(project_dir.path());
    let result = service
        .send_message(
            ChatContextType::Project,
            project.id.as_str(),
            "Verify the current plan",
            SendMessageOptions {
                conversation_id_override: Some(conversation_id),
                metadata: Some(
                    r#"{"ralphx_action_kind":"verify_plan","ralphx_action_context_id":"plan-session","ralphx_action_target_id":"plan-artifact"}"#
                        .to_string(),
                ),
                ..Default::default()
            },
        )
        .await
        .expect("Plan action should launch under the stored Codex runtime");

    let launched = state
        .agent_run_repo
        .get_latest_for_conversation(&conversation_id)
        .await
        .expect("load launched run")
        .expect("launched run should be persisted");
    assert_eq!(launched.id.as_str(), result.agent_run_id);
    assert_eq!(launched.harness, Some(AgentHarnessKind::Codex));
    assert_eq!(
        launched.provider_session_id.as_deref(),
        Some("codex-session")
    );
    assert_eq!(launched.effective_model_id.as_deref(), Some("gpt-5.6-sol"));
    assert_eq!(launched.logical_effort, Some(LogicalEffort::High));
    assert_eq!(
        launched.runtime_source,
        Some(crate::domain::entities::RuntimeSource::HarnessFallback)
    );
}

#[tokio::test]
async fn incompatible_model_is_rejected_before_agent_run_persistence() {
    let state = AppState::new_sqlite_test();
    let project_dir = tempfile::tempdir().expect("project directory");
    let cli_path = project_dir.path().join("codex-fixture");
    write_codex_fixture(&cli_path);
    enable_codex_fixture(&state, &cli_path).await;

    let project = Project::new(
        "Model compatibility".to_string(),
        project_dir.path().to_string_lossy().into_owned(),
    );
    state
        .project_repo
        .create(project.clone())
        .await
        .expect("persist project");
    let conversation = state
        .chat_conversation_repo
        .create(ChatConversation::new_project(project.id.clone()))
        .await
        .expect("persist conversation");

    let service = state
        .build_chat_service()
        .with_working_directory(project_dir.path());
    let error = service
        .send_message(
            ChatContextType::Project,
            project.id.as_str(),
            "Use a foreign model",
            SendMessageOptions {
                conversation_id_override: Some(conversation.id),
                harness_override: Some(AgentHarnessKind::Codex),
                model_override: Some("opus".to_string()),
                ..Default::default()
            },
        )
        .await
        .expect_err("foreign model must fail before spawn");

    assert!(matches!(
        error,
        ChatServiceError::SpawnValidation {
            harness: AgentHarnessKind::Codex,
            ref model,
            ..
        } if model == "opus"
    ));
    assert!(state
        .agent_run_repo
        .get_by_conversation(&conversation.id)
        .await
        .expect("load agent runs")
        .is_empty());
}
