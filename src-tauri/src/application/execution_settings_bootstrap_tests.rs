use super::*;
use crate::application::AppState;
use std::sync::Arc;

#[tokio::test]
async fn test_load_or_seed_execution_settings_defaults_seeds_pristine_rows() {
    let app_state = AppState::new_test();
    let desired_project_defaults = ExecutionSettings {
        max_concurrent_tasks: 12,
        project_ideation_max: 3,
        auto_commit: false,
        pause_on_failure: false,
        agent_workspace_pr_autofix_default: true,
        agent_workspace_pr_auto_merge_default: true,
    };
    let desired_global_defaults = GlobalExecutionSettings {
        global_max_concurrent: 24,
        global_ideation_max: 6,
        allow_ideation_borrow_idle_execution: true,
    };

    let result = load_or_seed_execution_settings_defaults(
        Arc::clone(&app_state.execution_settings_repo),
        Arc::clone(&app_state.global_execution_settings_repo),
        &desired_project_defaults,
        &desired_global_defaults,
    )
    .await
    .unwrap();

    assert!(result.seeded_project_defaults);
    assert!(result.seeded_global_defaults);
    assert_eq!(result.project_defaults, desired_project_defaults);
    assert_eq!(result.global_defaults, desired_global_defaults);
}

#[tokio::test]
async fn test_load_or_seed_execution_settings_defaults_preserves_customized_rows() {
    let app_state = AppState::new_test();
    let stored_project_defaults = ExecutionSettings {
        max_concurrent_tasks: 7,
        project_ideation_max: 1,
        auto_commit: true,
        pause_on_failure: false,
        agent_workspace_pr_autofix_default: false,
        agent_workspace_pr_auto_merge_default: true,
    };
    let stored_global_defaults = GlobalExecutionSettings {
        global_max_concurrent: 18,
        global_ideation_max: 2,
        allow_ideation_borrow_idle_execution: false,
    };
    let desired_project_defaults = ExecutionSettings {
        max_concurrent_tasks: 12,
        project_ideation_max: 3,
        auto_commit: false,
        pause_on_failure: false,
        agent_workspace_pr_autofix_default: true,
        agent_workspace_pr_auto_merge_default: true,
    };
    let desired_global_defaults = GlobalExecutionSettings {
        global_max_concurrent: 24,
        global_ideation_max: 6,
        allow_ideation_borrow_idle_execution: true,
    };

    app_state
        .execution_settings_repo
        .update_settings(None, &stored_project_defaults)
        .await
        .unwrap();
    app_state
        .global_execution_settings_repo
        .update_settings(&stored_global_defaults)
        .await
        .unwrap();

    let result = load_or_seed_execution_settings_defaults(
        Arc::clone(&app_state.execution_settings_repo),
        Arc::clone(&app_state.global_execution_settings_repo),
        &desired_project_defaults,
        &desired_global_defaults,
    )
    .await
    .unwrap();

    assert!(!result.seeded_project_defaults);
    assert!(!result.seeded_global_defaults);
    assert_eq!(result.project_defaults, stored_project_defaults);
    assert_eq!(result.global_defaults, stored_global_defaults);
}

#[tokio::test]
async fn test_load_or_seed_execution_settings_defaults_can_seed_only_global_row() {
    let app_state = AppState::new_test();
    let stored_project_defaults = ExecutionSettings {
        max_concurrent_tasks: 7,
        project_ideation_max: 1,
        auto_commit: true,
        pause_on_failure: false,
        agent_workspace_pr_autofix_default: false,
        agent_workspace_pr_auto_merge_default: true,
    };
    let desired_project_defaults = ExecutionSettings {
        max_concurrent_tasks: 12,
        project_ideation_max: 3,
        auto_commit: false,
        pause_on_failure: false,
        agent_workspace_pr_autofix_default: true,
        agent_workspace_pr_auto_merge_default: true,
    };
    let desired_global_defaults = GlobalExecutionSettings {
        global_max_concurrent: 24,
        global_ideation_max: 6,
        allow_ideation_borrow_idle_execution: true,
    };

    app_state
        .execution_settings_repo
        .update_settings(None, &stored_project_defaults)
        .await
        .unwrap();

    let result = load_or_seed_execution_settings_defaults(
        Arc::clone(&app_state.execution_settings_repo),
        Arc::clone(&app_state.global_execution_settings_repo),
        &desired_project_defaults,
        &desired_global_defaults,
    )
    .await
    .unwrap();

    assert!(!result.seeded_project_defaults);
    assert!(result.seeded_global_defaults);
    assert_eq!(result.project_defaults, stored_project_defaults);
    assert_eq!(result.global_defaults, desired_global_defaults);
}
