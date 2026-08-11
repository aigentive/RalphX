use std::{fmt, str::FromStr};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::{ChatConversationId, DelegatedSessionId, ProjectId};
use crate::agents::AgentHarnessKind;

macro_rules! string_id {
    ($name:ident) => {
        #[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
        pub struct $name(pub String);

        impl $name {
            pub fn new() -> Self {
                Self(uuid::Uuid::new_v4().to_string())
            }

            pub fn from_string(value: impl Into<String>) -> Self {
                Self(value.into())
            }

            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl Default for $name {
            fn default() -> Self {
                Self::new()
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(&self.0)
            }
        }
    };
}

string_id!(AgentWorkflowScriptId);
string_id!(AgentWorkflowRunId);
string_id!(AgentWorkflowPhaseId);
string_id!(AgentWorkflowInvocationId);

macro_rules! string_enum {
    ($name:ident { $($variant:ident => $value:literal),+ $(,)? }) => {
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
        #[serde(rename_all = "snake_case")]
        pub enum $name { $($variant),+ }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(match self { $(Self::$variant => $value),+ })
            }
        }

        impl FromStr for $name {
            type Err = String;

            fn from_str(value: &str) -> Result<Self, Self::Err> {
                match value {
                    $($value => Ok(Self::$variant),)+
                    other => Err(format!("Invalid {}: {other}", stringify!($name))),
                }
            }
        }
    };
}

string_enum!(AgentWorkflowRunStatus {
    AwaitingApproval => "awaiting_approval",
    Queued => "queued",
    Running => "running",
    PauseRequested => "pause_requested",
    Paused => "paused",
    Recovering => "recovering",
    Completed => "completed",
    Failed => "failed",
    Cancelled => "cancelled",
    Disabled => "disabled",
});

impl AgentWorkflowRunStatus {
    pub fn is_terminal(self) -> bool {
        matches!(self, Self::Completed | Self::Failed | Self::Cancelled)
    }
}

string_enum!(AgentWorkflowStepStatus {
    Pending => "pending",
    Running => "running",
    Completed => "completed",
    Failed => "failed",
    Cancelled => "cancelled",
    Skipped => "skipped",
});

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentWorkflowMeta {
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub phases: Vec<String>,
    #[serde(default = "default_max_concurrency")]
    pub max_concurrency: u16,
    #[serde(default = "default_max_invocations")]
    pub max_invocations: u32,
}

const fn default_max_concurrency() -> u16 {
    4
}

const fn default_max_invocations() -> u32 {
    64
}

impl AgentWorkflowMeta {
    pub fn validate(&self) -> Result<(), String> {
        if self.name.trim().is_empty() {
            return Err("Workflow name is required".to_string());
        }
        if self.max_concurrency == 0 || self.max_concurrency > 16 {
            return Err("Workflow max concurrency must be between 1 and 16".to_string());
        }
        if self.max_invocations == 0 || self.max_invocations > 1_000 {
            return Err("Workflow max invocations must be between 1 and 1000".to_string());
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentWorkflowScript {
    pub id: AgentWorkflowScriptId,
    pub conversation_id: ChatConversationId,
    pub project_id: ProjectId,
    pub source: String,
    pub script_hash: String,
    pub protocol_version: u16,
    pub meta: AgentWorkflowMeta,
    pub permission_summary_json: String,
    pub permission_hash: String,
    pub estimated_fanout: u32,
    pub approved_script_hash: Option<String>,
    pub approved_permission_hash: Option<String>,
    pub approved_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl AgentWorkflowScript {
    pub fn new(
        conversation_id: ChatConversationId,
        project_id: ProjectId,
        source: String,
        meta: AgentWorkflowMeta,
        permission_summary_json: String,
        estimated_fanout: u32,
    ) -> Result<Self, String> {
        meta.validate()?;
        if source.trim().is_empty() {
            return Err("Workflow script source is required".to_string());
        }
        let now = Utc::now();
        Ok(Self {
            id: AgentWorkflowScriptId::new(),
            conversation_id,
            project_id,
            script_hash: sha256_hex(source.as_bytes()),
            permission_hash: sha256_hex(permission_summary_json.as_bytes()),
            source,
            protocol_version: 1,
            meta,
            permission_summary_json,
            estimated_fanout,
            approved_script_hash: None,
            approved_permission_hash: None,
            approved_at: None,
            created_at: now,
            updated_at: now,
        })
    }

    pub fn is_approved_for_current_content(&self) -> bool {
        self.approved_script_hash.as_deref() == Some(self.script_hash.as_str())
            && self.approved_permission_hash.as_deref() == Some(self.permission_hash.as_str())
            && self.approved_at.is_some()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentWorkflowRun {
    pub id: AgentWorkflowRunId,
    pub script_id: AgentWorkflowScriptId,
    pub conversation_id: ChatConversationId,
    pub project_id: ProjectId,
    pub harness: AgentHarnessKind,
    pub script_hash: String,
    pub permission_hash: String,
    pub args_json: String,
    pub status: AgentWorkflowRunStatus,
    pub attempt: u32,
    pub runner_instance_id: Option<String>,
    pub lease_expires_at: Option<DateTime<Utc>>,
    pub heartbeat_at: Option<DateTime<Utc>>,
    pub pause_requested: bool,
    pub cancel_requested: bool,
    pub result_json: Option<String>,
    pub error: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub completed_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentWorkflowPhase {
    pub id: AgentWorkflowPhaseId,
    pub run_id: AgentWorkflowRunId,
    pub key: String,
    pub name: String,
    pub ordinal: u32,
    pub status: AgentWorkflowStepStatus,
    pub started_at: Option<DateTime<Utc>>,
    pub completed_at: Option<DateTime<Utc>>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentWorkflowInvocation {
    pub id: AgentWorkflowInvocationId,
    pub run_id: AgentWorkflowRunId,
    pub phase_id: Option<AgentWorkflowPhaseId>,
    pub logical_key: String,
    pub agent_name: String,
    pub prompt_hash: String,
    pub schema_hash: Option<String>,
    pub status: AgentWorkflowStepStatus,
    pub delegated_session_id: Option<DelegatedSessionId>,
    pub child_conversation_id: Option<ChatConversationId>,
    pub result_json: Option<String>,
    pub error: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub completed_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentWorkflowLogEntry {
    pub run_id: AgentWorkflowRunId,
    pub sequence: u64,
    pub level: String,
    pub message: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentWorkflowProgress {
    pub run: AgentWorkflowRun,
    pub phases: Vec<AgentWorkflowPhase>,
    pub invocations: Vec<AgentWorkflowInvocation>,
    pub logs: Vec<AgentWorkflowLogEntry>,
}

pub fn sha256_hex(value: &[u8]) -> String {
    format!("{:x}", Sha256::digest(value))
}
