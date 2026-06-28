use super::*;

#[tokio::test]
async fn test_get_default_settings() {
    let repo = MemoryReviewSettingsRepository::new();

    let settings = repo.get_settings().await.unwrap();
    assert!(settings.ai_review_enabled);
    assert!(settings.ai_review_auto_fix);
    assert!(!settings.require_fix_approval);
    assert!(!settings.require_human_review);
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
        max_fix_attempts: 7,
        max_revision_cycles: 10,
        auto_create_followup_agent_conversation: false,
    };

    let updated = repo.update_settings(&new_settings).await.unwrap();
    assert!(!updated.ai_review_enabled);
    assert_eq!(updated.max_revision_cycles, 10);
    assert!(!updated.auto_create_followup_agent_conversation);

    // Verify persistence
    let retrieved = repo.get_settings().await.unwrap();
    assert!(!retrieved.ai_review_enabled);
    assert!(retrieved.require_fix_approval);
    assert_eq!(retrieved.max_revision_cycles, 10);
}

#[tokio::test]
async fn test_with_settings() {
    let initial_settings = ReviewSettings {
        ai_review_enabled: false,
        ai_review_auto_fix: false,
        require_fix_approval: true,
        require_human_review: true,
        max_fix_attempts: 2,
        max_revision_cycles: 3,
        auto_create_followup_agent_conversation: false,
    };

    let repo = MemoryReviewSettingsRepository::with_settings(initial_settings);

    let settings = repo.get_settings().await.unwrap();
    assert!(!settings.ai_review_enabled);
    assert!(settings.require_fix_approval);
    assert_eq!(settings.max_revision_cycles, 3);
    assert!(!settings.auto_create_followup_agent_conversation);
}
