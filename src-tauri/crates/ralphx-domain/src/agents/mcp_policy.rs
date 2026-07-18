use std::collections::BTreeMap;
use std::fmt;
use std::str::FromStr;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use super::AgentHarnessKind;

pub const RALPHX_MCP_SERVER_IDS: [&str; 2] = ["ralphx", "ralphx_internal"];

/// Provider-neutral deny-only policy applied at a concrete CLI launch.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct McpLaunchPolicy {
    pub disabled_servers: Vec<String>,
    pub disabled_tools: BTreeMap<String, Vec<String>>,
}

impl McpLaunchPolicy {
    pub fn is_empty(&self) -> bool {
        self.disabled_servers.is_empty() && self.disabled_tools.values().all(Vec::is_empty)
    }

    pub fn claude_disallowed_tools(&self) -> Vec<String> {
        let mut denied = self
            .disabled_servers
            .iter()
            .map(|server| format!("mcp__{server}__*"))
            .collect::<Vec<_>>();
        denied.extend(self.disabled_tools.iter().flat_map(|(server, tools)| {
            tools
                .iter()
                .map(move |tool| format!("mcp__{server}__{tool}"))
        }));
        denied
    }

    pub fn codex_config_overrides(&self) -> Vec<String> {
        let mut overrides = self
            .disabled_servers
            .iter()
            .map(|server| format!("mcp_servers.{server}.enabled=false"))
            .collect::<Vec<_>>();
        overrides.extend(
            self.disabled_tools
                .iter()
                .filter(|(_, tools)| !tools.is_empty())
                .map(|(server, tools)| {
                    let tools = serde_json::to_string(tools).expect("MCP tool names serialize");
                    format!("mcp_servers.{server}.disabled_tools={tools}")
                }),
        );
        overrides
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct McpServerKey {
    pub provider: AgentHarnessKind,
    pub server_id: String,
}

impl McpServerKey {
    pub fn new(provider: AgentHarnessKind, server_id: impl Into<String>) -> Result<Self, String> {
        let server_id = server_id.into();
        validate_mcp_identifier("server", &server_id)?;
        Ok(Self {
            provider,
            server_id,
        })
    }

    pub fn is_ralphx_owned(&self) -> bool {
        RALPHX_MCP_SERVER_IDS.contains(&self.server_id.as_str())
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum McpOverrideState {
    #[default]
    Follow,
    Enabled,
    Disabled,
}

impl fmt::Display for McpOverrideState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Follow => write!(f, "follow"),
            Self::Enabled => write!(f, "enabled"),
            Self::Disabled => write!(f, "disabled"),
        }
    }
}

impl FromStr for McpOverrideState {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "follow" => Ok(Self::Follow),
            "enabled" => Ok(Self::Enabled),
            "disabled" => Ok(Self::Disabled),
            other => Err(format!("Invalid MCP override state: {other}")),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct McpPolicyOverride {
    pub project_id: Option<String>,
    pub key: McpServerKey,
    pub server_state: McpOverrideState,
    pub tool_states: BTreeMap<String, McpOverrideState>,
    pub updated_at: DateTime<Utc>,
}

impl McpPolicyOverride {
    pub fn validate(&self) -> Result<(), String> {
        validate_mcp_identifier("server", &self.key.server_id)?;
        if self.key.is_ralphx_owned()
            && (self.server_state == McpOverrideState::Disabled
                || self
                    .tool_states
                    .values()
                    .any(|state| *state == McpOverrideState::Disabled))
        {
            return Err(format!(
                "RalphX-owned MCP server '{}' cannot be disabled",
                self.key.server_id
            ));
        }
        for tool_name in self.tool_states.keys() {
            validate_mcp_identifier("tool", tool_name)?;
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NativeMcpState {
    Unknown,
    Enabled,
    Disabled,
    PendingApproval,
    AuthRequired,
    Untrusted,
    Unavailable,
}

impl NativeMcpState {
    pub fn permits_launch(self) -> bool {
        matches!(self, Self::Unknown | Self::Enabled)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NativeMcpServerSnapshot {
    pub key: McpServerKey,
    pub native_scope: Option<String>,
    pub native_state: NativeMcpState,
    pub known_tools: Vec<String>,
    pub diagnostic: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum McpPolicySource {
    RequiredInternal,
    ProjectUi,
    ProjectYaml,
    GlobalUi,
    GlobalYaml,
    ProviderNative,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EffectiveMcpServerPolicy {
    pub native: NativeMcpServerSnapshot,
    pub enabled: bool,
    pub server_state: McpOverrideState,
    pub server_source: McpPolicySource,
    pub tool_states: BTreeMap<String, McpOverrideState>,
    pub tool_sources: BTreeMap<String, McpPolicySource>,
    pub disabled_tools: Vec<String>,
    pub locked: bool,
    pub locked_reason: Option<String>,
}

pub fn validate_mcp_identifier(kind: &str, value: &str) -> Result<(), String> {
    let valid = !value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.'))
        && value != "."
        && value != "..";
    if valid {
        Ok(())
    } else {
        Err(format!(
            "Invalid MCP {kind} identifier '{value}'; use 1-128 ASCII letters, digits, '.', '_' or '-'"
        ))
    }
}
