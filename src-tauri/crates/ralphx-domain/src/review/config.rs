// Review Configuration types
// Global settings for AI and human code review

use serde::{Deserialize, Serialize};

fn default_auto_create_followup_agent_conversation() -> bool {
    false
}

fn default_require_workspace_review() -> bool {
    true
}

fn default_autofix_workspace_review_blocking_findings() -> bool {
    true
}

fn default_run_task_validations() -> bool {
    true
}

fn default_workspace_review_fixer_cycle_cap() -> i64 {
    3
}

/// Global review settings stored in project settings
///
/// Controls how the review system behaves including:
/// - Whether AI review is enabled
/// - Whether to auto-create fix tasks
/// - Human review requirements
/// - Max fix attempts before giving up
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ReviewSettings {
    /// Master toggle for AI review system
    /// Default: true
    pub ai_review_enabled: bool,

    /// Automatically create fix tasks when AI review fails
    /// If false, failed reviews go to backlog instead
    /// Default: true
    pub ai_review_auto_fix: bool,

    /// Require human approval before executing AI-proposed fix tasks
    /// Default: false
    pub require_fix_approval: bool,

    /// Require human review even after AI approval
    /// If true, AI-approved tasks still need human sign-off
    /// Default: false
    pub require_human_review: bool,

    /// Require workspace Review before publishing agent workspace branches
    /// Default: true
    #[serde(default = "default_require_workspace_review")]
    pub require_workspace_review: bool,

    /// Automatically spawn the workspace repair agent when Workspace Review blocks publishing
    /// Default: true
    #[serde(default = "default_autofix_workspace_review_blocking_findings")]
    pub autofix_workspace_review_blocking_findings: bool,

    /// Maximum automatic workspace Review fixer cycles per workspace. Zero disables auto-routing.
    #[serde(default = "default_workspace_review_fixer_cycle_cap")]
    pub workspace_review_fixer_cycle_cap: i64,

    /// Allow task execution pipeline agents to run/cache backend task validation
    /// Default: true
    #[serde(default = "default_run_task_validations")]
    pub run_task_validations: bool,

    /// Maximum fix attempts before giving up and moving to backlog
    /// Default: 3
    pub max_fix_attempts: u32,

    /// Maximum revision cycles (review → changes requested → re-execution) before failing
    /// Default: 5
    pub max_revision_cycles: u32,

    /// Automatically create visible follow-up Agent conversations for eligible drift/issues
    /// Default: false
    #[serde(default = "default_auto_create_followup_agent_conversation")]
    pub auto_create_followup_agent_conversation: bool,
}

impl Default for ReviewSettings {
    fn default() -> Self {
        Self {
            ai_review_enabled: true,
            ai_review_auto_fix: true,
            require_fix_approval: false,
            require_human_review: false,
            require_workspace_review: true,
            autofix_workspace_review_blocking_findings: true,
            workspace_review_fixer_cycle_cap: 3,
            run_task_validations: true,
            max_fix_attempts: 3,
            max_revision_cycles: 5,
            auto_create_followup_agent_conversation: false,
        }
    }
}

impl ReviewSettings {
    /// Create review settings with AI review disabled
    pub fn ai_disabled() -> Self {
        Self {
            ai_review_enabled: false,
            ..Default::default()
        }
    }

    /// Create review settings that require human review
    pub fn with_human_review() -> Self {
        Self {
            require_human_review: true,
            ..Default::default()
        }
    }

    /// Create review settings with fix approval required
    pub fn with_fix_approval() -> Self {
        Self {
            require_fix_approval: true,
            ..Default::default()
        }
    }

    /// Create review settings with custom max fix attempts
    pub fn with_max_attempts(max_attempts: u32) -> Self {
        Self {
            max_fix_attempts: max_attempts,
            ..Default::default()
        }
    }

    /// Check if AI review should run
    pub fn should_run_ai_review(&self) -> bool {
        self.ai_review_enabled
    }

    /// Check if fix tasks should be auto-created on review failure
    pub fn should_auto_create_fix(&self) -> bool {
        self.ai_review_auto_fix
    }

    /// Check if human review is required after AI approval
    pub fn needs_human_review(&self) -> bool {
        self.require_human_review
    }

    /// Check if fix tasks need human approval before execution
    pub fn needs_fix_approval(&self) -> bool {
        self.require_fix_approval
    }

    /// Check if we've exceeded the max fix attempts
    pub fn exceeded_max_attempts(&self, attempts: u32) -> bool {
        attempts >= self.max_fix_attempts
    }

    /// Check if we've exceeded the max revision cycles
    pub fn exceeded_max_revisions(&self, revision_count: u32) -> bool {
        revision_count >= self.max_revision_cycles
    }
}

#[cfg(test)]
#[path = "config_tests.rs"]
mod tests;
