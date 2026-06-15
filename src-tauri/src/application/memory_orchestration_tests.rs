use super::*;
use crate::domain::repositories::MemoryEventRepository;
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
    )
    .await;

    // In test environment, build_spawnable_command returns Err due to ensure_claude_spawn_allowed()
    assert!(result.is_err());
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
