use super::*;

#[tokio::test]
async fn test_get_default_global_settings() {
    let repo = MemoryExecutionSettingsRepository::new();

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
    let repo = MemoryExecutionSettingsRepository::new();

    let new_settings = ExecutionSettings {
        max_concurrent_tasks: 4,
        project_ideation_max: 3,
        auto_commit: false,
        pause_on_failure: false,
        agent_workspace_pr_autofix_default: true,
        agent_workspace_pr_auto_merge_default: true,
    };

    let updated = repo.update_settings(None, &new_settings).await.unwrap();
    assert_eq!(updated.max_concurrent_tasks, 4);

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
    let repo = MemoryExecutionSettingsRepository::new();
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
}

#[tokio::test]
async fn test_with_settings() {
    let initial_settings = ExecutionSettings {
        max_concurrent_tasks: 8,
        project_ideation_max: 4,
        auto_commit: true,
        pause_on_failure: false,
        agent_workspace_pr_autofix_default: false,
        agent_workspace_pr_auto_merge_default: true,
    };

    let repo = MemoryExecutionSettingsRepository::with_settings(initial_settings);

    let settings = repo.get_settings(None).await.unwrap();
    assert_eq!(settings.max_concurrent_tasks, 8);
    assert_eq!(settings.project_ideation_max, 4);
    assert!(settings.auto_commit);
    assert!(!settings.pause_on_failure);
    assert!(!settings.agent_workspace_pr_autofix_default);
    assert!(settings.agent_workspace_pr_auto_merge_default);
}

#[tokio::test]
async fn test_global_execution_settings() {
    let repo = MemoryGlobalExecutionSettingsRepository::new();

    // Get default global settings
    let settings = repo.get_settings().await.unwrap();
    assert_eq!(settings.global_max_concurrent, 20);
    assert_eq!(settings.global_ideation_max, 10);
    assert!(!settings.allow_ideation_borrow_idle_execution);

    // Update global settings
    let new_settings = GlobalExecutionSettings {
        global_max_concurrent: 30,
        global_ideation_max: 6,
        allow_ideation_borrow_idle_execution: true,
    };
    let updated = repo.update_settings(&new_settings).await.unwrap();
    assert_eq!(updated.global_max_concurrent, 30);
    assert_eq!(updated.global_ideation_max, 6);
    assert!(updated.allow_ideation_borrow_idle_execution);

    // Verify persistence
    let retrieved = repo.get_settings().await.unwrap();
    assert_eq!(retrieved.global_max_concurrent, 30);
    assert_eq!(retrieved.global_ideation_max, 6);
    assert!(retrieved.allow_ideation_borrow_idle_execution);
}

#[tokio::test]
async fn test_global_max_concurrent_capped_at_50() {
    let repo = MemoryGlobalExecutionSettingsRepository::new();

    // Try to set above max
    let new_settings = GlobalExecutionSettings {
        global_max_concurrent: 100,
        global_ideation_max: 100,
        allow_ideation_borrow_idle_execution: false,
    };
    let updated = repo.update_settings(&new_settings).await.unwrap();

    // Should be clamped to 50
    assert_eq!(updated.global_max_concurrent, 50);
    assert_eq!(updated.global_ideation_max, 50);

    // Verify persistence
    let retrieved = repo.get_settings().await.unwrap();
    assert_eq!(retrieved.global_max_concurrent, 50);
    assert_eq!(retrieved.global_ideation_max, 50);
    assert!(!retrieved.allow_ideation_borrow_idle_execution);
}
