use super::*;

#[test]
fn test_review_settings_default() {
    let settings = ReviewSettings::default();
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

#[test]
fn test_autofix_workspace_review_blocking_findings_default_helper() {
    assert!(default_autofix_workspace_review_blocking_findings());
}

#[test]
fn workspace_review_automation_override_uses_the_full_truth_table() {
    let globals_off = ReviewSettings {
        require_workspace_review: false,
        autofix_workspace_review_blocking_findings: false,
        ..Default::default()
    };
    let globals_on = ReviewSettings::default();

    assert_eq!(
        globals_off.effective_workspace_review_automation(None),
        EffectiveWorkspaceReviewAutomation {
            autofix_blocking_findings: false,
            auto_review: false,
        }
    );
    assert_eq!(
        globals_on.effective_workspace_review_automation(None),
        EffectiveWorkspaceReviewAutomation {
            autofix_blocking_findings: true,
            auto_review: true,
        }
    );
    assert_eq!(
        globals_off.effective_workspace_review_automation(Some(true)),
        EffectiveWorkspaceReviewAutomation {
            autofix_blocking_findings: true,
            auto_review: true,
        }
    );
    assert_eq!(
        globals_on.effective_workspace_review_automation(Some(false)),
        EffectiveWorkspaceReviewAutomation {
            autofix_blocking_findings: false,
            auto_review: false,
        }
    );
    assert_eq!(
        globals_off.effective_workspace_review_automation(Some(false)),
        EffectiveWorkspaceReviewAutomation {
            autofix_blocking_findings: false,
            auto_review: false,
        }
    );
}

#[test]
fn test_run_task_validations_default_helper() {
    assert!(default_run_task_validations());
}

#[test]
fn test_review_settings_ai_disabled() {
    let settings = ReviewSettings::ai_disabled();
    assert!(!settings.ai_review_enabled);
    // Other defaults preserved
    assert!(settings.ai_review_auto_fix);
    assert_eq!(settings.max_fix_attempts, 3);
}

#[test]
fn test_review_settings_with_human_review() {
    let settings = ReviewSettings::with_human_review();
    assert!(settings.require_human_review);
    // Other defaults preserved
    assert!(settings.ai_review_enabled);
}

#[test]
fn test_review_settings_with_fix_approval() {
    let settings = ReviewSettings::with_fix_approval();
    assert!(settings.require_fix_approval);
}

#[test]
fn test_review_settings_with_max_attempts() {
    let settings = ReviewSettings::with_max_attempts(5);
    assert_eq!(settings.max_fix_attempts, 5);
}

#[test]
fn test_should_run_ai_review() {
    let settings = ReviewSettings::default();
    assert!(settings.should_run_ai_review());

    let settings = ReviewSettings::ai_disabled();
    assert!(!settings.should_run_ai_review());
}

#[test]
fn test_should_auto_create_fix() {
    let settings = ReviewSettings::default();
    assert!(settings.should_auto_create_fix());

    let settings = ReviewSettings {
        ai_review_auto_fix: false,
        ..Default::default()
    };
    assert!(!settings.should_auto_create_fix());
}

#[test]
fn test_needs_human_review() {
    let settings = ReviewSettings::default();
    assert!(!settings.needs_human_review());

    let settings = ReviewSettings::with_human_review();
    assert!(settings.needs_human_review());
}

#[test]
fn test_needs_fix_approval() {
    let settings = ReviewSettings::default();
    assert!(!settings.needs_fix_approval());

    let settings = ReviewSettings::with_fix_approval();
    assert!(settings.needs_fix_approval());
}

#[test]
fn test_exceeded_max_attempts() {
    let settings = ReviewSettings::default();
    assert!(!settings.exceeded_max_attempts(0));
    assert!(!settings.exceeded_max_attempts(1));
    assert!(!settings.exceeded_max_attempts(2));
    assert!(settings.exceeded_max_attempts(3));
    assert!(settings.exceeded_max_attempts(5));

    let settings = ReviewSettings::with_max_attempts(1);
    assert!(!settings.exceeded_max_attempts(0));
    assert!(settings.exceeded_max_attempts(1));
}

#[test]
fn test_review_settings_serialize() {
    let settings = ReviewSettings::default();
    let json = serde_json::to_string(&settings).unwrap();
    assert!(json.contains("\"ai_review_enabled\":true"));
    assert!(json.contains("\"ai_review_auto_fix\":true"));
    assert!(json.contains("\"require_fix_approval\":false"));
    assert!(json.contains("\"require_human_review\":false"));
    assert!(json.contains("\"require_workspace_review\":true"));
    assert!(json.contains("\"autofix_workspace_review_blocking_findings\":true"));
    assert!(json.contains("\"workspace_review_fixer_cycle_cap\":3"));
    assert!(json.contains("\"max_fix_attempts\":3"));
    assert!(json.contains("\"auto_create_followup_agent_conversation\":false"));
    assert!(json.contains("\"run_task_validations\":true"));
}

#[test]
fn test_review_settings_deserialize() {
    let json = r#"{
        "ai_review_enabled": false,
        "ai_review_auto_fix": false,
        "require_fix_approval": true,
        "require_human_review": true,
        "require_workspace_review": false,
        "autofix_workspace_review_blocking_findings": false,
        "max_fix_attempts": 5,
        "max_revision_cycles": 8,
        "auto_create_followup_agent_conversation": false,
        "run_task_validations": false
    }"#;
    let settings: ReviewSettings = serde_json::from_str(json).unwrap();
    assert!(!settings.ai_review_enabled);
    assert!(!settings.ai_review_auto_fix);
    assert!(settings.require_fix_approval);
    assert!(settings.require_human_review);
    assert!(!settings.require_workspace_review);
    assert!(!settings.autofix_workspace_review_blocking_findings);
    assert_eq!(settings.workspace_review_fixer_cycle_cap, 3);
    assert_eq!(settings.max_fix_attempts, 5);
    assert_eq!(settings.max_revision_cycles, 8);
    assert!(!settings.auto_create_followup_agent_conversation);
    assert!(!settings.run_task_validations);
}

#[test]
fn test_review_settings_roundtrip() {
    let original = ReviewSettings {
        ai_review_enabled: true,
        ai_review_auto_fix: false,
        require_fix_approval: true,
        require_human_review: false,
        require_workspace_review: false,
        autofix_workspace_review_blocking_findings: false,
        workspace_review_fixer_cycle_cap: 2,
        max_fix_attempts: 7,
        max_revision_cycles: 8,
        auto_create_followup_agent_conversation: false,
        run_task_validations: false,
    };
    let json = serde_json::to_string(&original).unwrap();
    let deserialized: ReviewSettings = serde_json::from_str(&json).unwrap();
    assert_eq!(original, deserialized);
}

#[test]
fn test_review_settings_partial_json_with_defaults() {
    // Test that serde can handle partial JSON with defaults
    let json = r#"{
        "ai_review_enabled": true,
        "ai_review_auto_fix": true,
        "require_fix_approval": false,
        "require_human_review": false,
        "max_fix_attempts": 3,
        "max_revision_cycles": 5
    }"#;
    let settings: ReviewSettings = serde_json::from_str(json).unwrap();
    assert_eq!(settings, ReviewSettings::default());
    assert!(settings.require_workspace_review);
    assert!(settings.autofix_workspace_review_blocking_findings);
    assert!(settings.run_task_validations);
    assert!(!settings.auto_create_followup_agent_conversation);
}

#[test]
fn test_review_settings_deserialize_preserves_explicit_auto_followup_opt_in() {
    let mut json = serde_json::to_value(ReviewSettings::default()).unwrap();
    json["auto_create_followup_agent_conversation"] = serde_json::Value::Bool(true);

    let settings: ReviewSettings = serde_json::from_value(json).unwrap();

    assert!(settings.auto_create_followup_agent_conversation);
}

#[test]
fn test_exceeded_max_revisions() {
    let settings = ReviewSettings::default();
    assert!(!settings.exceeded_max_revisions(0));
    assert!(!settings.exceeded_max_revisions(1));
    assert!(!settings.exceeded_max_revisions(4));
    assert!(settings.exceeded_max_revisions(5));
    assert!(settings.exceeded_max_revisions(10));

    let settings = ReviewSettings {
        max_revision_cycles: 2,
        ..Default::default()
    };
    assert!(!settings.exceeded_max_revisions(0));
    assert!(!settings.exceeded_max_revisions(1));
    assert!(settings.exceeded_max_revisions(2));
}
