use super::*;
use crate::testing::SqliteTestDb;

#[tokio::test]
async fn test_get_default_global_settings() {
    let db = SqliteTestDb::new("sqlite_execution_settings_repo_tests-global-defaults");
    let repo = SqliteExecutionSettingsRepository::from_shared(db.shared_conn());

    // Get global defaults (project_id = None)
    let settings = repo.get_settings(None).await.unwrap();
    assert_eq!(settings.max_concurrent_tasks, 10);
    assert_eq!(settings.project_ideation_max, 5);
    assert!(settings.auto_commit);
    assert!(settings.pause_on_failure);
    assert!(!settings.agent_workspace_pr_autofix_default);
    assert!(!settings.agent_workspace_pr_auto_merge_default);
}

#[tokio::test]
async fn test_update_global_settings() {
    let db = SqliteTestDb::new("sqlite_execution_settings_repo_tests-update-global");
    let repo = SqliteExecutionSettingsRepository::from_shared(db.shared_conn());

    let new_settings = ExecutionSettings {
        max_concurrent_tasks: 4,
        project_ideation_max: 3,
        auto_commit: false,
        pause_on_failure: false,
        agent_workspace_pr_autofix_default: true,
        agent_workspace_pr_auto_merge_default: true,
    };

    // Update global defaults
    let updated = repo.update_settings(None, &new_settings).await.unwrap();
    assert_eq!(updated.max_concurrent_tasks, 4);
    assert_eq!(updated.project_ideation_max, 3);
    assert!(!updated.auto_commit);
    assert!(!updated.pause_on_failure);
    assert!(updated.agent_workspace_pr_autofix_default);
    assert!(updated.agent_workspace_pr_auto_merge_default);

    // Verify persistence
    let retrieved = repo.get_settings(None).await.unwrap();
    assert_eq!(retrieved.max_concurrent_tasks, 4);
    assert_eq!(retrieved.project_ideation_max, 3);
    assert!(!retrieved.auto_commit);
    assert!(!retrieved.pause_on_failure);
    assert!(retrieved.agent_workspace_pr_autofix_default);
    assert!(retrieved.agent_workspace_pr_auto_merge_default);
}

#[tokio::test]
async fn test_per_project_settings() {
    let db = SqliteTestDb::new("sqlite_execution_settings_repo_tests-per-project");
    let repo = SqliteExecutionSettingsRepository::from_shared(db.shared_conn());

    let project_id = ProjectId::from_string("test-project-123".to_string());

    // Initially, get_settings for a project should return global defaults
    let settings = repo.get_settings(Some(&project_id)).await.unwrap();
    assert_eq!(settings.max_concurrent_tasks, 10); // global default
    assert_eq!(settings.project_ideation_max, 5);

    // Create project-specific settings
    let project_settings = ExecutionSettings {
        max_concurrent_tasks: 5,
        project_ideation_max: 1,
        auto_commit: false,
        pause_on_failure: true,
        agent_workspace_pr_autofix_default: true,
        agent_workspace_pr_auto_merge_default: false,
    };

    repo.update_settings(Some(&project_id), &project_settings)
        .await
        .unwrap();

    // Now get_settings should return project-specific values
    let retrieved = repo.get_settings(Some(&project_id)).await.unwrap();
    assert_eq!(retrieved.max_concurrent_tasks, 5);
    assert_eq!(retrieved.project_ideation_max, 1);
    assert!(!retrieved.auto_commit);
    assert!(retrieved.pause_on_failure);
    assert!(retrieved.agent_workspace_pr_autofix_default);
    assert!(!retrieved.agent_workspace_pr_auto_merge_default);

    // Global settings should remain unchanged
    let global = repo.get_settings(None).await.unwrap();
    assert_eq!(global.max_concurrent_tasks, 10);
    assert_eq!(global.project_ideation_max, 5);
    assert!(!global.agent_workspace_pr_autofix_default);
    assert!(!global.agent_workspace_pr_auto_merge_default);
}

#[tokio::test]
async fn test_global_execution_settings() {
    let db = SqliteTestDb::new("sqlite_execution_settings_repo_tests-global");
    let repo = SqliteGlobalExecutionSettingsRepository::from_shared(db.shared_conn());

    // Get default global settings
    let settings = repo.get_settings().await.unwrap();
    assert_eq!(settings.global_max_concurrent, 20);
    assert_eq!(settings.workspace_max_concurrent, 10);
    assert_eq!(settings.global_ideation_max, 10);
    assert!(!settings.allow_ideation_borrow_idle_execution);

    // Update global settings
    let new_settings = GlobalExecutionSettings {
        global_max_concurrent: 30,
        workspace_max_concurrent: 8,
        global_ideation_max: 6,
        allow_ideation_borrow_idle_execution: true,
    };
    let updated = repo.update_settings(&new_settings).await.unwrap();
    assert_eq!(updated.global_max_concurrent, 30);
    assert_eq!(updated.workspace_max_concurrent, 8);
    assert_eq!(updated.global_ideation_max, 6);
    assert!(updated.allow_ideation_borrow_idle_execution);

    // Verify persistence
    let retrieved = repo.get_settings().await.unwrap();
    assert_eq!(retrieved.global_max_concurrent, 30);
    assert_eq!(retrieved.workspace_max_concurrent, 8);
    assert_eq!(retrieved.global_ideation_max, 6);
    assert!(retrieved.allow_ideation_borrow_idle_execution);
}

#[tokio::test]
async fn test_global_max_concurrent_capped_at_50() {
    let db = SqliteTestDb::new("sqlite_execution_settings_repo_tests-global-cap");
    let repo = SqliteGlobalExecutionSettingsRepository::from_shared(db.shared_conn());

    // Try to set above max
    let new_settings = GlobalExecutionSettings {
        global_max_concurrent: 100,
        workspace_max_concurrent: 100,
        global_ideation_max: 100,
        allow_ideation_borrow_idle_execution: false,
    };
    let updated = repo.update_settings(&new_settings).await.unwrap();

    // Should be clamped to 50
    assert_eq!(updated.global_max_concurrent, 50);
    assert_eq!(updated.workspace_max_concurrent, 50);
    assert_eq!(updated.global_ideation_max, 50);

    // Verify persistence
    let retrieved = repo.get_settings().await.unwrap();
    assert_eq!(retrieved.global_max_concurrent, 50);
    assert_eq!(retrieved.workspace_max_concurrent, 50);
    assert_eq!(retrieved.global_ideation_max, 50);
    assert!(!retrieved.allow_ideation_borrow_idle_execution);
}

#[tokio::test]
async fn test_shared_connection() {
    let db = SqliteTestDb::new("sqlite_execution_settings_repo_tests-shared");
    let shared_conn = db.shared_conn();

    let repo = SqliteExecutionSettingsRepository::from_shared(Arc::clone(&shared_conn));

    let settings = repo.get_settings(None).await.unwrap();
    assert_eq!(settings.max_concurrent_tasks, 10);
}

#[tokio::test]
async fn test_global_execution_settings_are_not_per_project() {
    // Verifies that SqliteGlobalExecutionSettingsRepository manages a single shared row,
    // not per-project rows — changing global settings affects all consumers.
    let db = SqliteTestDb::new("sqlite_execution_settings_repo_tests-global-shared");
    let shared_conn = db.shared_conn();

    let repo_a = SqliteGlobalExecutionSettingsRepository::from_shared(Arc::clone(&shared_conn));
    let repo_b = SqliteGlobalExecutionSettingsRepository::from_shared(Arc::clone(&shared_conn));

    // Update via repo_a
    repo_a
        .update_settings(&GlobalExecutionSettings {
            global_max_concurrent: 25,
            workspace_max_concurrent: 9,
            global_ideation_max: 5,
            allow_ideation_borrow_idle_execution: true,
        })
        .await
        .unwrap();

    // repo_b (same underlying storage) should see the same value
    let settings_b = repo_b.get_settings().await.unwrap();
    assert_eq!(settings_b.global_max_concurrent, 25);
    assert_eq!(settings_b.workspace_max_concurrent, 9);
    assert_eq!(settings_b.global_ideation_max, 5);
    assert!(settings_b.allow_ideation_borrow_idle_execution);
}

#[tokio::test]
async fn test_project_settings_do_not_bleed_across_projects() {
    let db = SqliteTestDb::new("sqlite_execution_settings_repo_tests-projects");
    let repo = SqliteExecutionSettingsRepository::from_shared(db.shared_conn());

    let project_x = ProjectId::from_string("project-x".to_string());
    let project_y = ProjectId::from_string("project-y".to_string());

    // Set distinct settings for each project
    repo.update_settings(
        Some(&project_x),
        &ExecutionSettings {
            max_concurrent_tasks: 3,
            project_ideation_max: 1,
            auto_commit: false,
            pause_on_failure: false,
            agent_workspace_pr_autofix_default: true,
            agent_workspace_pr_auto_merge_default: false,
        },
    )
    .await
    .unwrap();

    repo.update_settings(
        Some(&project_y),
        &ExecutionSettings {
            max_concurrent_tasks: 7,
            project_ideation_max: 2,
            auto_commit: true,
            pause_on_failure: true,
            agent_workspace_pr_autofix_default: false,
            agent_workspace_pr_auto_merge_default: true,
        },
    )
    .await
    .unwrap();

    let x_settings = repo.get_settings(Some(&project_x)).await.unwrap();
    let y_settings = repo.get_settings(Some(&project_y)).await.unwrap();

    assert_eq!(x_settings.max_concurrent_tasks, 3);
    assert_eq!(y_settings.max_concurrent_tasks, 7);
    assert_eq!(x_settings.project_ideation_max, 1);
    assert_eq!(y_settings.project_ideation_max, 2);
    assert!(!x_settings.auto_commit);
    assert!(y_settings.auto_commit);
    assert!(x_settings.agent_workspace_pr_autofix_default);
    assert!(!x_settings.agent_workspace_pr_auto_merge_default);
    assert!(!y_settings.agent_workspace_pr_autofix_default);
    assert!(y_settings.agent_workspace_pr_auto_merge_default);
}
