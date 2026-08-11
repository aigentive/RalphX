use super::*;
use crate::testing::SqliteTestDb;

#[tokio::test]
async fn test_get_default_settings() {
    let db = SqliteTestDb::new("sqlite_review_settings_repo_tests-default");
    let repo = SqliteReviewSettingsRepository::from_shared(db.shared_conn());

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
    assert!(!settings.auto_create_followup_agent_conversation);
}

#[tokio::test]
async fn test_update_settings() {
    let db = SqliteTestDb::new("sqlite_review_settings_repo_tests-update");
    let repo = SqliteReviewSettingsRepository::from_shared(db.shared_conn());

    let new_settings = ReviewSettings {
        ai_review_enabled: false,
        ai_review_auto_fix: false,
        require_fix_approval: true,
        require_human_review: true,
        require_workspace_review: false,
        autofix_workspace_review_blocking_findings: false,
        workspace_review_fixer_cycle_cap: 2,
        max_fix_attempts: 5,
        max_revision_cycles: 10,
        auto_create_followup_agent_conversation: false,
        run_task_validations: false,
    };

    let updated = repo.update_settings(&new_settings).await.unwrap();
    assert!(!updated.ai_review_enabled);
    assert!(!updated.ai_review_auto_fix);
    assert!(updated.require_fix_approval);
    assert!(updated.require_human_review);
    assert!(!updated.require_workspace_review);
    assert!(!updated.autofix_workspace_review_blocking_findings);
    assert_eq!(updated.workspace_review_fixer_cycle_cap, 2);
    assert_eq!(updated.max_fix_attempts, 5);
    assert_eq!(updated.max_revision_cycles, 10);
    assert!(!updated.auto_create_followup_agent_conversation);
    assert!(!updated.run_task_validations);

    // Verify persistence
    let retrieved = repo.get_settings().await.unwrap();
    assert!(!retrieved.ai_review_enabled);
    assert!(!retrieved.ai_review_auto_fix);
    assert!(retrieved.require_fix_approval);
    assert!(retrieved.require_human_review);
    assert!(!retrieved.require_workspace_review);
    assert!(!retrieved.autofix_workspace_review_blocking_findings);
    assert_eq!(retrieved.workspace_review_fixer_cycle_cap, 2);
    assert_eq!(retrieved.max_fix_attempts, 5);
    assert_eq!(retrieved.max_revision_cycles, 10);
    assert!(!retrieved.auto_create_followup_agent_conversation);
    assert!(!retrieved.run_task_validations);
}

#[tokio::test]
async fn test_update_max_revision_cycles() {
    let db = SqliteTestDb::new("sqlite_review_settings_repo_tests-max-cycles");
    let repo = SqliteReviewSettingsRepository::from_shared(db.shared_conn());

    let new_settings = ReviewSettings {
        max_revision_cycles: 2,
        ..Default::default()
    };

    repo.update_settings(&new_settings).await.unwrap();
    let retrieved = repo.get_settings().await.unwrap();
    assert_eq!(retrieved.max_revision_cycles, 2);
}

#[tokio::test]
async fn test_update_settings_persists_explicit_auto_followup_opt_in() {
    let db = SqliteTestDb::new("sqlite_review_settings_repo_tests-auto-followup-opt-in");
    let repo = SqliteReviewSettingsRepository::from_shared(db.shared_conn());
    let settings = ReviewSettings {
        auto_create_followup_agent_conversation: true,
        ..Default::default()
    };

    let updated = repo.update_settings(&settings).await.unwrap();
    let retrieved = repo.get_settings().await.unwrap();

    assert!(updated.auto_create_followup_agent_conversation);
    assert!(retrieved.auto_create_followup_agent_conversation);
}
