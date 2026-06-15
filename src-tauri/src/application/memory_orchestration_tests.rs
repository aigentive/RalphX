use super::*;
use crate::domain::agents::{AgentHarnessKind, LogicalEffort};
use crate::domain::repositories::MemoryEventRepository;
use crate::infrastructure::agents::mock::{MockAgenticClient, MockCallType};
use crate::infrastructure::memory::{
    InMemoryMemoryEventRepository, MemoryProjectMemorySettingsRepository,
};
use std::path::PathBuf;
use std::sync::Arc;

#[test]
fn test_context_to_category_mapping() {
    assert_eq!(
        MemoryCategory::from_context_type(ChatContextType::Ideation),
        MemoryCategory::Planning
    );
    assert_eq!(
        MemoryCategory::from_context_type(ChatContextType::Task),
        MemoryCategory::Execution
    );
    assert_eq!(
        MemoryCategory::from_context_type(ChatContextType::TaskExecution),
        MemoryCategory::Execution
    );
    assert_eq!(
        MemoryCategory::from_context_type(ChatContextType::Delegation),
        MemoryCategory::Execution
    );
    assert_eq!(
        MemoryCategory::from_context_type(ChatContextType::Review),
        MemoryCategory::Review
    );
    assert_eq!(
        MemoryCategory::from_context_type(ChatContextType::Merge),
        MemoryCategory::Merge
    );
    assert_eq!(
        MemoryCategory::from_context_type(ChatContextType::Project),
        MemoryCategory::ProjectChat
    );
}

#[test]
fn test_skip_reason_as_str() {
    assert_eq!(
        MemoryPipelineSkipReason::NoProjectId.as_str(),
        "no_project_id"
    );
    assert_eq!(
        MemoryPipelineSkipReason::RecursionGuard.as_str(),
        "recursion_guard"
    );
    assert_eq!(MemoryPipelineSkipReason::Disabled.as_str(), "disabled");
    assert_eq!(
        MemoryPipelineSkipReason::NoEnabledCategory.as_str(),
        "no_enabled_category"
    );
}

#[test]
fn test_category_as_str() {
    assert_eq!(MemoryCategory::Planning.as_str(), "planning");
    assert_eq!(MemoryCategory::Execution.as_str(), "execution");
    assert_eq!(MemoryCategory::Review.as_str(), "review");
    assert_eq!(MemoryCategory::Merge.as_str(), "merge");
    assert_eq!(MemoryCategory::ProjectChat.as_str(), "project_chat");
}

#[test]
fn test_default_settings() {
    let project_id = ProjectId::from_string("proj-default".to_string());
    let settings = ProjectMemorySettings::default_for_project(project_id.clone());
    assert_eq!(settings.project_id, project_id);
    assert!(settings.enabled);
    assert!(settings
        .maintenance_categories
        .contains(&"execution".to_string()));
    assert!(settings
        .maintenance_categories
        .contains(&"review".to_string()));
    assert!(settings
        .maintenance_categories
        .contains(&"merge".to_string()));
    assert!(settings
        .capture_categories
        .contains(&"planning".to_string()));
    assert!(settings
        .capture_categories
        .contains(&"execution".to_string()));
    assert!(settings.capture_categories.contains(&"review".to_string()));
}

#[test]
fn test_default_settings_maintenance_categories_count() {
    let settings = ProjectMemorySettings::default_for_project(ProjectId::from_string(
        "proj-default".to_string(),
    ));
    assert_eq!(settings.maintenance_categories.len(), 3);
}

#[test]
fn test_default_settings_capture_categories_count() {
    let settings = ProjectMemorySettings::default_for_project(ProjectId::from_string(
        "proj-default".to_string(),
    ));
    assert_eq!(settings.capture_categories.len(), 3);
}

#[tokio::test]
async fn test_trigger_memory_pipelines_no_project_id() {
    // Should return early without panicking
    let conv_id = ChatConversationId::from_string("conv-123".to_string());
    let cli_path = PathBuf::from("/usr/bin/claude");
    let plugin_dir = PathBuf::from("/plugins");
    let wd = PathBuf::from("/tmp");

    trigger_memory_pipelines(
        ChatContextType::TaskExecution,
        "task-123",
        &conv_id,
        None, // No project ID
        None,
        &cli_path,
        &plugin_dir,
        &wd,
        None,
        None,
        None,
        None,
    )
    .await;
    // Test passes if no panic
}

#[tokio::test]
async fn test_trigger_memory_pipelines_recursion_guard_maintainer() {
    // Should return early when agent is ralphx-memory-maintainer
    let project_id = ProjectId::from_string("proj-123".to_string());
    let conv_id = ChatConversationId::from_string("conv-123".to_string());
    let cli_path = PathBuf::from("/usr/bin/claude");
    let plugin_dir = PathBuf::from("/plugins");
    let wd = PathBuf::from("/tmp");

    trigger_memory_pipelines(
        ChatContextType::TaskExecution,
        "task-123",
        &conv_id,
        Some(&project_id),
        Some("ralphx-memory-maintainer"), // Recursion guard
        &cli_path,
        &plugin_dir,
        &wd,
        None,
        None,
        None,
        None,
    )
    .await;
    // Test passes if no spawn happens (verified via logs in real scenario)
}

#[tokio::test]
async fn test_trigger_memory_pipelines_recursion_guard_capture() {
    // Should return early when agent is ralphx-memory-capture
    let project_id = ProjectId::from_string("proj-123".to_string());
    let conv_id = ChatConversationId::from_string("conv-123".to_string());
    let cli_path = PathBuf::from("/usr/bin/claude");
    let plugin_dir = PathBuf::from("/plugins");
    let wd = PathBuf::from("/tmp");

    trigger_memory_pipelines(
        ChatContextType::TaskExecution,
        "task-123",
        &conv_id,
        Some(&project_id),
        Some("ralphx-memory-capture"), // Recursion guard
        &cli_path,
        &plugin_dir,
        &wd,
        None,
        None,
        None,
        None,
    )
    .await;
    // Test passes if no spawn happens
}

#[tokio::test]
async fn test_spawn_memory_maintainer_fails_in_test_env() {
    let project_id = ProjectId::from_string("proj-123".to_string());
    let conv_id = ChatConversationId::from_string("conv-123".to_string());
    let cli_path = PathBuf::from("/usr/bin/claude");
    let plugin_dir = PathBuf::from("/plugins");
    let wd = PathBuf::from("/tmp");

    let result = spawn_memory_maintainer(
        &conv_id,
        ChatContextType::TaskExecution,
        "task-123",
        &project_id,
        &cli_path,
        &plugin_dir,
        &wd,
        None,
    )
    .await;

    // In test environment, build_spawnable_command returns Err due to ensure_claude_spawn_allowed()
    assert!(result.is_err());
}

#[tokio::test]
async fn test_spawn_memory_capture_fails_in_test_env() {
    let project_id = ProjectId::from_string("proj-123".to_string());
    let conv_id = ChatConversationId::from_string("conv-123".to_string());
    let cli_path = PathBuf::from("/usr/bin/claude");
    let plugin_dir = PathBuf::from("/plugins");
    let wd = PathBuf::from("/tmp");

    let result = spawn_memory_capture(
        &conv_id,
        ChatContextType::TaskExecution,
        "task-123",
        &project_id,
        &cli_path,
        &plugin_dir,
        &wd,
        None,
    )
    .await;

    // In test environment, build_spawnable_command returns Err due to ensure_claude_spawn_allowed()
    assert!(result.is_err());
}

#[test]
fn test_build_memory_agent_config_uses_resolved_provider_model_and_env() {
    let project_id = ProjectId::from_string("proj-runtime".to_string());
    let conv_id = ChatConversationId::from_string("conv-runtime".to_string());
    let client: Arc<dyn crate::domain::agents::AgenticClient> =
        Arc::new(crate::infrastructure::MockAgenticClient::new());
    let runtime = ResolvedBackgroundAgentRuntime {
        client,
        harness: Some(AgentHarnessKind::Codex),
        model: Some("gpt-5.4".to_string()),
        cli_path_override: None,
        logical_effort: Some(LogicalEffort::Medium),
        approval_policy: Some("never".to_string()),
        sandbox_mode: Some("danger-full-access".to_string()),
    };

    let config = build_memory_agent_config(
        MemoryAgentKind::Capture,
        &runtime,
        "Capture learning".to_string(),
        &conv_id,
        ChatContextType::TaskExecution,
        "task-123",
        &project_id,
        PathBuf::from("/tmp/project").as_path(),
    );
    let conv_id_str = conv_id.as_str();

    assert_eq!(config.harness, Some(AgentHarnessKind::Codex));
    assert_eq!(config.model.as_deref(), Some("gpt-5.4"));
    assert_eq!(config.logical_effort, Some(LogicalEffort::Medium));
    assert_eq!(config.approval_policy.as_deref(), Some("never"));
    assert_eq!(config.sandbox_mode.as_deref(), Some("danger-full-access"));
    assert_eq!(config.agent.as_deref(), Some(MEMORY_CAPTURE_AGENT));
    assert_eq!(
        config.env.get("RALPHX_CONVERSATION_ID").map(String::as_str),
        Some(conv_id_str.as_str())
    );
    assert_eq!(
        config.env.get("RALPHX_PROJECT_ID").map(String::as_str),
        Some(project_id.as_str())
    );
}

#[test]
fn test_resolve_pipelines_parallel_spawn_both_enabled() {
    // "execution" is in both maintenance_categories AND capture_categories by default
    let project_id = ProjectId::from_string("proj-123".to_string());
    let settings = ProjectMemorySettings::default_for_project(project_id.clone());

    let result = resolve_pipelines(
        ChatContextType::TaskExecution,
        Some(&project_id),
        Some("ralphx:ralphx-execution-worker"),
        &settings,
    );

    assert!(
        result.is_some(),
        "Should return Some when category is enabled"
    );
    let (should_maintain, should_capture) = result.unwrap();
    assert!(
        should_maintain,
        "execution should be in maintenance_categories"
    );
    assert!(should_capture, "execution should be in capture_categories");
}

#[test]
fn test_resolve_pipelines_disabled_project_skips_spawn() {
    let project_id = ProjectId::from_string("proj-123".to_string());
    let settings = ProjectMemorySettings {
        project_id: project_id.clone(),
        enabled: false,
        maintenance_categories: vec!["execution".to_string()],
        capture_categories: vec!["execution".to_string()],
    };

    let result = resolve_pipelines(
        ChatContextType::TaskExecution,
        Some(&project_id),
        Some("ralphx:ralphx-execution-worker"),
        &settings,
    );

    assert!(
        result.is_none(),
        "Should return None when memory is disabled"
    );
}

#[test]
fn test_resolve_pipelines_with_reason_reports_skip_reasons() {
    let project_id = ProjectId::from_string("proj-skip-reasons".to_string());
    let enabled_settings = ProjectMemorySettings {
        project_id: project_id.clone(),
        enabled: true,
        maintenance_categories: vec!["execution".to_string()],
        capture_categories: Vec::new(),
    };

    assert_eq!(
        resolve_pipelines_with_reason(
            ChatContextType::TaskExecution,
            None,
            Some("ralphx:ralphx-execution-worker"),
            &enabled_settings,
        ),
        Err(MemoryPipelineSkipReason::NoProjectId)
    );
    assert_eq!(
        resolve_pipelines_with_reason(
            ChatContextType::TaskExecution,
            Some(&project_id),
            Some("ralphx-memory-capture"),
            &enabled_settings,
        ),
        Err(MemoryPipelineSkipReason::RecursionGuard)
    );

    let disabled_settings = ProjectMemorySettings {
        enabled: false,
        ..enabled_settings.clone()
    };
    assert_eq!(
        resolve_pipelines_with_reason(
            ChatContextType::TaskExecution,
            Some(&project_id),
            Some("ralphx:ralphx-execution-worker"),
            &disabled_settings,
        ),
        Err(MemoryPipelineSkipReason::Disabled)
    );

    assert_eq!(
        resolve_pipelines_with_reason(
            ChatContextType::Project,
            Some(&project_id),
            Some("ralphx:ralphx-chat"),
            &enabled_settings,
        ),
        Err(MemoryPipelineSkipReason::NoEnabledCategory)
    );
}

#[test]
fn test_resolve_pipelines_separates_maintenance_and_capture_categories() {
    let project_id = ProjectId::from_string("proj-category-split".to_string());
    let settings = ProjectMemorySettings {
        project_id: project_id.clone(),
        enabled: true,
        maintenance_categories: vec!["review".to_string()],
        capture_categories: vec!["project_chat".to_string()],
    };

    assert_eq!(
        resolve_pipelines_with_reason(
            ChatContextType::Review,
            Some(&project_id),
            Some("ralphx:ralphx-execution-reviewer"),
            &settings,
        ),
        Ok((true, false))
    );
    assert_eq!(
        resolve_pipelines_with_reason(
            ChatContextType::Project,
            Some(&project_id),
            Some("ralphx:ralphx-chat"),
            &settings,
        ),
        Ok((false, true))
    );
}

#[tokio::test]
async fn test_trigger_memory_pipelines_uses_repository_settings_and_logs_disabled_skip() {
    let project_id = ProjectId::from_string("proj-repo-settings".to_string());
    let conv_id = ChatConversationId::from_string("conv-repo-settings".to_string());
    let cli_path = PathBuf::from("/usr/bin/claude");
    let plugin_dir = PathBuf::from("/plugins");
    let wd = PathBuf::from("/tmp");

    let settings_repo = Arc::new(MemoryProjectMemorySettingsRepository::new());
    settings_repo
        .insert(ProjectMemorySettings {
            project_id: project_id.clone(),
            enabled: false,
            maintenance_categories: vec!["execution".to_string()],
            capture_categories: vec!["execution".to_string()],
        })
        .await;
    let event_repo = Arc::new(InMemoryMemoryEventRepository::new());

    trigger_memory_pipelines(
        ChatContextType::TaskExecution,
        "task-123",
        &conv_id,
        Some(&project_id),
        Some("ralphx:ralphx-execution-worker"),
        &cli_path,
        &plugin_dir,
        &wd,
        None,
        Some(event_repo.clone()),
        Some(settings_repo),
        None,
    )
    .await;

    let events = event_repo
        .get_by_type("memory_pipeline_skipped")
        .await
        .unwrap();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].project_id, project_id);
    assert_eq!(events[0].details["reason"], "disabled");
    assert_eq!(events[0].details["conversation_id"], conv_id.as_str());
}

#[tokio::test]
async fn test_trigger_memory_pipelines_logs_no_enabled_category_skip() {
    let project_id = ProjectId::from_string("proj-no-category".to_string());
    let conv_id = ChatConversationId::from_string("conv-no-category".to_string());
    let cli_path = PathBuf::from("/usr/bin/claude");
    let plugin_dir = PathBuf::from("/plugins");
    let wd = PathBuf::from("/tmp");
    let event_repo = Arc::new(InMemoryMemoryEventRepository::new());

    trigger_memory_pipelines(
        ChatContextType::Project,
        "conversation-project",
        &conv_id,
        Some(&project_id),
        Some("ralphx:ralphx-chat"),
        &cli_path,
        &plugin_dir,
        &wd,
        Some(ProjectMemorySettings {
            project_id: project_id.clone(),
            enabled: true,
            maintenance_categories: vec!["execution".to_string()],
            capture_categories: Vec::new(),
        }),
        Some(event_repo.clone()),
        None,
        None,
    )
    .await;

    let events = event_repo
        .get_by_type("memory_pipeline_skipped")
        .await
        .unwrap();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].details["reason"], "no_enabled_category");
    assert_eq!(events[0].details["context_type"], "project");
}

#[tokio::test]
async fn test_trigger_memory_pipelines_uses_provided_settings_before_repository_settings() {
    let project_id = ProjectId::from_string("proj-provided-settings".to_string());
    let conv_id = ChatConversationId::from_string("conv-provided-settings".to_string());
    let cli_path = PathBuf::from("/usr/bin/claude");
    let plugin_dir = PathBuf::from("/plugins");
    let wd = PathBuf::from("/tmp");
    let settings_repo = Arc::new(MemoryProjectMemorySettingsRepository::new());
    settings_repo
        .insert(ProjectMemorySettings {
            project_id: project_id.clone(),
            enabled: false,
            maintenance_categories: vec!["planning".to_string()],
            capture_categories: vec!["planning".to_string()],
        })
        .await;
    let event_repo = Arc::new(InMemoryMemoryEventRepository::new());

    trigger_memory_pipelines(
        ChatContextType::Ideation,
        "session-provided",
        &conv_id,
        Some(&project_id),
        Some("ralphx:ralphx-planner"),
        &cli_path,
        &plugin_dir,
        &wd,
        Some(ProjectMemorySettings {
            project_id: project_id.clone(),
            enabled: true,
            maintenance_categories: Vec::new(),
            capture_categories: vec!["planning".to_string()],
        }),
        Some(event_repo.clone()),
        Some(settings_repo),
        None,
    )
    .await;

    let skipped = event_repo
        .get_by_type("memory_pipeline_skipped")
        .await
        .unwrap();
    assert!(skipped.is_empty());
    let spawned = event_repo
        .get_by_type("memory_pipeline_spawn_requested")
        .await
        .unwrap();
    assert_eq!(spawned.len(), 1);
    assert_eq!(spawned[0].actor_type, MemoryActorType::MemoryCapture);
}

#[tokio::test]
async fn test_trigger_memory_pipelines_logs_enabled_capture_spawn_request() {
    let project_id = ProjectId::from_string("proj-capture-enabled".to_string());
    let conv_id = ChatConversationId::from_string("conv-capture-enabled".to_string());
    let cli_path = PathBuf::from("/usr/bin/claude");
    let plugin_dir = PathBuf::from("/plugins");
    let wd = PathBuf::from("/tmp");
    let event_repo = Arc::new(InMemoryMemoryEventRepository::new());

    trigger_memory_pipelines(
        ChatContextType::Ideation,
        "session-123",
        &conv_id,
        Some(&project_id),
        Some("ralphx:ralphx-planner"),
        &cli_path,
        &plugin_dir,
        &wd,
        Some(ProjectMemorySettings {
            project_id: project_id.clone(),
            enabled: true,
            maintenance_categories: Vec::new(),
            capture_categories: vec!["planning".to_string()],
        }),
        Some(event_repo.clone()),
        None,
        None,
    )
    .await;

    let events = event_repo
        .get_by_type("memory_pipeline_spawn_requested")
        .await
        .unwrap();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].project_id, project_id);
    assert_eq!(events[0].actor_type, MemoryActorType::MemoryCapture);
    assert_eq!(events[0].details["agent"], "ralphx-memory-capture");
    assert_eq!(events[0].details["conversation_id"], conv_id.as_str());
    assert_eq!(events[0].details["context_type"], "ideation");
    assert_eq!(events[0].details["context_id"], "session-123");
}

#[tokio::test]
async fn test_spawn_memory_agent_with_runtime_records_spawn_and_completion() {
    let project_id = ProjectId::from_string("proj-runtime-spawn".to_string());
    let conv_id = ChatConversationId::from_string("conv-runtime-spawn".to_string());
    let mock_client = Arc::new(MockAgenticClient::new());
    let client: Arc<dyn crate::domain::agents::AgenticClient> = mock_client.clone();
    let runtime = ResolvedBackgroundAgentRuntime {
        client,
        harness: Some(AgentHarnessKind::Codex),
        model: Some("gpt-5.4-mini".to_string()),
        cli_path_override: Some(PathBuf::from("/usr/local/bin/codex")),
        logical_effort: Some(LogicalEffort::Low),
        approval_policy: Some("never".to_string()),
        sandbox_mode: Some("danger-full-access".to_string()),
    };

    spawn_memory_agent_with_runtime(
        MemoryAgentKind::Maintainer,
        runtime,
        "Maintain project memory".to_string(),
        &conv_id,
        ChatContextType::Merge,
        "merge-123",
        &project_id,
        PathBuf::from("/tmp/project").as_path(),
    )
    .await
    .unwrap();

    tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    let calls = mock_client.get_calls().await;
    assert!(calls.iter().any(|call| matches!(
        &call.call_type,
        MockCallType::Spawn { prompt, .. } if prompt == "Maintain project memory"
    )));
    assert!(calls
        .iter()
        .any(|call| matches!(&call.call_type, MockCallType::WaitForCompletion { .. })));
}
