use std::fmt;
use std::str::FromStr;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::entities::ProjectId;

/// The model level for an ideation agent workflow.
/// Maps to Claude's `--model` flag values.
/// `Inherit` means "fall through to the next resolution level".
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ModelLevel {
    /// Use the next level in the resolution chain (YAML config or global default)
    Inherit,
    Sonnet,
    Opus,
    #[serde(rename = "claude-opus-4-7")]
    ClaudeOpus47,
    #[serde(rename = "claude-opus-4-8")]
    ClaudeOpus48,
    #[serde(rename = "claude-opus-5")]
    ClaudeOpus5,
    Haiku,
    Fable,
}

impl Default for ModelLevel {
    fn default() -> Self {
        Self::Inherit
    }
}

impl fmt::Display for ModelLevel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Inherit => write!(f, "inherit"),
            Self::Sonnet => write!(f, "sonnet"),
            Self::Opus => write!(f, "opus"),
            Self::ClaudeOpus47 => write!(f, "claude-opus-4-7"),
            Self::ClaudeOpus48 => write!(f, "claude-opus-4-8"),
            Self::ClaudeOpus5 => write!(f, "claude-opus-5"),
            Self::Haiku => write!(f, "haiku"),
            Self::Fable => write!(f, "fable"),
        }
    }
}

impl FromStr for ModelLevel {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "inherit" => Ok(Self::Inherit),
            "sonnet" => Ok(Self::Sonnet),
            "opus" => Ok(Self::Opus),
            "claude-opus-4-7" => Ok(Self::ClaudeOpus47),
            "claude-opus-4-8" => Ok(Self::ClaudeOpus48),
            "claude-opus-5" => Ok(Self::ClaudeOpus5),
            "haiku" => Ok(Self::Haiku),
            "fable" => Ok(Self::Fable),
            other => Err(format!(
                "Invalid model level '{}'. Valid values: inherit, sonnet, opus, claude-opus-4-7, claude-opus-4-8, claude-opus-5, haiku, fable",
                other
            )),
        }
    }
}

/// Which workflow bucket an ideation agent belongs to.
/// Used to select the correct model setting column.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ModelBucket {
    /// Primary ideation agents: ralphx-ideation and ralphx-ideation-readonly.
    Primary,
    /// Verification agents: ralphx-plan-verifier
    Verifier,
    /// Cap-resolution bucket for subagents spawned by ralphx-plan-verifier (critics/specialists).
    /// Not mapped by model_bucket_for_agent() — resolved separately at runtime.
    VerifierSubagent,
    /// Cap-resolution bucket for subagents spawned by the main ideation path
    /// (`ralphx-ideation`).
    /// Not mapped by model_bucket_for_agent() — resolved separately at runtime.
    IdeationSubagent,
}

/// Per-project or global ideation model settings row.
/// `project_id IS NULL` = global row; `project_id = Some(...)` = per-project override.
#[derive(Debug, Clone, PartialEq)]
pub struct IdeationModelSettings {
    pub id: i64,
    pub project_id: Option<ProjectId>,
    pub primary_model: ModelLevel,
    pub verifier_model: ModelLevel,
    pub verifier_subagent_model: ModelLevel,
    /// Cap for subagents spawned by the main ideation path (`ralphx-ideation`).
    /// Defaults to `Inherit` (fall through to next resolution level).
    pub ideation_subagent_model: ModelLevel,
    pub updated_at: DateTime<Utc>,
}

impl IdeationModelSettings {
    /// Return the model level for the given bucket.
    pub fn model_for_bucket(&self, bucket: &ModelBucket) -> &ModelLevel {
        match bucket {
            ModelBucket::Primary => &self.primary_model,
            ModelBucket::Verifier => &self.verifier_model,
            ModelBucket::VerifierSubagent => &self.verifier_subagent_model,
            ModelBucket::IdeationSubagent => &self.ideation_subagent_model,
        }
    }
}

/// Map an agent name to its `ModelBucket`, or `None` for non-ideation agents.
pub fn model_bucket_for_agent(agent_name: &str) -> Option<ModelBucket> {
    let normalized = agent_name.strip_prefix("ralphx:").unwrap_or(agent_name);
    match normalized {
        "ralphx-ideation"
        | "orchestrator-ideation"
        | "ralphx-ideation-readonly"
        | "orchestrator-ideation-readonly" => Some(ModelBucket::Primary),
        "ralphx-plan-verifier" | "plan-verifier" => Some(ModelBucket::Verifier),
        _ => None,
    }
}

#[cfg(test)]
#[path = "model_settings_tests.rs"]
mod tests;
