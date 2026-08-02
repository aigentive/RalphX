use super::*;

#[tokio::test]
async fn test_get_default_settings() {
    let repo = MemoryReviewSettingsRepository::new();

    let settings = repo.get_settings().await.unwrap();
    assert!(settings.ai_review_enabled);
    assert!(settings.ai_review_auto_fix);
    assert!(!settings.require_fix_approval);
    assert!(!settings.require_human_review);
    assert!(settings.require_workspace_review);
    assert!(settings.autofix_workspace_review_blocking_findings);
    assert_eq!(settings.workspace_review_fixer_cycle_cap, 3);
    assert!(settings.run_task_validations);
    assert_eq!(settings.max_fix_attempts, 3);
    assert_eq!(settings.max_revision_cycles, 5);
}

#[tokio::test]
async fn test_update_settings() {
    let repo = MemoryReviewSettingsRepository::new();

    let new_settings = ReviewSettings {
        ai_review_enabled: false,
        ai_review_auto_fix: false,
        require_fix_approval: true,
        require_human_review: true,
        require_workspace_review: false,
        autofix_workspace_review_blocking_findings: false,
        workspace_review_fixer_cycle_cap: 2,
        max_fix_attempts: 7,
        max_revision_cycles: 10,
        auto_create_followup_agent_conversation: false,
        run_task_validations: false,
    };

    let updated = repo.update_settings(&new_settings).await.unwrap();
    assert!(!updated.ai_review_enabled);
    assert!(!updated.require_workspace_review);
    assert!(!updated.autofix_workspace_review_blocking_findings);
    assert_eq!(updated.workspace_review_fixer_cycle_cap, 2);
    assert_eq!(updated.max_revision_cycles, 10);
    assert!(!updated.auto_create_followup_agent_conversation);
    assert!(!updated.run_task_validations);

    // Verify persistence
    let retrieved = repo.get_settings().await.unwrap();
    assert!(!retrieved.ai_review_enabled);
    assert!(retrieved.require_fix_approval);
    assert_eq!(retrieved.max_revision_cycles, 10);
    assert!(!retrieved.run_task_validations);
}

#[tokio::test]
async fn test_with_settings() {
    let initial_settings = ReviewSettings {
        ai_review_enabled: false,
        ai_review_auto_fix: false,
        require_fix_approval: true,
        require_human_review: true,
        require_workspace_review: false,
        autofix_workspace_review_blocking_findings: false,
        workspace_review_fixer_cycle_cap: 1,
        max_fix_attempts: 2,
        max_revision_cycles: 3,
        auto_create_followup_agent_conversation: false,
        run_task_validations: false,
    };

    let repo = MemoryReviewSettingsRepository::with_settings(initial_settings);

    let settings = repo.get_settings().await.unwrap();
    assert!(!settings.ai_review_enabled);
    assert!(settings.require_fix_approval);
    assert!(!settings.require_workspace_review);
    assert!(!settings.autofix_workspace_review_blocking_findings);
    assert_eq!(settings.workspace_review_fixer_cycle_cap, 1);
    assert_eq!(settings.max_revision_cycles, 3);
    assert!(!settings.auto_create_followup_agent_conversation);
    assert!(!settings.run_task_validations);
}
