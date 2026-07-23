use serde::{Deserialize, Serialize};

use super::ProjectId;
use crate::error::{AppError, AppResult};

pub const DEFAULT_PROJECT_SKILLS_ENABLED: bool = true;
pub const DEFAULT_PROJECT_SKILLS_AUTO_INJECT: bool = true;
pub const DEFAULT_PROJECT_SKILLS_AUTO_DISTILL: bool = true;
pub const DEFAULT_PROJECT_SKILL_INJECTION_MAX_SKILLS: i64 = 4;
pub const DEFAULT_PROJECT_SKILL_INJECTION_MAX_CHARS: i64 = 6_000;
pub const DEFAULT_PROJECT_SKILL_GUIDANCE_MAX_CHARS: i64 = 400;
pub const DEFAULT_PROJECT_SKILL_REPORT_MIN_OUTCOMES: i64 = 5;
pub const DEFAULT_PROJECT_SKILL_VERIFICATION_CORPUS_GATE: i64 = 0;
pub const DEFAULT_PROJECT_SKILL_EXPORT_ENABLED: bool = false;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProjectSkillSettings {
    pub project_id: ProjectId,
    pub enabled: bool,
    pub auto_inject: bool,
    pub auto_distill: bool,
    pub injection_max_skills: i64,
    pub injection_max_chars: i64,
    pub injection_guidance_max_chars: i64,
    pub report_min_outcomes: i64,
    pub verification_corpus_gate: i64,
    pub export_enabled: bool,
}

impl ProjectSkillSettings {
    pub fn default_for_project(project_id: ProjectId) -> Self {
        Self {
            project_id,
            enabled: DEFAULT_PROJECT_SKILLS_ENABLED,
            auto_inject: DEFAULT_PROJECT_SKILLS_AUTO_INJECT,
            auto_distill: DEFAULT_PROJECT_SKILLS_AUTO_DISTILL,
            injection_max_skills: DEFAULT_PROJECT_SKILL_INJECTION_MAX_SKILLS,
            injection_max_chars: DEFAULT_PROJECT_SKILL_INJECTION_MAX_CHARS,
            injection_guidance_max_chars: DEFAULT_PROJECT_SKILL_GUIDANCE_MAX_CHARS,
            report_min_outcomes: DEFAULT_PROJECT_SKILL_REPORT_MIN_OUTCOMES,
            verification_corpus_gate: DEFAULT_PROJECT_SKILL_VERIFICATION_CORPUS_GATE,
            export_enabled: DEFAULT_PROJECT_SKILL_EXPORT_ENABLED,
        }
    }

    pub fn validate(&self) -> AppResult<()> {
        if self.injection_max_skills <= 0
            || self.injection_max_chars <= 0
            || self.injection_guidance_max_chars <= 0
            || self.report_min_outcomes <= 0
            || self.verification_corpus_gate < 0
        {
            return Err(AppError::Validation(
                "project skill settings budgets are out of range".to_string(),
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProjectSkillSettingsPatch {
    pub enabled: Option<bool>,
    pub auto_inject: Option<bool>,
    pub auto_distill: Option<bool>,
    pub injection_max_skills: Option<i64>,
    pub injection_max_chars: Option<i64>,
    pub injection_guidance_max_chars: Option<i64>,
    pub report_min_outcomes: Option<i64>,
    pub verification_corpus_gate: Option<i64>,
    pub export_enabled: Option<bool>,
}

impl ProjectSkillSettingsPatch {
    pub fn validate(&self) -> AppResult<()> {
        if self == &Self::default() {
            return Err(AppError::Validation(
                "project skill settings patch must not be empty".to_string(),
            ));
        }
        if self.injection_max_skills.is_some_and(|value| value <= 0)
            || self.injection_max_chars.is_some_and(|value| value <= 0)
            || self
                .injection_guidance_max_chars
                .is_some_and(|value| value <= 0)
            || self.report_min_outcomes.is_some_and(|value| value <= 0)
            || self.verification_corpus_gate.is_some_and(|value| value < 0)
        {
            return Err(AppError::Validation(
                "project skill settings patch is out of range".to_string(),
            ));
        }
        Ok(())
    }

    pub fn apply_to(self, settings: &mut ProjectSkillSettings) {
        macro_rules! assign {
            ($field:ident) => {
                if let Some(value) = self.$field {
                    settings.$field = value;
                }
            };
        }
        assign!(enabled);
        assign!(auto_inject);
        assign!(auto_distill);
        assign!(injection_max_skills);
        assign!(injection_max_chars);
        assign!(injection_guidance_max_chars);
        assign!(report_min_outcomes);
        assign!(verification_corpus_gate);
        assign!(export_enabled);
    }
}
