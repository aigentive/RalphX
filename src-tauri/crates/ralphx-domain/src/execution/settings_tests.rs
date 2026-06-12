use super::*;

#[test]
fn test_execution_settings_default() {
    let settings = ExecutionSettings::default();
    assert_eq!(settings.max_concurrent_tasks, 10);
    assert_eq!(settings.project_ideation_max, 5);
    assert!(settings.auto_commit);
    assert!(settings.pause_on_failure);
    assert!(!settings.agent_workspace_pr_autofix_default);
    assert!(!settings.agent_workspace_pr_auto_merge_default);
}

#[test]
fn test_execution_settings_serialization() {
    let settings = ExecutionSettings {
        max_concurrent_tasks: 4,
        project_ideation_max: 3,
        auto_commit: false,
        pause_on_failure: false,
        agent_workspace_pr_autofix_default: true,
        agent_workspace_pr_auto_merge_default: true,
    };

    let json = serde_json::to_string(&settings).unwrap();
    let deserialized: ExecutionSettings = serde_json::from_str(&json).unwrap();

    assert_eq!(deserialized.max_concurrent_tasks, 4);
    assert_eq!(deserialized.project_ideation_max, 3);
    assert!(!deserialized.auto_commit);
    assert!(!deserialized.pause_on_failure);
    assert!(deserialized.agent_workspace_pr_autofix_default);
    assert!(deserialized.agent_workspace_pr_auto_merge_default);
}

#[test]
fn test_execution_settings_clone() {
    let settings = ExecutionSettings {
        max_concurrent_tasks: 3,
        project_ideation_max: 1,
        auto_commit: true,
        pause_on_failure: false,
        agent_workspace_pr_autofix_default: false,
        agent_workspace_pr_auto_merge_default: true,
    };

    let cloned = settings.clone();
    assert_eq!(cloned, settings);
}

// Phase 82: GlobalExecutionSettings tests

#[test]
fn test_global_execution_settings_default() {
    let settings = GlobalExecutionSettings::default();
    assert_eq!(settings.global_max_concurrent, 20);
    assert_eq!(settings.global_ideation_max, 10);
    assert!(!settings.allow_ideation_borrow_idle_execution);
}

#[test]
fn test_global_execution_settings_validate_within_range() {
    let settings = GlobalExecutionSettings {
        global_max_concurrent: 30,
        global_ideation_max: 6,
        allow_ideation_borrow_idle_execution: true,
    };
    let validated = settings.validate();
    assert_eq!(validated.global_max_concurrent, 30);
    assert_eq!(validated.global_ideation_max, 6);
    assert!(validated.allow_ideation_borrow_idle_execution);
}

#[test]
fn test_global_execution_settings_validate_clamped_to_max() {
    let settings = GlobalExecutionSettings {
        global_max_concurrent: 100,
        global_ideation_max: 100,
        allow_ideation_borrow_idle_execution: false,
    };
    let validated = settings.validate();
    assert_eq!(
        validated.global_max_concurrent,
        GlobalExecutionSettings::MAX_ALLOWED
    );
    assert_eq!(
        validated.global_ideation_max,
        GlobalExecutionSettings::MAX_ALLOWED
    );
}

#[test]
fn test_global_execution_settings_validate_clamped_to_min() {
    let settings = GlobalExecutionSettings {
        global_max_concurrent: 0,
        global_ideation_max: 0,
        allow_ideation_borrow_idle_execution: false,
    };
    let validated = settings.validate();
    assert_eq!(validated.global_max_concurrent, 1);
    assert_eq!(validated.global_ideation_max, 1);
}
