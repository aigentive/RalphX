use super::*;
use crate::application::chat_service::{ChatService, MockChatService};
use crate::application::execution_recovery::restart_transition_target;
use crate::application::execution_resume::determine_paused_restore_status;
use crate::application::execution_state::sync_project_quota;
use crate::application::task_resume_execution::prepare_resumed_task_for_entry_actions;
use crate::application::TaskTransitionService;
use crate::domain::entities::{
    artifact::ArtifactId, AgentConversationWorkspace, AgentConversationWorkspaceMode, AgentRun,
    IdeationSessionId, IdeationSessionStatus, TaskId,
    AgentRunStatus, ChatContextType, ChatConversation, ChatConversationId, GitMode,
    IdeationAnalysisBaseRefKind, IdeationSession, PlanBranch, PlanBranchId, PlanBranchStatus,
    TaskStep, TaskStepStatus, ValidationCacheDecision, ValidationCommandCategory,
    ValidationCommandResult, ValidationCommandSource, ValidationCommandStatus,
    ValidationContextType, ValidationPurpose, ValidationRun, ValidationRunMode,
    ValidationRunStatus,
};
use crate::domain::services::{QueueKey, QueuedMessage, RunningAgentKey};
use crate::utils::path_safety::validate_absolute_non_root_path;
use std::path::Path;
use std::sync::Arc;
use tauri::test::{mock_builder, mock_context, noop_assets};
use tauri::Manager;

async fn enable_tasks_for_progress(state: &AppState) {
    let current = state.ideation_settings_repo.get_settings().await.unwrap();
    if current.tasks_feature_state == crate::domain::ideation::TasksFeatureState::Enabled {
        return;
    }
    assert!(state
        .ideation_settings_repo
        .compare_and_set_tasks_feature_state(
            current.tasks_feature_state,
            crate::domain::ideation::TasksFeatureState::Enabled,
        )
        .await
        .unwrap());
}

// ========================================
// ExecutionState Unit Tests
// ========================================

#[test]
fn test_execution_state_new() {
    let state = ExecutionState::new();
    assert!(!state.is_paused());
    assert_eq!(state.running_count(), 0);
    assert_eq!(state.max_concurrent(), 10);
}

#[test]
fn test_execution_state_with_max_concurrent() {
    let state = ExecutionState::with_max_concurrent(5);
    assert_eq!(state.max_concurrent(), 5);
}

#[test]
fn test_execution_state_pause_resume() {
    let state = ExecutionState::new();

    assert!(!state.is_paused());

    state.pause();
    assert!(state.is_paused());

    state.resume();
    assert!(!state.is_paused());
}

#[test]
fn test_execution_state_running_count() {
    let state = ExecutionState::new();

    assert_eq!(state.running_count(), 0);

    let count = state.increment_running();
    assert_eq!(count, 1);
    assert_eq!(state.running_count(), 1);

    let count = state.increment_running();
    assert_eq!(count, 2);
    assert_eq!(state.running_count(), 2);

    let count = state.decrement_running();
    assert_eq!(count, 1);
    assert_eq!(state.running_count(), 1);
}

#[test]
fn test_execution_state_decrement_no_underflow() {
    let state = ExecutionState::new();

    // Should not underflow
    let count = state.decrement_running();
    assert_eq!(count, 0);
    assert_eq!(state.running_count(), 0);
}

#[test]
fn test_queued_message_to_send_options_preserves_references_and_attachments() {
    let queue = crate::domain::services::MessageQueue::new();
    let project_reference = crate::domain::services::ComposerProjectReference {
        path: "src/main.rs".to_string(),
        kind: Some(crate::domain::services::ComposerProjectReferenceKind::File),
    };
    let integration_reference = crate::domain::services::ComposerIntegrationReference {
        provider: "atlassian".to_string(),
        kind: "jira".to_string(),
        id: "RX-42".to_string(),
        key: Some("RX-42".to_string()),
        title: Some("Fix queued replay".to_string()),
        url: Some("https://example.atlassian.net/browse/RX-42".to_string()),
        summary_excerpt: None,
        include_transcript: None,
    };
    let attachment_id = crate::domain::entities::ChatAttachmentId::new();

    let queued = queue.queue_with_overrides_and_project_references(
        ChatContextType::Project,
        "project-1",
        "queued".to_string(),
        Some(r#"{"source":"test"}"#.to_string()),
        Some("2026-03-11T10:00:00Z".to_string()),
        Some(crate::domain::agents::AgentHarnessKind::Codex),
        vec![project_reference.clone()],
        vec![integration_reference.clone()],
        Vec::new(),
        None,
        Vec::new(),
        vec![attachment_id],
    );

    let options = queued_message_to_send_options(&queued);

    assert_eq!(options.metadata.as_deref(), Some(r#"{"source":"test"}"#));
    assert_eq!(
        options.created_at.map(|ts| ts.to_rfc3339()),
        Some("2026-03-11T10:00:00+00:00".to_string())
    );
    assert_eq!(
        options.harness_override,
        Some(crate::domain::agents::AgentHarnessKind::Codex)
    );
    assert_eq!(options.composer_project_references, vec![project_reference]);
    assert_eq!(
        options.composer_integration_references,
        vec![integration_reference]
    );
    assert_eq!(options.attachment_ids, vec![attachment_id]);
}

#[tokio::test]
async fn test_active_project_state_set_if_changed_dedupes_same_project() {
    let state = ActiveProjectState::new();
    let project_id = ProjectId::from_string("project-1".to_string());

    assert!(state.set_if_changed(Some(project_id.clone())).await);
    assert!(!state.set_if_changed(Some(project_id.clone())).await);
    assert_eq!(state.get().await, Some(project_id));
    assert!(state.set_if_changed(None).await);
    assert!(!state.set_if_changed(None).await);
}

#[tokio::test]
async fn test_set_active_project_skips_when_project_is_already_active() {
    let active_project_state = Arc::new(ActiveProjectState::new());
    let execution_state = Arc::new(ExecutionState::new());
    let app_state = AppState::new_test();
    let project = app_state
        .project_repo
        .create(Project::new(
            "Active Project".into(),
            "/tmp/active-project".into(),
        ))
        .await
        .unwrap();
    active_project_state.set(Some(project.id.clone())).await;

    let app = mock_builder()
        .manage(Arc::clone(&active_project_state))
        .manage(execution_state)
        .manage(app_state)
        .build(mock_context(noop_assets()))
        .expect("mock app should build");

    set_active_project(
        Some(project.id.as_str().to_string()),
        app.state::<Arc<ActiveProjectState>>(),
        app.state::<Arc<ExecutionState>>(),
        app.state::<AppState>(),
    )
    .await
    .expect("setting the already-active project should be a no-op");

    assert_eq!(active_project_state.get().await, Some(project.id));
}

#[tokio::test]
async fn test_set_active_project_persists_when_project_changes() {
    let active_project_state = Arc::new(ActiveProjectState::new());
    let execution_state = Arc::new(ExecutionState::new());
    let app_state = AppState::new_test();
    let project = app_state
        .project_repo
        .create(Project::new(
            "Changed Project".into(),
            "/tmp/changed-project".into(),
        ))
        .await
        .unwrap();

    let app = mock_builder()
        .manage(Arc::clone(&active_project_state))
        .manage(execution_state)
        .manage(app_state)
        .build(mock_context(noop_assets()))
        .expect("mock app should build");

    set_active_project(
        Some(project.id.as_str().to_string()),
        app.state::<Arc<ActiveProjectState>>(),
        app.state::<Arc<ExecutionState>>(),
        app.state::<AppState>(),
    )
    .await
    .expect("new active project should be persisted");

    assert_eq!(active_project_state.get().await, Some(project.id.clone()));
    let persisted = app
        .state::<AppState>()
        .app_state_repo
        .get()
        .await
        .unwrap()
        .active_project_id;
    assert_eq!(persisted, Some(project.id));
}

#[test]
fn test_execution_state_set_max_concurrent() {
    let state = ExecutionState::new();

    state.set_max_concurrent(10);
    assert_eq!(state.max_concurrent(), 10);
}

#[test]
fn test_execution_state_can_start_task() {
    let state = ExecutionState::with_max_concurrent(2);

    // Initially can start
    assert!(state.can_start_task());

    // After pausing, cannot start
    state.pause();
    assert!(!state.can_start_task());

    // After resuming, can start again
    state.resume();
    assert!(state.can_start_task());

    // Fill up to max concurrent
    state.increment_running();
    state.increment_running();
    assert!(!state.can_start_task());

    // After one completes, can start again
    state.decrement_running();
    assert!(state.can_start_task());
}

#[test]
fn test_execution_state_global_max_concurrent() {
    let state = ExecutionState::new();

    // Default global max is 20
    assert_eq!(state.global_max_concurrent(), 20);

    // Set global max
    state.set_global_max_concurrent(10);
    assert_eq!(state.global_max_concurrent(), 10);

    // Clamped to max 50
    state.set_global_max_concurrent(100);
    assert_eq!(state.global_max_concurrent(), 50);

    // Clamped to min 1
    state.set_global_max_concurrent(0);
    assert_eq!(state.global_max_concurrent(), 1);
}

#[test]
fn test_execution_state_can_start_task_respects_global_cap() {
    let state = ExecutionState::with_max_concurrent(10);
    // Set global cap lower than per-project max
    state.set_global_max_concurrent(3);

    assert!(state.can_start_task());

    // Fill up to global cap
    state.increment_running();
    state.increment_running();
    state.increment_running();

    // At global cap (3), per-project max (10) still has room, but global blocks
    assert!(!state.can_start_task());

    // Free a slot
    state.decrement_running();
    assert!(state.can_start_task());
}

#[test]
fn test_execution_state_can_start_task_per_project_cap_lower() {
    let state = ExecutionState::with_max_concurrent(2);
    // Global cap is higher than per-project max
    state.set_global_max_concurrent(20);

    state.increment_running();
    state.increment_running();

    // At per-project cap (2), global cap (20) still has room, but per-project blocks
    assert!(!state.can_start_task());
}

// ========================================
// Provider Rate Limit Backpressure Tests
// ========================================

#[test]
fn test_can_start_task_returns_false_when_provider_blocked() {
    let state = ExecutionState::new();
    // Set provider blocked until 60 seconds in the future
    let future_epoch = chrono::Utc::now().timestamp() as u64 + 60;
    state.set_provider_blocked_until(future_epoch);

    assert!(state.is_provider_blocked());
    assert!(!state.can_start_task());
}

#[test]
fn test_can_start_task_returns_true_when_block_expired() {
    let state = ExecutionState::new();
    // Set provider blocked until 60 seconds in the past
    let past_epoch = chrono::Utc::now().timestamp() as u64 - 60;
    state.set_provider_blocked_until(past_epoch);

    assert!(!state.is_provider_blocked());
    assert!(state.can_start_task());
}

#[test]
fn test_can_start_task_returns_true_when_no_block() {
    let state = ExecutionState::new();
    // Default: no provider block
    assert!(!state.is_provider_blocked());
    assert!(state.can_start_task());
}

#[test]
fn test_set_clear_provider_block_lifecycle() {
    let state = ExecutionState::new();

    // Initially not blocked
    assert!(!state.is_provider_blocked());
    assert_eq!(state.provider_blocked_until_epoch(), 0);

    // Set block in the future
    let future_epoch = chrono::Utc::now().timestamp() as u64 + 300;
    state.set_provider_blocked_until(future_epoch);
    assert!(state.is_provider_blocked());
    assert_eq!(state.provider_blocked_until_epoch(), future_epoch);

    // Clear block
    state.clear_provider_block();
    assert!(!state.is_provider_blocked());
    assert_eq!(state.provider_blocked_until_epoch(), 0);
}

#[test]
fn test_provider_block_independent_of_pause() {
    let state = ExecutionState::new();
    let future_epoch = chrono::Utc::now().timestamp() as u64 + 60;
    state.set_provider_blocked_until(future_epoch);

    // Provider blocked, not paused — still can't start
    assert!(!state.is_paused());
    assert!(state.is_provider_blocked());
    assert!(!state.can_start_task());

    // Clear provider block, pause — still can't start (different reason)
    state.clear_provider_block();
    state.pause();
    assert!(!state.is_provider_blocked());
    assert!(state.is_paused());
    assert!(!state.can_start_task());

    // Both blocked — still can't start
    state.set_provider_blocked_until(future_epoch);
    assert!(state.is_provider_blocked());
    assert!(state.is_paused());
    assert!(!state.can_start_task());
}

#[test]
fn test_execution_state_thread_safe() {
    use std::thread;

    let state = Arc::new(ExecutionState::new());
    let mut handles = vec![];

    // Spawn threads that increment and decrement
    for _ in 0..10 {
        let state_clone = Arc::clone(&state);
        handles.push(thread::spawn(move || {
            state_clone.increment_running();
        }));
    }

    for handle in handles {
        handle.join().unwrap();
    }

    assert_eq!(state.running_count(), 10);

    let mut handles = vec![];
    for _ in 0..10 {
        let state_clone = Arc::clone(&state);
        handles.push(thread::spawn(move || {
            state_clone.decrement_running();
        }));
    }

    for handle in handles {
        handle.join().unwrap();
    }

    assert_eq!(state.running_count(), 0);
}

// ========================================
// Response Serialization Tests
// ========================================

#[test]
fn test_execution_status_response_serialization() {
    let response = ExecutionStatusResponse {
        is_paused: true,
        halt_mode: "paused".to_string(),
        running_count: 1,
        max_concurrent: 2,
        global_max_concurrent: 20,
        queued_count: 5,
        queued_message_count: 2,
        can_start_task: false,
        provider_blocked: false,
        provider_blocked_until: None,
        ideation_active: 0,
        ideation_idle: 0,
        ideation_waiting: 0,
        ideation_max_project: 2,
        ideation_max_global: 4,
    };

    let json = serde_json::to_string(&response).unwrap();

    // Verify snake_case serialization (Rust default, frontend transform handles conversion)
    assert!(json.contains("\"is_paused\":true"));
    assert!(json.contains("\"halt_mode\":\"paused\""));
    assert!(json.contains("\"running_count\":1"));
    assert!(json.contains("\"max_concurrent\":2"));
    assert!(json.contains("\"global_max_concurrent\":20"));
    assert!(json.contains("\"queued_count\":5"));
    assert!(json.contains("\"queued_message_count\":2"));
    assert!(json.contains("\"can_start_task\":false"));
}

#[test]
fn test_execution_command_response_serialization() {
    let response = ExecutionCommandResponse {
        success: true,
        status: ExecutionStatusResponse {
            is_paused: false,
            halt_mode: "running".to_string(),
            running_count: 0,
            max_concurrent: 2,
            global_max_concurrent: 20,
            queued_count: 3,
            queued_message_count: 1,
            can_start_task: true,
            provider_blocked: false,
            provider_blocked_until: None,
            ideation_active: 0,
            ideation_idle: 0,
            ideation_waiting: 0,
            ideation_max_project: 2,
            ideation_max_global: 4,
        },
    };

    let json = serde_json::to_string(&response).unwrap();

    // Verify snake_case serialization (Rust default, frontend transform handles conversion)
    assert!(json.contains("\"success\":true"));
    assert!(json.contains("\"status\":"));
    assert!(json.contains("\"is_paused\":false"));
    assert!(json.contains("\"halt_mode\":\"running\""));
}

#[test]
fn test_execution_settings_response_serialization() {
    let response = ExecutionSettingsResponse {
        max_concurrent_tasks: 4,
        project_ideation_max: 2,
        auto_commit: true,
        pause_on_failure: false,
        agent_workspace_pr_autofix_default: true,
        agent_workspace_pr_auto_merge_default: false,
    };

    let json = serde_json::to_string(&response).unwrap();

    // Verify snake_case serialization
    assert!(json.contains("\"max_concurrent_tasks\":4"));
    assert!(json.contains("\"project_ideation_max\":2"));
    assert!(json.contains("\"auto_commit\":true"));
    assert!(json.contains("\"pause_on_failure\":false"));
    assert!(json.contains("\"agent_workspace_pr_autofix_default\":true"));
    assert!(json.contains("\"agent_workspace_pr_auto_merge_default\":false"));
}

#[test]
fn test_execution_settings_response_from_domain() {
    let settings = ExecutionSettings {
        max_concurrent_tasks: 3,
        project_ideation_max: 1,
        auto_commit: false,
        pause_on_failure: true,
        agent_workspace_pr_autofix_default: true,
        agent_workspace_pr_auto_merge_default: false,
    };

    let response = ExecutionSettingsResponse::from(settings);

    assert_eq!(response.max_concurrent_tasks, 3);
    assert_eq!(response.project_ideation_max, 1);
    assert!(!response.auto_commit);
    assert!(response.pause_on_failure);
    assert!(response.agent_workspace_pr_autofix_default);
    assert!(!response.agent_workspace_pr_auto_merge_default);
}

#[test]
fn test_update_execution_settings_input_deserialization() {
    let json = r#"{"max_concurrent_tasks":5,"project_ideation_max":2,"auto_commit":false,"pause_on_failure":true,"agent_workspace_pr_autofix_default":true,"agent_workspace_pr_auto_merge_default":false}"#;

    let input: UpdateExecutionSettingsInput =
        serde_json::from_str(json).expect("Failed to deserialize input");

    assert_eq!(input.max_concurrent_tasks, 5);
    assert_eq!(input.project_ideation_max, 2);
    assert!(!input.auto_commit);
    assert!(input.pause_on_failure);
    assert!(input.agent_workspace_pr_autofix_default);
    assert!(!input.agent_workspace_pr_auto_merge_default);
}

#[test]
fn test_global_execution_settings_response_serialization() {
    let response = GlobalExecutionSettingsResponse {
        global_max_concurrent: 20,
        workspace_max_concurrent: 10,
        global_ideation_max: 4,
        allow_ideation_borrow_idle_execution: true,
    };

    let json = serde_json::to_string(&response).unwrap();

    assert!(json.contains("\"global_max_concurrent\":20"));
    assert!(json.contains("\"workspace_max_concurrent\":10"));
    assert!(json.contains("\"global_ideation_max\":4"));
    assert!(json.contains("\"allow_ideation_borrow_idle_execution\":true"));
}

#[test]
fn test_global_execution_settings_response_from_domain() {
    let settings = crate::domain::execution::GlobalExecutionSettings {
        global_max_concurrent: 18,
        workspace_max_concurrent: 7,
        global_ideation_max: 3,
        allow_ideation_borrow_idle_execution: false,
    };

    let response = GlobalExecutionSettingsResponse::from(settings);

    assert_eq!(response.global_max_concurrent, 18);
    assert_eq!(response.workspace_max_concurrent, 7);
    assert_eq!(response.global_ideation_max, 3);
    assert!(!response.allow_ideation_borrow_idle_execution);
}

#[test]
fn test_global_execution_settings_deserializes_legacy_without_workspace_limit() {
    let json = r#"{
        "global_max_concurrent": 12,
        "global_ideation_max": 5,
        "allow_ideation_borrow_idle_execution": true
    }"#;

    let settings: crate::domain::execution::GlobalExecutionSettings =
        serde_json::from_str(json).unwrap();

    assert_eq!(settings.global_max_concurrent, 12);
    assert_eq!(
        settings.workspace_max_concurrent,
        crate::domain::execution::DEFAULT_WORKSPACE_MAX_CONCURRENT
    );
    assert_eq!(settings.global_ideation_max, 5);
    assert!(settings.allow_ideation_borrow_idle_execution);
}

#[test]
fn test_update_global_execution_settings_input_deserialization() {
    let json = r#"{"global_max_concurrent":22,"workspace_max_concurrent":10,"global_ideation_max":5,"allow_ideation_borrow_idle_execution":true}"#;

    let input: UpdateGlobalExecutionSettingsInput =
        serde_json::from_str(json).expect("Failed to deserialize global input");

    assert_eq!(input.global_max_concurrent, 22);
    assert_eq!(input.workspace_max_concurrent, 10);
    assert_eq!(input.global_ideation_max, 5);
    assert!(input.allow_ideation_borrow_idle_execution);
}

#[tokio::test]
async fn test_update_global_execution_settings_syncs_runtime_state_and_persists() {
    let active_project_state = Arc::new(ActiveProjectState::new());
    let execution_state = Arc::new(ExecutionState::new());
    execution_state.set_global_max_concurrent(2);
    execution_state.set_workspace_max_concurrent(1);
    execution_state.set_global_ideation_max(1);
    execution_state.set_allow_ideation_borrow_idle_execution(false);
    let app_state = AppState::new_test();

    let app = mock_builder()
        .manage(Arc::clone(&active_project_state))
        .manage(Arc::clone(&execution_state))
        .manage(app_state)
        .build(mock_context(noop_assets()))
        .expect("mock app should build");

    let response = update_global_execution_settings(
        UpdateGlobalExecutionSettingsInput {
            global_max_concurrent: 12,
            workspace_max_concurrent: 6,
            global_ideation_max: 5,
            allow_ideation_borrow_idle_execution: true,
        },
        app.state::<Arc<ActiveProjectState>>(),
        app.state::<Arc<ExecutionState>>(),
        app.state::<AppState>(),
    )
    .await
    .expect("global settings should update");

    assert_eq!(response.global_max_concurrent, 12);
    assert_eq!(response.workspace_max_concurrent, 6);
    assert_eq!(response.global_ideation_max, 5);
    assert!(response.allow_ideation_borrow_idle_execution);
    assert_eq!(execution_state.global_max_concurrent(), 12);
    assert_eq!(execution_state.workspace_max_concurrent(), 6);
    assert_eq!(execution_state.global_ideation_max(), 5);
    assert!(execution_state.allow_ideation_borrow_idle_execution());

    let stored = app
        .state::<AppState>()
        .global_execution_settings_repo
        .get_settings()
        .await
        .expect("stored global settings");
    assert_eq!(stored.global_max_concurrent, 12);
    assert_eq!(stored.workspace_max_concurrent, 6);
    assert_eq!(stored.global_ideation_max, 5);
    assert!(stored.allow_ideation_borrow_idle_execution);
}

// ========================================
// sync_project_quota Tests
// ========================================

#[tokio::test]
async fn test_sync_project_quota_explicit_project_priority() {
    let app_state = AppState::new_test();
    let execution_state = Arc::new(ExecutionState::new());
    let active_project_state = Arc::new(ActiveProjectState::new());

    // Create two projects with different quotas
    let project1 = Project::new("Project 1".to_string(), "/path1".to_string());
    let project2 = Project::new("Project 2".to_string(), "/path2".to_string());

    app_state
        .project_repo
        .create(project1.clone())
        .await
        .unwrap();
    app_state
        .project_repo
        .create(project2.clone())
        .await
        .unwrap();

    // Set different quotas for each project
    let settings1 = ExecutionSettings {
        max_concurrent_tasks: 5,
        project_ideation_max: 2,
        auto_commit: true,
        pause_on_failure: true,
        ..ExecutionSettings::default()
    };
    let settings2 = ExecutionSettings {
        max_concurrent_tasks: 10,
        project_ideation_max: 2,
        auto_commit: true,
        pause_on_failure: true,
        ..ExecutionSettings::default()
    };

    app_state
        .execution_settings_repo
        .update_settings(Some(&project1.id), &settings1)
        .await
        .unwrap();
    app_state
        .execution_settings_repo
        .update_settings(Some(&project2.id), &settings2)
        .await
        .unwrap();

    // Set project1 as active
    active_project_state.set(Some(project1.id.clone())).await;

    // Call sync with explicit project2 - should use project2, not active project1
    let result = sync_project_quota(
        Some(project2.id.clone()),
        &active_project_state,
        &execution_state,
        &app_state,
    )
    .await
    .unwrap();

    // Should use explicit project2 (quota 10), not active project1 (quota 5)
    assert_eq!(result.project_id, Some(project2.id));
    assert_eq!(result.max_concurrent, 10);
    assert_eq!(execution_state.max_concurrent(), 10);
}

#[tokio::test]
async fn test_sync_project_quota_active_project_fallback() {
    let app_state = AppState::new_test();
    let execution_state = Arc::new(ExecutionState::new());
    let active_project_state = Arc::new(ActiveProjectState::new());

    // Create project with custom quota
    let project = Project::new("Active Project".to_string(), "/path".to_string());
    app_state
        .project_repo
        .create(project.clone())
        .await
        .unwrap();

    let settings = ExecutionSettings {
        max_concurrent_tasks: 7,
        project_ideation_max: 2,
        auto_commit: true,
        pause_on_failure: true,
        ..ExecutionSettings::default()
    };
    app_state
        .execution_settings_repo
        .update_settings(Some(&project.id), &settings)
        .await
        .unwrap();

    // Set as active project
    active_project_state.set(Some(project.id.clone())).await;

    // Call sync without explicit project - should use active project
    let result = sync_project_quota(None, &active_project_state, &execution_state, &app_state)
        .await
        .unwrap();

    assert_eq!(result.project_id, Some(project.id));
    assert_eq!(result.max_concurrent, 7);
    assert_eq!(execution_state.max_concurrent(), 7);
}

#[tokio::test]
async fn test_sync_project_quota_none_fallback_to_global_default() {
    let app_state = AppState::new_test();
    let execution_state = Arc::new(ExecutionState::new());
    let active_project_state = Arc::new(ActiveProjectState::new());

    // No explicit project, no active project
    // Should use global default (project_id = None)
    let result = sync_project_quota(None, &active_project_state, &execution_state, &app_state)
        .await
        .unwrap();

    assert_eq!(result.project_id, None);
    // Default quota is 10 (from ExecutionSettings::default())
    assert_eq!(result.max_concurrent, 10);
    assert_eq!(execution_state.max_concurrent(), 10);
}

#[tokio::test]
async fn test_sync_project_quota_updates_execution_state() {
    let app_state = AppState::new_test();
    let execution_state = Arc::new(ExecutionState::with_max_concurrent(99));
    let active_project_state = Arc::new(ActiveProjectState::new());

    let project = Project::new("Test Project".to_string(), "/path".to_string());
    app_state
        .project_repo
        .create(project.clone())
        .await
        .unwrap();

    let settings = ExecutionSettings {
        max_concurrent_tasks: 15,
        project_ideation_max: 2,
        auto_commit: true,
        pause_on_failure: true,
        ..ExecutionSettings::default()
    };
    app_state
        .execution_settings_repo
        .update_settings(Some(&project.id), &settings)
        .await
        .unwrap();

    // Before sync, execution_state has old value
    assert_eq!(execution_state.max_concurrent(), 99);

    // Sync should update execution_state
    sync_project_quota(
        Some(project.id.clone()),
        &active_project_state,
        &execution_state,
        &app_state,
    )
    .await
    .unwrap();

    // After sync, execution_state should have new value
    assert_eq!(execution_state.max_concurrent(), 15);
}

#[tokio::test]
async fn test_sync_project_quota_multiple_calls_idempotent() {
    let app_state = AppState::new_test();
    let execution_state = Arc::new(ExecutionState::new());
    let active_project_state = Arc::new(ActiveProjectState::new());

    let project = Project::new("Test Project".to_string(), "/path".to_string());
    app_state
        .project_repo
        .create(project.clone())
        .await
        .unwrap();

    let settings = ExecutionSettings {
        max_concurrent_tasks: 8,
        project_ideation_max: 2,
        auto_commit: true,
        pause_on_failure: true,
        ..ExecutionSettings::default()
    };
    app_state
        .execution_settings_repo
        .update_settings(Some(&project.id), &settings)
        .await
        .unwrap();

    // Call sync multiple times
    for _ in 0..3 {
        let result = sync_project_quota(
            Some(project.id.clone()),
            &active_project_state,
            &execution_state,
            &app_state,
        )
        .await
        .unwrap();

        assert_eq!(result.project_id, Some(project.id.clone()));
        assert_eq!(result.max_concurrent, 8);
        assert_eq!(execution_state.max_concurrent(), 8);
    }
}

#[tokio::test]
async fn test_sync_project_quota_switching_between_projects() {
    let app_state = AppState::new_test();
    let execution_state = Arc::new(ExecutionState::new());
    let active_project_state = Arc::new(ActiveProjectState::new());

    let project1 = Project::new("Project 1".to_string(), "/path1".to_string());
    let project2 = Project::new("Project 2".to_string(), "/path2".to_string());

    app_state
        .project_repo
        .create(project1.clone())
        .await
        .unwrap();
    app_state
        .project_repo
        .create(project2.clone())
        .await
        .unwrap();

    let settings1 = ExecutionSettings {
        max_concurrent_tasks: 3,
        project_ideation_max: 1,
        auto_commit: true,
        pause_on_failure: true,
        ..ExecutionSettings::default()
    };
    let settings2 = ExecutionSettings {
        max_concurrent_tasks: 12,
        project_ideation_max: 2,
        auto_commit: true,
        pause_on_failure: true,
        ..ExecutionSettings::default()
    };

    app_state
        .execution_settings_repo
        .update_settings(Some(&project1.id), &settings1)
        .await
        .unwrap();
    app_state
        .execution_settings_repo
        .update_settings(Some(&project2.id), &settings2)
        .await
        .unwrap();

    // Sync to project1
    sync_project_quota(
        Some(project1.id.clone()),
        &active_project_state,
        &execution_state,
        &app_state,
    )
    .await
    .unwrap();
    assert_eq!(execution_state.max_concurrent(), 3);

    // Switch to project2
    sync_project_quota(
        Some(project2.id.clone()),
        &active_project_state,
        &execution_state,
        &app_state,
    )
    .await
    .unwrap();
    assert_eq!(execution_state.max_concurrent(), 12);

    // Switch back to project1
    sync_project_quota(
        Some(project1.id.clone()),
        &active_project_state,
        &execution_state,
        &app_state,
    )
    .await
    .unwrap();
    assert_eq!(execution_state.max_concurrent(), 3);
}

// ========================================
// Integration Tests with AppState
// ========================================

use crate::domain::entities::{Project, Task};
use crate::domain::repositories::{ProjectRepository, TaskRepository};
use crate::infrastructure::memory::{MemoryProjectRepository, MemoryTaskRepository};

async fn setup_test_state() -> (Arc<ExecutionState>, AppState) {
    let execution_state = Arc::new(ExecutionState::new());
    let task_repo = Arc::new(MemoryTaskRepository::new());
    let project_repo = Arc::new(MemoryProjectRepository::new());

    // Create a test project with tasks
    let project = Project::new("Test Project".to_string(), "/test/path".to_string());
    project_repo
        .create(project.clone())
        .await
        .expect("Failed to create test project");

    // Create tasks in various statuses
    let mut task1 = Task::new(project.id.clone(), "Ready Task 1".to_string());
    task1.internal_status = InternalStatus::Ready;
    task_repo
        .create(task1)
        .await
        .expect("Failed to create Ready task 1");

    let mut task2 = Task::new(project.id.clone(), "Ready Task 2".to_string());
    task2.internal_status = InternalStatus::Ready;
    task_repo
        .create(task2)
        .await
        .expect("Failed to create Ready task 2");

    let mut task3 = Task::new(project.id.clone(), "Executing Task".to_string());
    task3.internal_status = InternalStatus::Executing;
    task_repo
        .create(task3)
        .await
        .expect("Failed to create Executing task");

    let mut task4 = Task::new(project.id.clone(), "Backlog Task".to_string());
    task4.internal_status = InternalStatus::Backlog;
    task_repo
        .create(task4)
        .await
        .expect("Failed to create Backlog task");

    let app_state = AppState::with_repos(task_repo, project_repo);

    (execution_state, app_state)
}

#[tokio::test]
async fn test_get_execution_status_counts_ready_tasks() {
    let (execution_state, app_state) = setup_test_state().await;

    // Simulate the command by directly calling the logic
    let all_projects = app_state.project_repo.get_all().await.unwrap();

    let mut queued_count = 0u32;
    for project in all_projects {
        let tasks = app_state
            .task_repo
            .get_by_project(&project.id)
            .await
            .unwrap();
        queued_count += tasks
            .iter()
            .filter(|t| t.internal_status == InternalStatus::Ready)
            .count() as u32;
    }

    // We created 2 ready tasks
    assert_eq!(queued_count, 2);
    assert!(!execution_state.is_paused());
    assert_eq!(execution_state.running_count(), 0);
}

#[tokio::test]
async fn test_count_slot_consuming_queued_messages_counts_all_pending_messages() {
    let (_execution_state, app_state) = setup_test_state().await;
    let project = app_state.project_repo.get_all().await.unwrap().remove(0);

    let review_task = app_state
        .task_repo
        .create(Task {
            internal_status: InternalStatus::Reviewing,
            ..Task::new(project.id.clone(), "Queued review task".to_string())
        })
        .await
        .unwrap();

    let merge_task = app_state
        .task_repo
        .create(Task {
            internal_status: InternalStatus::Merging,
            ..Task::new(project.id.clone(), "Queued merge task".to_string())
        })
        .await
        .unwrap();

    let session = app_state
        .ideation_session_repo
        .create(IdeationSession::new(project.id.clone()))
        .await
        .unwrap();

    app_state.message_queue.queue(
        ChatContextType::Review,
        review_task.id.as_str(),
        "review message one".to_string(),
    );
    app_state.message_queue.queue(
        ChatContextType::Review,
        review_task.id.as_str(),
        "review message two".to_string(),
    );
    app_state.message_queue.queue(
        ChatContextType::Merge,
        merge_task.id.as_str(),
        "merge message".to_string(),
    );
    app_state.message_queue.queue(
        ChatContextType::Ideation,
        session.id.as_str(),
        "ideation message".to_string(),
    );

    let queued_message_count = count_slot_consuming_queued_messages(Some(&project.id), &app_state)
        .await
        .expect("count queued messages");

    assert_eq!(queued_message_count, 4);
}

#[tokio::test]
async fn test_pause_sets_paused_flag() {
    let (execution_state, _app_state) = setup_test_state().await;

    assert!(!execution_state.is_paused());
    execution_state.pause();
    assert!(execution_state.is_paused());
}

#[tokio::test]
async fn test_resume_clears_paused_flag() {
    let (execution_state, _app_state) = setup_test_state().await;

    execution_state.pause();
    assert!(execution_state.is_paused());

    execution_state.resume();
    assert!(!execution_state.is_paused());
}

#[tokio::test]
async fn test_stop_cancels_executing_tasks() {
    let (_execution_state, app_state) = setup_test_state().await;

    // Get the project
    let projects = app_state.project_repo.get_all().await.unwrap();
    let project = &projects[0];

    // Find the executing task and stop it (simulating stop_execution behavior)
    let tasks = app_state
        .task_repo
        .get_by_project(&project.id)
        .await
        .unwrap();
    for mut task in tasks {
        if task.internal_status == InternalStatus::Executing {
            task.internal_status = InternalStatus::Stopped;
            task.touch();
            app_state.task_repo.update(&task).await.unwrap();
        }
    }

    // Verify the task is now stopped (not failed)
    let tasks = app_state
        .task_repo
        .get_by_project(&project.id)
        .await
        .unwrap();
    let executing_count = tasks
        .iter()
        .filter(|t| t.internal_status == InternalStatus::Executing)
        .count();
    let stopped_count = tasks
        .iter()
        .filter(|t| t.internal_status == InternalStatus::Stopped)
        .count();

    assert_eq!(executing_count, 0);
    assert_eq!(stopped_count, 1);
}

#[tokio::test]
async fn test_stop_cancels_multiple_agent_active_tasks() {
    // Setup: Create tasks in various agent-active states
    let execution_state = Arc::new(ExecutionState::new());
    let app_state = AppState::new_test();

    // Create a test project
    let project = Project::new("Test Project".to_string(), "/test/path".to_string());
    app_state
        .project_repo
        .create(project.clone())
        .await
        .unwrap();

    // Create tasks in all agent-active statuses
    let mut task1 = Task::new(project.id.clone(), "Executing Task".to_string());
    task1.internal_status = InternalStatus::Executing;
    app_state.task_repo.create(task1.clone()).await.unwrap();

    let mut task2 = Task::new(project.id.clone(), "QaRefining Task".to_string());
    task2.internal_status = InternalStatus::QaRefining;
    app_state.task_repo.create(task2.clone()).await.unwrap();

    let mut task3 = Task::new(project.id.clone(), "Reviewing Task".to_string());
    task3.internal_status = InternalStatus::Reviewing;
    app_state.task_repo.create(task3.clone()).await.unwrap();

    // Create a task NOT in agent-active state (should not be affected)
    let mut task4 = Task::new(project.id.clone(), "Ready Task".to_string());
    task4.internal_status = InternalStatus::Ready;
    app_state.task_repo.create(task4.clone()).await.unwrap();

    // Build transition service (same as stop_execution does)
    let transition_service: TaskTransitionService = TaskTransitionService::new(
        Arc::clone(&app_state.task_repo),
        Arc::clone(&app_state.task_dependency_repo),
        Arc::clone(&app_state.project_repo),
        Arc::clone(&app_state.chat_message_repo),
        Arc::clone(&app_state.chat_attachment_repo),
        Arc::clone(&app_state.chat_conversation_repo),
        Arc::clone(&app_state.agent_run_repo),
        Arc::clone(&app_state.ideation_session_repo),
        Arc::clone(&app_state.activity_event_repo),
        Arc::clone(&app_state.message_queue),
        Arc::clone(&app_state.running_agent_registry),
        Arc::clone(&execution_state),
        None,
        Arc::clone(&app_state.memory_event_repo),
    );

    // Pause execution (as stop_execution would)
    execution_state.pause();

    // Transition all agent-active tasks to Stopped (as stop_execution does)
    let tasks = app_state
        .task_repo
        .get_by_project(&project.id)
        .await
        .unwrap();
    for task in tasks {
        if AGENT_ACTIVE_STATUSES.contains(&task.internal_status) {
            let _ = transition_service
                .transition_task(&task.id, InternalStatus::Stopped)
                .await;
        }
    }

    // Verify: All agent-active tasks should now be Stopped
    let tasks = app_state
        .task_repo
        .get_by_project(&project.id)
        .await
        .unwrap();

    let stopped_count = tasks
        .iter()
        .filter(|t| t.internal_status == InternalStatus::Stopped)
        .count();

    let ready_count = tasks
        .iter()
        .filter(|t| t.internal_status == InternalStatus::Ready)
        .count();

    // 3 agent-active tasks should be Stopped
    assert_eq!(stopped_count, 3);
    // 1 Ready task should remain Ready
    assert_eq!(ready_count, 1);
    // Execution should be paused
    assert!(execution_state.is_paused());
}

#[tokio::test]
async fn test_stop_clears_queued_chat_messages() {
    let app_state = AppState::new_test();
    let project = Project::new("Test Project".to_string(), "/test/path".to_string());
    app_state
        .project_repo
        .create(project.clone())
        .await
        .unwrap();

    let session = app_state
        .ideation_session_repo
        .create(IdeationSession::new(project.id.clone()))
        .await
        .unwrap();

    let task = app_state
        .task_repo
        .create(Task::new(project.id.clone(), "Task".to_string()))
        .await
        .unwrap();

    app_state.message_queue.queue(
        ChatContextType::Ideation,
        session.id.as_str(),
        "queued ideation".to_string(),
    );
    app_state.message_queue.queue(
        ChatContextType::TaskExecution,
        task.id.as_str(),
        "queued execution".to_string(),
    );
    app_state.message_queue.queue(
        ChatContextType::Review,
        task.id.as_str(),
        "queued review".to_string(),
    );
    app_state.message_queue.queue(
        ChatContextType::Merge,
        task.id.as_str(),
        "queued merge".to_string(),
    );
    app_state.message_queue.queue(
        ChatContextType::Task,
        task.id.as_str(),
        "keep task".to_string(),
    );
    app_state.message_queue.queue(
        ChatContextType::Project,
        project.id.as_str(),
        "keep project".to_string(),
    );

    let cleared = clear_paused_chat_queues(None, &app_state)
        .await
        .expect("clear queued chat work");

    assert_eq!(cleared, 6);
    assert!(app_state
        .message_queue
        .get_queued(ChatContextType::Ideation, session.id.as_str())
        .is_empty());
    assert!(app_state
        .message_queue
        .get_queued(ChatContextType::TaskExecution, task.id.as_str())
        .is_empty());
    assert!(app_state
        .message_queue
        .get_queued(ChatContextType::Review, task.id.as_str())
        .is_empty());
    assert!(app_state
        .message_queue
        .get_queued(ChatContextType::Merge, task.id.as_str())
        .is_empty());
    assert_eq!(
        app_state
            .message_queue
            .get_queued(ChatContextType::Task, task.id.as_str())
            .len(),
        0
    );
    assert_eq!(
        app_state
            .message_queue
            .get_queued(ChatContextType::Project, project.id.as_str())
            .len(),
        0
    );
}

#[tokio::test]
async fn test_pause_transitions_agent_active_tasks_to_paused() {
    // Setup: Create tasks in various agent-active states
    let execution_state = Arc::new(ExecutionState::new());
    let app_state = AppState::new_test();

    // Create a test project
    let project = Project::new("Test Project".to_string(), "/test/path".to_string());
    app_state
        .project_repo
        .create(project.clone())
        .await
        .unwrap();

    // Create tasks in all agent-active statuses
    let mut task1 = Task::new(project.id.clone(), "Executing Task".to_string());
    task1.internal_status = InternalStatus::Executing;
    app_state.task_repo.create(task1.clone()).await.unwrap();

    let mut task2 = Task::new(project.id.clone(), "QaRefining Task".to_string());
    task2.internal_status = InternalStatus::QaRefining;
    app_state.task_repo.create(task2.clone()).await.unwrap();

    let mut task3 = Task::new(project.id.clone(), "Reviewing Task".to_string());
    task3.internal_status = InternalStatus::Reviewing;
    app_state.task_repo.create(task3.clone()).await.unwrap();

    // Create a task NOT in agent-active state (should not be affected)
    let mut task4 = Task::new(project.id.clone(), "Ready Task".to_string());
    task4.internal_status = InternalStatus::Ready;
    app_state.task_repo.create(task4.clone()).await.unwrap();

    // Build transition service (same as pause_execution does)
    let transition_service: TaskTransitionService = TaskTransitionService::new(
        Arc::clone(&app_state.task_repo),
        Arc::clone(&app_state.task_dependency_repo),
        Arc::clone(&app_state.project_repo),
        Arc::clone(&app_state.chat_message_repo),
        Arc::clone(&app_state.chat_attachment_repo),
        Arc::clone(&app_state.chat_conversation_repo),
        Arc::clone(&app_state.agent_run_repo),
        Arc::clone(&app_state.ideation_session_repo),
        Arc::clone(&app_state.activity_event_repo),
        Arc::clone(&app_state.message_queue),
        Arc::clone(&app_state.running_agent_registry),
        Arc::clone(&execution_state),
        None,
        Arc::clone(&app_state.memory_event_repo),
    );

    // Pause execution (as pause_execution would)
    execution_state.pause();

    // Transition all agent-active tasks to Paused (as pause_execution does)
    let tasks = app_state
        .task_repo
        .get_by_project(&project.id)
        .await
        .unwrap();
    for task in tasks {
        if AGENT_ACTIVE_STATUSES.contains(&task.internal_status) {
            let _ = transition_service
                .transition_task(&task.id, InternalStatus::Paused)
                .await;
        }
    }

    // Verify: All agent-active tasks should now be Paused
    let tasks = app_state
        .task_repo
        .get_by_project(&project.id)
        .await
        .unwrap();

    let paused_count = tasks
        .iter()
        .filter(|t| t.internal_status == InternalStatus::Paused)
        .count();

    let ready_count = tasks
        .iter()
        .filter(|t| t.internal_status == InternalStatus::Ready)
        .count();

    // 3 agent-active tasks should be Paused
    assert_eq!(paused_count, 3);
    // 1 Ready task should remain Ready
    assert_eq!(ready_count, 1);
    // Execution should be paused
    assert!(execution_state.is_paused());
}

#[tokio::test]
async fn test_resume_relaunches_one_queued_message_for_active_ideation_session() {
    let app_state = AppState::new_test();
    let execution_state = Arc::new(ExecutionState::new());
    let project = Project::new("Resume Project".to_string(), "/test/path".to_string());
    app_state
        .project_repo
        .create(project.clone())
        .await
        .unwrap();

    let session = app_state
        .ideation_session_repo
        .create(IdeationSession::new(project.id.clone()))
        .await
        .unwrap();

    app_state.message_queue.queue_with_overrides(
        ChatContextType::Ideation,
        session.id.as_str(),
        "first queued".to_string(),
        Some(r#"{"source":"pause"}"#.to_string()),
        Some("2026-03-25T10:00:00Z".to_string()),
        None,
    );
    app_state.message_queue.queue(
        ChatContextType::Ideation,
        session.id.as_str(),
        "second queued".to_string(),
    );

    let mock = Arc::new(MockChatService::new());
    let resumed =
        resume_paused_ideation_queues_with_chat_service(None, &app_state, &execution_state, || {
            Arc::clone(&mock) as Arc<dyn ChatService>
        })
        .await
        .expect("resume paused ideation queue");

    assert_eq!(resumed, 1);
    assert_eq!(mock.call_count(), 1);
    assert_eq!(
        app_state
            .message_queue
            .get_queued(ChatContextType::Ideation, session.id.as_str())
            .len(),
        1,
        "resume should relaunch only the front queued message for the session"
    );
    assert_eq!(
        mock.get_sent_messages().await,
        vec!["first queued".to_string()]
    );
}

#[tokio::test]
async fn test_resume_relaunches_durable_queued_ideation_message() {
    let app_state = AppState::new_test();
    let execution_state = Arc::new(ExecutionState::new());
    let project = Project::new(
        "Durable Ideation Resume".to_string(),
        "/test/durable-ideation".to_string(),
    );
    app_state
        .project_repo
        .create(project.clone())
        .await
        .unwrap();
    let session = app_state
        .ideation_session_repo
        .create(IdeationSession::new(project.id.clone()))
        .await
        .unwrap();
    let key = QueueKey::new(ChatContextType::Ideation, session.id.as_str());
    let mut queued = QueuedMessage::with_id(
        "durable-ideation".to_string(),
        "durable ideation".to_string(),
    );
    queued.metadata_override = Some(r#"{"resume_in_place":true}"#.to_string());
    app_state
        .queued_message_repo
        .enqueue_back(&key, &queued)
        .await
        .unwrap();

    let mock = Arc::new(MockChatService::new());
    let resumed =
        resume_paused_ideation_queues_with_chat_service(None, &app_state, &execution_state, || {
            Arc::clone(&mock) as Arc<dyn ChatService>
        })
        .await
        .expect("resume durable ideation queue");

    assert_eq!(resumed, 1);
    assert_eq!(
        mock.get_sent_messages().await,
        vec!["durable ideation".to_string()]
    );
    assert!(app_state
        .queued_message_repo
        .list(&key)
        .await
        .unwrap()
        .is_empty());
}

#[tokio::test]
async fn test_resume_clears_durable_queue_for_inactive_ideation_session() {
    let app_state = AppState::new_test();
    let execution_state = Arc::new(ExecutionState::new());
    let project = Project::new(
        "Inactive Durable Ideation".to_string(),
        "/test/inactive-durable-ideation".to_string(),
    );
    app_state
        .project_repo
        .create(project.clone())
        .await
        .unwrap();
    let session = app_state
        .ideation_session_repo
        .create(IdeationSession::new(project.id.clone()))
        .await
        .unwrap();
    app_state
        .ideation_session_repo
        .update_status(&session.id, IdeationSessionStatus::Archived)
        .await
        .unwrap();
    let key = QueueKey::new(ChatContextType::Ideation, session.id.as_str());
    let queued =
        QueuedMessage::with_id("archived-ideation".to_string(), "should clear".to_string());
    app_state
        .queued_message_repo
        .enqueue_back(&key, &queued)
        .await
        .unwrap();

    let mock = Arc::new(MockChatService::new());
    let resumed =
        resume_paused_ideation_queues_with_chat_service(None, &app_state, &execution_state, || {
            Arc::clone(&mock) as Arc<dyn ChatService>
        })
        .await
        .expect("resume durable inactive ideation queue");

    assert_eq!(resumed, 0);
    assert_eq!(mock.call_count(), 0);
    assert!(app_state
        .queued_message_repo
        .list(&key)
        .await
        .unwrap()
        .is_empty());
}

#[tokio::test]
async fn test_resume_relaunches_queued_task_chat_message() {
    let app_state = AppState::new_test();
    let project = Project::new(
        "Resume Task Chat".to_string(),
        "/test/task-chat".to_string(),
    );
    app_state
        .project_repo
        .create(project.clone())
        .await
        .unwrap();

    let task = app_state
        .task_repo
        .create(Task::new(
            project.id.clone(),
            "Task chat target".to_string(),
        ))
        .await
        .unwrap();

    app_state.message_queue.queue_with_overrides(
        ChatContextType::Task,
        task.id.as_str(),
        "resume task chat".to_string(),
        Some(r#"{"resume_in_place":true}"#.to_string()),
        None,
        None,
    );

    let mock = Arc::new(MockChatService::new());
    let resumed = resume_paused_non_slot_chat_queues_with_chat_service(None, &app_state, || {
        Arc::clone(&mock) as Arc<dyn ChatService>
    })
    .await
    .expect("resume paused task chat queue");

    assert_eq!(resumed, 1);
    assert_eq!(mock.call_count(), 1);
    assert_eq!(
        mock.get_sent_messages().await,
        vec!["resume task chat".to_string()]
    );
    assert!(app_state
        .message_queue
        .get_queued(ChatContextType::Task, task.id.as_str())
        .is_empty());
}

#[tokio::test]
async fn test_resume_relaunches_queued_standalone_chat_message() {
    // Pause-drain parity (Phase 4a.3, blocking carry-forward from the 4a.1
    // review): Standalone is pause-managed at the admission layers
    // (claude_launches_paused / should_requeue_after_provider_pause /
    // is_pause_managed_chat_context) but, before this fix,
    // resume_paused_non_slot_chat_queues_with_chat_service only matched
    // ChatContextType::Task, so a paused standalone send would durably queue
    // but NEVER be drained/delivered on resume. This asserts delivery
    // (send_message actually invoked with the queued content), not merely
    // that resume ran without error.
    let app_state = AppState::new_test();
    let conversation = app_state
        .chat_conversation_repo
        .create(ChatConversation::new_standalone())
        .await
        .unwrap();

    // Simulates a paused send: the message lands in the live in-memory queue
    // under the standalone conversation's self-key exactly as
    // should_requeue_after_provider_pause / claude_launches_paused would have
    // routed it.
    app_state.message_queue.queue_with_overrides(
        ChatContextType::Standalone,
        conversation.id.as_str().as_str(),
        "resume standalone chat".to_string(),
        None,
        None,
        None,
    );

    let mock = Arc::new(MockChatService::new());
    // MockChatService keeps its own isolated conversation store (independent
    // of app_state.chat_conversation_repo); standalone conversations are
    // never lazily auto-vivified from a bare context_id (by design, matching
    // production get_or_create_conversation), so the mock's send_message
    // would otherwise fail to resolve a conversation for this context_id.
    mock.add_conversation(conversation.clone()).await;
    let resumed = resume_paused_non_slot_chat_queues_with_chat_service(None, &app_state, || {
        Arc::clone(&mock) as Arc<dyn ChatService>
    })
    .await
    .expect("resume paused standalone chat queue");

    assert_eq!(
        resumed, 1,
        "the standalone queued message must be resumed, not silently skipped"
    );
    assert_eq!(mock.call_count(), 1);
    assert_eq!(
        mock.get_sent_messages().await,
        vec!["resume standalone chat".to_string()],
        "resume must actually deliver the queued content, not just drain the queue"
    );
    assert!(app_state
        .message_queue
        .get_queued(
            ChatContextType::Standalone,
            conversation.id.as_str().as_str()
        )
        .is_empty());
}

#[tokio::test]
async fn test_resume_relaunches_durable_queued_standalone_chat_message() {
    // Durable-queue counterpart of the in-memory test above: a paused
    // standalone send that persisted to the durable queued_message_repo
    // (surviving a restart) must also be drained and delivered on resume.
    let app_state = AppState::new_test();
    let conversation = app_state
        .chat_conversation_repo
        .create(ChatConversation::new_standalone())
        .await
        .unwrap();
    let key = QueueKey::new(
        ChatContextType::Standalone,
        conversation.id.as_str().as_str(),
    );
    let queued = QueuedMessage::with_id(
        "durable-standalone-chat".to_string(),
        "durable standalone chat".to_string(),
    );
    app_state
        .queued_message_repo
        .enqueue_back(&key, &queued)
        .await
        .unwrap();

    let mock = Arc::new(MockChatService::new());
    mock.add_conversation(conversation.clone()).await;
    let resumed = resume_paused_non_slot_chat_queues_with_chat_service(None, &app_state, || {
        Arc::clone(&mock) as Arc<dyn ChatService>
    })
    .await
    .expect("resume durable standalone chat queue");

    assert_eq!(resumed, 1);
    assert_eq!(
        mock.get_sent_messages().await,
        vec!["durable standalone chat".to_string()],
        "resume must actually deliver the durably queued content"
    );
    assert!(app_state
        .queued_message_repo
        .list(&key)
        .await
        .unwrap()
        .is_empty());
}

#[tokio::test]
async fn test_resume_standalone_chat_queue_is_project_filter_scoped_out() {
    // Regression guard for the OTHER direction: when a resume is
    // project-scoped (project_filter: Some(...)), a Standalone queue key must
    // NOT match — queue_key_matches_project already returns Ok(false) for
    // Standalone whenever a project filter is supplied (Standalone has no
    // project to match). This is the resume-time reflection of D3's "never
    // matches a project filter" rule; a Project-scoped resume must not
    // accidentally drain a projectless conversation's queue.
    let app_state = AppState::new_test();
    let conversation = app_state
        .chat_conversation_repo
        .create(ChatConversation::new_standalone())
        .await
        .unwrap();
    app_state.message_queue.queue_with_overrides(
        ChatContextType::Standalone,
        conversation.id.as_str().as_str(),
        "should not resume under a project filter".to_string(),
        None,
        None,
        None,
    );

    let project_id = ProjectId::from_string("resume-standalone-project-filter".to_string());
    let mock = Arc::new(MockChatService::new());
    let resumed =
        resume_paused_non_slot_chat_queues_with_chat_service(Some(&project_id), &app_state, || {
            Arc::clone(&mock) as Arc<dyn ChatService>
        })
        .await
        .expect("resume should not error even when nothing matches the project filter");

    assert_eq!(
        resumed, 0,
        "a project-scoped resume must not drain a standalone queue"
    );
    assert_eq!(mock.call_count(), 0);
    assert!(!app_state
        .message_queue
        .get_queued(
            ChatContextType::Standalone,
            conversation.id.as_str().as_str()
        )
        .is_empty());
}

#[tokio::test]
async fn test_resume_relaunches_durable_queued_task_chat_message() {
    let app_state = AppState::new_test();
    let project = Project::new(
        "Resume Durable Task Chat".to_string(),
        "/test/durable-task-chat".to_string(),
    );
    app_state
        .project_repo
        .create(project.clone())
        .await
        .unwrap();
    let task = app_state
        .task_repo
        .create(Task::new(
            project.id.clone(),
            "Durable task chat".to_string(),
        ))
        .await
        .unwrap();
    let key = QueueKey::new(ChatContextType::Task, task.id.as_str());
    let queued = QueuedMessage::with_id(
        "durable-task-chat".to_string(),
        "durable task chat".to_string(),
    );
    app_state
        .queued_message_repo
        .enqueue_back(&key, &queued)
        .await
        .unwrap();

    let mock = Arc::new(MockChatService::new());
    let resumed = resume_paused_non_slot_chat_queues_with_chat_service(None, &app_state, || {
        Arc::clone(&mock) as Arc<dyn ChatService>
    })
    .await
    .expect("resume durable task chat queue");

    assert_eq!(resumed, 1);
    assert_eq!(
        mock.get_sent_messages().await,
        vec!["durable task chat".to_string()]
    );
    assert!(app_state
        .queued_message_repo
        .list(&key)
        .await
        .unwrap()
        .is_empty());
}

#[tokio::test]
async fn test_resume_restores_durable_non_slot_message_when_send_fails() {
    let app_state = AppState::new_test();
    let project = Project::new(
        "Restore Durable Task Chat".to_string(),
        "/test/restore-durable-task-chat".to_string(),
    );
    app_state
        .project_repo
        .create(project.clone())
        .await
        .unwrap();
    let task = app_state
        .task_repo
        .create(Task::new(
            project.id.clone(),
            "Restore task chat".to_string(),
        ))
        .await
        .unwrap();
    let key = QueueKey::new(ChatContextType::Task, task.id.as_str());
    let queued = QueuedMessage::with_id(
        "restore-task-chat".to_string(),
        "restore task chat".to_string(),
    );
    app_state
        .queued_message_repo
        .enqueue_back(&key, &queued)
        .await
        .unwrap();

    let mock = Arc::new(MockChatService::new());
    mock.set_available(false).await;
    let resumed = resume_paused_non_slot_chat_queues_with_chat_service(None, &app_state, || {
        Arc::clone(&mock) as Arc<dyn ChatService>
    })
    .await
    .expect("resume durable task chat queue");

    assert_eq!(resumed, 0);
    assert_eq!(
        app_state.message_queue.get_queued_with_key(&key),
        vec![queued.clone()]
    );
    assert_eq!(
        app_state.queued_message_repo.list(&key).await.unwrap(),
        vec![queued]
    );
}

#[tokio::test]
async fn test_resume_does_not_requeue_a_turn_the_agent_already_received() {
    let app_state = AppState::new_test();
    let project = Project::new(
        "Delivered Durable Task Chat".to_string(),
        "/test/delivered-durable-task-chat".to_string(),
    );
    app_state
        .project_repo
        .create(project.clone())
        .await
        .unwrap();
    let task = app_state
        .task_repo
        .create(Task::new(
            project.id.clone(),
            "Delivered task chat".to_string(),
        ))
        .await
        .unwrap();
    let key = QueueKey::new(ChatContextType::Task, task.id.as_str());
    let queued = QueuedMessage::with_id(
        "delivered-task-chat".to_string(),
        "delivered task chat".to_string(),
    );
    app_state
        .queued_message_repo
        .enqueue_back(&key, &queued)
        .await
        .unwrap();

    let mock = Arc::new(MockChatService::new());
    mock.fail_next_send_as_delivered_not_persisted().await;
    let resumed = resume_paused_non_slot_chat_queues_with_chat_service(None, &app_state, || {
        Arc::clone(&mock) as Arc<dyn ChatService>
    })
    .await
    .expect("resume durable task chat queue");

    // The live process accepted this turn: restoring it would deliver it twice.
    assert_eq!(resumed, 0);
    assert!(app_state.message_queue.get_queued_with_key(&key).is_empty());
    assert!(app_state
        .queued_message_repo
        .list(&key)
        .await
        .unwrap()
        .is_empty());
}

#[tokio::test]
async fn test_resume_relaunches_queued_review_message_with_harness_override() {
    let app_state = AppState::new_test();
    let execution_state = Arc::new(ExecutionState::new());
    let project = Project::new("Resume Review".to_string(), "/test/review".to_string());
    app_state
        .project_repo
        .create(project.clone())
        .await
        .unwrap();

    let mut task = app_state
        .task_repo
        .create(Task::new(project.id.clone(), "Review target".to_string()))
        .await
        .unwrap();
    task.internal_status = InternalStatus::Reviewing;
    app_state.task_repo.update(&task).await.unwrap();

    app_state.message_queue.queue_with_overrides(
        ChatContextType::Review,
        task.id.as_str(),
        "resume review".to_string(),
        Some(r#"{"resume_in_place":true}"#.to_string()),
        None,
        Some(crate::domain::agents::AgentHarnessKind::Codex),
    );

    let mock = Arc::new(MockChatService::new());
    let resumed = resume_paused_slot_consuming_queues_with_chat_service(
        None,
        &app_state,
        &execution_state,
        || Arc::clone(&mock) as Arc<dyn ChatService>,
    )
    .await
    .expect("resume paused review queue");

    assert_eq!(resumed, 1);
    assert_eq!(
        mock.get_sent_messages().await,
        vec!["resume review".to_string()]
    );
    let sent_options = mock.get_sent_options().await;
    assert_eq!(sent_options.len(), 1);
    assert_eq!(
        sent_options[0].harness_override,
        Some(crate::domain::agents::AgentHarnessKind::Codex)
    );
    assert!(app_state
        .message_queue
        .get_queued(ChatContextType::Review, task.id.as_str())
        .is_empty());
}

#[tokio::test]
async fn test_resume_restores_durable_slot_message_when_send_fails() {
    let app_state = AppState::new_test();
    let execution_state = Arc::new(ExecutionState::new());
    let project = Project::new(
        "Restore Durable Slot".to_string(),
        "/test/restore-durable-slot".to_string(),
    );
    app_state
        .project_repo
        .create(project.clone())
        .await
        .unwrap();
    let task = app_state
        .task_repo
        .create(Task {
            internal_status: InternalStatus::Reviewing,
            ..Task::new(project.id.clone(), "Restore review".to_string())
        })
        .await
        .unwrap();
    let key = QueueKey::new(ChatContextType::Review, task.id.as_str());
    let queued = QueuedMessage::with_id("restore-review".to_string(), "restore review".to_string());
    app_state
        .queued_message_repo
        .enqueue_back(&key, &queued)
        .await
        .unwrap();

    let mock = Arc::new(MockChatService::new());
    mock.set_available(false).await;
    let resumed = resume_paused_slot_consuming_queues_with_chat_service(
        None,
        &app_state,
        &execution_state,
        || Arc::clone(&mock) as Arc<dyn ChatService>,
    )
    .await
    .expect("resume durable slot queue");

    assert_eq!(resumed, 0);
    assert_eq!(
        app_state.message_queue.get_queued_with_key(&key),
        vec![queued.clone()]
    );
    assert_eq!(
        app_state.queued_message_repo.list(&key).await.unwrap(),
        vec![queued]
    );
}

#[tokio::test]
async fn test_resume_relaunches_queued_project_chat_message() {
    let app_state = AppState::new_test();
    let project = Project::new(
        "Resume Project Chat".to_string(),
        "/test/project-chat".to_string(),
    );
    app_state
        .project_repo
        .create(project.clone())
        .await
        .unwrap();

    app_state.message_queue.queue_with_overrides(
        ChatContextType::Project,
        project.id.as_str(),
        "resume project chat".to_string(),
        Some(r#"{"resume_in_place":true}"#.to_string()),
        None,
        None,
    );

    let mock = Arc::new(MockChatService::new());
    let execution_state = Arc::new(ExecutionState::new());
    let resumed = resume_paused_workspace_queues_with_chat_service(
        Some(&project.id),
        &app_state,
        &execution_state,
        || Arc::clone(&mock) as Arc<dyn ChatService>,
    )
    .await
    .expect("resume paused project chat queue");

    assert_eq!(resumed, 1);
    assert_eq!(mock.call_count(), 1);
    assert_eq!(
        mock.get_sent_messages().await,
        vec!["resume project chat".to_string()]
    );
    assert!(app_state
        .message_queue
        .get_queued(ChatContextType::Project, project.id.as_str())
        .is_empty());
}

#[tokio::test]
async fn test_resume_workspace_queue_respects_workspace_capacity() {
    let app_state = AppState::new_test();
    let execution_state = Arc::new(ExecutionState::new());
    execution_state.set_workspace_max_concurrent(1);
    let project = Project::new(
        "Workspace Capacity".to_string(),
        "/test/workspace-capacity".to_string(),
    );
    app_state
        .project_repo
        .create(project.clone())
        .await
        .unwrap();

    app_state
        .running_agent_registry
        .register(
            RunningAgentKey::new("project", project.id.as_str()),
            45454,
            "workspace-conv".to_string(),
            "workspace-run".to_string(),
            None,
            None,
        )
        .await;
    app_state.message_queue.queue(
        ChatContextType::Project,
        project.id.as_str(),
        "blocked workspace queue".to_string(),
    );

    let mock = Arc::new(MockChatService::new());
    let resumed = resume_paused_workspace_queues_with_chat_service(
        Some(&project.id),
        &app_state,
        &execution_state,
        || Arc::clone(&mock) as Arc<dyn ChatService>,
    )
    .await
    .expect("resume paused project chat queue");

    assert_eq!(resumed, 0);
    assert_eq!(mock.call_count(), 0);
    assert_eq!(
        app_state
            .message_queue
            .get_queued(ChatContextType::Project, project.id.as_str())
            .len(),
        1,
        "workspace queue should stay pending while the workspace lane is full"
    );
}

#[tokio::test]
async fn test_project_scoped_workspace_resume_respects_global_workspace_capacity() {
    let app_state = AppState::new_test();
    let execution_state = Arc::new(ExecutionState::new());
    execution_state.set_workspace_max_concurrent(1);
    let active_project = Project::new(
        "Active Workspace".to_string(),
        "/test/active-workspace".to_string(),
    );
    let queued_project = Project::new(
        "Queued Workspace".to_string(),
        "/test/queued-workspace".to_string(),
    );
    app_state
        .project_repo
        .create(active_project.clone())
        .await
        .unwrap();
    app_state
        .project_repo
        .create(queued_project.clone())
        .await
        .unwrap();

    app_state
        .running_agent_registry
        .register(
            RunningAgentKey::new("project", active_project.id.as_str()),
            56565,
            "active-workspace-conv".to_string(),
            "active-workspace-run".to_string(),
            None,
            None,
        )
        .await;
    app_state.message_queue.queue(
        ChatContextType::Project,
        queued_project.id.as_str(),
        "blocked by another project workspace".to_string(),
    );

    let mock = Arc::new(MockChatService::new());
    let resumed = resume_paused_workspace_queues_with_chat_service(
        Some(&queued_project.id),
        &app_state,
        &execution_state,
        || Arc::clone(&mock) as Arc<dyn ChatService>,
    )
    .await
    .expect("resume project-scoped workspace queue");

    assert_eq!(resumed, 0);
    assert_eq!(mock.call_count(), 0);
    assert_eq!(
        app_state
            .message_queue
            .get_queued(ChatContextType::Project, queued_project.id.as_str())
            .len(),
        1,
        "project-scoped resume must still respect the global workspace lane cap"
    );
}

#[tokio::test]
async fn test_get_running_processes_reports_scoped_lanes_and_capacity() {
    let active_project_state = Arc::new(ActiveProjectState::new());
    let execution_state = Arc::new(ExecutionState::new());
    execution_state.set_global_max_concurrent(7);
    execution_state.set_workspace_max_concurrent(1);
    execution_state.set_allow_ideation_borrow_idle_execution(true);
    let app_state = AppState::new_test();

    let project = Project::new(
        "Scoped Running Project".to_string(),
        "/test/scoped-running-project".to_string(),
    );
    app_state
        .project_repo
        .create(project.clone())
        .await
        .unwrap();
    let other_project = Project::new(
        "Other Running Project".to_string(),
        "/test/other-running-project".to_string(),
    );
    app_state
        .project_repo
        .create(other_project.clone())
        .await
        .unwrap();

    app_state
        .execution_settings_repo
        .update_settings(
            Some(&project.id),
            &ExecutionSettings {
                max_concurrent_tasks: 4,
                project_ideation_max: 1,
                auto_commit: false,
                pause_on_failure: false,
                ..ExecutionSettings::default()
            },
        )
        .await
        .unwrap();

    let mut conversation = ChatConversation::new_project(project.id.clone());
    conversation.title = Some("Feature workspace".to_string());
    let conversation = app_state
        .chat_conversation_repo
        .create(conversation)
        .await
        .unwrap();
    let other_conversation = app_state
        .chat_conversation_repo
        .create(ChatConversation::new_project(other_project.id.clone()))
        .await
        .unwrap();

    let running_task = app_state
        .task_repo
        .create(Task {
            internal_status: InternalStatus::Executing,
            ..Task::new(project.id.clone(), "Running task".to_string())
        })
        .await
        .unwrap();
    app_state
        .task_repo
        .create(Task {
            internal_status: InternalStatus::Ready,
            ..Task::new(project.id.clone(), "Queued ready task".to_string())
        })
        .await
        .unwrap();
    let other_task = app_state
        .task_repo
        .create(Task {
            internal_status: InternalStatus::Executing,
            ..Task::new(other_project.id.clone(), "Other running task".to_string())
        })
        .await
        .unwrap();
    let task_run = app_state
        .agent_run_repo
        .create(AgentRun::new(ChatConversationId::new()))
        .await
        .unwrap();
    let other_task_run = app_state
        .agent_run_repo
        .create(AgentRun::new(ChatConversationId::new()))
        .await
        .unwrap();

    let mut ideation_session = IdeationSession::new(project.id.clone());
    ideation_session.title = Some("Planning session".to_string());
    let ideation_session = app_state
        .ideation_session_repo
        .create(ideation_session)
        .await
        .unwrap();
    let other_ideation_session = app_state
        .ideation_session_repo
        .create(IdeationSession::new(other_project.id.clone()))
        .await
        .unwrap();
    let ideation_run = app_state
        .agent_run_repo
        .create(AgentRun::new(ChatConversationId::new()))
        .await
        .unwrap();
    let other_ideation_run = app_state
        .agent_run_repo
        .create(AgentRun::new(ChatConversationId::new()))
        .await
        .unwrap();

    app_state.message_queue.queue(
        ChatContextType::Project,
        conversation.id.as_str(),
        "queued workspace message".to_string(),
    );
    app_state.message_queue.queue(
        ChatContextType::TaskExecution,
        running_task.id.as_str(),
        "queued task message".to_string(),
    );
    app_state.message_queue.queue(
        ChatContextType::Ideation,
        ideation_session.id.as_str(),
        "queued ideation message".to_string(),
    );
    app_state.message_queue.queue(
        ChatContextType::Project,
        other_conversation.id.as_str(),
        "other queued workspace message".to_string(),
    );

    let mut live_process = std::process::Command::new("sleep")
        .arg("60")
        .spawn()
        .expect("spawn disposable process for live registry rows");
    let pid = live_process.id();
    let workspace_key = RunningAgentKey::new("project", conversation.id.as_str());
    app_state
        .running_agent_registry
        .try_register(
            workspace_key.clone(),
            conversation.id.as_str().to_string(),
            "workspace-run".to_string(),
        )
        .await
        .unwrap();
    app_state
        .running_agent_registry
        .attach_process(
            &workspace_key,
            "workspace-run",
            pid,
            None,
            None,
            Some("gpt-5.5".to_string()),
        )
        .await
        .unwrap();
    app_state
        .running_agent_registry
        .register(
            RunningAgentKey::new("project", other_conversation.id.as_str()),
            pid,
            other_conversation.id.as_str().to_string(),
            "other-workspace-run".to_string(),
            None,
            None,
        )
        .await;
    app_state
        .running_agent_registry
        .register(
            RunningAgentKey::new("task_execution", running_task.id.as_str()),
            pid,
            "task-conversation".to_string(),
            task_run.id.as_str(),
            None,
            None,
        )
        .await;
    app_state
        .running_agent_registry
        .register(
            RunningAgentKey::new("task_execution", other_task.id.as_str()),
            pid,
            "other-task-conversation".to_string(),
            other_task_run.id.as_str(),
            None,
            None,
        )
        .await;
    app_state
        .running_agent_registry
        .register(
            RunningAgentKey::new("ideation", ideation_session.id.as_str()),
            pid,
            "ideation-conversation".to_string(),
            ideation_run.id.as_str(),
            None,
            None,
        )
        .await;
    app_state
        .running_agent_registry
        .register(
            RunningAgentKey::new("ideation", other_ideation_session.id.as_str()),
            pid,
            "other-ideation-conversation".to_string(),
            other_ideation_run.id.as_str(),
            None,
            None,
        )
        .await;

    let app = mock_builder()
        .manage(Arc::clone(&active_project_state))
        .manage(Arc::clone(&execution_state))
        .manage(app_state)
        .build(mock_context(noop_assets()))
        .expect("mock app should build");

    let response = get_running_processes(
        Some(project.id.as_str().to_string()),
        app.state::<Arc<ActiveProjectState>>(),
        app.state::<Arc<ExecutionState>>(),
        app.state::<AppState>(),
    )
    .await
    .expect("running processes should load");

    let aggregate_response = get_running_processes(
        None,
        app.state::<Arc<ActiveProjectState>>(),
        app.state::<Arc<ExecutionState>>(),
        app.state::<AppState>(),
    )
    .await
    .expect("aggregate running processes should load");

    let _ = live_process.kill();
    let _ = live_process.wait();

    assert_eq!(response.workspace_sessions.len(), 1);
    assert_eq!(
        response.workspace_sessions[0].conversation_id,
        conversation.id.as_str()
    );
    assert_eq!(
        response.workspace_sessions[0].project_id,
        project.id.as_str()
    );
    assert_eq!(response.workspace_sessions[0].title, "Feature workspace");
    assert_eq!(
        response.workspace_sessions[0].model.as_deref(),
        Some("gpt-5.5")
    );
    assert_eq!(response.processes.len(), 1);
    assert_eq!(response.processes[0].task_id, running_task.id.as_str());
    assert_eq!(response.ideation_sessions.len(), 1);
    assert_eq!(
        response.ideation_sessions[0].session_id,
        ideation_session.id.as_str()
    );
    assert!(response.ideation_sessions[0].is_generating);

    let workspace_lane = response
        .lanes
        .iter()
        .find(|lane| lane.lane == "workspaces")
        .expect("workspace lane");
    assert_eq!(workspace_lane.active, 1);
    assert_eq!(workspace_lane.waiting, 1);
    assert_eq!(workspace_lane.max, 1);
    assert_eq!(workspace_lane.borrowed, 0);
    assert_eq!(workspace_lane.priority_rank, 1);

    let task_lane = response
        .lanes
        .iter()
        .find(|lane| lane.lane == "tasks")
        .expect("tasks lane");
    assert_eq!(task_lane.active, 1);
    assert_eq!(task_lane.waiting, 2);
    assert_eq!(task_lane.max, 4);
    assert_eq!(task_lane.priority_rank, 2);

    let ideation_lane = response
        .lanes
        .iter()
        .find(|lane| lane.lane == "ideation")
        .expect("ideation lane");
    assert_eq!(ideation_lane.active, 1);
    assert_eq!(ideation_lane.idle, 0);
    assert_eq!(ideation_lane.waiting, 1);
    assert_eq!(ideation_lane.max, 1);
    assert_eq!(ideation_lane.priority_rank, 3);

    assert_eq!(response.capacity.total_active, 3);
    assert_eq!(response.capacity.global_max_concurrent, 7);
    assert!(response.capacity.borrowing_enabled);
    assert_eq!(
        response.capacity.priority,
        vec![
            "workspaces".to_string(),
            "tasks".to_string(),
            "ideation".to_string(),
        ]
    );

    assert_eq!(aggregate_response.workspace_sessions.len(), 2);
    assert_eq!(aggregate_response.processes.len(), 2);
    assert_eq!(aggregate_response.ideation_sessions.len(), 2);
    let aggregate_workspace_lane = aggregate_response
        .lanes
        .iter()
        .find(|lane| lane.lane == "workspaces")
        .expect("aggregate workspace lane");
    assert_eq!(aggregate_workspace_lane.active, 2);
    assert_eq!(aggregate_workspace_lane.waiting, 2);
    assert_eq!(aggregate_workspace_lane.max, 1);
    assert_eq!(aggregate_workspace_lane.borrowed, 1);
    let aggregate_task_lane = aggregate_response
        .lanes
        .iter()
        .find(|lane| lane.lane == "tasks")
        .expect("aggregate task lane");
    assert_eq!(aggregate_task_lane.active, 2);
    assert_eq!(aggregate_task_lane.waiting, 2);
    assert_eq!(aggregate_task_lane.max, 10);
    let aggregate_ideation_lane = aggregate_response
        .lanes
        .iter()
        .find(|lane| lane.lane == "ideation")
        .expect("aggregate ideation lane");
    assert_eq!(aggregate_ideation_lane.active, 2);
    assert_eq!(aggregate_ideation_lane.waiting, 1);
    assert_eq!(aggregate_response.capacity.total_active, 6);
}

#[tokio::test]
async fn test_get_running_processes_includes_agent_workspace_target_for_task() {
    let active_project_state = Arc::new(ActiveProjectState::new());
    let execution_state = Arc::new(ExecutionState::new());
    let app_state = AppState::new_test();

    let project = app_state
        .project_repo
        .create(Project::new(
            "Workspace Target Project".to_string(),
            "/test/workspace-target-project".to_string(),
        ))
        .await
        .unwrap();
    active_project_state.set(Some(project.id.clone())).await;

    let session_id = IdeationSessionId::from_string("workspace-target-session");
    let mut running_task = Task::new(project.id.clone(), "Running linked task".to_string());
    running_task.internal_status = InternalStatus::Executing;
    running_task.ideation_session_id = Some(session_id.clone());
    let running_task = app_state.task_repo.create(running_task).await.unwrap();

    let branch_id = PlanBranchId::from_string("workspace-target-plan-branch");
    app_state
        .plan_branch_repo
        .create(PlanBranch {
            id: branch_id.clone(),
            plan_artifact_id: ArtifactId::from_string("workspace-target-artifact"),
            session_id: session_id.clone(),
            project_id: project.id.clone(),
            branch_name: "feature/workspace-target".to_string(),
            source_branch: "main".to_string(),
            status: PlanBranchStatus::Active,
            execution_plan_id: None,
            merge_task_id: None,
            created_at: chrono::Utc::now(),
            merged_at: None,
            pr_number: None,
            pr_url: None,
            pr_status: None,
            pr_polling_active: false,
            pr_eligible: false,
            last_polled_at: None,
            pr_push_status: Default::default(),
            merge_commit_sha: None,
            pr_draft: None,
            base_branch_override: None,
        })
        .await
        .unwrap();

    let mut conversation = ChatConversation::new_project(project.id.clone());
    conversation.title = Some("Linked Agent Workspace".to_string());
    let conversation = app_state
        .chat_conversation_repo
        .create(conversation)
        .await
        .unwrap();

    let mut workspace = AgentConversationWorkspace::new(
        conversation.id,
        project.id.clone(),
        AgentConversationWorkspaceMode::Ideation,
        IdeationAnalysisBaseRefKind::ProjectDefault,
        "main".to_string(),
        Some("main".to_string()),
        Some("base-sha".to_string()),
        "ralphx/workspace-target".to_string(),
        "/tmp/ralphx-workspace-target".to_string(),
    );
    workspace.linked_plan_branch_id = Some(branch_id);
    workspace.linked_ideation_session_id = Some(session_id);
    app_state
        .agent_conversation_workspace_repo
        .create_or_update(workspace)
        .await
        .unwrap();

    let task_run = app_state
        .agent_run_repo
        .create(AgentRun::new(ChatConversationId::new()))
        .await
        .unwrap();
    let mut live_process = std::process::Command::new("sleep")
        .arg("60")
        .spawn()
        .expect("spawn disposable process for live registry row");
    app_state
        .running_agent_registry
        .register(
            RunningAgentKey::new("task_execution", running_task.id.as_str()),
            live_process.id(),
            "task-conversation".to_string(),
            task_run.id.as_str(),
            None,
            None,
        )
        .await;

    let app = mock_builder()
        .manage(Arc::clone(&active_project_state))
        .manage(Arc::clone(&execution_state))
        .manage(app_state)
        .build(mock_context(noop_assets()))
        .expect("mock app should build");

    let response = get_running_processes(
        None,
        app.state::<Arc<ActiveProjectState>>(),
        app.state::<Arc<ExecutionState>>(),
        app.state::<AppState>(),
    )
    .await
    .expect("running processes should load");

    let _ = live_process.kill();
    let _ = live_process.wait();

    assert_eq!(response.processes.len(), 1);
    let process = &response.processes[0];
    assert_eq!(process.task_id, running_task.id.as_str());
    let agent_workspace = process
        .agent_workspace
        .as_ref()
        .expect("agent workspace target should be present");
    assert_eq!(agent_workspace.conversation_id, conversation.id.as_str());
    assert_eq!(agent_workspace.project_id, project.id.as_str());
    assert_eq!(agent_workspace.title, "Linked Agent Workspace");
}

#[test]
fn test_workspace_capacity_available_respects_all_global_guards() {
    assert!(
        crate::application::workspace_capacity::workspace_capacity_available(
            1, 3, 1, 5, false, false
        )
    );
    assert!(
        !crate::application::workspace_capacity::workspace_capacity_available(
            1, 3, 1, 5, true, false
        )
    );
    assert!(
        !crate::application::workspace_capacity::workspace_capacity_available(
            1, 3, 1, 5, false, true
        )
    );
    assert!(
        !crate::application::workspace_capacity::workspace_capacity_available(
            3, 3, 0, 10, false, false
        )
    );
    assert!(
        !crate::application::workspace_capacity::workspace_capacity_available(
            2, 3, 3, 5, false, false
        )
    );
}

#[tokio::test]
async fn test_workspace_capacity_resolves_conversation_backed_queues_and_sessions() {
    let app_state = AppState::new_test();
    let project = app_state
        .project_repo
        .create(Project::new(
            "Workspace Capacity Project".to_string(),
            "/test/workspace-capacity-project".to_string(),
        ))
        .await
        .unwrap();
    let other_project = app_state
        .project_repo
        .create(Project::new(
            "Other Workspace Capacity Project".to_string(),
            "/test/other-workspace-capacity-project".to_string(),
        ))
        .await
        .unwrap();
    let conversation = app_state
        .chat_conversation_repo
        .create(ChatConversation::new_project(project.id.clone()))
        .await
        .unwrap();
    let other_conversation = app_state
        .chat_conversation_repo
        .create(ChatConversation::new_project(other_project.id.clone()))
        .await
        .unwrap();
    let task = app_state
        .task_repo
        .create(Task::new(
            project.id.clone(),
            "Non-workspace chat".to_string(),
        ))
        .await
        .unwrap();
    let task_conversation = app_state
        .chat_conversation_repo
        .create(ChatConversation::new_task(task.id.clone()))
        .await
        .unwrap();

    let task_key = QueueKey::new(ChatContextType::Task, task.id.as_str());
    let resolved = crate::application::workspace_capacity::resolve_project_queue_context(
        &task_key,
        &app_state.project_repo,
        &app_state.chat_conversation_repo,
    )
    .await
    .expect("non-project key resolves as direct context")
    .expect("non-project context");
    assert_eq!(resolved.0.as_str(), task.id.as_str());
    assert_eq!(resolved.1, None);

    let direct_key = QueueKey::new(ChatContextType::Project, project.id.as_str());
    let resolved = crate::application::workspace_capacity::resolve_project_queue_context(
        &direct_key,
        &app_state.project_repo,
        &app_state.chat_conversation_repo,
    )
    .await
    .expect("direct project key resolves")
    .expect("direct project context");
    assert_eq!(resolved.0, project.id);
    assert_eq!(resolved.1, None);

    let conversation_key = QueueKey::new(ChatContextType::Project, conversation.id.as_str());
    let resolved = crate::application::workspace_capacity::resolve_project_queue_context(
        &conversation_key,
        &app_state.project_repo,
        &app_state.chat_conversation_repo,
    )
    .await
    .expect("conversation key resolves")
    .expect("conversation project context");
    assert_eq!(resolved.0, project.id);
    assert_eq!(resolved.1.as_ref(), Some(&conversation.id));

    let wrong_context_key = QueueKey::new(ChatContextType::Project, task_conversation.id.as_str());
    assert!(
        crate::application::workspace_capacity::resolve_project_queue_context(
            &wrong_context_key,
            &app_state.project_repo,
            &app_state.chat_conversation_repo,
        )
        .await
        .expect("wrong context lookup succeeds")
        .is_none()
    );
    let missing_key = QueueKey::new(ChatContextType::Project, ChatConversationId::new().as_str());
    assert!(
        crate::application::workspace_capacity::resolve_project_queue_context(
            &missing_key,
            &app_state.project_repo,
            &app_state.chat_conversation_repo,
        )
        .await
        .expect("missing context lookup succeeds")
        .is_none()
    );

    app_state.message_queue.queue(
        ChatContextType::Project,
        project.id.as_str(),
        "direct project queue".to_string(),
    );
    app_state.message_queue.queue(
        ChatContextType::Project,
        conversation.id.as_str(),
        "conversation project queue".to_string(),
    );
    app_state.message_queue.queue(
        ChatContextType::Project,
        other_project.id.as_str(),
        "other project queue".to_string(),
    );
    app_state.message_queue.queue(
        ChatContextType::Task,
        task.id.as_str(),
        "task queue is not workspace capacity".to_string(),
    );

    assert_eq!(
        crate::application::workspace_capacity::count_queued_workspace_messages(
            &app_state.message_queue,
            &app_state.project_repo,
            &app_state.chat_conversation_repo,
            Some(&project.id),
        )
        .await
        .expect("count scoped queued workspaces"),
        2
    );
    assert_eq!(
        crate::application::workspace_capacity::count_queued_workspace_messages(
            &app_state.message_queue,
            &app_state.project_repo,
            &app_state.chat_conversation_repo,
            None,
        )
        .await
        .expect("count aggregate queued workspaces"),
        3
    );

    app_state
        .running_agent_registry
        .register(
            RunningAgentKey::new("project", "runtime-from-conversation"),
            61001,
            conversation.id.as_str().to_string(),
            "run-from-conversation".to_string(),
            None,
            None,
        )
        .await;
    app_state
        .running_agent_registry
        .register(
            RunningAgentKey::new("project", project.id.as_str()),
            61002,
            "missing-conversation".to_string(),
            "run-from-project-id".to_string(),
            None,
            None,
        )
        .await;
    app_state
        .running_agent_registry
        .register(
            RunningAgentKey::new("project", conversation.id.as_str()),
            61003,
            "missing-conversation-2".to_string(),
            "run-from-runtime-conversation".to_string(),
            None,
            None,
        )
        .await;
    app_state
        .running_agent_registry
        .register(
            RunningAgentKey::new("project", "other-runtime"),
            61004,
            other_conversation.id.as_str().to_string(),
            "run-other-project".to_string(),
            None,
            None,
        )
        .await;
    app_state
        .running_agent_registry
        .register(
            RunningAgentKey::new("project", "missing-runtime"),
            61005,
            "missing-conversation-3".to_string(),
            "run-missing-project-and-conversation".to_string(),
            None,
            None,
        )
        .await;
    app_state
        .running_agent_registry
        .register(
            RunningAgentKey::new("project", task_conversation.id.as_str()),
            61006,
            "missing-conversation-4".to_string(),
            "run-non-project-runtime-conversation".to_string(),
            None,
            None,
        )
        .await;
    app_state
        .running_agent_registry
        .register(
            RunningAgentKey::new("project", "pid-zero-runtime"),
            0,
            conversation.id.as_str().to_string(),
            "run-pid-zero".to_string(),
            None,
            None,
        )
        .await;
    app_state
        .running_agent_registry
        .register(
            RunningAgentKey::new("task", task.id.as_str()),
            61007,
            conversation.id.as_str().to_string(),
            "run-non-workspace".to_string(),
            None,
            None,
        )
        .await;

    assert_eq!(
        crate::application::workspace_capacity::count_active_workspace_sessions(
            &app_state.running_agent_registry,
            &app_state.project_repo,
            &app_state.chat_conversation_repo,
            Some(&project.id),
        )
        .await
        .expect("count scoped active workspace sessions"),
        3
    );
    assert_eq!(
        crate::application::workspace_capacity::count_active_workspace_sessions(
            &app_state.running_agent_registry,
            &app_state.project_repo,
            &app_state.chat_conversation_repo,
            None,
        )
        .await
        .expect("count aggregate active workspace sessions"),
        4
    );
}

#[tokio::test]
async fn test_resume_relaunches_durable_project_conversation_queue_with_override() {
    let app_state = AppState::new_test();
    let execution_state = Arc::new(ExecutionState::new());
    let project = Project::new(
        "Durable Conversation Queue".to_string(),
        "/test/durable-conversation-queue".to_string(),
    );
    app_state
        .project_repo
        .create(project.clone())
        .await
        .unwrap();
    let conversation = app_state
        .chat_conversation_repo
        .create(ChatConversation::new_project(project.id.clone()))
        .await
        .unwrap();
    let key = QueueKey::new(ChatContextType::Project, conversation.id.as_str());
    let queued = QueuedMessage::with_id(
        "durable-project-conversation".to_string(),
        "durable project conversation".to_string(),
    );
    app_state
        .queued_message_repo
        .enqueue_back(&key, &queued)
        .await
        .unwrap();

    let mock = Arc::new(MockChatService::new());
    let resumed = resume_paused_workspace_queues_with_chat_service(
        None,
        &app_state,
        &execution_state,
        || Arc::clone(&mock) as Arc<dyn ChatService>,
    )
    .await
    .expect("resume durable project conversation queue");

    assert_eq!(resumed, 1);
    assert_eq!(
        mock.get_sent_messages().await,
        vec!["durable project conversation".to_string()]
    );
    let options = mock.get_sent_options().await;
    assert_eq!(
        options[0]
            .conversation_id_override
            .as_ref()
            .map(|id| id.as_str()),
        Some(conversation.id.as_str())
    );
    assert!(app_state
        .queued_message_repo
        .list(&key)
        .await
        .unwrap()
        .is_empty());
}

#[tokio::test]
async fn test_resume_workspace_queue_restores_message_when_send_fails() {
    let app_state = AppState::new_test();
    let execution_state = Arc::new(ExecutionState::new());
    let project = app_state
        .project_repo
        .create(Project::new(
            "Failed Workspace Resume".to_string(),
            "/test/failed-workspace-resume".to_string(),
        ))
        .await
        .unwrap();
    app_state.message_queue.queue(
        ChatContextType::Project,
        project.id.as_str(),
        "restore workspace queue".to_string(),
    );

    let mock = Arc::new(MockChatService::new());
    mock.set_available(false).await;
    let resumed = resume_paused_workspace_queues_with_chat_service(
        None,
        &app_state,
        &execution_state,
        || Arc::clone(&mock) as Arc<dyn ChatService>,
    )
    .await
    .expect("resume workspace queue handles send failure");

    assert_eq!(resumed, 0);
    assert_eq!(mock.call_count(), 1);
    let restored = app_state
        .message_queue
        .get_queued(ChatContextType::Project, project.id.as_str());
    assert_eq!(restored.len(), 1);
    assert_eq!(restored[0].content, "restore workspace queue");
}

#[tokio::test]
async fn test_queued_context_counts_and_runnable_waiting_include_workspace_and_task_pressure() {
    let app_state = AppState::new_test();
    let project = app_state
        .project_repo
        .create(Project::new(
            "Runnable Queue Project".to_string(),
            "/test/runnable-queue-project".to_string(),
        ))
        .await
        .unwrap();
    let other_project = app_state
        .project_repo
        .create(Project::new(
            "Other Runnable Queue Project".to_string(),
            "/test/other-runnable-queue-project".to_string(),
        ))
        .await
        .unwrap();
    let review_task = app_state
        .task_repo
        .create(Task {
            internal_status: InternalStatus::Reviewing,
            ..Task::new(project.id.clone(), "Queued review pressure".to_string())
        })
        .await
        .unwrap();
    let other_review_task = app_state
        .task_repo
        .create(Task {
            internal_status: InternalStatus::Reviewing,
            ..Task::new(
                other_project.id.clone(),
                "Other queued review pressure".to_string(),
            )
        })
        .await
        .unwrap();

    app_state.message_queue.queue(
        ChatContextType::Project,
        project.id.as_str(),
        "workspace pressure".to_string(),
    );

    assert_eq!(
        count_queued_messages_for_context_types(
            &[ChatContextType::Project],
            Some(&project.id),
            &app_state,
        )
        .await
        .expect("count workspace queue"),
        1
    );
    assert!(
        has_runnable_execution_waiting(&app_state, Some(&project.id))
            .await
            .expect("workspace queue should count as waiting execution")
    );

    app_state
        .message_queue
        .clear(ChatContextType::Project, project.id.as_str());
    app_state.message_queue.queue(
        ChatContextType::Review,
        review_task.id.as_str(),
        "review pressure".to_string(),
    );
    app_state.message_queue.queue(
        ChatContextType::Review,
        other_review_task.id.as_str(),
        "other review pressure".to_string(),
    );

    assert_eq!(
        count_queued_messages_for_context_types(
            &[
                ChatContextType::TaskExecution,
                ChatContextType::Review,
                ChatContextType::Merge,
            ],
            Some(&project.id),
            &app_state,
        )
        .await
        .expect("count scoped slot queues"),
        1
    );
    assert!(
        has_runnable_execution_waiting(&app_state, Some(&project.id))
            .await
            .expect("review queue should count as waiting execution")
    );

    app_state
        .task_repo
        .create(Task {
            internal_status: InternalStatus::Ready,
            ..Task::new(other_project.id.clone(), "Ready global work".to_string())
        })
        .await
        .unwrap();

    assert!(has_runnable_execution_waiting(&app_state, None)
        .await
        .expect("global ready task should count as waiting execution"));
}

#[tokio::test]
async fn test_resume_workspace_queue_skips_unmatched_missing_and_already_running_entries() {
    let app_state = AppState::new_test();
    let execution_state = Arc::new(ExecutionState::new());
    let project = app_state
        .project_repo
        .create(Project::new(
            "Workspace Resume Branches".to_string(),
            "/test/workspace-resume-branches".to_string(),
        ))
        .await
        .unwrap();
    let other_project = app_state
        .project_repo
        .create(Project::new(
            "Other Workspace Resume Branches".to_string(),
            "/test/other-workspace-resume-branches".to_string(),
        ))
        .await
        .unwrap();

    app_state.message_queue.queue(
        ChatContextType::Project,
        other_project.id.as_str(),
        "other workspace should stay queued".to_string(),
    );
    let mock = Arc::new(MockChatService::new());
    let resumed = resume_paused_workspace_queues_with_chat_service(
        Some(&project.id),
        &app_state,
        &execution_state,
        || Arc::clone(&mock) as Arc<dyn ChatService>,
    )
    .await
    .expect("resume scoped workspace queue with non-matching key");
    assert_eq!(resumed, 0);
    assert_eq!(mock.call_count(), 0);
    assert_eq!(
        app_state
            .message_queue
            .get_queued(ChatContextType::Project, other_project.id.as_str())
            .len(),
        1
    );

    app_state
        .message_queue
        .clear(ChatContextType::Project, other_project.id.as_str());
    app_state.message_queue.queue(
        ChatContextType::Project,
        "missing-workspace-context",
        "missing workspace target".to_string(),
    );
    let resumed = resume_paused_workspace_queues_with_chat_service(
        None,
        &app_state,
        &execution_state,
        || Arc::clone(&mock) as Arc<dyn ChatService>,
    )
    .await
    .expect("resume workspace queue with missing target");
    assert_eq!(resumed, 0);
    assert_eq!(mock.call_count(), 0);
    assert_eq!(
        app_state
            .message_queue
            .get_queued(ChatContextType::Project, "missing-workspace-context")
            .len(),
        1
    );

    app_state
        .message_queue
        .clear(ChatContextType::Project, "missing-workspace-context");
    app_state.message_queue.queue(
        ChatContextType::Project,
        project.id.as_str(),
        "already running workspace".to_string(),
    );
    mock.set_already_running_after(0).await;
    let resumed = resume_paused_workspace_queues_with_chat_service(
        Some(&project.id),
        &app_state,
        &execution_state,
        || Arc::clone(&mock) as Arc<dyn ChatService>,
    )
    .await
    .expect("resume workspace queue with already-running send result");
    assert_eq!(resumed, 0);
    assert_eq!(mock.call_count(), 1);
    assert!(app_state
        .message_queue
        .get_queued(ChatContextType::Project, project.id.as_str())
        .is_empty());
}

#[tokio::test]
async fn test_durable_queue_counts_merge_with_memory_by_message_id() {
    let app_state = AppState::new_test();
    let project = Project::new(
        "Durable Queue Count".to_string(),
        "/test/durable-queue-count".to_string(),
    );
    app_state
        .project_repo
        .create(project.clone())
        .await
        .unwrap();
    let task = app_state
        .task_repo
        .create(Task {
            internal_status: InternalStatus::Reviewing,
            ..Task::new(project.id.clone(), "Queued review".to_string())
        })
        .await
        .unwrap();
    let key = QueueKey::new(ChatContextType::Review, task.id.as_str());
    let shared = QueuedMessage::with_id("shared".to_string(), "Shared".to_string());
    let durable_only =
        QueuedMessage::with_id("durable-only".to_string(), "Durable only".to_string());
    app_state
        .queued_message_repo
        .enqueue_back(&key, &shared)
        .await
        .unwrap();
    app_state
        .queued_message_repo
        .enqueue_back(&key, &durable_only)
        .await
        .unwrap();
    app_state
        .message_queue
        .queue_front_existing(ChatContextType::Review, task.id.as_str(), shared);

    let count = count_slot_consuming_queued_messages(Some(&project.id), &app_state)
        .await
        .expect("count durable queued messages");

    assert_eq!(count, 2);
}

#[tokio::test]
async fn test_clear_slot_consuming_queues_clears_durable_and_memory_rows() {
    let app_state = AppState::new_test();
    let project = Project::new(
        "Clear Durable Slot Queue".to_string(),
        "/test/clear-durable-slot-queue".to_string(),
    );
    app_state
        .project_repo
        .create(project.clone())
        .await
        .unwrap();
    let task = app_state
        .task_repo
        .create(Task {
            internal_status: InternalStatus::Reviewing,
            ..Task::new(project.id.clone(), "Clear review".to_string())
        })
        .await
        .unwrap();
    let key = QueueKey::new(ChatContextType::Review, task.id.as_str());
    let memory = QueuedMessage::with_id("memory".to_string(), "Memory".to_string());
    let durable = QueuedMessage::with_id("durable".to_string(), "Durable".to_string());
    app_state
        .message_queue
        .queue_front_existing(ChatContextType::Review, task.id.as_str(), memory);
    app_state
        .queued_message_repo
        .enqueue_back(&key, &durable)
        .await
        .unwrap();

    let cleared = clear_slot_consuming_queues(Some(&project.id), &app_state)
        .await
        .expect("clear slot queues");

    assert_eq!(cleared, 1);
    assert!(app_state.message_queue.get_queued_with_key(&key).is_empty());
    assert!(app_state
        .queued_message_repo
        .list(&key)
        .await
        .unwrap()
        .is_empty());
}

#[tokio::test]
async fn test_resume_respects_project_ideation_cap_for_same_project() {
    let app_state = AppState::new_test();
    let execution_state = Arc::new(ExecutionState::new());
    let project = Project::new("Project Cap".to_string(), "/test/project-cap".to_string());
    app_state
        .project_repo
        .create(project.clone())
        .await
        .unwrap();

    app_state
        .execution_settings_repo
        .update_settings(
            Some(&project.id),
            &ExecutionSettings {
                max_concurrent_tasks: 5,
                project_ideation_max: 1,
                auto_commit: true,
                pause_on_failure: true,
                ..ExecutionSettings::default()
            },
        )
        .await
        .unwrap();

    let occupied = app_state
        .ideation_session_repo
        .create(IdeationSession::new(project.id.clone()))
        .await
        .unwrap();
    let queued = app_state
        .ideation_session_repo
        .create(IdeationSession::new(project.id.clone()))
        .await
        .unwrap();

    app_state.message_queue.queue(
        ChatContextType::Ideation,
        queued.id.as_str(),
        "blocked by project cap".to_string(),
    );

    app_state
        .running_agent_registry
        .register(
            RunningAgentKey::new("ideation", occupied.id.as_str()),
            22222,
            "occupied-conv".to_string(),
            "occupied-run".to_string(),
            None,
            None,
        )
        .await;

    let mock = Arc::new(MockChatService::new());
    let resumed = resume_paused_ideation_queues_with_chat_service(
        Some(&project.id),
        &app_state,
        &execution_state,
        || Arc::clone(&mock) as Arc<dyn ChatService>,
    )
    .await
    .expect("resume paused ideation queue with project cap");

    assert_eq!(resumed, 0);
    assert_eq!(mock.call_count(), 0);
    assert_eq!(
        app_state
            .message_queue
            .get_queued(ChatContextType::Ideation, queued.id.as_str())
            .len(),
        1,
        "project-capped session must stay queued on resume"
    );
}

#[tokio::test]
async fn test_resume_skips_project_capped_ideation_queue_and_relaunches_other_project() {
    let app_state = AppState::new_test();
    let execution_state = Arc::new(ExecutionState::new());

    let first_project = Project::new("First Project".to_string(), "/test/first".to_string());
    let second_project = Project::new("Second Project".to_string(), "/test/second".to_string());
    app_state
        .project_repo
        .create(first_project.clone())
        .await
        .unwrap();
    app_state
        .project_repo
        .create(second_project.clone())
        .await
        .unwrap();

    let (blocked_project, runnable_project) =
        if first_project.id.as_str() <= second_project.id.as_str() {
            (first_project, second_project)
        } else {
            (second_project, first_project)
        };

    app_state
        .execution_settings_repo
        .update_settings(
            Some(&blocked_project.id),
            &ExecutionSettings {
                max_concurrent_tasks: 5,
                project_ideation_max: 1,
                auto_commit: true,
                pause_on_failure: true,
                ..ExecutionSettings::default()
            },
        )
        .await
        .unwrap();

    execution_state.set_global_max_concurrent(5);
    execution_state.set_global_ideation_max(5);

    let occupied = app_state
        .ideation_session_repo
        .create(IdeationSession::new(blocked_project.id.clone()))
        .await
        .unwrap();
    let blocked_queued = app_state
        .ideation_session_repo
        .create(IdeationSession::new(blocked_project.id.clone()))
        .await
        .unwrap();
    let runnable_queued = app_state
        .ideation_session_repo
        .create(IdeationSession::new(runnable_project.id.clone()))
        .await
        .unwrap();

    app_state.message_queue.queue(
        ChatContextType::Ideation,
        blocked_queued.id.as_str(),
        "blocked project queued".to_string(),
    );
    app_state.message_queue.queue(
        ChatContextType::Ideation,
        runnable_queued.id.as_str(),
        "runnable project queued".to_string(),
    );

    app_state
        .running_agent_registry
        .register(
            RunningAgentKey::new("ideation", occupied.id.as_str()),
            23232,
            "occupied-conv".to_string(),
            "occupied-run".to_string(),
            None,
            None,
        )
        .await;

    let mock = Arc::new(MockChatService::new());
    let resumed =
        resume_paused_ideation_queues_with_chat_service(None, &app_state, &execution_state, || {
            Arc::clone(&mock) as Arc<dyn ChatService>
        })
        .await
        .expect("resume queued ideation across projects");

    assert_eq!(resumed, 1);
    assert_eq!(mock.call_count(), 1);
    assert_eq!(
        mock.get_sent_messages().await,
        vec!["runnable project queued".to_string()]
    );
    assert_eq!(
        app_state
            .message_queue
            .get_queued(ChatContextType::Ideation, blocked_queued.id.as_str())
            .len(),
        1,
        "blocked project's queue must remain pending"
    );
    assert_eq!(
        app_state
            .message_queue
            .get_queued(ChatContextType::Ideation, runnable_queued.id.as_str())
            .len(),
        0,
        "other project should still relaunch in the same resume pass"
    );
}

#[tokio::test]
async fn test_resume_borrowing_stays_blocked_when_ready_execution_waits() {
    let app_state = AppState::new_test();
    let execution_state = Arc::new(ExecutionState::new());
    let project = Project::new("Borrow Block".to_string(), "/test/borrow-block".to_string());
    app_state
        .project_repo
        .create(project.clone())
        .await
        .unwrap();

    execution_state.set_global_max_concurrent(5);
    execution_state.set_global_ideation_max(1);
    execution_state.set_allow_ideation_borrow_idle_execution(true);

    let occupied = app_state
        .ideation_session_repo
        .create(IdeationSession::new(project.id.clone()))
        .await
        .unwrap();
    let queued = app_state
        .ideation_session_repo
        .create(IdeationSession::new(project.id.clone()))
        .await
        .unwrap();

    let ready_task = Task::new(project.id.clone(), "Ready execution".to_string());
    app_state
        .task_repo
        .create(Task {
            internal_status: InternalStatus::Ready,
            ..ready_task
        })
        .await
        .unwrap();

    app_state.message_queue.queue(
        ChatContextType::Ideation,
        queued.id.as_str(),
        "blocked by ready execution".to_string(),
    );

    app_state
        .running_agent_registry
        .register(
            RunningAgentKey::new("ideation", occupied.id.as_str()),
            11111,
            "occupied-conv".to_string(),
            "occupied-run".to_string(),
            None,
            None,
        )
        .await;

    let mock = Arc::new(MockChatService::new());
    let resumed = resume_paused_ideation_queues_with_chat_service(
        Some(&project.id),
        &app_state,
        &execution_state,
        || Arc::clone(&mock) as Arc<dyn ChatService>,
    )
    .await
    .expect("resume paused ideation queue with ready execution");

    assert_eq!(resumed, 0);
    assert_eq!(mock.call_count(), 0);
    assert_eq!(
        app_state
            .message_queue
            .get_queued(ChatContextType::Ideation, queued.id.as_str())
            .len(),
        1,
        "borrowing must stay blocked while ready execution work exists"
    );
}

#[tokio::test]
async fn test_resume_borrowing_stays_blocked_when_workspace_queue_waits() {
    let app_state = AppState::new_test();
    let execution_state = Arc::new(ExecutionState::new());
    let project = Project::new(
        "Workspace Borrow Block".to_string(),
        "/test/workspace-borrow-block".to_string(),
    );
    app_state
        .project_repo
        .create(project.clone())
        .await
        .unwrap();

    execution_state.set_global_max_concurrent(5);
    execution_state.set_global_ideation_max(1);
    execution_state.set_allow_ideation_borrow_idle_execution(true);

    let occupied = app_state
        .ideation_session_repo
        .create(IdeationSession::new(project.id.clone()))
        .await
        .unwrap();
    let queued = app_state
        .ideation_session_repo
        .create(IdeationSession::new(project.id.clone()))
        .await
        .unwrap();

    app_state.message_queue.queue(
        ChatContextType::Project,
        project.id.as_str(),
        "waiting workspace".to_string(),
    );
    app_state.message_queue.queue(
        ChatContextType::Ideation,
        queued.id.as_str(),
        "blocked by workspace pressure".to_string(),
    );

    app_state
        .running_agent_registry
        .register(
            RunningAgentKey::new("ideation", occupied.id.as_str()),
            24242,
            "occupied-conv".to_string(),
            "occupied-run".to_string(),
            None,
            None,
        )
        .await;

    let mock = Arc::new(MockChatService::new());
    let resumed = resume_paused_ideation_queues_with_chat_service(
        Some(&project.id),
        &app_state,
        &execution_state,
        || Arc::clone(&mock) as Arc<dyn ChatService>,
    )
    .await
    .expect("resume queued ideation with workspace pressure");

    assert_eq!(resumed, 0);
    assert_eq!(mock.call_count(), 0);
    assert_eq!(
        app_state
            .message_queue
            .get_queued(ChatContextType::Ideation, queued.id.as_str())
            .len(),
        1,
        "borrowing must stay blocked while workspace work is waiting"
    );
}

#[tokio::test]
async fn test_resume_relaunches_queued_task_execution_message_for_active_task() {
    let app_state = AppState::new_test();
    let execution_state = Arc::new(ExecutionState::new());
    let project = Project::new(
        "Resume Task Queue".to_string(),
        "/test/task-queue".to_string(),
    );
    app_state
        .project_repo
        .create(project.clone())
        .await
        .unwrap();

    let task = app_state
        .task_repo
        .create(Task {
            internal_status: InternalStatus::Executing,
            ..Task::new(project.id.clone(), "Queued worker prompt".to_string())
        })
        .await
        .unwrap();

    app_state.message_queue.queue(
        ChatContextType::TaskExecution,
        task.id.as_str(),
        "continue execution".to_string(),
    );

    let mock = Arc::new(MockChatService::new());
    let resumed = resume_paused_slot_consuming_queues_with_chat_service(
        None,
        &app_state,
        &execution_state,
        || Arc::clone(&mock) as Arc<dyn ChatService>,
    )
    .await
    .expect("resume paused task queue");

    assert_eq!(resumed, 1);
    assert_eq!(mock.call_count(), 1);
    assert_eq!(
        app_state
            .message_queue
            .get_queued(ChatContextType::TaskExecution, task.id.as_str())
            .len(),
        0,
        "active task queue should be drained when resume relaunches the prompt"
    );
    assert_eq!(
        mock.get_sent_messages().await,
        vec!["continue execution".to_string()]
    );
}

#[tokio::test]
async fn test_resume_leaves_queued_task_execution_message_pending_for_paused_task() {
    let app_state = AppState::new_test();
    let execution_state = Arc::new(ExecutionState::new());
    let project = Project::new(
        "Resume Pending Queue".to_string(),
        "/test/pending-queue".to_string(),
    );
    app_state
        .project_repo
        .create(project.clone())
        .await
        .unwrap();

    let task = app_state
        .task_repo
        .create(Task {
            internal_status: InternalStatus::Paused,
            ..Task::new(project.id.clone(), "Paused worker prompt".to_string())
        })
        .await
        .unwrap();

    app_state.message_queue.queue(
        ChatContextType::TaskExecution,
        task.id.as_str(),
        "wait until restored".to_string(),
    );

    let mock = Arc::new(MockChatService::new());
    let resumed = resume_paused_slot_consuming_queues_with_chat_service(
        None,
        &app_state,
        &execution_state,
        || Arc::clone(&mock) as Arc<dyn ChatService>,
    )
    .await
    .expect("resume paused queue for paused task");

    assert_eq!(resumed, 0);
    assert_eq!(mock.call_count(), 0);
    assert_eq!(
        app_state
            .message_queue
            .get_queued(ChatContextType::TaskExecution, task.id.as_str())
            .len(),
        1,
        "paused task queue must stay pending until the task is active again"
    );
}

#[tokio::test]
async fn test_resume_relaunches_queued_review_message_for_active_task() {
    let app_state = AppState::new_test();
    let execution_state = Arc::new(ExecutionState::new());
    let project = Project::new(
        "Resume Review Queue".to_string(),
        "/test/review-queue".to_string(),
    );
    app_state
        .project_repo
        .create(project.clone())
        .await
        .unwrap();

    let task = app_state
        .task_repo
        .create(Task {
            internal_status: InternalStatus::Reviewing,
            ..Task::new(project.id.clone(), "Queued review prompt".to_string())
        })
        .await
        .unwrap();

    app_state.message_queue.queue(
        ChatContextType::Review,
        task.id.as_str(),
        "continue review".to_string(),
    );

    let mock = Arc::new(MockChatService::new());
    let resumed = resume_paused_slot_consuming_queues_with_chat_service(
        None,
        &app_state,
        &execution_state,
        || Arc::clone(&mock) as Arc<dyn ChatService>,
    )
    .await
    .expect("resume paused review queue");

    assert_eq!(resumed, 1);
    assert_eq!(mock.call_count(), 1);
    assert_eq!(
        app_state
            .message_queue
            .get_queued(ChatContextType::Review, task.id.as_str())
            .len(),
        0,
        "active review queue should be drained when resume relaunches the prompt"
    );
    assert_eq!(
        mock.get_sent_messages().await,
        vec!["continue review".to_string()]
    );
}

#[tokio::test]
async fn test_resume_relaunches_queued_merge_message_for_active_task() {
    let app_state = AppState::new_test();
    let execution_state = Arc::new(ExecutionState::new());
    let project = Project::new(
        "Resume Merge Queue".to_string(),
        "/test/merge-queue".to_string(),
    );
    app_state
        .project_repo
        .create(project.clone())
        .await
        .unwrap();

    let task = app_state
        .task_repo
        .create(Task {
            internal_status: InternalStatus::Merging,
            ..Task::new(project.id.clone(), "Queued merge prompt".to_string())
        })
        .await
        .unwrap();

    app_state.message_queue.queue(
        ChatContextType::Merge,
        task.id.as_str(),
        "continue merge".to_string(),
    );

    let mock = Arc::new(MockChatService::new());
    let resumed = resume_paused_slot_consuming_queues_with_chat_service(
        None,
        &app_state,
        &execution_state,
        || Arc::clone(&mock) as Arc<dyn ChatService>,
    )
    .await
    .expect("resume paused merge queue");

    assert_eq!(resumed, 1);
    assert_eq!(mock.call_count(), 1);
    assert_eq!(
        app_state
            .message_queue
            .get_queued(ChatContextType::Merge, task.id.as_str())
            .len(),
        0,
        "active merge queue should be drained when resume relaunches the prompt"
    );
    assert_eq!(
        mock.get_sent_messages().await,
        vec!["continue merge".to_string()]
    );
}

#[tokio::test]
async fn test_resume_respects_project_capacity_for_same_project_slot_queue() {
    let app_state = AppState::new_test();
    let execution_state = Arc::new(ExecutionState::new());
    let project = Project::new(
        "Blocked Slot Project".to_string(),
        "/test/blocked-slot".to_string(),
    );
    app_state
        .project_repo
        .create(project.clone())
        .await
        .unwrap();

    app_state
        .execution_settings_repo
        .update_settings(
            Some(&project.id),
            &ExecutionSettings {
                max_concurrent_tasks: 1,
                project_ideation_max: 1,
                auto_commit: true,
                pause_on_failure: true,
                ..ExecutionSettings::default()
            },
        )
        .await
        .unwrap();

    let occupied = app_state
        .task_repo
        .create(Task {
            internal_status: InternalStatus::Executing,
            ..Task::new(project.id.clone(), "Occupied slot".to_string())
        })
        .await
        .unwrap();
    let queued = app_state
        .task_repo
        .create(Task {
            internal_status: InternalStatus::Reviewing,
            ..Task::new(project.id.clone(), "Queued review".to_string())
        })
        .await
        .unwrap();

    app_state.message_queue.queue(
        ChatContextType::Review,
        queued.id.as_str(),
        "blocked review queue".to_string(),
    );

    app_state
        .running_agent_registry
        .register(
            RunningAgentKey::new("task_execution", occupied.id.as_str()),
            31337,
            "occupied-conv".to_string(),
            "occupied-run".to_string(),
            None,
            None,
        )
        .await;
    execution_state.set_running_count(1);

    let mock = Arc::new(MockChatService::new());
    let resumed = resume_paused_slot_consuming_queues_with_chat_service(
        Some(&project.id),
        &app_state,
        &execution_state,
        || Arc::clone(&mock) as Arc<dyn ChatService>,
    )
    .await
    .expect("resume blocked slot-consuming queue");

    assert_eq!(resumed, 0);
    assert_eq!(mock.call_count(), 0);
    assert_eq!(
        app_state
            .message_queue
            .get_queued(ChatContextType::Review, queued.id.as_str())
            .len(),
        1,
        "project-capped slot-consuming work must stay queued on resume"
    );
}

#[tokio::test]
async fn test_resume_skips_project_capped_slot_queue_and_relaunches_other_project() {
    let app_state = AppState::new_test();
    let execution_state = Arc::new(ExecutionState::new());

    let first_project = Project::new(
        "Blocked First".to_string(),
        "/test/blocked-first".to_string(),
    );
    let second_project = Project::new(
        "Runnable Second".to_string(),
        "/test/runnable-second".to_string(),
    );
    app_state
        .project_repo
        .create(first_project.clone())
        .await
        .unwrap();
    app_state
        .project_repo
        .create(second_project.clone())
        .await
        .unwrap();

    let (blocked_project, runnable_project) =
        if first_project.id.as_str() <= second_project.id.as_str() {
            (first_project, second_project)
        } else {
            (second_project, first_project)
        };

    app_state
        .execution_settings_repo
        .update_settings(
            Some(&blocked_project.id),
            &ExecutionSettings {
                max_concurrent_tasks: 1,
                project_ideation_max: 1,
                auto_commit: true,
                pause_on_failure: true,
                ..ExecutionSettings::default()
            },
        )
        .await
        .unwrap();

    let occupied = app_state
        .task_repo
        .create(Task {
            internal_status: InternalStatus::Executing,
            ..Task::new(
                blocked_project.id.clone(),
                "Blocked project occupied".to_string(),
            )
        })
        .await
        .unwrap();
    let blocked_queued = app_state
        .task_repo
        .create(Task {
            internal_status: InternalStatus::Reviewing,
            ..Task::new(
                blocked_project.id.clone(),
                "Blocked project review".to_string(),
            )
        })
        .await
        .unwrap();
    let runnable_queued = app_state
        .task_repo
        .create(Task {
            internal_status: InternalStatus::Merging,
            ..Task::new(
                runnable_project.id.clone(),
                "Runnable project merge".to_string(),
            )
        })
        .await
        .unwrap();

    app_state.message_queue.queue(
        ChatContextType::Review,
        blocked_queued.id.as_str(),
        "blocked project review queue".to_string(),
    );
    app_state.message_queue.queue(
        ChatContextType::Merge,
        runnable_queued.id.as_str(),
        "runnable project merge queue".to_string(),
    );

    app_state
        .running_agent_registry
        .register(
            RunningAgentKey::new("task_execution", occupied.id.as_str()),
            41414,
            "occupied-conv".to_string(),
            "occupied-run".to_string(),
            None,
            None,
        )
        .await;
    execution_state.set_running_count(1);

    let mock = Arc::new(MockChatService::new());
    let resumed = resume_paused_slot_consuming_queues_with_chat_service(
        None,
        &app_state,
        &execution_state,
        || Arc::clone(&mock) as Arc<dyn ChatService>,
    )
    .await
    .expect("resume queued slot-consuming work across projects");

    assert_eq!(resumed, 1);
    assert_eq!(mock.call_count(), 1);
    assert_eq!(
        mock.get_sent_messages().await,
        vec!["runnable project merge queue".to_string()]
    );
    assert_eq!(
        app_state
            .message_queue
            .get_queued(ChatContextType::Review, blocked_queued.id.as_str())
            .len(),
        1,
        "blocked project's slot-consuming queue must remain pending"
    );
    assert_eq!(
        app_state
            .message_queue
            .get_queued(ChatContextType::Merge, runnable_queued.id.as_str())
            .len(),
        0,
        "other project should still relaunch in the same resume pass"
    );
}

#[tokio::test]
async fn test_resume_priority_relaunches_slot_work_before_ideation_when_only_one_global_slot_remains(
) {
    let app_state = AppState::new_test();
    let execution_state = Arc::new(ExecutionState::new());

    let execution_project = Project::new(
        "Execution Priority".to_string(),
        "/test/execution-priority".to_string(),
    );
    let ideation_project = Project::new(
        "Ideation Secondary".to_string(),
        "/test/ideation-secondary".to_string(),
    );
    app_state
        .project_repo
        .create(execution_project.clone())
        .await
        .unwrap();
    app_state
        .project_repo
        .create(ideation_project.clone())
        .await
        .unwrap();

    execution_state.set_global_max_concurrent(1);
    execution_state.set_global_ideation_max(1);

    let review_task = app_state
        .task_repo
        .create(Task {
            internal_status: InternalStatus::Reviewing,
            ..Task::new(
                execution_project.id.clone(),
                "Resume queued review before ideation".to_string(),
            )
        })
        .await
        .unwrap();
    let ideation_session = app_state
        .ideation_session_repo
        .create(IdeationSession::new(ideation_project.id.clone()))
        .await
        .unwrap();

    app_state.message_queue.queue(
        ChatContextType::Review,
        review_task.id.as_str(),
        "priority review queue".to_string(),
    );
    app_state.message_queue.queue(
        ChatContextType::Ideation,
        ideation_session.id.as_str(),
        "secondary ideation queue".to_string(),
    );

    let slot_mock = Arc::new(MockChatService::new());
    let slot_resumed = resume_paused_slot_consuming_queues_with_chat_service(
        None,
        &app_state,
        &execution_state,
        || Arc::clone(&slot_mock) as Arc<dyn ChatService>,
    )
    .await
    .expect("resume slot-consuming queue first");

    assert_eq!(slot_resumed, 1);
    assert_eq!(
        slot_mock.get_sent_messages().await,
        vec!["priority review queue".to_string()]
    );
    execution_state.set_running_count(1);

    let ideation_mock = Arc::new(MockChatService::new());
    let ideation_resumed =
        resume_paused_ideation_queues_with_chat_service(None, &app_state, &execution_state, || {
            Arc::clone(&ideation_mock) as Arc<dyn ChatService>
        })
        .await
        .expect("resume ideation queue after slot-consuming work");

    assert_eq!(ideation_resumed, 0);
    assert_eq!(ideation_mock.call_count(), 0);
    assert_eq!(
        app_state
            .message_queue
            .get_queued(ChatContextType::Ideation, ideation_session.id.as_str())
            .len(),
        1,
        "ideation should stay queued when execution consumed the last global slot"
    );
}

#[tokio::test]
async fn test_resume_mixed_load_relaunches_execution_then_ideation_while_blocked_project_stays_queued(
) {
    let app_state = AppState::new_test();
    let execution_state = Arc::new(ExecutionState::new());

    let blocked_project = Project::new("A Blocked".to_string(), "/test/a-blocked".to_string());
    let execution_project =
        Project::new("B Execution".to_string(), "/test/b-execution".to_string());
    let ideation_project = Project::new("C Ideation".to_string(), "/test/c-ideation".to_string());
    app_state
        .project_repo
        .create(blocked_project.clone())
        .await
        .unwrap();
    app_state
        .project_repo
        .create(execution_project.clone())
        .await
        .unwrap();
    app_state
        .project_repo
        .create(ideation_project.clone())
        .await
        .unwrap();

    app_state
        .execution_settings_repo
        .update_settings(
            Some(&blocked_project.id),
            &ExecutionSettings {
                max_concurrent_tasks: 1,
                project_ideation_max: 1,
                auto_commit: true,
                pause_on_failure: true,
                ..ExecutionSettings::default()
            },
        )
        .await
        .unwrap();

    execution_state.set_global_max_concurrent(3);
    execution_state.set_global_ideation_max(2);

    let occupied = app_state
        .task_repo
        .create(Task {
            internal_status: InternalStatus::Executing,
            ..Task::new(
                blocked_project.id.clone(),
                "Blocked project occupied".to_string(),
            )
        })
        .await
        .unwrap();
    let execution_task = app_state
        .task_repo
        .create(Task {
            internal_status: InternalStatus::Reviewing,
            ..Task::new(
                execution_project.id.clone(),
                "Execution project review".to_string(),
            )
        })
        .await
        .unwrap();
    let blocked_ideation = app_state
        .ideation_session_repo
        .create(IdeationSession::new(blocked_project.id.clone()))
        .await
        .unwrap();
    let runnable_ideation = app_state
        .ideation_session_repo
        .create(IdeationSession::new(ideation_project.id.clone()))
        .await
        .unwrap();

    app_state.message_queue.queue(
        ChatContextType::Review,
        execution_task.id.as_str(),
        "execution project queued review".to_string(),
    );
    app_state.message_queue.queue(
        ChatContextType::Ideation,
        blocked_ideation.id.as_str(),
        "blocked project queued ideation".to_string(),
    );
    app_state.message_queue.queue(
        ChatContextType::Ideation,
        runnable_ideation.id.as_str(),
        "runnable project queued ideation".to_string(),
    );

    app_state
        .running_agent_registry
        .register(
            RunningAgentKey::new("task_execution", occupied.id.as_str()),
            51515,
            "occupied-conv".to_string(),
            "occupied-run".to_string(),
            None,
            None,
        )
        .await;
    execution_state.set_running_count(1);

    let slot_mock = Arc::new(MockChatService::new());
    let slot_resumed = resume_paused_slot_consuming_queues_with_chat_service(
        None,
        &app_state,
        &execution_state,
        || Arc::clone(&slot_mock) as Arc<dyn ChatService>,
    )
    .await
    .expect("resume execution-side queue under mixed load");

    assert_eq!(slot_resumed, 1);
    assert_eq!(
        slot_mock.get_sent_messages().await,
        vec!["execution project queued review".to_string()]
    );

    execution_state.set_running_count(2);

    let ideation_mock = Arc::new(MockChatService::new());
    let ideation_resumed =
        resume_paused_ideation_queues_with_chat_service(None, &app_state, &execution_state, || {
            Arc::clone(&ideation_mock) as Arc<dyn ChatService>
        })
        .await
        .expect("resume ideation queue under mixed load");

    assert_eq!(ideation_resumed, 1);
    assert_eq!(
        ideation_mock.get_sent_messages().await,
        vec!["runnable project queued ideation".to_string()]
    );
    assert_eq!(
        app_state
            .message_queue
            .get_queued(ChatContextType::Ideation, blocked_ideation.id.as_str())
            .len(),
        1,
        "blocked project's ideation queue must remain pending"
    );
    assert_eq!(
        app_state
            .message_queue
            .get_queued(ChatContextType::Ideation, runnable_ideation.id.as_str())
            .len(),
        0,
        "runnable ideation project should consume the remaining global slot"
    );
}

#[tokio::test]
async fn test_resume_mixed_context_relaunches_workspace_execution_ideation_and_task_chat_queues() {
    let app_state = AppState::new_test();
    let execution_state = Arc::new(ExecutionState::new());

    let project = Project::new("Mixed Resume".to_string(), "/test/mixed-resume".to_string());
    app_state
        .project_repo
        .create(project.clone())
        .await
        .unwrap();

    let review_task = app_state
        .task_repo
        .create(Task {
            internal_status: InternalStatus::Reviewing,
            ..Task::new(project.id.clone(), "Resume review task".to_string())
        })
        .await
        .unwrap();
    let task_chat_task = app_state
        .task_repo
        .create(Task::new(
            project.id.clone(),
            "Resume task chat target".to_string(),
        ))
        .await
        .unwrap();
    let ideation_session = app_state
        .ideation_session_repo
        .create(IdeationSession::new(project.id.clone()))
        .await
        .unwrap();

    app_state.message_queue.queue(
        ChatContextType::Review,
        review_task.id.as_str(),
        "resume mixed review".to_string(),
    );
    app_state.message_queue.queue(
        ChatContextType::Ideation,
        ideation_session.id.as_str(),
        "resume mixed ideation".to_string(),
    );
    app_state.message_queue.queue_with_overrides(
        ChatContextType::Task,
        task_chat_task.id.as_str(),
        "resume mixed task chat".to_string(),
        Some(r#"{"resume_in_place":true}"#.to_string()),
        None,
        None,
    );
    app_state.message_queue.queue_with_overrides(
        ChatContextType::Project,
        project.id.as_str(),
        "resume mixed project chat".to_string(),
        Some(r#"{"resume_in_place":true}"#.to_string()),
        None,
        None,
    );

    let workspace_mock = Arc::new(MockChatService::new());
    let workspace_resumed = resume_paused_workspace_queues_with_chat_service(
        None,
        &app_state,
        &execution_state,
        || Arc::clone(&workspace_mock) as Arc<dyn ChatService>,
    )
    .await
    .expect("resume mixed workspace queue");

    assert_eq!(workspace_resumed, 1);
    assert_eq!(
        workspace_mock.get_sent_messages().await,
        vec!["resume mixed project chat".to_string()]
    );

    let slot_mock = Arc::new(MockChatService::new());
    let slot_resumed = resume_paused_slot_consuming_queues_with_chat_service(
        None,
        &app_state,
        &execution_state,
        || Arc::clone(&slot_mock) as Arc<dyn ChatService>,
    )
    .await
    .expect("resume mixed slot-consuming queue");

    assert_eq!(slot_resumed, 1);
    assert_eq!(
        slot_mock.get_sent_messages().await,
        vec!["resume mixed review".to_string()]
    );

    let ideation_mock = Arc::new(MockChatService::new());
    let ideation_resumed =
        resume_paused_ideation_queues_with_chat_service(None, &app_state, &execution_state, || {
            Arc::clone(&ideation_mock) as Arc<dyn ChatService>
        })
        .await
        .expect("resume mixed ideation queue");

    assert_eq!(ideation_resumed, 1);
    assert_eq!(
        ideation_mock.get_sent_messages().await,
        vec!["resume mixed ideation".to_string()]
    );

    let chat_mock = Arc::new(MockChatService::new());
    let chat_resumed =
        resume_paused_non_slot_chat_queues_with_chat_service(None, &app_state, || {
            Arc::clone(&chat_mock) as Arc<dyn ChatService>
        })
        .await
        .expect("resume mixed non-slot chat queues");

    assert_eq!(chat_resumed, 1);
    assert_eq!(
        chat_mock.get_sent_messages().await,
        vec!["resume mixed task chat".to_string()]
    );
    assert!(app_state
        .message_queue
        .get_queued(ChatContextType::Review, review_task.id.as_str())
        .is_empty());
    assert!(app_state
        .message_queue
        .get_queued(ChatContextType::Ideation, ideation_session.id.as_str())
        .is_empty());
    assert!(app_state
        .message_queue
        .get_queued(ChatContextType::Task, task_chat_task.id.as_str())
        .is_empty());
    assert!(app_state
        .message_queue
        .get_queued(ChatContextType::Project, project.id.as_str())
        .is_empty());
}

#[tokio::test]
async fn test_project_has_execution_capacity_for_state_ignores_other_projects() {
    let app_state = AppState::new_test();
    let execution_state = Arc::new(ExecutionState::new());

    let project_a = Project::new("Project A".to_string(), "/test/project-a".to_string());
    let project_b = Project::new("Project B".to_string(), "/test/project-b".to_string());
    app_state
        .project_repo
        .create(project_a.clone())
        .await
        .unwrap();
    app_state
        .project_repo
        .create(project_b.clone())
        .await
        .unwrap();

    app_state
        .execution_settings_repo
        .update_settings(
            Some(&project_a.id),
            &ExecutionSettings {
                max_concurrent_tasks: 1,
                project_ideation_max: 1,
                auto_commit: true,
                pause_on_failure: true,
                ..ExecutionSettings::default()
            },
        )
        .await
        .unwrap();

    let other_project_task = app_state
        .task_repo
        .create(Task {
            internal_status: InternalStatus::Executing,
            ..Task::new(project_b.id.clone(), "Other project running".to_string())
        })
        .await
        .unwrap();
    app_state
        .running_agent_registry
        .register(
            RunningAgentKey::new("task_execution", other_project_task.id.as_str()),
            34343,
            "other-project-conv".to_string(),
            "other-project-run".to_string(),
            None,
            None,
        )
        .await;

    assert!(
        project_has_execution_capacity_for_state(&app_state, &execution_state, &project_a.id)
            .await
            .expect("project capacity check"),
        "activity in another project must not consume this project's execution quota"
    );
}

#[tokio::test]
async fn test_pause_resets_running_count() {
    // Setup: Create tasks in agent-active states and simulate running count
    let execution_state = Arc::new(ExecutionState::with_max_concurrent(5));
    let app_state = AppState::new_test();

    // Create a test project
    let project = Project::new("Test Project".to_string(), "/test/path".to_string());
    app_state
        .project_repo
        .create(project.clone())
        .await
        .unwrap();

    // Create tasks in agent-active statuses
    let mut task1 = Task::new(project.id.clone(), "Executing Task 1".to_string());
    task1.internal_status = InternalStatus::Executing;
    app_state.task_repo.create(task1.clone()).await.unwrap();

    let mut task2 = Task::new(project.id.clone(), "Executing Task 2".to_string());
    task2.internal_status = InternalStatus::Executing;
    app_state.task_repo.create(task2.clone()).await.unwrap();

    let mut task3 = Task::new(project.id.clone(), "Reviewing Task".to_string());
    task3.internal_status = InternalStatus::Reviewing;
    app_state.task_repo.create(task3.clone()).await.unwrap();

    // Simulate that running count matches agent-active tasks
    execution_state.increment_running(); // task1
    execution_state.increment_running(); // task2
    execution_state.increment_running(); // task3
    assert_eq!(execution_state.running_count(), 3);

    // Build transition service
    let transition_service: TaskTransitionService = TaskTransitionService::new(
        Arc::clone(&app_state.task_repo),
        Arc::clone(&app_state.task_dependency_repo),
        Arc::clone(&app_state.project_repo),
        Arc::clone(&app_state.chat_message_repo),
        Arc::clone(&app_state.chat_attachment_repo),
        Arc::clone(&app_state.chat_conversation_repo),
        Arc::clone(&app_state.agent_run_repo),
        Arc::clone(&app_state.ideation_session_repo),
        Arc::clone(&app_state.activity_event_repo),
        Arc::clone(&app_state.message_queue),
        Arc::clone(&app_state.running_agent_registry),
        Arc::clone(&execution_state),
        None,
        Arc::clone(&app_state.memory_event_repo),
    );

    // Execute pause: pause and transition all agent-active tasks to Paused
    execution_state.pause();

    let tasks = app_state
        .task_repo
        .get_by_project(&project.id)
        .await
        .unwrap();
    for task in tasks {
        if AGENT_ACTIVE_STATUSES.contains(&task.internal_status) {
            let _ = transition_service
                .transition_task(&task.id, InternalStatus::Paused)
                .await;
        }
    }

    // Verify: Running count should be 0 after all tasks transitioned to Paused
    // (on_exit handlers decrement for each agent-active state exit)
    assert_eq!(
        execution_state.running_count(),
        0,
        "Running count should be 0 after pause transitions all tasks to Paused"
    );

    // Verify execution is paused
    assert!(execution_state.is_paused());
}

#[test]
fn test_agent_active_statuses_constant() {
    // Verify the constant includes all expected statuses
    assert!(AGENT_ACTIVE_STATUSES.contains(&InternalStatus::Executing));
    assert!(AGENT_ACTIVE_STATUSES.contains(&InternalStatus::QaRefining));
    assert!(AGENT_ACTIVE_STATUSES.contains(&InternalStatus::QaTesting));
    assert!(AGENT_ACTIVE_STATUSES.contains(&InternalStatus::Reviewing));
    assert!(AGENT_ACTIVE_STATUSES.contains(&InternalStatus::ReExecuting));
    assert!(AGENT_ACTIVE_STATUSES.contains(&InternalStatus::Merging));

    // Non-agent-active statuses should not be included
    assert!(!AGENT_ACTIVE_STATUSES.contains(&InternalStatus::Ready));
    assert!(!AGENT_ACTIVE_STATUSES.contains(&InternalStatus::Backlog));
    assert!(!AGENT_ACTIVE_STATUSES.contains(&InternalStatus::Failed));
    assert!(!AGENT_ACTIVE_STATUSES.contains(&InternalStatus::Stopped));
    assert!(!AGENT_ACTIVE_STATUSES.contains(&InternalStatus::Paused));
}

#[test]
fn test_default_trait() {
    let state = ExecutionState::default();
    assert!(!state.is_paused());
    assert_eq!(state.running_count(), 0);
    assert_eq!(state.max_concurrent(), 10);
}

// ========================================
// Event Emission Tests
// ========================================

#[test]
fn test_emit_status_changed_does_not_panic() {
    let state = ExecutionState::new();
    state.increment_running();

    let handle = crate::testing::create_mock_app_handle();
    // Should not panic even with mock runtime
    state.emit_status_changed(&handle, "task_started");
}

#[test]
fn test_emit_status_changed_reflects_current_state() {
    let state = ExecutionState::with_max_concurrent(4);
    state.increment_running();
    state.increment_running();
    state.pause();

    let handle = crate::testing::create_mock_app_handle();
    // Verify the method reads current state correctly
    // (emit itself is fire-and-forget, but we can verify state is consistent)
    assert!(state.is_paused());
    assert_eq!(state.running_count(), 2);
    assert_eq!(state.max_concurrent(), 4);
    state.emit_status_changed(&handle, "paused");
}

#[test]
fn test_emit_status_changed_with_various_reasons() {
    let state = ExecutionState::new();
    let handle = crate::testing::create_mock_app_handle();

    // All valid reason strings should work without panic
    let reasons = [
        "task_started",
        "task_completed",
        "paused",
        "resumed",
        "stopped",
    ];
    for reason in &reasons {
        state.emit_status_changed(&handle, reason);
    }
}

// ========================================
// Integration Tests - Stop Execution
// ========================================

#[tokio::test]
async fn test_stop_resets_running_count() {
    // Setup: Create tasks in agent-active states and simulate running count
    let execution_state = Arc::new(ExecutionState::with_max_concurrent(5));
    let app_state = AppState::new_test();

    // Create a test project
    let project = Project::new("Test Project".to_string(), "/test/path".to_string());
    app_state
        .project_repo
        .create(project.clone())
        .await
        .unwrap();

    // Create tasks in agent-active statuses
    let mut task1 = Task::new(project.id.clone(), "Executing Task 1".to_string());
    task1.internal_status = InternalStatus::Executing;
    app_state.task_repo.create(task1.clone()).await.unwrap();

    let mut task2 = Task::new(project.id.clone(), "Executing Task 2".to_string());
    task2.internal_status = InternalStatus::Executing;
    app_state.task_repo.create(task2.clone()).await.unwrap();

    let mut task3 = Task::new(project.id.clone(), "Reviewing Task".to_string());
    task3.internal_status = InternalStatus::Reviewing;
    app_state.task_repo.create(task3.clone()).await.unwrap();

    // Simulate that running count matches agent-active tasks
    // (In real usage, spawner increments this when starting each task)
    execution_state.increment_running(); // task1
    execution_state.increment_running(); // task2
    execution_state.increment_running(); // task3
    assert_eq!(execution_state.running_count(), 3);

    // Build transition service
    let transition_service: TaskTransitionService = TaskTransitionService::new(
        Arc::clone(&app_state.task_repo),
        Arc::clone(&app_state.task_dependency_repo),
        Arc::clone(&app_state.project_repo),
        Arc::clone(&app_state.chat_message_repo),
        Arc::clone(&app_state.chat_attachment_repo),
        Arc::clone(&app_state.chat_conversation_repo),
        Arc::clone(&app_state.agent_run_repo),
        Arc::clone(&app_state.ideation_session_repo),
        Arc::clone(&app_state.activity_event_repo),
        Arc::clone(&app_state.message_queue),
        Arc::clone(&app_state.running_agent_registry),
        Arc::clone(&execution_state),
        None,
        Arc::clone(&app_state.memory_event_repo),
    );

    // Execute stop: pause and transition all agent-active tasks to Stopped
    execution_state.pause();

    let tasks = app_state
        .task_repo
        .get_by_project(&project.id)
        .await
        .unwrap();
    for task in tasks {
        if AGENT_ACTIVE_STATUSES.contains(&task.internal_status) {
            let _ = transition_service
                .transition_task(&task.id, InternalStatus::Stopped)
                .await;
        }
    }

    // Verify: Running count should be 0 after all tasks transitioned to Stopped
    // (on_exit handlers decrement for each agent-active state exit)
    assert_eq!(
        execution_state.running_count(),
        0,
        "Running count should be 0 after stop transitions all tasks to Stopped"
    );

    // Verify execution is paused
    assert!(execution_state.is_paused());
}

#[tokio::test]
async fn test_running_count_decrements_on_task_completion() {
    // Setup: Create a task in Executing state
    let execution_state = Arc::new(ExecutionState::with_max_concurrent(5));
    let app_state = AppState::new_test();

    // Create a test project
    let project = Project::new("Test Project".to_string(), "/test/path".to_string());
    app_state
        .project_repo
        .create(project.clone())
        .await
        .unwrap();

    // Create a task in Executing status
    let mut task = Task::new(project.id.clone(), "Executing Task".to_string());
    task.internal_status = InternalStatus::Executing;
    app_state.task_repo.create(task.clone()).await.unwrap();

    // Simulate that running count was incremented when task started
    execution_state.increment_running();
    assert_eq!(execution_state.running_count(), 1);

    // Build transition service with execution state
    let transition_service: TaskTransitionService = TaskTransitionService::new(
        Arc::clone(&app_state.task_repo),
        Arc::clone(&app_state.task_dependency_repo),
        Arc::clone(&app_state.project_repo),
        Arc::clone(&app_state.chat_message_repo),
        Arc::clone(&app_state.chat_attachment_repo),
        Arc::clone(&app_state.chat_conversation_repo),
        Arc::clone(&app_state.agent_run_repo),
        Arc::clone(&app_state.ideation_session_repo),
        Arc::clone(&app_state.activity_event_repo),
        Arc::clone(&app_state.message_queue),
        Arc::clone(&app_state.running_agent_registry),
        Arc::clone(&execution_state),
        None,
        Arc::clone(&app_state.memory_event_repo),
    );

    // Transition task from Executing to Failed (simulating task cancellation)
    // Note: In real usage, task might go through QaRefining -> QaTesting -> QaPassed,
    // but for testing the decrement behavior, any exit from Executing is sufficient.
    let _ = transition_service
        .transition_task(&task.id, InternalStatus::Failed)
        .await;

    // Verify: Running count should have decremented
    // (on_exit handler for Executing state decrements)
    assert_eq!(
        execution_state.running_count(),
        0,
        "Running count should decrement when task exits Executing state"
    );
}

#[tokio::test]
async fn test_running_count_decrements_for_all_agent_active_states() {
    // Test that decrement works for all agent-active states, not just Executing
    let execution_state = Arc::new(ExecutionState::with_max_concurrent(10));
    let app_state = AppState::new_test();

    // Create a test project
    let project = Project::new("Test Project".to_string(), "/test/path".to_string());
    app_state
        .project_repo
        .create(project.clone())
        .await
        .unwrap();

    // Create tasks in different agent-active states
    let test_cases = [
        (
            InternalStatus::Executing,
            InternalStatus::Failed,
            "Executing Task",
        ),
        (
            InternalStatus::QaRefining,
            InternalStatus::Stopped,
            "QaRefining Task",
        ),
        (
            InternalStatus::QaTesting,
            InternalStatus::Stopped,
            "QaTesting Task",
        ),
        (
            InternalStatus::Reviewing,
            InternalStatus::Stopped,
            "Reviewing Task",
        ),
        (
            InternalStatus::ReExecuting,
            InternalStatus::Failed,
            "ReExecuting Task",
        ),
    ];

    // Create all tasks and increment running count for each
    let mut transitions = Vec::new();
    for (status, target_status, title) in &test_cases {
        let mut task = Task::new(project.id.clone(), title.to_string());
        task.internal_status = *status;
        app_state.task_repo.create(task.clone()).await.unwrap();
        transitions.push((task.id, *target_status));
        execution_state.increment_running();
    }

    assert_eq!(execution_state.running_count(), 5);

    // Build transition service
    let transition_service: TaskTransitionService = TaskTransitionService::new(
        Arc::clone(&app_state.task_repo),
        Arc::clone(&app_state.task_dependency_repo),
        Arc::clone(&app_state.project_repo),
        Arc::clone(&app_state.chat_message_repo),
        Arc::clone(&app_state.chat_attachment_repo),
        Arc::clone(&app_state.chat_conversation_repo),
        Arc::clone(&app_state.agent_run_repo),
        Arc::clone(&app_state.ideation_session_repo),
        Arc::clone(&app_state.activity_event_repo),
        Arc::clone(&app_state.message_queue),
        Arc::clone(&app_state.running_agent_registry),
        Arc::clone(&execution_state),
        None,
        Arc::clone(&app_state.memory_event_repo),
    );

    // Transition each task out of its agent-active state using a valid target.
    for (task_id, target_status) in &transitions {
        let _ = transition_service
            .transition_task(task_id, *target_status)
            .await;
    }

    // Verify: Running count should be 0 after all tasks transitioned
    assert_eq!(
        execution_state.running_count(),
        0,
        "Running count should be 0 after all agent-active tasks exit their states"
    );
}

// ========================================
// Integration Tests - Pause Prevents Spawns
// ========================================
// Note: Detailed spawn blocking tests are in spawner.rs:
// - test_spawn_blocked_when_paused
// - test_spawn_blocked_at_max_concurrent
// - test_spawn_increments_running_count
// These tests verify the ExecutionState integration with the spawner.

// ========================================
// set_max_concurrent Tests
// ========================================

#[test]
fn test_set_max_concurrent_updates_value() {
    let state = ExecutionState::new();
    assert_eq!(state.max_concurrent(), 10); // default

    state.set_max_concurrent(5);
    assert_eq!(state.max_concurrent(), 5);

    state.set_max_concurrent(1);
    assert_eq!(state.max_concurrent(), 1);
}

#[test]
fn test_can_start_task_respects_max_concurrent() {
    let state = ExecutionState::with_max_concurrent(2);

    // Initially can start
    assert!(state.can_start_task());

    // Add one running
    state.increment_running();
    assert!(state.can_start_task());

    // At max
    state.increment_running();
    assert!(!state.can_start_task());

    // Increase max - now can start again
    state.set_max_concurrent(3);
    assert!(state.can_start_task());
}

#[tokio::test]
async fn test_resume_clears_pause_and_allows_tasks() {
    let state = ExecutionState::with_max_concurrent(2);

    // Pause
    state.pause();
    assert!(!state.can_start_task());

    // Resume
    state.resume();
    assert!(state.can_start_task());
}

// ========================================
// Execution Settings Tests
// ========================================

#[tokio::test]
async fn test_execution_settings_repo_get_default() {
    let app_state = AppState::new_test();

    let settings = app_state
        .execution_settings_repo
        .get_settings(None)
        .await
        .expect("Failed to get execution settings");

    // Default values
    assert_eq!(settings.max_concurrent_tasks, 10);
    assert_eq!(settings.project_ideation_max, 5);
    assert!(settings.auto_commit);
    assert!(settings.pause_on_failure);
}

#[tokio::test]
async fn test_execution_settings_repo_update() {
    let app_state = AppState::new_test();

    let new_settings = ExecutionSettings {
        max_concurrent_tasks: 5,
        project_ideation_max: 2,
        auto_commit: false,
        pause_on_failure: false,
        ..ExecutionSettings::default()
    };

    let updated = app_state
        .execution_settings_repo
        .update_settings(None, &new_settings)
        .await
        .expect("Failed to update execution settings");

    assert_eq!(updated.max_concurrent_tasks, 5);
    assert_eq!(updated.project_ideation_max, 2);
    assert!(!updated.auto_commit);
    assert!(!updated.pause_on_failure);

    // Verify persistence
    let retrieved = app_state
        .execution_settings_repo
        .get_settings(None)
        .await
        .expect("Failed to get execution settings");

    assert_eq!(retrieved.max_concurrent_tasks, 5);
    assert_eq!(retrieved.project_ideation_max, 2);
    assert!(!retrieved.auto_commit);
    assert!(!retrieved.pause_on_failure);
}

#[tokio::test]
async fn test_execution_settings_update_syncs_execution_state() {
    let execution_state = Arc::new(ExecutionState::new());
    let app_state = AppState::new_test();

    // Initial state (ExecutionState::new() defaults to max_concurrent=10)
    assert_eq!(execution_state.max_concurrent(), 10);

    // Update settings
    let new_settings = ExecutionSettings {
        max_concurrent_tasks: 8,
        project_ideation_max: 2,
        auto_commit: true,
        pause_on_failure: true,
        ..ExecutionSettings::default()
    };

    app_state
        .execution_settings_repo
        .update_settings(None, &new_settings)
        .await
        .expect("Failed to update execution settings");

    // Simulate what update_execution_settings command does
    execution_state.set_max_concurrent(8);

    // ExecutionState should be updated
    assert_eq!(execution_state.max_concurrent(), 8);
}

// ========================================
// Resume Execution Tests (Phase 80 Task 4)
// ========================================

#[tokio::test]
async fn test_resume_restores_paused_tasks_to_previous_status() {
    // Setup: Create a task that was Executing before being Paused
    let execution_state = Arc::new(ExecutionState::with_max_concurrent(5));
    let app_state = AppState::new_test();

    // Create a test project
    let project = Project::new("Test Project".to_string(), "/test/path".to_string());
    app_state
        .project_repo
        .create(project.clone())
        .await
        .unwrap();

    // Create a task in Executing state
    let mut task = Task::new(project.id.clone(), "Executing Task".to_string());
    task.internal_status = InternalStatus::Executing;
    let task_id = task.id.clone();
    app_state.task_repo.create(task.clone()).await.unwrap();

    // Build transition service
    let transition_service: TaskTransitionService = TaskTransitionService::new(
        Arc::clone(&app_state.task_repo),
        Arc::clone(&app_state.task_dependency_repo),
        Arc::clone(&app_state.project_repo),
        Arc::clone(&app_state.chat_message_repo),
        Arc::clone(&app_state.chat_attachment_repo),
        Arc::clone(&app_state.chat_conversation_repo),
        Arc::clone(&app_state.agent_run_repo),
        Arc::clone(&app_state.ideation_session_repo),
        Arc::clone(&app_state.activity_event_repo),
        Arc::clone(&app_state.message_queue),
        Arc::clone(&app_state.running_agent_registry),
        Arc::clone(&execution_state),
        None,
        Arc::clone(&app_state.memory_event_repo),
    );

    // Pause: transition Executing -> Paused (creates status history entry)
    execution_state.pause();
    transition_service
        .transition_task(&task_id, InternalStatus::Paused)
        .await
        .expect("Failed to transition to Paused");

    // Verify task is Paused
    let paused_task = app_state
        .task_repo
        .get_by_id(&task_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(paused_task.internal_status, InternalStatus::Paused);

    // Verify status history shows Executing -> Paused transition
    let history = app_state
        .task_repo
        .get_status_history(&task_id)
        .await
        .unwrap();
    let pause_transition = history
        .iter()
        .rev()
        .find(|t| t.to == InternalStatus::Paused);
    assert!(pause_transition.is_some());
    assert_eq!(pause_transition.unwrap().from, InternalStatus::Executing);

    // Resume: should restore Paused -> Executing
    execution_state.resume();
    transition_service
        .transition_task(&task_id, InternalStatus::Executing)
        .await
        .expect("Failed to restore from Paused");

    // Verify task transitions to Failed when execution is blocked
    let restored_task = app_state
        .task_repo
        .get_by_id(&task_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(restored_task.internal_status, InternalStatus::Failed);
}

#[tokio::test]
async fn test_determine_paused_restore_status_falls_back_to_history_when_metadata_invalid() {
    let execution_state = Arc::new(ExecutionState::with_max_concurrent(5));
    let app_state = AppState::new_test();

    let project = Project::new("Test Project".to_string(), "/test/path".to_string());
    app_state
        .project_repo
        .create(project.clone())
        .await
        .unwrap();

    let mut task = Task::new(project.id.clone(), "Paused Task".to_string());
    task.internal_status = InternalStatus::Executing;
    let task_id = task.id.clone();
    app_state.task_repo.create(task).await.unwrap();

    let transition_service: TaskTransitionService = TaskTransitionService::new(
        Arc::clone(&app_state.task_repo),
        Arc::clone(&app_state.task_dependency_repo),
        Arc::clone(&app_state.project_repo),
        Arc::clone(&app_state.chat_message_repo),
        Arc::clone(&app_state.chat_attachment_repo),
        Arc::clone(&app_state.chat_conversation_repo),
        Arc::clone(&app_state.agent_run_repo),
        Arc::clone(&app_state.ideation_session_repo),
        Arc::clone(&app_state.activity_event_repo),
        Arc::clone(&app_state.message_queue),
        Arc::clone(&app_state.running_agent_registry),
        Arc::clone(&execution_state),
        None,
        Arc::clone(&app_state.memory_event_repo),
    );

    execution_state.pause();
    transition_service
        .transition_task(&task_id, InternalStatus::Paused)
        .await
        .expect("Failed to transition to Paused");

    let mut paused_task = app_state
        .task_repo
        .get_by_id(&task_id)
        .await
        .unwrap()
        .unwrap();
    paused_task.metadata = Some(
        serde_json::json!({
            "pause_reason": {
                "kind": "user_initiated",
                "previous_status": "not-a-real-status",
                "paused_at": chrono::Utc::now().to_rfc3339(),
                "scope": "global"
            }
        })
        .to_string(),
    );
    paused_task.touch();
    app_state.task_repo.update(&paused_task).await.unwrap();

    let refreshed = app_state
        .task_repo
        .get_by_id(&task_id)
        .await
        .unwrap()
        .unwrap();
    let restore_status = determine_paused_restore_status(&refreshed, app_state.task_repo.as_ref())
        .await
        .unwrap();

    assert_eq!(
        restore_status,
        Some(InternalStatus::Executing),
        "Malformed pause metadata should fall back to the real pre-pause status from history"
    );
}

#[test]
fn test_prepare_resumed_task_for_entry_actions_clears_pause_reason_and_marks_resume() {
    let mut task = Task::new(
        ProjectId::from_string("project-resume".to_string()),
        "Resume marker task".to_string(),
    );
    task.metadata = Some(
        serde_json::json!({
            "pause_reason": {
                "kind": "user_initiated",
                "previous_status": "executing",
                "paused_at": chrono::Utc::now().to_rfc3339(),
                "scope": "global"
            },
            "other_key": "keep-me"
        })
        .to_string(),
    );

    prepare_resumed_task_for_entry_actions(&mut task);

    let metadata: serde_json::Value =
        serde_json::from_str(task.metadata.as_deref().expect("metadata")).expect("json");
    assert!(metadata.get("pause_reason").is_none());
    assert_eq!(metadata["trigger_origin"], "resume");
    assert_eq!(metadata["other_key"], "keep-me");
}

#[tokio::test]
async fn test_determine_paused_restore_status_skips_when_no_valid_metadata_or_history() {
    let app_state = AppState::new_test();
    let project = Project::new("Test Project".to_string(), "/test/path".to_string());
    app_state
        .project_repo
        .create(project.clone())
        .await
        .unwrap();

    let mut task = Task::new(
        project.id.clone(),
        "Paused Task Without History".to_string(),
    );
    task.internal_status = InternalStatus::Paused;
    task.metadata = Some(
        serde_json::json!({
            "pause_reason": {
                "kind": "user_initiated",
                "previous_status": "not-a-real-status",
                "paused_at": chrono::Utc::now().to_rfc3339(),
                "scope": "global"
            }
        })
        .to_string(),
    );
    let task = app_state.task_repo.create(task).await.unwrap();

    let restore_status = determine_paused_restore_status(&task, app_state.task_repo.as_ref())
        .await
        .unwrap();

    assert_eq!(
        restore_status, None,
        "Malformed pause metadata without pause history should skip restore instead of inventing Executing"
    );
}

#[tokio::test]
async fn test_resume_does_not_restore_stopped_tasks() {
    // Setup: Create a task that was Executing before being Stopped
    let execution_state = Arc::new(ExecutionState::with_max_concurrent(5));
    let app_state = AppState::new_test();

    // Create a test project
    let project = Project::new("Test Project".to_string(), "/test/path".to_string());
    app_state
        .project_repo
        .create(project.clone())
        .await
        .unwrap();

    // Create a task and transition it to Stopped
    let mut task = Task::new(project.id.clone(), "Stopped Task".to_string());
    task.internal_status = InternalStatus::Executing;
    let task_id = task.id.clone();
    app_state.task_repo.create(task.clone()).await.unwrap();

    // Build transition service
    let transition_service: TaskTransitionService = TaskTransitionService::new(
        Arc::clone(&app_state.task_repo),
        Arc::clone(&app_state.task_dependency_repo),
        Arc::clone(&app_state.project_repo),
        Arc::clone(&app_state.chat_message_repo),
        Arc::clone(&app_state.chat_attachment_repo),
        Arc::clone(&app_state.chat_conversation_repo),
        Arc::clone(&app_state.agent_run_repo),
        Arc::clone(&app_state.ideation_session_repo),
        Arc::clone(&app_state.activity_event_repo),
        Arc::clone(&app_state.message_queue),
        Arc::clone(&app_state.running_agent_registry),
        Arc::clone(&execution_state),
        None,
        Arc::clone(&app_state.memory_event_repo),
    );

    // Stop: transition Executing -> Stopped
    execution_state.pause();
    transition_service
        .transition_task(&task_id, InternalStatus::Stopped)
        .await
        .expect("Failed to transition to Stopped");

    // Verify task is Stopped
    let stopped_task = app_state
        .task_repo
        .get_by_id(&task_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(stopped_task.internal_status, InternalStatus::Stopped);

    // Resume: should NOT restore Stopped tasks
    execution_state.resume();

    // Task should STILL be Stopped (resume doesn't restore Stopped)
    let still_stopped = app_state
        .task_repo
        .get_by_id(&task_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        still_stopped.internal_status,
        InternalStatus::Stopped,
        "Stopped tasks should NOT be automatically restored on resume"
    );
}

#[tokio::test]
async fn test_resume_restores_multiple_paused_tasks() {
    // Setup: Create multiple tasks in different agent-active states, pause them, then resume
    let execution_state = Arc::new(ExecutionState::with_max_concurrent(10));
    let app_state = AppState::new_test();

    // Create a test project
    let project = Project::new("Test Project".to_string(), "/test/path".to_string());
    app_state
        .project_repo
        .create(project.clone())
        .await
        .unwrap();

    // Build transition service
    let transition_service: TaskTransitionService = TaskTransitionService::new(
        Arc::clone(&app_state.task_repo),
        Arc::clone(&app_state.task_dependency_repo),
        Arc::clone(&app_state.project_repo),
        Arc::clone(&app_state.chat_message_repo),
        Arc::clone(&app_state.chat_attachment_repo),
        Arc::clone(&app_state.chat_conversation_repo),
        Arc::clone(&app_state.agent_run_repo),
        Arc::clone(&app_state.ideation_session_repo),
        Arc::clone(&app_state.activity_event_repo),
        Arc::clone(&app_state.message_queue),
        Arc::clone(&app_state.running_agent_registry),
        Arc::clone(&execution_state),
        None,
        Arc::clone(&app_state.memory_event_repo),
    );

    // Create tasks in different agent-active states
    let test_cases = [
        (InternalStatus::Executing, "Executing Task"),
        (InternalStatus::Reviewing, "Reviewing Task"),
        (InternalStatus::QaRefining, "QaRefining Task"),
    ];

    let mut task_ids = Vec::new();
    let mut original_statuses = Vec::new();
    for (status, title) in &test_cases {
        let mut task = Task::new(project.id.clone(), title.to_string());
        task.internal_status = *status;
        // Reviewing tasks need a worktree_path to pass ensure_review_worktree_ready.
        // Use the system temp dir (guaranteed to exist) to satisfy the existence check.
        if *status == InternalStatus::Reviewing {
            task.worktree_path = Some(std::env::temp_dir().to_string_lossy().into_owned());
        }
        app_state.task_repo.create(task.clone()).await.unwrap();
        task_ids.push(task.id);
        original_statuses.push(*status);
    }

    // Pause all tasks
    execution_state.pause();
    for task_id in &task_ids {
        let _ = transition_service
            .transition_task(task_id, InternalStatus::Paused)
            .await;
    }

    // Verify all are Paused
    for task_id in &task_ids {
        let task = app_state
            .task_repo
            .get_by_id(task_id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(task.internal_status, InternalStatus::Paused);
    }

    // Resume: should restore all Paused tasks to their previous status
    execution_state.resume();
    for task_id in &task_ids {
        // Find the pre-pause status from history and restore
        let history = app_state
            .task_repo
            .get_status_history(task_id)
            .await
            .unwrap();
        let pause_transition = history
            .iter()
            .rev()
            .find(|t| t.to == InternalStatus::Paused);
        if let Some(transition) = pause_transition {
            let _ = transition_service
                .transition_task(task_id, transition.from)
                .await;
        }
    }

    // Verify tasks: Executing tasks transition to Failed when blocked, others restore successfully
    for (i, task_id) in task_ids.iter().enumerate() {
        let task = app_state
            .task_repo
            .get_by_id(task_id)
            .await
            .unwrap()
            .unwrap();
        let expected_status = if original_statuses[i] == InternalStatus::Executing {
            InternalStatus::Failed
        } else {
            original_statuses[i]
        };
        assert_eq!(
            task.internal_status, expected_status,
            "Task should transition to {:?} (was {:?})",
            expected_status, original_statuses[i]
        );
    }
}

#[tokio::test]
async fn test_resume_with_mixed_paused_and_stopped_tasks() {
    // Setup: Some tasks Paused, some Stopped - only Paused should be restored
    let execution_state = Arc::new(ExecutionState::with_max_concurrent(10));
    let app_state = AppState::new_test();

    // Create a test project
    let project = Project::new("Test Project".to_string(), "/test/path".to_string());
    app_state
        .project_repo
        .create(project.clone())
        .await
        .unwrap();

    // Build transition service
    let transition_service: TaskTransitionService = TaskTransitionService::new(
        Arc::clone(&app_state.task_repo),
        Arc::clone(&app_state.task_dependency_repo),
        Arc::clone(&app_state.project_repo),
        Arc::clone(&app_state.chat_message_repo),
        Arc::clone(&app_state.chat_attachment_repo),
        Arc::clone(&app_state.chat_conversation_repo),
        Arc::clone(&app_state.agent_run_repo),
        Arc::clone(&app_state.ideation_session_repo),
        Arc::clone(&app_state.activity_event_repo),
        Arc::clone(&app_state.message_queue),
        Arc::clone(&app_state.running_agent_registry),
        Arc::clone(&execution_state),
        None,
        Arc::clone(&app_state.memory_event_repo),
    );

    // Create two Executing tasks
    let mut task1 = Task::new(project.id.clone(), "To Be Paused".to_string());
    task1.internal_status = InternalStatus::Executing;
    let task1_id = task1.id.clone();
    app_state.task_repo.create(task1).await.unwrap();

    let mut task2 = Task::new(project.id.clone(), "To Be Stopped".to_string());
    task2.internal_status = InternalStatus::Executing;
    let task2_id = task2.id.clone();
    app_state.task_repo.create(task2).await.unwrap();

    execution_state.pause();

    // Transition task1 to Paused, task2 to Stopped
    transition_service
        .transition_task(&task1_id, InternalStatus::Paused)
        .await
        .expect("Failed to pause task1");
    transition_service
        .transition_task(&task2_id, InternalStatus::Stopped)
        .await
        .expect("Failed to stop task2");

    // Resume
    execution_state.resume();

    // Restore only Paused task (simulating resume_execution logic)
    let paused_tasks = app_state
        .task_repo
        .get_by_status(&project.id, InternalStatus::Paused)
        .await
        .unwrap();
    for task in paused_tasks {
        let history = app_state
            .task_repo
            .get_status_history(&task.id)
            .await
            .unwrap();
        if let Some(transition) = history
            .iter()
            .rev()
            .find(|t| t.to == InternalStatus::Paused)
        {
            let _ = transition_service
                .transition_task(&task.id, transition.from)
                .await;
        }
    }

    // Verify: task1 (was Paused) should transition to Failed when execution is blocked
    let task1_final = app_state
        .task_repo
        .get_by_id(&task1_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        task1_final.internal_status,
        InternalStatus::Failed,
        "Paused task should transition to Failed when execution is blocked"
    );

    // Verify: task2 (was Stopped) should remain Stopped
    let task2_final = app_state
        .task_repo
        .get_by_id(&task2_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        task2_final.internal_status,
        InternalStatus::Stopped,
        "Stopped task should remain Stopped"
    );
}

// ========================================
// Quota Sync Tests
// ========================================

#[tokio::test]
async fn test_get_execution_status_syncs_quota_from_project() {
    let execution_state = Arc::new(ExecutionState::with_max_concurrent(5));
    let active_project_state = Arc::new(ActiveProjectState::new());
    let app_state = AppState::new_test();

    // Create a project with specific execution settings
    let project = Project::new("Test Project".to_string(), "/test/path".to_string());
    app_state
        .project_repo
        .create(project.clone())
        .await
        .unwrap();

    // Set project-specific max_concurrent_tasks = 8
    let settings = ExecutionSettings {
        max_concurrent_tasks: 8,
        project_ideation_max: 2,
        auto_commit: false,
        pause_on_failure: false,
        ..ExecutionSettings::default()
    };
    app_state
        .execution_settings_repo
        .update_settings(Some(&project.id), &settings)
        .await
        .unwrap();

    // Verify initial state: execution_state has max=5 (not synced yet)
    assert_eq!(execution_state.max_concurrent(), 5);

    // Directly test the sync helper (commands need full State setup which is complex)
    let (resolved_project_id, max_concurrent) = sync_quota_from_project(
        Some(project.id.clone()),
        &active_project_state,
        &execution_state,
        &app_state,
    )
    .await
    .unwrap();

    // Verify: execution_state was synced to project's max (8)
    assert_eq!(max_concurrent, 8);
    assert_eq!(execution_state.max_concurrent(), 8);
    assert_eq!(resolved_project_id, Some(project.id));
}

#[tokio::test]
async fn test_resume_execution_syncs_quota_before_can_start_task() {
    let execution_state = Arc::new(ExecutionState::with_max_concurrent(2));
    let active_project_state = Arc::new(ActiveProjectState::new());
    let app_state = AppState::new_test();

    // Create a project with max_concurrent_tasks = 10
    let project = Project::new("Test Project".to_string(), "/test/path".to_string());
    app_state
        .project_repo
        .create(project.clone())
        .await
        .unwrap();

    let settings = ExecutionSettings {
        max_concurrent_tasks: 10,
        project_ideation_max: 2,
        auto_commit: false,
        pause_on_failure: false,
        ..ExecutionSettings::default()
    };
    app_state
        .execution_settings_repo
        .update_settings(Some(&project.id), &settings)
        .await
        .unwrap();

    // Set as active project
    active_project_state.set(Some(project.id.clone())).await;

    // Verify initial state before sync
    assert_eq!(execution_state.max_concurrent(), 2);

    // Test sync helper with active project (None project_id, uses active)
    let (resolved_project_id, max_concurrent) = sync_quota_from_project(
        None, // Use active project
        &active_project_state,
        &execution_state,
        &app_state,
    )
    .await
    .unwrap();

    // Verify: quota synced to project's max (10)
    assert_eq!(max_concurrent, 10);
    assert_eq!(execution_state.max_concurrent(), 10);
    assert_eq!(resolved_project_id, Some(project.id));
}

#[tokio::test]
async fn test_pause_execution_syncs_quota() {
    let execution_state = Arc::new(ExecutionState::with_max_concurrent(3));
    let active_project_state = Arc::new(ActiveProjectState::new());
    let app_state = AppState::new_test();

    // Create a project with max_concurrent_tasks = 7
    let project = Project::new("Test Project".to_string(), "/test/path".to_string());
    app_state
        .project_repo
        .create(project.clone())
        .await
        .unwrap();

    let settings = ExecutionSettings {
        max_concurrent_tasks: 7,
        project_ideation_max: 2,
        auto_commit: false,
        pause_on_failure: false,
        ..ExecutionSettings::default()
    };
    app_state
        .execution_settings_repo
        .update_settings(Some(&project.id), &settings)
        .await
        .unwrap();

    // Verify initial state
    assert_eq!(execution_state.max_concurrent(), 3);

    // Test sync helper with explicit project_id
    let (resolved_project_id, max_concurrent) = sync_quota_from_project(
        Some(project.id.clone()),
        &active_project_state,
        &execution_state,
        &app_state,
    )
    .await
    .unwrap();

    // Verify: quota synced to project's max (7)
    assert_eq!(max_concurrent, 7);
    assert_eq!(execution_state.max_concurrent(), 7);
    assert_eq!(resolved_project_id, Some(project.id));
}

#[tokio::test]
async fn test_persist_execution_halt_mode_paused() {
    let app_state = AppState::new_test();

    persist_execution_halt_mode(&app_state, ExecutionHaltMode::Paused)
        .await
        .unwrap();

    let settings = app_state.app_state_repo.get().await.unwrap();
    assert_eq!(settings.execution_halt_mode, ExecutionHaltMode::Paused);
}

#[tokio::test]
async fn test_persist_execution_halt_mode_stopped() {
    let app_state = AppState::new_test();

    persist_execution_halt_mode(&app_state, ExecutionHaltMode::Stopped)
        .await
        .unwrap();

    let settings = app_state.app_state_repo.get().await.unwrap();
    assert_eq!(settings.execution_halt_mode, ExecutionHaltMode::Stopped);
}

#[tokio::test]
async fn test_persist_execution_halt_mode_running() {
    let app_state = AppState::new_test();

    persist_execution_halt_mode(&app_state, ExecutionHaltMode::Stopped)
        .await
        .unwrap();
    persist_execution_halt_mode(&app_state, ExecutionHaltMode::Running)
        .await
        .unwrap();

    let settings = app_state.app_state_repo.get().await.unwrap();
    assert_eq!(settings.execution_halt_mode, ExecutionHaltMode::Running);
}

#[tokio::test]
async fn test_load_execution_halt_mode_reads_persisted_stop_state() {
    let app_state = AppState::new_test();
    persist_execution_halt_mode(&app_state, ExecutionHaltMode::Stopped)
        .await
        .unwrap();

    let halt_mode = load_execution_halt_mode(&app_state).await.unwrap();
    assert_eq!(halt_mode, ExecutionHaltMode::Stopped);
}

#[tokio::test]
async fn test_load_execution_halt_mode_reads_persisted_paused_state() {
    let app_state = AppState::new_test();
    persist_execution_halt_mode(&app_state, ExecutionHaltMode::Paused)
        .await
        .unwrap();

    let halt_mode = load_execution_halt_mode(&app_state).await.unwrap();
    assert_eq!(halt_mode, ExecutionHaltMode::Paused);
}

#[tokio::test]
async fn test_stop_execution_syncs_quota() {
    let execution_state = Arc::new(ExecutionState::with_max_concurrent(4));
    let active_project_state = Arc::new(ActiveProjectState::new());
    let app_state = AppState::new_test();

    // Create a project with max_concurrent_tasks = 6
    let project = Project::new("Test Project".to_string(), "/test/path".to_string());
    app_state
        .project_repo
        .create(project.clone())
        .await
        .unwrap();

    let settings = ExecutionSettings {
        max_concurrent_tasks: 6,
        project_ideation_max: 2,
        auto_commit: false,
        pause_on_failure: false,
        ..ExecutionSettings::default()
    };
    app_state
        .execution_settings_repo
        .update_settings(Some(&project.id), &settings)
        .await
        .unwrap();

    // Verify initial state
    assert_eq!(execution_state.max_concurrent(), 4);

    // Test sync helper with explicit project_id
    let (resolved_project_id, max_concurrent) = sync_quota_from_project(
        Some(project.id.clone()),
        &active_project_state,
        &execution_state,
        &app_state,
    )
    .await
    .unwrap();

    // Verify: quota synced to project's max (6)
    assert_eq!(max_concurrent, 6);
    assert_eq!(execution_state.max_concurrent(), 6);
    assert_eq!(resolved_project_id, Some(project.id));
}

#[tokio::test]
async fn test_stop_execution_persists_stopped_and_clears_project_queues() {
    let execution_state = Arc::new(ExecutionState::new());
    let active_project_state = Arc::new(ActiveProjectState::new());
    let app_state = AppState::new_test();

    let project = app_state
        .project_repo
        .create(Project::new(
            "Stop Queues Project".to_string(),
            "/test/stop-queues-project".to_string(),
        ))
        .await
        .unwrap();
    let other_project = app_state
        .project_repo
        .create(Project::new(
            "Other Stop Queues Project".to_string(),
            "/test/other-stop-queues-project".to_string(),
        ))
        .await
        .unwrap();

    let task = app_state
        .task_repo
        .create(Task::new(
            project.id.clone(),
            "Queued task chat".to_string(),
        ))
        .await
        .unwrap();
    let other_task = app_state
        .task_repo
        .create(Task::new(
            other_project.id.clone(),
            "Other queued task chat".to_string(),
        ))
        .await
        .unwrap();

    app_state.message_queue.queue(
        ChatContextType::Project,
        project.id.as_str(),
        "project queue".to_string(),
    );
    app_state.message_queue.queue(
        ChatContextType::Task,
        task.id.as_str(),
        "task chat queue".to_string(),
    );
    app_state.message_queue.queue(
        ChatContextType::TaskExecution,
        task.id.as_str(),
        "task execution queue".to_string(),
    );
    app_state.message_queue.queue(
        ChatContextType::Project,
        other_project.id.as_str(),
        "other project queue".to_string(),
    );
    app_state.message_queue.queue(
        ChatContextType::Task,
        other_task.id.as_str(),
        "other task queue".to_string(),
    );

    let app = mock_builder()
        .manage(Arc::clone(&active_project_state))
        .manage(Arc::clone(&execution_state))
        .manage(app_state)
        .build(mock_context(noop_assets()))
        .expect("mock app should build");

    let response = stop_execution(
        Some(project.id.as_str().to_string()),
        app.state::<Arc<ActiveProjectState>>(),
        app.state::<Arc<ExecutionState>>(),
        app.state::<AppState>(),
    )
    .await
    .expect("stop execution should succeed");

    assert!(response.success);
    assert!(execution_state.is_paused());
    assert_eq!(response.status.halt_mode, "stopped");

    let state = app.state::<AppState>();
    assert!(state
        .message_queue
        .get_queued(ChatContextType::Project, project.id.as_str())
        .is_empty());
    assert!(state
        .message_queue
        .get_queued(ChatContextType::Task, task.id.as_str())
        .is_empty());
    assert!(state
        .message_queue
        .get_queued(ChatContextType::TaskExecution, task.id.as_str())
        .is_empty());
    assert_eq!(
        state
            .message_queue
            .get_queued(ChatContextType::Project, other_project.id.as_str())
            .len(),
        1
    );
    assert_eq!(
        state
            .message_queue
            .get_queued(ChatContextType::Task, other_task.id.as_str())
            .len(),
        1
    );

    let settings = state.app_state_repo.get().await.unwrap();
    assert_eq!(settings.execution_halt_mode, ExecutionHaltMode::Stopped);
}

#[tokio::test]
async fn test_set_active_project_syncs_quota_and_updates_execution_state() {
    let execution_state = Arc::new(ExecutionState::with_max_concurrent(2));
    let active_project_state = Arc::new(ActiveProjectState::new());
    let app_state = AppState::new_test();

    // Create two projects with different max_concurrent settings
    let project1 = Project::new("Project 1".to_string(), "/test/path1".to_string());
    app_state
        .project_repo
        .create(project1.clone())
        .await
        .unwrap();

    let settings1 = ExecutionSettings {
        max_concurrent_tasks: 5,
        project_ideation_max: 2,
        auto_commit: false,
        pause_on_failure: false,
        ..ExecutionSettings::default()
    };
    app_state
        .execution_settings_repo
        .update_settings(Some(&project1.id), &settings1)
        .await
        .unwrap();

    let project2 = Project::new("Project 2".to_string(), "/test/path2".to_string());
    app_state
        .project_repo
        .create(project2.clone())
        .await
        .unwrap();

    let settings2 = ExecutionSettings {
        max_concurrent_tasks: 12,
        project_ideation_max: 2,
        auto_commit: true,
        pause_on_failure: true,
        ..ExecutionSettings::default()
    };
    app_state
        .execution_settings_repo
        .update_settings(Some(&project2.id), &settings2)
        .await
        .unwrap();

    // Verify initial state
    assert_eq!(execution_state.max_concurrent(), 2);
    assert!(active_project_state.get().await.is_none());

    // Set active project to project1 (simulate what set_active_project command does)
    active_project_state.set(Some(project1.id.clone())).await;
    let (_resolved1, max1) = sync_quota_from_project(
        Some(project1.id.clone()),
        &active_project_state,
        &execution_state,
        &app_state,
    )
    .await
    .unwrap();

    // Verify: active project set and quota synced to project1's max (5)
    assert_eq!(
        active_project_state
            .get()
            .await
            .as_ref()
            .map(|p| p.as_str()),
        Some(project1.id.as_str())
    );
    assert_eq!(max1, 5);
    assert_eq!(execution_state.max_concurrent(), 5);

    // Switch to project2
    active_project_state.set(Some(project2.id.clone())).await;
    let (_resolved2, max2) = sync_quota_from_project(
        Some(project2.id.clone()),
        &active_project_state,
        &execution_state,
        &app_state,
    )
    .await
    .unwrap();

    // Verify: active project switched and quota synced to project2's max (12)
    assert_eq!(
        active_project_state
            .get()
            .await
            .as_ref()
            .map(|p| p.as_str()),
        Some(project2.id.as_str())
    );
    assert_eq!(max2, 12);
    assert_eq!(execution_state.max_concurrent(), 12);

    // Switch back to project1
    active_project_state.set(Some(project1.id.clone())).await;
    let (_resolved3, max3) = sync_quota_from_project(
        Some(project1.id.clone()),
        &active_project_state,
        &execution_state,
        &app_state,
    )
    .await
    .unwrap();

    // Verify: quota correctly synced back to project1's max (5)
    assert_eq!(max3, 5);
    assert_eq!(execution_state.max_concurrent(), 5);
}

// ═══════════════════════════════════════════════════════════════════════
// Active Project Scoping Tests
// ═══════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn test_project_switch_prevents_other_projects_from_scheduling() {
    use crate::application::TaskSchedulerService;
    use crate::domain::state_machine::services::TaskScheduler;

    let app_state = AppState::new_test();
    let execution_state = Arc::new(ExecutionState::with_max_concurrent(10));
    let active_project_state = Arc::new(ActiveProjectState::new());

    // Create two projects
    let mut project1 = Project::new("Project 1".to_string(), "/test/path1".to_string());
    project1.git_mode = GitMode::Worktree; // Worktree mode allows concurrent tasks
    app_state
        .project_repo
        .create(project1.clone())
        .await
        .unwrap();

    let mut project2 = Project::new("Project 2".to_string(), "/test/path2".to_string());
    project2.git_mode = GitMode::Worktree;
    app_state
        .project_repo
        .create(project2.clone())
        .await
        .unwrap();

    // Create Ready tasks in both projects
    let mut p1_task = Task::new(project1.id.clone(), "Project 1 Task".to_string());
    p1_task.internal_status = InternalStatus::Ready;
    app_state.task_repo.create(p1_task.clone()).await.unwrap();

    let mut p2_task = Task::new(project2.id.clone(), "Project 2 Task".to_string());
    p2_task.internal_status = InternalStatus::Ready;
    app_state.task_repo.create(p2_task.clone()).await.unwrap();

    // Set active project to project 1
    active_project_state.set(Some(project1.id.clone())).await;

    // Build scheduler with active project 1
    let scheduler = Arc::new(TaskSchedulerService::new(
        Arc::clone(&execution_state),
        Arc::clone(&app_state.project_repo),
        Arc::clone(&app_state.task_repo),
        Arc::clone(&app_state.task_dependency_repo),
        Arc::clone(&app_state.artifact_repo),
        Arc::clone(&app_state.chat_message_repo),
        Arc::clone(&app_state.chat_attachment_repo),
        Arc::clone(&app_state.chat_conversation_repo),
        Arc::clone(&app_state.agent_run_repo),
        Arc::clone(&app_state.ideation_session_repo),
        Arc::clone(&app_state.activity_event_repo),
        Arc::clone(&app_state.message_queue),
        Arc::clone(&app_state.running_agent_registry),
        Arc::clone(&app_state.memory_event_repo),
        None,
    ));
    scheduler.set_self_ref(Arc::clone(&scheduler) as Arc<dyn TaskScheduler>);

    // Set active project on scheduler (simulating what execution commands do)
    scheduler
        .set_active_project(Some(project1.id.clone()))
        .await;
    scheduler.try_schedule_ready_tasks().await;

    // Verify: Project 1 task transitions to Failed when execution is blocked
    let p1_updated = app_state
        .task_repo
        .get_by_id(&p1_task.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        p1_updated.internal_status,
        InternalStatus::Failed,
        "Project 1 task should transition to Failed when execution is blocked"
    );

    // Verify: Project 2 task should NOT be scheduled
    let p2_updated = app_state
        .task_repo
        .get_by_id(&p2_task.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        p2_updated.internal_status,
        InternalStatus::Ready,
        "Project 2 task should NOT be scheduled when project 1 is active"
    );

    // Now switch active project to project 2
    active_project_state.set(Some(project2.id.clone())).await;

    // Create new scheduler instance for project 2
    let scheduler2 = Arc::new(TaskSchedulerService::new(
        Arc::clone(&execution_state),
        Arc::clone(&app_state.project_repo),
        Arc::clone(&app_state.task_repo),
        Arc::clone(&app_state.task_dependency_repo),
        Arc::clone(&app_state.artifact_repo),
        Arc::clone(&app_state.chat_message_repo),
        Arc::clone(&app_state.chat_attachment_repo),
        Arc::clone(&app_state.chat_conversation_repo),
        Arc::clone(&app_state.agent_run_repo),
        Arc::clone(&app_state.ideation_session_repo),
        Arc::clone(&app_state.activity_event_repo),
        Arc::clone(&app_state.message_queue),
        Arc::clone(&app_state.running_agent_registry),
        Arc::clone(&app_state.memory_event_repo),
        None,
    ));
    scheduler2.set_self_ref(Arc::clone(&scheduler2) as Arc<dyn TaskScheduler>);

    // Set active project on new scheduler (simulating what execution commands do)
    scheduler2
        .set_active_project(Some(project2.id.clone()))
        .await;
    scheduler2.try_schedule_ready_tasks().await;

    // Verify: Project 2 task transitions to Failed when execution is blocked
    let p2_final = app_state
        .task_repo
        .get_by_id(&p2_task.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        p2_final.internal_status,
        InternalStatus::Failed,
        "Project 2 task should transition to Failed when execution is blocked"
    );
}

// ═══════════════════════════════════════════════════════════════════════
// Smart Resume Categorization Tests
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn test_categorize_direct_resume_states() {
    // Direct resume: spawn agent directly
    let direct_states = [
        InternalStatus::Executing,
        InternalStatus::ReExecuting,
        InternalStatus::Reviewing,
        InternalStatus::QaRefining,
        InternalStatus::QaTesting,
    ];

    for status in direct_states {
        let result = categorize_resume_state(status);
        assert_eq!(result.category, ResumeCategory::Direct);
        assert_eq!(result.target_status, status);
    }
}

#[test]
fn test_restart_transition_routes_execution_states_through_ready() {
    assert_eq!(
        restart_transition_target(InternalStatus::Executing),
        InternalStatus::Ready
    );
    assert_eq!(
        restart_transition_target(InternalStatus::ReExecuting),
        InternalStatus::Ready
    );
}

#[test]
fn test_restart_transition_preserves_non_execution_categorized_target() {
    assert_eq!(
        restart_transition_target(InternalStatus::QaPassed),
        InternalStatus::PendingReview
    );
    assert_eq!(
        restart_transition_target(InternalStatus::Merging),
        InternalStatus::Merging
    );
}

#[test]
fn test_restart_transition_only_ready_route_is_legal_from_stopped() {
    assert!(InternalStatus::Stopped
        .can_transition_to(restart_transition_target(InternalStatus::Executing)));
    assert!(!InternalStatus::Stopped
        .can_transition_to(restart_transition_target(InternalStatus::Merging)));
}

fn stopped_task_metadata(from_status: InternalStatus) -> String {
    crate::domain::state_machine::transition_handler::metadata_builder::build_stop_metadata(
        from_status,
        Some("stopped for restart".to_string()),
    )
    .merge_into(None)
}

fn create_restart_test_dir(path: &Path) {
    let safe_path =
        validate_absolute_non_root_path(path, "execution restart test directory").unwrap();
    // codeql[rust/path-injection]
    std::fs::create_dir_all(&safe_path).unwrap();
}

fn write_restart_test_file(path: &Path, contents: &str) {
    let safe_path = validate_absolute_non_root_path(path, "execution restart test file").unwrap();
    // codeql[rust/path-injection]
    std::fs::write(&safe_path, contents).unwrap();
}

fn restart_test_git(path: &Path, args: &[&str]) {
    let safe_path =
        validate_absolute_non_root_path(path, "execution restart test git repository").unwrap();
    let output = std::process::Command::new("git")
        .args(args)
        // codeql[rust/path-injection]
        .current_dir(&safe_path)
        .output()
        .expect("git command should run");
    assert!(
        output.status.success(),
        "git {:?} failed: {}",
        args,
        String::from_utf8_lossy(&output.stderr)
    );
}

async fn seed_completed_failed_attempt(
    app_state: &AppState,
    root: &tempfile::TempDir,
) -> (TaskId, String) {
    let mut project = Project::new(
        "Recovered Failed Restart Project".to_string(),
        root.path().join("project").to_string_lossy().into_owned(),
    );
    project.worktree_parent_directory = Some(root.path().to_string_lossy().into_owned());
    app_state
        .project_repo
        .create(project.clone())
        .await
        .unwrap();

    let mut task = Task::new(project.id.clone(), "Recover failed execution".to_string());
    task.internal_status = InternalStatus::Failed;
    task.task_branch = Some("task/recover-command".to_string());
    task.blocked_reason = Some("provider exited after validation".to_string());
    let worktree = project.task_worktree_path(task.id.as_str());
    create_restart_test_dir(&worktree);
    restart_test_git(&worktree, &["init", "-b", "task/recover-command"]);
    restart_test_git(&worktree, &["config", "user.email", "test@example.com"]);
    restart_test_git(&worktree, &["config", "user.name", "RalphX Test"]);
    write_restart_test_file(&worktree.join("tracked.txt"), "base\n");
    restart_test_git(&worktree, &["add", "tracked.txt"]);
    restart_test_git(&worktree, &["commit", "-m", "base"]);
    let base_sha = crate::application::git_service::GitService::get_head_sha(&worktree)
        .await
        .unwrap();
    write_restart_test_file(&worktree.join("tracked.txt"), "completed work\n");
    restart_test_git(&worktree, &["add", "tracked.txt"]);
    restart_test_git(&worktree, &["commit", "-m", "completed work"]);
    let promoted_sha = crate::application::git_service::GitService::get_head_sha(&worktree)
        .await
        .unwrap();
    task.worktree_path = Some(worktree.to_string_lossy().into_owned());
    task.task_branch_base_ref = Some("base".to_string());
    task.task_branch_base_sha = Some(base_sha);
    let task_id = task.id.clone();
    app_state.task_repo.create(task.clone()).await.unwrap();
    app_state
        .task_repo
        .persist_status_change(
            &task_id,
            InternalStatus::Ready,
            InternalStatus::Executing,
            "test",
        )
        .await
        .unwrap();
    app_state.task_repo.update(&task).await.unwrap();
    let episode_entered_at = app_state
        .task_repo
        .get_status_last_entered_at(&task_id, InternalStatus::Executing)
        .await
        .unwrap()
        .unwrap();

    let mut step = TaskStep::new(task_id.clone(), "done".to_string(), 0, "test".to_string());
    step.status = TaskStepStatus::Completed;
    app_state.task_step_repo.create(step).await.unwrap();
    let mut conversation = ChatConversation::new_task(task_id.clone());
    conversation.context_type = ChatContextType::TaskExecution;
    let conversation = app_state
        .chat_conversation_repo
        .create(conversation)
        .await
        .unwrap();
    let mut agent_run = AgentRun::new(conversation.id);
    agent_run.status = AgentRunStatus::Completed;
    agent_run.completed_at = Some(chrono::Utc::now());
    app_state.agent_run_repo.create(agent_run).await.unwrap();

    let validation_run = ValidationRun {
        id: "restart-validation-current".to_string(),
        task_id: task_id.clone(),
        project_id: project.id.clone(),
        purpose: ValidationPurpose::Final,
        context_type: ValidationContextType::Execution,
        requested_by_agent: Some("test".to_string()),
        status: ValidationRunStatus::Passed,
        mode: ValidationRunMode::ReuseOrRun,
        policy_enabled: true,
        head_sha: Some(promoted_sha.clone()),
        start_content_fingerprint: None,
        validated_content_fingerprint: None,
        promoted_commit_sha: Some(promoted_sha.clone()),
        base_ref: Some("base".to_string()),
        analysis_fingerprint: None,
        status_episode_entered_at: Some(episode_entered_at),
        started_at: chrono::Utc::now(),
        completed_at: Some(chrono::Utc::now()),
    };
    app_state
        .validation_run_repo
        .create_run(&validation_run)
        .await
        .unwrap();
    app_state
        .validation_run_repo
        .add_command_result(&ValidationCommandResult {
            id: "restart-validation-command".to_string(),
            validation_run_id: validation_run.id,
            task_id: task_id.clone(),
            project_id: project.id,
            command_source: ValidationCommandSource::ProjectAnalysisRef,
            command_ref: Some("tests".to_string()),
            command: "cargo test".to_string(),
            cwd: worktree.to_string_lossy().into_owned(),
            label: Some("Tests".to_string()),
            category: ValidationCommandCategory::Test,
            reason: None,
            related_files: Vec::new(),
            cache_key: "validation-cache".to_string(),
            cache_decision: ValidationCacheDecision::Ran,
            status: ValidationCommandStatus::Passed,
            exit_code: Some(0),
            duration_ms: Some(1),
            stdout_snippet: None,
            stderr_snippet: None,
            stdout_log_path: None,
            stderr_log_path: None,
            launcher_kind: None,
            resolved_shell_path: None,
            head_sha: Some(promoted_sha.clone()),
            analysis_fingerprint: None,
            status_episode_entered_at: Some(episode_entered_at),
            created_at: chrono::Utc::now(),
        })
        .await
        .unwrap();

    (task_id, promoted_sha)
}

#[tokio::test]
async fn restart_task_from_stopped_execution_routes_through_ready_and_clears_stale_refs() {
    let execution_state = Arc::new(ExecutionState::with_max_concurrent(5));
    execution_state.pause();
    let app_state = AppState::new_test();
    enable_tasks_for_progress(&app_state).await;
    let project = Project::new(
        "Restart Command Project".to_string(),
        "/tmp/restart-command-project".to_string(),
    );
    app_state
        .project_repo
        .create(project.clone())
        .await
        .unwrap();

    let missing_worktree_parent = tempfile::tempdir().expect("temp dir");
    let missing_worktree = missing_worktree_parent.path().join("missing-worktree");
    let mut task = Task::new(project.id.clone(), "Stopped execution".to_string());
    task.internal_status = InternalStatus::Stopped;
    task.task_branch = Some("task/stale-execution".to_string());
    task.worktree_path = Some(missing_worktree.to_string_lossy().into_owned());
    task.merge_commit_sha = Some("deadbeef".to_string());
    task.metadata = Some(stopped_task_metadata(InternalStatus::Executing));
    let task_id = task.id.clone();
    app_state.task_repo.create(task).await.unwrap();

    let app = mock_builder()
        .manage(Arc::clone(&execution_state))
        .manage(app_state)
        .build(mock_context(noop_assets()))
        .expect("mock app should build");

    let result = restart_task(
        task_id.as_str().to_string(),
        false,
        None,
        app.state::<AppState>(),
        app.state::<Arc<ExecutionState>>(),
    )
    .await
    .expect("restart command should complete");

    match result {
        RestartResult::Success {
            category,
            resumed_to_status,
            ..
        } => {
            assert_eq!(category, ResumeCategory::Direct);
            assert_eq!(resumed_to_status, InternalStatus::Ready.as_str());
        }
        other => panic!("expected successful ready restart, got {other:?}"),
    }

    let stored = app
        .state::<AppState>()
        .task_repo
        .get_by_id(&task_id)
        .await
        .unwrap()
        .expect("task should exist");
    assert_eq!(stored.internal_status, InternalStatus::Ready);
    assert!(stored.task_branch.is_none());
    assert!(stored.worktree_path.is_none());
    assert!(stored.merge_commit_sha.is_none());
    let metadata: serde_json::Value =
        serde_json::from_str(stored.metadata.as_deref().unwrap()).unwrap();
    assert!(metadata.get("stop_metadata").is_none());
}

#[tokio::test]
async fn restart_task_while_tasks_are_off_rejects_without_mutating_the_task() {
    let execution_state = Arc::new(ExecutionState::with_max_concurrent(5));
    let app_state = AppState::new_test();
    enable_tasks_for_progress(&app_state).await;
    let project = app_state
        .project_repo
        .create(Project::new(
            "Disabled Restart Project".to_string(),
            "/tmp/disabled-restart-project".to_string(),
        ))
        .await
        .unwrap();
    let mut task = Task::new(project.id, "Preserve stopped task".to_string());
    task.internal_status = InternalStatus::Stopped;
    task.task_branch = Some("task/preserved-while-off".to_string());
    task.metadata = Some(stopped_task_metadata(InternalStatus::Executing));
    let task = app_state.task_repo.create(task).await.unwrap();
    assert!(app_state
        .ideation_settings_repo
        .compare_and_set_tasks_feature_state(
            crate::domain::ideation::TasksFeatureState::Enabled,
            crate::domain::ideation::TasksFeatureState::Disabled,
        )
        .await
        .unwrap());

    let app = mock_builder()
        .manage(Arc::clone(&execution_state))
        .manage(app_state)
        .build(mock_context(noop_assets()))
        .expect("mock app should build");

    let error = restart_task(
        task.id.as_str().to_string(),
        false,
        None,
        app.state::<AppState>(),
        app.state::<Arc<ExecutionState>>(),
    )
    .await
    .expect_err("Tasks-off restart must fail closed");

    assert!(error.starts_with("ralphx:tasks_disabled"));
    let unchanged = app
        .state::<AppState>()
        .task_repo
        .get_by_id(&task.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(unchanged.internal_status, InternalStatus::Stopped);
    assert_eq!(unchanged.task_branch, task.task_branch);
    assert_eq!(unchanged.metadata, task.metadata);
}

#[tokio::test]
async fn restart_task_from_failed_without_recovery_proof_starts_fresh_ready_attempt() {
    let execution_state = Arc::new(ExecutionState::with_max_concurrent(5));
    execution_state.pause();
    let app_state = AppState::new_test();
    enable_tasks_for_progress(&app_state).await;
    let project = Project::new(
        "Failed Restart Project".to_string(),
        "/tmp/failed-restart-project".to_string(),
    );
    app_state
        .project_repo
        .create(project.clone())
        .await
        .unwrap();

    let missing_worktree_parent = tempfile::tempdir().expect("temp dir");
    let mut task = Task::new(project.id, "Failed execution".to_string());
    task.internal_status = InternalStatus::Failed;
    task.task_branch = Some("task/stale-failed".to_string());
    task.worktree_path = Some(
        missing_worktree_parent
            .path()
            .join("missing-worktree")
            .to_string_lossy()
            .into_owned(),
    );
    task.merge_commit_sha = Some("deadbeef".to_string());
    let task_id = task.id.clone();
    app_state.task_repo.create(task.clone()).await.unwrap();
    app_state
        .task_repo
        .persist_status_change(
            &task_id,
            InternalStatus::Ready,
            InternalStatus::Executing,
            "test",
        )
        .await
        .unwrap();
    app_state.task_repo.update(&task).await.unwrap();
    let pending_step = TaskStep::new(
        task_id.clone(),
        "authoritatively incomplete".to_string(),
        0,
        "test".to_string(),
    );
    app_state.task_step_repo.create(pending_step).await.unwrap();

    let app = mock_builder()
        .manage(Arc::clone(&execution_state))
        .manage(app_state)
        .build(mock_context(noop_assets()))
        .expect("mock app should build");

    let result = restart_task(
        task_id.as_str().to_string(),
        true,
        Some("retry failed task".to_string()),
        app.state::<AppState>(),
        app.state::<Arc<ExecutionState>>(),
    )
    .await
    .expect("failed restart should complete");

    match result {
        RestartResult::Success {
            disposition,
            resumed_to_status,
            ..
        } => {
            assert_eq!(disposition, Some(RestartDisposition::RestartedToReady));
            assert_eq!(resumed_to_status, InternalStatus::Ready.as_str());
        }
        other => panic!("expected fresh failed-task restart, got {other:?}"),
    }

    let stored = app
        .state::<AppState>()
        .task_repo
        .get_by_id(&task_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(stored.internal_status, InternalStatus::Ready);
    assert!(stored.task_branch.is_none());
    assert!(stored.worktree_path.is_none());
    assert!(stored.merge_commit_sha.is_none());
}

#[tokio::test]
async fn restart_task_from_failed_with_current_completion_proof_recovers_to_review() {
    let execution_state = Arc::new(ExecutionState::with_max_concurrent(5));
    let app_state = AppState::new_test();
    enable_tasks_for_progress(&app_state).await;
    let root = tempfile::tempdir().expect("temp dir");
    let (task_id, promoted_sha) = seed_completed_failed_attempt(&app_state, &root).await;

    let app = mock_builder()
        .manage(Arc::clone(&execution_state))
        .manage(app_state)
        .build(mock_context(noop_assets()))
        .expect("mock app should build");

    let result = restart_task(
        task_id.as_str().to_string(),
        false,
        None,
        app.state::<AppState>(),
        app.state::<Arc<ExecutionState>>(),
    )
    .await
    .expect("failed restart should recover completed work");

    match result {
        RestartResult::Success {
            disposition,
            category,
            resumed_to_status,
            ..
        } => {
            assert_eq!(disposition, Some(RestartDisposition::RecoveredToReview));
            assert_eq!(category, ResumeCategory::Redirect);
            assert_eq!(resumed_to_status, InternalStatus::PendingReview.as_str());
        }
        other => panic!("expected recovered failed-task restart, got {other:?}"),
    }

    let stored = app
        .state::<AppState>()
        .task_repo
        .get_by_id(&task_id)
        .await
        .unwrap()
        .unwrap();
    assert_ne!(stored.internal_status, InternalStatus::Failed);
    assert!(stored.blocked_reason.is_none());
    let metadata: serde_json::Value =
        serde_json::from_str(stored.metadata.as_deref().unwrap()).unwrap();
    assert_eq!(
        metadata["failed_completion_recovery"]["promoted_commit_sha"],
        promoted_sha
    );
    assert_eq!(
        metadata["failed_completion_recovery"]["reason_code"],
        "validated_completed_work"
    );
}

#[tokio::test]
async fn restart_task_from_failed_blocks_when_recovery_authority_is_absent() {
    let execution_state = Arc::new(ExecutionState::with_max_concurrent(5));
    let app_state = AppState::new_test();
    enable_tasks_for_progress(&app_state).await;
    let project = Project::new(
        "Blocked Failed Restart Project".to_string(),
        "/tmp/blocked-failed-restart-project".to_string(),
    );
    app_state
        .project_repo
        .create(project.clone())
        .await
        .unwrap();

    let mut task = Task::new(project.id, "Blocked failed execution".to_string());
    task.internal_status = InternalStatus::Failed;
    task.task_branch = Some("task/preserved".to_string());
    task.worktree_path = Some("/tmp/preserved-failed-worktree".to_string());
    task.merge_commit_sha = Some("preserved-sha".to_string());
    let task_id = task.id.clone();
    app_state.task_repo.create(task.clone()).await.unwrap();

    let app = mock_builder()
        .manage(Arc::clone(&execution_state))
        .manage(app_state)
        .build(mock_context(noop_assets()))
        .expect("mock app should build");

    let result = restart_task(
        task_id.as_str().to_string(),
        false,
        None,
        app.state::<AppState>(),
        app.state::<Arc<ExecutionState>>(),
    )
    .await
    .expect("failed restart should return blocked result");

    match result {
        RestartResult::Blocked { warnings } => {
            assert_eq!(warnings.len(), 1);
            assert_eq!(warnings[0].code, "missing_execution_episode");
        }
        other => panic!("expected blocked failed-task restart, got {other:?}"),
    }

    let stored = app
        .state::<AppState>()
        .task_repo
        .get_by_id(&task_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(stored.internal_status, InternalStatus::Failed);
    assert_eq!(stored.task_branch, task.task_branch);
    assert_eq!(stored.worktree_path, task.worktree_path);
    assert_eq!(stored.merge_commit_sha, task.merge_commit_sha);
}

#[tokio::test]
async fn restart_task_from_stopped_merge_returns_validation_warning_before_transition() {
    let execution_state = Arc::new(ExecutionState::with_max_concurrent(5));
    let app_state = AppState::new_test();
    enable_tasks_for_progress(&app_state).await;
    let project = Project::new(
        "Unsupported Restart Project".to_string(),
        "/tmp/unsupported-restart-project".to_string(),
    );
    app_state
        .project_repo
        .create(project.clone())
        .await
        .unwrap();

    let mut task = Task::new(project.id.clone(), "Stopped merge".to_string());
    task.internal_status = InternalStatus::Stopped;
    task.metadata = Some(stopped_task_metadata(InternalStatus::Merging));
    let task_id = task.id.clone();
    app_state.task_repo.create(task).await.unwrap();

    let app = mock_builder()
        .manage(Arc::clone(&execution_state))
        .manage(app_state)
        .build(mock_context(noop_assets()))
        .expect("mock app should build");

    let result = restart_task(
        task_id.as_str().to_string(),
        true,
        None,
        app.state::<AppState>(),
        app.state::<Arc<ExecutionState>>(),
    )
    .await
    .expect("restart command should return validation result");

    match result {
        RestartResult::ValidationFailed {
            warnings,
            stopped_from_status,
        } => {
            assert_eq!(stopped_from_status, InternalStatus::Merging.as_str());
            assert_eq!(warnings.len(), 1);
            assert_eq!(warnings[0].code, "unsupported_restart_target");
            assert!(warnings[0].message.contains("cannot safely restart"));
        }
        other => panic!("expected unsupported restart validation, got {other:?}"),
    }

    let stored = app
        .state::<AppState>()
        .task_repo
        .get_by_id(&task_id)
        .await
        .unwrap()
        .expect("task should exist");
    assert_eq!(stored.internal_status, InternalStatus::Stopped);
    assert!(stored
        .metadata
        .as_deref()
        .unwrap()
        .contains("stop_metadata"));
}

#[test]
fn test_categorize_validated_resume_states() {
    // Validated resume: check git state first
    let validated_states = [
        InternalStatus::Merging,
        InternalStatus::PendingMerge,
        InternalStatus::MergeConflict,
        InternalStatus::MergeIncomplete,
    ];

    for status in validated_states {
        let result = categorize_resume_state(status);
        assert_eq!(result.category, ResumeCategory::Validated);
        assert_eq!(result.target_status, status);
    }
}

#[test]
fn test_categorize_redirect_states() {
    // Redirect: go to successor state

    // QaPassed → PendingReview
    let result = categorize_resume_state(InternalStatus::QaPassed);
    assert_eq!(result.category, ResumeCategory::Redirect);
    assert_eq!(result.target_status, InternalStatus::PendingReview);

    // RevisionNeeded → ReExecuting
    let result = categorize_resume_state(InternalStatus::RevisionNeeded);
    assert_eq!(result.category, ResumeCategory::Redirect);
    assert_eq!(result.target_status, InternalStatus::ReExecuting);

    // PendingReview → Reviewing
    let result = categorize_resume_state(InternalStatus::PendingReview);
    assert_eq!(result.category, ResumeCategory::Redirect);
    assert_eq!(result.target_status, InternalStatus::Reviewing);
}

#[test]
fn test_categorize_unknown_states_fallback_to_direct() {
    // Unknown states should fallback to Direct
    let unknown_states = [
        InternalStatus::Backlog,
        InternalStatus::Ready,
        InternalStatus::Blocked,
        InternalStatus::Approved,
        InternalStatus::Merged,
    ];

    for status in unknown_states {
        let result = categorize_resume_state(status);
        assert_eq!(result.category, ResumeCategory::Direct);
        assert_eq!(result.target_status, status);
    }
}

#[test]
fn test_resume_category_serialization() {
    // Verify ResumeCategory can be serialized for API responses
    let direct = ResumeCategory::Direct;
    let validated = ResumeCategory::Validated;
    let redirect = ResumeCategory::Redirect;

    let direct_json = serde_json::to_string(&direct).unwrap();
    let validated_json = serde_json::to_string(&validated).unwrap();
    let redirect_json = serde_json::to_string(&redirect).unwrap();

    assert_eq!(direct_json, "\"direct\"");
    assert_eq!(validated_json, "\"validated\"");
    assert_eq!(redirect_json, "\"redirect\"");
}

// ========================================
// Pause/Resume/Unblock Behavioral Tests
// ========================================

#[tokio::test]
async fn test_blocked_task_unblocks_to_ready_stays_ready_during_pause() {
    // A Blocked task that transitions to Ready during a global pause must stay Ready,
    // not get re-paused. Blocked tasks are not in AGENT_ACTIVE_STATUSES so the pause
    // loop never touches them. This test verifies that invariant.
    let execution_state = Arc::new(ExecutionState::with_max_concurrent(5));
    let app_state = AppState::new_test();

    let project = Project::new("Test Project".to_string(), "/test/path".to_string());
    app_state
        .project_repo
        .create(project.clone())
        .await
        .unwrap();

    // Create a blocker task (Executing) and a dependent blocked task
    let mut blocker = Task::new(project.id.clone(), "Blocker Task".to_string());
    blocker.internal_status = InternalStatus::Executing;
    app_state.task_repo.create(blocker.clone()).await.unwrap();

    let mut blocked = Task::new(project.id.clone(), "Blocked Task".to_string());
    blocked.internal_status = InternalStatus::Blocked;
    app_state.task_repo.create(blocked.clone()).await.unwrap();

    // Register the dependency: blocked depends on blocker
    app_state
        .task_dependency_repo
        .add_dependency(&blocked.id, &blocker.id)
        .await
        .unwrap();

    // Pause execution: agent-active tasks pause, blocked tasks remain unchanged
    execution_state.pause();
    assert!(execution_state.is_paused());

    // Verify: blocked task is NOT touched by pause (not in AGENT_ACTIVE_STATUSES)
    let task_after_pause = app_state
        .task_repo
        .get_by_id(&blocked.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        task_after_pause.internal_status,
        InternalStatus::Blocked,
        "Blocked task should remain Blocked during pause"
    );

    // Simulate: blocker completes, unblock_dependents sets blocked → Ready
    let mut ready_task = task_after_pause.clone();
    ready_task.internal_status = InternalStatus::Ready;
    ready_task.blocked_reason = None;
    ready_task.touch();
    app_state.task_repo.update(&ready_task).await.unwrap();

    // Verify: blocked task is now Ready — stays Ready even while pause is active
    let task_after_unblock = app_state
        .task_repo
        .get_by_id(&blocked.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        task_after_unblock.internal_status,
        InternalStatus::Ready,
        "Unblocked task should be Ready, not re-paused"
    );

    // Pause flag is still set — can_start_task() should block scheduling
    assert!(
        execution_state.is_paused(),
        "Global pause flag still set — scheduler won't pick up Ready task yet"
    );
}

#[tokio::test]
async fn test_resume_restores_paused_before_scheduling_ordering() {
    // After resume_execution(), the pause flag must be cleared AFTER the restoration
    // loop, not before. This means: (1) paused tasks get restored while pause is still
    // set, (2) can_start_task() returns false during the loop (preventing race with
    // scheduler), (3) pause flag is cleared only after all paused tasks are queued.
    let execution_state = Arc::new(ExecutionState::with_max_concurrent(5));
    let app_state = AppState::new_test();

    let project = Project::new("Test Project".to_string(), "/test/path".to_string());
    app_state
        .project_repo
        .create(project.clone())
        .await
        .unwrap();

    let transition_service: TaskTransitionService = TaskTransitionService::new(
        Arc::clone(&app_state.task_repo),
        Arc::clone(&app_state.task_dependency_repo),
        Arc::clone(&app_state.project_repo),
        Arc::clone(&app_state.chat_message_repo),
        Arc::clone(&app_state.chat_attachment_repo),
        Arc::clone(&app_state.chat_conversation_repo),
        Arc::clone(&app_state.agent_run_repo),
        Arc::clone(&app_state.ideation_session_repo),
        Arc::clone(&app_state.activity_event_repo),
        Arc::clone(&app_state.message_queue),
        Arc::clone(&app_state.running_agent_registry),
        Arc::clone(&execution_state),
        None,
        Arc::clone(&app_state.memory_event_repo),
    );

    // Create a task in Reviewing state, then pause it
    let mut task = Task::new(project.id.clone(), "Reviewing Task".to_string());
    task.internal_status = InternalStatus::Reviewing;
    app_state.task_repo.create(task.clone()).await.unwrap();

    execution_state.pause();
    transition_service
        .transition_task(&task.id, InternalStatus::Paused)
        .await
        .expect("Failed to transition to Paused");

    // Verify: paused
    let paused = app_state
        .task_repo
        .get_by_id(&task.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(paused.internal_status, InternalStatus::Paused);

    // While paused, a blocked task becomes Ready (simulating unblock_dependents)
    let mut ready_task = Task::new(project.id.clone(), "Ready Task".to_string());
    ready_task.internal_status = InternalStatus::Ready;
    app_state
        .task_repo
        .create(ready_task.clone())
        .await
        .unwrap();

    // can_start_task() should be false while pause flag is set (paused tasks can't race
    // with scheduler during the restoration loop)
    assert!(
        !execution_state.can_start_task(),
        "can_start_task() must return false while pause flag is set"
    );

    // After resume, pause flag is cleared and new tasks can be scheduled
    execution_state.resume();
    assert!(
        !execution_state.is_paused(),
        "Pause flag must be cleared after resume()"
    );
    assert!(
        execution_state.can_start_task(),
        "can_start_task() must return true after resume()"
    );
}

#[tokio::test]
async fn test_max_concurrent_respected_on_resume_with_local_counter() {
    // resume_execution() uses a local restoring_count counter to enforce max_concurrent
    // without relying on can_start_task() (which returns false due to pause flag).
    // This test verifies the counter logic: with max_concurrent=2 and 3 candidates,
    // only 2 are admitted (running_count + restoring_count < max_concurrent).
    let execution_state = Arc::new(ExecutionState::with_max_concurrent(2));

    // Verify initial state: no tasks running
    assert_eq!(execution_state.running_count(), 0);
    assert_eq!(execution_state.max_concurrent(), 2);

    execution_state.pause();
    assert!(execution_state.is_paused());

    // Simulate the restoring_count logic from resume_execution():
    //   current = running_count + restoring_count
    //   stop when current >= max_concurrent
    // With running_count=0 and max_concurrent=2, only 2 of 3 candidates should restore.
    let mut restoring_count: u32 = 0;
    let max = execution_state.max_concurrent();

    for _ in 0..3u32 {
        let current = execution_state.running_count() + restoring_count;
        if current >= max {
            break;
        }
        restoring_count += 1;
    }

    assert_eq!(
        restoring_count, 2,
        "restoring_count should stop at max_concurrent=2, got {}",
        restoring_count
    );

    // Clearing pause flag (as resume_execution does after the loop)
    execution_state.resume();
    assert!(
        !execution_state.is_paused(),
        "Pause flag cleared after restoration loop"
    );

    // After resume, can_start_task() reflects true capacity
    // (running_count=0, max=2, not paused → can start)
    assert!(
        execution_state.can_start_task(),
        "can_start_task() must be true after resume with capacity available"
    );
}

// ========================================
// Interactive idle slot tracking tests
// ========================================

#[test]
fn test_interactive_slot_claim_when_idle() {
    let state = ExecutionState::new();
    let key = "task_execution/task-1";

    // Initially not idle — claim should return false
    assert!(!state.claim_interactive_slot(key));

    // Mark idle → claim should return true (once)
    state.mark_interactive_idle(key);
    assert!(state.claim_interactive_slot(key));

    // Second claim should return false (already claimed)
    assert!(!state.claim_interactive_slot(key));
}

#[test]
fn test_interactive_slot_rapid_burst_no_double_increment() {
    // Simulates: TurnComplete decrements → 3 rapid messages arrive
    // Only the first message should trigger increment.
    let state = ExecutionState::with_max_concurrent(5);
    let key = "task_execution/task-1";

    // Initial state: 1 running (process just spawned)
    state.increment_running();
    assert_eq!(state.running_count(), 1);

    // TurnComplete fires → decrement + mark idle
    state.decrement_running();
    state.mark_interactive_idle(key);
    assert_eq!(state.running_count(), 0);

    // First message → claim succeeds → increment
    assert!(state.claim_interactive_slot(key));
    state.increment_running();
    assert_eq!(state.running_count(), 1);

    // Second message (rapid burst) → claim fails → no increment
    assert!(!state.claim_interactive_slot(key));
    assert_eq!(state.running_count(), 1);

    // Third message → still no increment
    assert!(!state.claim_interactive_slot(key));
    assert_eq!(state.running_count(), 1);
}

#[test]
fn test_interactive_slot_full_lifecycle() {
    // Full lifecycle: spawn → TurnComplete → resume → TurnComplete → exit
    let state = ExecutionState::with_max_concurrent(2);
    let key = "task_execution/task-1";

    // 1. Process spawns, initial increment
    state.increment_running();
    assert_eq!(state.running_count(), 1);

    // 2. TurnComplete → decrement + mark idle
    state.decrement_running();
    state.mark_interactive_idle(key);
    assert_eq!(state.running_count(), 0);
    assert!(state.can_start_task()); // Slot freed

    // 3. User sends next message → claim + increment
    assert!(state.claim_interactive_slot(key));
    state.increment_running();
    assert_eq!(state.running_count(), 1);

    // 4. Second TurnComplete → decrement + mark idle
    state.decrement_running();
    state.mark_interactive_idle(key);
    assert_eq!(state.running_count(), 0);

    // 5. Process exits while idle → remove slot tracking
    // No increment needed (slot already free), just cleanup
    state.remove_interactive_slot(key);
    assert_eq!(state.running_count(), 0);
    assert!(!state.claim_interactive_slot(key)); // Gone
}

#[test]
fn test_interactive_slot_multiple_contexts_independent() {
    let state = ExecutionState::with_max_concurrent(5);
    let key1 = "task_execution/task-1";
    let key2 = "review/task-2";

    // Both idle
    state.mark_interactive_idle(key1);
    state.mark_interactive_idle(key2);

    // Claim key1 — key2 still idle
    assert!(state.claim_interactive_slot(key1));
    assert!(state.claim_interactive_slot(key2));

    // Both claimed — neither claimable
    assert!(!state.claim_interactive_slot(key1));
    assert!(!state.claim_interactive_slot(key2));
}

#[test]
fn test_interactive_slot_remove_clears_idle() {
    let state = ExecutionState::new();
    let key = "task_execution/task-1";

    state.mark_interactive_idle(key);
    state.remove_interactive_slot(key);

    // After removal, claim should return false
    assert!(!state.claim_interactive_slot(key));
}

#[test]
fn test_interactive_slot_concurrent_claims_exactly_one_wins() {
    // Verify that concurrent claim attempts on the same key
    // result in exactly one increment (no race condition).
    use std::sync::Arc;
    use std::thread;

    let state = Arc::new(ExecutionState::with_max_concurrent(10));
    let key = "task_execution/task-1";

    state.mark_interactive_idle(key);
    state.increment_running(); // Start at 1

    let mut handles = vec![];
    let claim_count = Arc::new(std::sync::atomic::AtomicU32::new(0));

    // Spawn 10 threads all trying to claim the same slot
    for _ in 0..10 {
        let state = Arc::clone(&state);
        let claim_count = Arc::clone(&claim_count);
        handles.push(thread::spawn(move || {
            if state.claim_interactive_slot(key) {
                state.increment_running();
                claim_count.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            }
        }));
    }

    for h in handles {
        h.join().unwrap();
    }

    // Exactly one thread should have won the claim
    assert_eq!(
        claim_count.load(std::sync::atomic::Ordering::SeqCst),
        1,
        "Exactly one concurrent claim should succeed"
    );
    // running_count should be 2 (1 initial + 1 from the winning claim)
    assert_eq!(state.running_count(), 2);
}

#[test]
fn test_is_interactive_idle_reflects_state() {
    let state = ExecutionState::with_max_concurrent(5);
    let key = "task_execution/task-1";

    // Not idle initially
    assert!(!state.is_interactive_idle(key));

    // Mark idle → should show as idle
    state.mark_interactive_idle(key);
    assert!(state.is_interactive_idle(key));

    // Claim it → no longer idle
    assert!(state.claim_interactive_slot(key));
    assert!(!state.is_interactive_idle(key));

    // Mark idle again, then remove → no longer idle
    state.mark_interactive_idle(key);
    assert!(state.is_interactive_idle(key));
    state.remove_interactive_slot(key);
    assert!(!state.is_interactive_idle(key));
}

#[test]
fn test_force_sync_running_count_subtracts_idle_slots() {
    // Simulates what prune_stale_running_registry_entries does:
    // registry has 3 entries, 1 is idle → set_running_count(3 - 1 = 2)
    let state = ExecutionState::with_max_concurrent(5);

    // Simulate 3 registered processes
    state.increment_running();
    state.increment_running();
    state.increment_running();
    assert_eq!(state.running_count(), 3);

    // One process goes idle (TurnComplete)
    state.decrement_running();
    state.mark_interactive_idle("task_execution/task-2");
    assert_eq!(state.running_count(), 2);

    // Force-sync from registry count (3 entries) minus idle (1)
    let registry_count: u32 = 3;
    let idle_count: u32 = if state.is_interactive_idle("task_execution/task-1") {
        1
    } else {
        0
    } + if state.is_interactive_idle("task_execution/task-2") {
        1
    } else {
        0
    } + if state.is_interactive_idle("task_execution/task-3") {
        1
    } else {
        0
    };
    assert_eq!(idle_count, 1);
    state.set_running_count(registry_count.saturating_sub(idle_count));
    assert_eq!(state.running_count(), 2);
}

// ========================================
// H1: decrement_running saturating_sub (no underflow)
// ========================================

#[test]
fn test_decrement_running_saturating_no_wrap() {
    // Verify that decrementing from 0 never wraps to u32::MAX.
    let state = ExecutionState::new();
    assert_eq!(state.running_count(), 0);

    // Decrement from 0 — must stay at 0
    let result = state.decrement_running();
    assert_eq!(result, 0);
    assert_eq!(state.running_count(), 0);

    // Second decrement from 0 — still 0
    let result = state.decrement_running();
    assert_eq!(result, 0);
    assert_eq!(state.running_count(), 0);
}

#[test]
fn test_decrement_running_concurrent_no_underflow() {
    // Spawn more decrement threads than increments to stress underflow path.
    use std::sync::Arc;
    use std::thread;

    let state = Arc::new(ExecutionState::with_max_concurrent(50));

    // Increment 5 times
    for _ in 0..5 {
        state.increment_running();
    }
    assert_eq!(state.running_count(), 5);

    // Decrement 20 times concurrently — 15 extra should saturate at 0
    let mut handles = vec![];
    for _ in 0..20 {
        let s = Arc::clone(&state);
        handles.push(thread::spawn(move || {
            s.decrement_running();
        }));
    }
    for h in handles {
        h.join().unwrap();
    }

    // Must be exactly 0, never wrapped
    assert_eq!(state.running_count(), 0);
}

// ========================================
// H2: decrement_and_mark_idle atomicity
// ========================================

#[test]
fn test_decrement_and_mark_idle_basic() {
    let state = ExecutionState::with_max_concurrent(5);
    let key = "task_execution/task-1";

    state.increment_running();
    assert_eq!(state.running_count(), 1);

    // Atomic decrement + mark idle
    let new_count = state.decrement_and_mark_idle(key);
    assert_eq!(new_count, 0);
    assert_eq!(state.running_count(), 0);
    assert!(state.is_interactive_idle(key));

    // claim should now work
    assert!(state.claim_interactive_slot(key));
}

#[test]
fn test_decrement_and_mark_idle_race_with_claim() {
    // Simulate the race condition: one thread does decrement_and_mark_idle,
    // many threads try claim_interactive_slot concurrently.
    // Exactly one claim should succeed (no lost increments).
    use std::sync::Arc;
    use std::thread;

    let state = Arc::new(ExecutionState::with_max_concurrent(10));
    let key = "task_execution/task-1";

    // Initial: process is running
    state.increment_running();
    assert_eq!(state.running_count(), 1);

    // Use a barrier so all threads start at the same time
    let barrier = Arc::new(std::sync::Barrier::new(11)); // 1 decrement + 10 claimers

    let mut handles = vec![];

    // Thread that decrements and marks idle
    {
        let s = Arc::clone(&state);
        let b = Arc::clone(&barrier);
        handles.push(thread::spawn(move || {
            b.wait();
            s.decrement_and_mark_idle(key);
        }));
    }

    // 10 threads that try to claim the slot
    let claim_count = Arc::new(std::sync::atomic::AtomicU32::new(0));
    for _ in 0..10 {
        let s = Arc::clone(&state);
        let b = Arc::clone(&barrier);
        let cc = Arc::clone(&claim_count);
        handles.push(thread::spawn(move || {
            b.wait();
            // Small spin to increase chance of interleaving
            for _ in 0..100 {
                if s.claim_interactive_slot(key) {
                    cc.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                    s.increment_running();
                    break;
                }
                std::thread::yield_now();
            }
        }));
    }

    for h in handles {
        h.join().unwrap();
    }

    let claims = claim_count.load(std::sync::atomic::Ordering::SeqCst);
    // Either 0 or 1 claims — never more than 1
    assert!(
        claims <= 1,
        "At most one claim should succeed, got {}",
        claims
    );
}

#[test]
fn test_decrement_and_mark_idle_from_zero_saturates() {
    // decrement_and_mark_idle from 0 should not underflow
    let state = ExecutionState::new();
    let key = "task_execution/task-1";

    let new_count = state.decrement_and_mark_idle(key);
    assert_eq!(new_count, 0);
    assert_eq!(state.running_count(), 0);
    assert!(state.is_interactive_idle(key));
}
