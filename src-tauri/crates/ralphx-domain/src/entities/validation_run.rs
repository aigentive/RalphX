use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use super::{ProjectId, TaskId};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ValidationPurpose {
    Baseline,
    WaveGate,
    Final,
    ReExecution,
    Merge,
    Other,
}

impl ValidationPurpose {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Baseline => "baseline",
            Self::WaveGate => "wave_gate",
            Self::Final => "final",
            Self::ReExecution => "re_execution",
            Self::Merge => "merge",
            Self::Other => "other",
        }
    }

    pub fn parse(value: &str) -> Self {
        match value {
            "baseline" => Self::Baseline,
            "wave_gate" => Self::WaveGate,
            "final" => Self::Final,
            "re_execution" | "re-execution" => Self::ReExecution,
            "merge" => Self::Merge,
            _ => Self::Other,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ValidationContextType {
    Execution,
    ReExecution,
    Review,
    AgentConversation,
    Merge,
    Unknown,
}

impl ValidationContextType {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Execution => "execution",
            Self::ReExecution => "re_execution",
            Self::Review => "review",
            Self::AgentConversation => "agent_conversation",
            Self::Merge => "merge",
            Self::Unknown => "unknown",
        }
    }

    pub fn parse(value: &str) -> Self {
        match value {
            "execution" => Self::Execution,
            "re_execution" | "re-execution" => Self::ReExecution,
            "review" => Self::Review,
            "agent_conversation" | "agent-conversation" => Self::AgentConversation,
            "merge" => Self::Merge,
            _ => Self::Unknown,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ValidationRunMode {
    ReuseOrRun,
    Force,
    DryRun,
}

impl ValidationRunMode {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::ReuseOrRun => "reuse_or_run",
            Self::Force => "force",
            Self::DryRun => "dry_run",
        }
    }

    pub fn parse(value: &str) -> Self {
        match value {
            "force" => Self::Force,
            "dry_run" | "dry-run" => Self::DryRun,
            _ => Self::ReuseOrRun,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ValidationRunStatus {
    Running,
    Passed,
    Failed,
    Error,
    Cancelled,
    Skipped,
}

impl ValidationRunStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Running => "running",
            Self::Passed => "passed",
            Self::Failed => "failed",
            Self::Error => "error",
            Self::Cancelled => "cancelled",
            Self::Skipped => "skipped",
        }
    }

    pub fn parse(value: &str) -> Self {
        match value {
            "passed" => Self::Passed,
            "failed" => Self::Failed,
            "error" => Self::Error,
            "cancelled" => Self::Cancelled,
            "skipped" => Self::Skipped,
            _ => Self::Running,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ValidationCommandSource {
    ProjectAnalysisRef,
    AgentSelected,
}

impl ValidationCommandSource {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::ProjectAnalysisRef => "project_analysis_ref",
            Self::AgentSelected => "agent_selected",
        }
    }

    pub fn parse(value: &str) -> Self {
        match value {
            "project_analysis_ref" => Self::ProjectAnalysisRef,
            _ => Self::AgentSelected,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ValidationCommandCategory {
    Test,
    Lint,
    Typecheck,
    Build,
    Format,
    Other,
}

impl ValidationCommandCategory {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Test => "test",
            Self::Lint => "lint",
            Self::Typecheck => "typecheck",
            Self::Build => "build",
            Self::Format => "format",
            Self::Other => "other",
        }
    }

    pub fn parse(value: &str) -> Self {
        match value {
            "test" => Self::Test,
            "lint" => Self::Lint,
            "typecheck" | "type_check" => Self::Typecheck,
            "build" => Self::Build,
            "format" => Self::Format,
            _ => Self::Other,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ValidationCacheDecision {
    Ran,
    Cached,
    Stale,
    Forced,
    Skipped,
}

impl ValidationCacheDecision {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Ran => "ran",
            Self::Cached => "cached",
            Self::Stale => "stale",
            Self::Forced => "forced",
            Self::Skipped => "skipped",
        }
    }

    pub fn parse(value: &str) -> Self {
        match value {
            "cached" => Self::Cached,
            "stale" => Self::Stale,
            "forced" => Self::Forced,
            "skipped" => Self::Skipped,
            _ => Self::Ran,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ValidationCommandStatus {
    Passed,
    Failed,
    Error,
    Skipped,
    Cached,
}

impl ValidationCommandStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Passed => "passed",
            Self::Failed => "failed",
            Self::Error => "error",
            Self::Skipped => "skipped",
            Self::Cached => "cached",
        }
    }

    pub fn parse(value: &str) -> Self {
        match value {
            "failed" => Self::Failed,
            "error" => Self::Error,
            "skipped" => Self::Skipped,
            "cached" => Self::Cached,
            _ => Self::Passed,
        }
    }

    pub fn is_success_like(&self) -> bool {
        matches!(self, Self::Passed | Self::Cached)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValidationRun {
    pub id: String,
    pub task_id: TaskId,
    pub project_id: ProjectId,
    pub purpose: ValidationPurpose,
    pub context_type: ValidationContextType,
    pub requested_by_agent: Option<String>,
    pub status: ValidationRunStatus,
    pub mode: ValidationRunMode,
    pub policy_enabled: bool,
    pub head_sha: Option<String>,
    pub start_content_fingerprint: Option<String>,
    pub validated_content_fingerprint: Option<String>,
    pub promoted_commit_sha: Option<String>,
    pub base_ref: Option<String>,
    pub analysis_fingerprint: Option<String>,
    pub status_episode_entered_at: Option<DateTime<Utc>>,
    pub started_at: DateTime<Utc>,
    pub completed_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValidationCommandResult {
    pub id: String,
    pub validation_run_id: String,
    pub task_id: TaskId,
    pub project_id: ProjectId,
    pub command_source: ValidationCommandSource,
    pub command_ref: Option<String>,
    pub command: String,
    pub cwd: String,
    pub label: Option<String>,
    pub category: ValidationCommandCategory,
    pub reason: Option<String>,
    pub related_files: Vec<String>,
    pub cache_key: String,
    pub cache_decision: ValidationCacheDecision,
    pub status: ValidationCommandStatus,
    pub exit_code: Option<i32>,
    pub duration_ms: Option<u64>,
    pub stdout_snippet: Option<String>,
    pub stderr_snippet: Option<String>,
    pub stdout_log_path: Option<String>,
    pub stderr_log_path: Option<String>,
    pub launcher_kind: Option<String>,
    pub resolved_shell_path: Option<String>,
    pub head_sha: Option<String>,
    pub analysis_fingerprint: Option<String>,
    pub status_episode_entered_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValidationRunWithResults {
    pub run: ValidationRun,
    pub commands: Vec<ValidationCommandResult>,
}

#[cfg(test)]
#[path = "validation_run_tests.rs"]
mod tests;
