use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use super::harness::{
    AgentHarnessKind, LogicalEffort, CLAUDE_DEFAULT_ALLOW_DANGEROUSLY_SKIP_PERMISSIONS,
    CLAUDE_DEFAULT_DANGEROUSLY_SKIP_PERMISSIONS, CLAUDE_DEFAULT_PERMISSION_MODE,
    CODEX_DEFAULT_APPROVAL_POLICY, CODEX_DEFAULT_SANDBOX_MODE,
};
use super::model_registry::{default_effort_for_provider, default_model_for_provider};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentProviderSettings {
    pub provider: AgentHarnessKind,
    pub enabled: bool,
    pub is_default: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub effort: Option<LogicalEffort>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub approval_policy: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sandbox_mode: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub claude_permission_mode: Option<String>,
    pub claude_dangerously_skip_permissions: bool,
    pub claude_allow_dangerously_skip_permissions: bool,
    pub updated_at: DateTime<Utc>,
}

impl AgentProviderSettings {
    pub fn disabled_defaults(provider: AgentHarnessKind) -> Self {
        Self {
            provider,
            enabled: false,
            is_default: false,
            model: Some(default_model_for_provider(provider).to_string()),
            effort: Some(default_effort_for_provider(provider)),
            approval_policy: default_approval_policy(provider).map(str::to_string),
            sandbox_mode: default_sandbox_mode(provider).map(str::to_string),
            claude_permission_mode: default_claude_permission_mode(provider).map(str::to_string),
            claude_dangerously_skip_permissions: match provider {
                AgentHarnessKind::Claude => CLAUDE_DEFAULT_DANGEROUSLY_SKIP_PERMISSIONS,
                AgentHarnessKind::Codex => false,
            },
            claude_allow_dangerously_skip_permissions: match provider {
                AgentHarnessKind::Claude => CLAUDE_DEFAULT_ALLOW_DANGEROUSLY_SKIP_PERMISSIONS,
                AgentHarnessKind::Codex => false,
            },
            updated_at: Utc::now(),
        }
    }
}

fn default_approval_policy(provider: AgentHarnessKind) -> Option<&'static str> {
    match provider {
        AgentHarnessKind::Claude => None,
        AgentHarnessKind::Codex => Some(CODEX_DEFAULT_APPROVAL_POLICY),
    }
}

fn default_sandbox_mode(provider: AgentHarnessKind) -> Option<&'static str> {
    match provider {
        AgentHarnessKind::Claude => None,
        AgentHarnessKind::Codex => Some(CODEX_DEFAULT_SANDBOX_MODE),
    }
}

fn default_claude_permission_mode(provider: AgentHarnessKind) -> Option<&'static str> {
    match provider {
        AgentHarnessKind::Claude => Some(CLAUDE_DEFAULT_PERMISSION_MODE),
        AgentHarnessKind::Codex => None,
    }
}

#[cfg(test)]
#[path = "provider_settings_tests.rs"]
mod tests;
