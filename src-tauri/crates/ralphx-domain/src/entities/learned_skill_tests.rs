use std::str::FromStr;

use super::learned_skill::{
    ProjectSkillId, ProjectSkillLifecycleStatus, SkillUsageEventId, TaskOutcomeId,
    TaskOutcomeStatus,
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
