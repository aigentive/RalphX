use std::str::FromStr;

use super::learned_skill::{
    ProjectSkillId, ProjectSkillLifecycleStatus, SkillUsageEventId, SkillUsageInjectionKind,
    TaskOutcomeClass, TaskOutcomeId, TaskOutcomeSource, TaskOutcomeStatus,
};

#[test]
fn learned_skill_ids_preserve_explicit_values() {
    let task_outcome_id = TaskOutcomeId::from_string("outcome-1");
    let project_skill_id = ProjectSkillId::from_string("skill-1");
    let usage_event_id = SkillUsageEventId::from_string("usage-1");

    assert_eq!(task_outcome_id.as_str(), "outcome-1");
    assert_eq!(project_skill_id.as_str(), "skill-1");
    assert_eq!(usage_event_id.as_str(), "usage-1");
}

#[test]
fn learned_skill_ids_generate_non_empty_defaults() {
    assert!(!TaskOutcomeId::new().as_str().is_empty());
    assert!(!ProjectSkillId::new().as_str().is_empty());
    assert!(!SkillUsageEventId::new().as_str().is_empty());
}

#[test]
fn task_outcome_status_round_trips_snake_case_values() {
    let cases = [
        ("unknown", TaskOutcomeStatus::Unknown),
        ("eligible", TaskOutcomeStatus::Eligible),
        ("ineligible", TaskOutcomeStatus::Ineligible),
        ("succeeded", TaskOutcomeStatus::Succeeded),
        ("failed", TaskOutcomeStatus::Failed),
    ];

    for (value, expected) in cases {
        assert_eq!(TaskOutcomeStatus::from_str(value).unwrap(), expected);
        assert_eq!(expected.to_string(), value);
    }

    assert!(TaskOutcomeStatus::from_str("pending").is_err());
}

#[test]
fn project_skill_lifecycle_status_round_trips_snake_case_values() {
    let cases = [
        ("staged", ProjectSkillLifecycleStatus::Staged),
        ("approved", ProjectSkillLifecycleStatus::Approved),
        ("rejected", ProjectSkillLifecycleStatus::Rejected),
        ("stale", ProjectSkillLifecycleStatus::Stale),
        ("archived", ProjectSkillLifecycleStatus::Archived),
        ("retired", ProjectSkillLifecycleStatus::Retired),
    ];

    for (value, expected) in cases {
        assert_eq!(
            ProjectSkillLifecycleStatus::from_str(value).unwrap(),
            expected
        );
        assert_eq!(expected.to_string(), value);
    }

    assert!(ProjectSkillLifecycleStatus::from_str("draft").is_err());
}

#[test]
fn task_outcome_sources_round_trip_exact_live_and_compatibility_values() {
    let live_cases = [
        ("agent_session", TaskOutcomeSource::AgentSession),
        ("agent_workspace", TaskOutcomeSource::AgentWorkspace),
        ("agent_workspace_pr", TaskOutcomeSource::AgentWorkspacePr),
        ("github_pr_review", TaskOutcomeSource::GithubPrReview),
        ("agent_conversation", TaskOutcomeSource::AgentConversation),
        ("review", TaskOutcomeSource::Review),
        ("git_commit_history", TaskOutcomeSource::GitCommitHistory),
        ("github_pr_history", TaskOutcomeSource::GithubPrHistory),
        ("plan_mode", TaskOutcomeSource::PlanMode),
        ("merge", TaskOutcomeSource::Merge),
        ("merge_validation", TaskOutcomeSource::MergeValidation),
        ("verification", TaskOutcomeSource::Verification),
    ];

    for (value, source) in live_cases {
        assert_eq!(TaskOutcomeSource::from_str(value).unwrap(), source);
        assert_eq!(source.to_string(), value);
        assert!(source.is_live());
    }

    let compatibility = TaskOutcomeSource::from_str("task_pipeline").unwrap();
    assert_eq!(compatibility, TaskOutcomeSource::TaskPipeline);
    assert_eq!(compatibility.to_string(), "task_pipeline");
    assert!(!compatibility.is_live());
    assert!(TaskOutcomeSource::from_str("qa").is_err());
}

#[test]
fn task_outcome_classes_preserve_known_unknown_empty_and_null_values() {
    let known = TaskOutcomeClass::from("merge_completed");
    assert_eq!(known, TaskOutcomeClass::MergeCompleted);
    assert_eq!(known.as_str(), "merge_completed");

    let unknown = TaskOutcomeClass::from("future_failure_class");
    assert_eq!(
        unknown,
        TaskOutcomeClass::Other("future_failure_class".to_string())
    );
    assert_eq!(unknown.as_str(), "future_failure_class");

    let empty = TaskOutcomeClass::from("");
    assert_eq!(empty, TaskOutcomeClass::Other(String::new()));
    assert_eq!(empty.as_str(), "");

    let optional: Option<TaskOutcomeClass> = None;
    assert!(optional.is_none());
}

#[test]
fn plan_revision_requested_class_round_trips_canonically() {
    let class = TaskOutcomeClass::from("plan_mode_revision_requested");
    assert_eq!(class, TaskOutcomeClass::PlanModeRevisionRequested);
    assert_eq!(class.as_str(), "plan_mode_revision_requested");
    assert_eq!(
        serde_json::from_str::<TaskOutcomeClass>(
            &serde_json::to_string(&class).expect("serialize class")
        )
        .expect("deserialize class"),
        class
    );
}

#[test]
fn skill_usage_injection_kinds_round_trip_closed_vocabulary() {
    let cases = [
        ("compact_index", SkillUsageInjectionKind::CompactIndex),
        ("full_load", SkillUsageInjectionKind::FullLoad),
        (
            "composer_directive",
            SkillUsageInjectionKind::ComposerDirective,
        ),
        (
            "interactive_stdin_unattributed",
            SkillUsageInjectionKind::InteractiveStdinUnattributed,
        ),
    ];

    for (value, kind) in cases {
        assert_eq!(SkillUsageInjectionKind::from_str(value).unwrap(), kind);
        assert_eq!(kind.to_string(), value);
    }

    assert!(SkillUsageInjectionKind::from_str("explicit").is_err());
}
