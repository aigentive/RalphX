use std::fmt;
use std::str::FromStr;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::entities::ProjectId;

/// The effort level for an ideation agent workflow.
/// Maps to Claude's `--effort` flag values.
/// `Inherit` means "fall through to the next resolution level".
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EffortLevel {
    Low,
    Medium,
    High,
    Max,
    Ultra,
    /// Use the next level in the resolution chain (YAML config or global default)
    Inherit,
}

impl Default for EffortLevel {
    fn default() -> Self {
        Self::Inherit
    }
}

impl fmt::Display for EffortLevel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Low => write!(f, "low"),
            Self::Medium => write!(f, "medium"),
            Self::High => write!(f, "high"),
            Self::Max => write!(f, "max"),
            Self::Ultra => write!(f, "ultra"),
            Self::Inherit => write!(f, "inherit"),
        }
    }
}

impl FromStr for EffortLevel {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "low" => Ok(Self::Low),
            "medium" => Ok(Self::Medium),
            "high" => Ok(Self::High),
            "max" => Ok(Self::Max),
            "ultra" => Ok(Self::Ultra),
            "inherit" => Ok(Self::Inherit),
            other => Err(format!(
                "Invalid effort level '{}'. Valid values: low, medium, high, max, ultra, inherit",
                other
            )),
        }
    }
}

/// Which workflow bucket an ideation agent belongs to.
/// Used to select the correct effort setting column.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EffortBucket {
    /// Primary ideation agents: ralphx-ideation and ralphx-ideation-readonly.
    Primary,
    /// Verification agents: ralphx-plan-verifier
    Verifier,
}

/// Per-project or global ideation effort settings row.
/// `project_id IS NULL` = global row; `project_id = Some(...)` = per-project override.
#[derive(Debug, Clone, PartialEq)]
pub struct IdeationEffortSettings {
    pub id: i64,
    pub project_id: Option<ProjectId>,
    pub primary_effort: EffortLevel,
    pub verifier_effort: EffortLevel,
    pub updated_at: DateTime<Utc>,
}

impl IdeationEffortSettings {
    /// Return the effort level for the given bucket.
    pub fn effort_for_bucket(&self, bucket: &EffortBucket) -> &EffortLevel {
        match bucket {
            EffortBucket::Primary => &self.primary_effort,
            EffortBucket::Verifier => &self.verifier_effort,
        }
    }
}

#[cfg(test)]
#[path = "effort_settings_tests.rs"]
mod tests;
