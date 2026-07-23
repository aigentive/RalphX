use super::project_skill_settings::{ProjectSkillSettings, ProjectSkillSettingsPatch};
use super::types::ProjectId;

#[test]
fn project_skill_settings_defaults_match_b1_contract() {
    let settings =
        ProjectSkillSettings::default_for_project(ProjectId::from_string("project-1".to_string()));
    assert!(settings.enabled);
    assert!(settings.auto_inject);
    assert!(settings.auto_distill);
    assert_eq!(settings.injection_max_skills, 4);
    assert_eq!(settings.injection_max_chars, 6_000);
    assert_eq!(settings.injection_guidance_max_chars, 400);
    assert_eq!(settings.report_min_outcomes, 5);
    assert_eq!(settings.verification_corpus_gate, 0);
    assert!(!settings.export_enabled);
    settings.validate().unwrap();
}

#[test]
fn project_skill_settings_reject_invalid_budgets_and_empty_patches() {
    let mut settings =
        ProjectSkillSettings::default_for_project(ProjectId::from_string("project-1".to_string()));
    settings.injection_max_skills = 0;
    assert!(settings.validate().is_err());

    assert!(ProjectSkillSettingsPatch::default().validate().is_err());
    assert!(ProjectSkillSettingsPatch {
        export_enabled: Some(true),
        ..Default::default()
    }
    .validate()
    .is_ok());
    assert!(ProjectSkillSettingsPatch {
        injection_max_chars: Some(0),
        ..Default::default()
    }
    .validate()
    .is_err());
}
