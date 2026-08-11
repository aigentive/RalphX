// Agent types
// Core types for agent configuration and identification

use std::collections::HashMap;
use std::path::PathBuf;

use super::harness::{AgentHarnessKind, LogicalEffort};

/// Role of an agent in the RalphX system
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum AgentRole {
    /// Worker agent that executes tasks
    Worker,
    /// Reviewer agent that reviews completed work
    Reviewer,
    /// QA prep agent that generates acceptance criteria
    QaPrep,
    /// QA refiner agent that refines QA plan based on implementation
    QaRefiner,
    /// QA tester agent that executes tests
    QaTester,
    /// Supervisor agent that monitors and coordinates
    Supervisor,
    /// Custom agent role with a user-defined name
    Custom(String),
}

impl std::fmt::Display for AgentRole {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AgentRole::Worker => write!(f, "worker"),
            AgentRole::Reviewer => write!(f, "reviewer"),
            AgentRole::QaPrep => write!(f, "qa-prep"),
            AgentRole::QaRefiner => write!(f, "qa-refiner"),
            AgentRole::QaTester => write!(f, "qa-tester"),
            AgentRole::Supervisor => write!(f, "supervisor"),
            AgentRole::Custom(name) => write!(f, "{}", name),
        }
    }
}

/// Type of agentic client
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum ClientType {
    /// Claude Code CLI client
    ClaudeCode,
    /// OpenAI Codex client (future)
    Codex,
    /// Google Gemini client (future)
    Gemini,
    /// Mock client for testing
    Mock,
    /// Custom client with a user-defined name
    Custom(String),
}

impl std::fmt::Display for ClientType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ClientType::ClaudeCode => write!(f, "claude-code"),
            ClientType::Codex => write!(f, "codex"),
            ClientType::Gemini => write!(f, "gemini"),
            ClientType::Mock => write!(f, "mock"),
            ClientType::Custom(name) => write!(f, "{}", name),
        }
    }
}

/// Configuration for spawning an agent
#[derive(Debug, Clone)]
pub struct AgentConfig {
    /// Role of the agent
    pub role: AgentRole,
    /// Initial prompt to send to the agent
    pub prompt: String,
    /// Working directory for the agent
    pub working_directory: PathBuf,
    /// Optional plugin directory for agent/skill discovery (e.g., "./plugins/app")
    pub plugin_dir: Option<PathBuf>,
    /// Optional agent name to use (resolved via plugin_dir)
    pub agent: Option<String>,
    /// Optional model override (e.g., "claude-sonnet-5")
    pub model: Option<String>,
    /// Optional provider harness override for the spawn.
    pub harness: Option<AgentHarnessKind>,
    /// Optional provider CLI executable path override for this spawn.
    pub cli_path_override: Option<PathBuf>,
    /// Optional provider-neutral reasoning effort.
    pub logical_effort: Option<LogicalEffort>,
    /// Optional provider approval policy.
    pub approval_policy: Option<String>,
    /// Optional provider sandbox mode.
    pub sandbox_mode: Option<String>,
    /// Optional provider service tier.
    pub service_tier: Option<String>,
    /// Optional max tokens for response
    pub max_tokens: Option<u32>,
    /// Optional timeout in seconds
    pub timeout_secs: Option<u64>,
    /// Additional environment variables
    pub env: HashMap<String, String>,
    /// Resolved deny-only MCP policy for this concrete launch.
    pub mcp_launch_policy: super::McpLaunchPolicy,
}

impl Default for AgentConfig {
    fn default() -> Self {
        Self {
            role: AgentRole::Worker,
            prompt: String::new(),
            working_directory: std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
            plugin_dir: Some(PathBuf::from("./plugins/app")),
            agent: None,
            model: None,
            harness: None,
            cli_path_override: None,
            logical_effort: None,
            approval_policy: None,
            sandbox_mode: None,
            service_tier: None,
            max_tokens: None,
            timeout_secs: None,
            env: HashMap::new(),
            mcp_launch_policy: super::McpLaunchPolicy::default(),
        }
    }
}

impl AgentConfig {
    /// Create a new config for a worker agent
    pub fn worker(prompt: impl Into<String>) -> Self {
        Self {
            role: AgentRole::Worker,
            prompt: prompt.into(),
            ..Default::default()
        }
    }

    /// Create a new config for a reviewer agent
    pub fn reviewer(prompt: impl Into<String>) -> Self {
        Self {
            role: AgentRole::Reviewer,
            prompt: prompt.into(),
            ..Default::default()
        }
    }

    /// Create a new config for a QA prep agent
    pub fn qa_prep(prompt: impl Into<String>) -> Self {
        Self {
            role: AgentRole::QaPrep,
            prompt: prompt.into(),
            ..Default::default()
        }
    }

    /// Set the working directory
    pub fn with_working_dir(mut self, path: impl Into<PathBuf>) -> Self {
        self.working_directory = path.into();
        self
    }

    /// Set the model
    pub fn with_model(mut self, model: impl Into<String>) -> Self {
        self.model = Some(model.into());
        self
    }

    /// Set the provider harness.
    pub fn with_harness(mut self, harness: AgentHarnessKind) -> Self {
        self.harness = Some(harness);
        self
    }

    /// Set the provider CLI executable path override.
    pub fn with_cli_path_override(mut self, path: impl Into<PathBuf>) -> Self {
        self.cli_path_override = Some(path.into());
        self
    }

    /// Set the provider-neutral reasoning effort.
    pub fn with_logical_effort(mut self, effort: LogicalEffort) -> Self {
        self.logical_effort = Some(effort);
        self
    }

    /// Set the provider approval policy.
    pub fn with_approval_policy(mut self, approval_policy: impl Into<String>) -> Self {
        self.approval_policy = Some(approval_policy.into());
        self
    }

    /// Set the provider sandbox mode.
    pub fn with_sandbox_mode(mut self, sandbox_mode: impl Into<String>) -> Self {
        self.sandbox_mode = Some(sandbox_mode.into());
        self
    }

    /// Set the provider service tier.
    pub fn with_service_tier(mut self, service_tier: impl Into<String>) -> Self {
        self.service_tier = Some(service_tier.into());
        self
    }

    /// Set the timeout
    pub fn with_timeout(mut self, secs: u64) -> Self {
        self.timeout_secs = Some(secs);
        self
    }

    /// Add an environment variable
    pub fn with_env(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.env.insert(key.into(), value.into());
        self
    }

    /// Set the plugin directory for agent/skill discovery
    pub fn with_plugin_dir(mut self, path: impl Into<PathBuf>) -> Self {
        self.plugin_dir = Some(path.into());
        self
    }

    /// Set the agent name (resolved via plugin discovery)
    pub fn with_agent(mut self, agent: impl Into<String>) -> Self {
        self.agent = Some(agent.into());
        self
    }
}

/// Handle to a spawned agent
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct AgentHandle {
    /// Unique identifier for this agent instance
    pub id: String,
    /// Type of client running this agent
    pub client_type: ClientType,
    /// Role of this agent
    pub role: AgentRole,
}

impl AgentHandle {
    /// Create a new handle with a generated UUID
    pub fn new(client_type: ClientType, role: AgentRole) -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            client_type,
            role,
        }
    }

    /// Create a mock handle for testing
    pub fn mock(role: AgentRole) -> Self {
        Self::new(ClientType::Mock, role)
    }

    /// Create a handle with a specific ID (for testing or restoration)
    pub fn with_id(id: impl Into<String>, client_type: ClientType, role: AgentRole) -> Self {
        Self {
            id: id.into(),
            client_type,
            role,
        }
    }
}

/// Output from a completed agent
#[derive(Debug, Clone, Default)]
pub struct AgentOutput {
    /// Whether the agent completed successfully
    pub success: bool,
    /// Content produced by the agent
    pub content: String,
    /// Exit code if applicable
    pub exit_code: Option<i32>,
    /// Duration in milliseconds
    pub duration_ms: Option<u64>,
}

impl AgentOutput {
    /// Create a successful output
    pub fn success(content: impl Into<String>) -> Self {
        Self {
            success: true,
            content: content.into(),
            exit_code: Some(0),
            duration_ms: None,
        }
    }

    /// Create a failed output
    pub fn failed(content: impl Into<String>, exit_code: i32) -> Self {
        Self {
            success: false,
            content: content.into(),
            exit_code: Some(exit_code),
            duration_ms: None,
        }
    }

    /// Set the duration
    pub fn with_duration(mut self, ms: u64) -> Self {
        self.duration_ms = Some(ms);
        self
    }
}

/// Response from an agent to a prompt
#[derive(Debug, Clone, Default)]
pub struct AgentResponse {
    /// Content of the response
    pub content: String,
    /// Model that generated the response
    pub model: Option<String>,
    /// Number of tokens used
    pub tokens_used: Option<u32>,
}

impl AgentResponse {
    /// Create a new response with content
    pub fn new(content: impl Into<String>) -> Self {
        Self {
            content: content.into(),
            model: None,
            tokens_used: None,
        }
    }

    /// Set the model
    pub fn with_model(mut self, model: impl Into<String>) -> Self {
        self.model = Some(model.into());
        self
    }

    /// Set tokens used
    pub fn with_tokens(mut self, tokens: u32) -> Self {
        self.tokens_used = Some(tokens);
        self
    }
}

/// A chunk from a streaming response
#[derive(Debug, Clone)]
pub struct ResponseChunk {
    /// Content of this chunk
    pub content: String,
    /// Whether this is the final chunk
    pub is_final: bool,
}

impl ResponseChunk {
    /// Create a new non-final chunk
    pub fn new(content: impl Into<String>) -> Self {
        Self {
            content: content.into(),
            is_final: false,
        }
    }

    /// Create a final chunk
    pub fn final_chunk(content: impl Into<String>) -> Self {
        Self {
            content: content.into(),
            is_final: true,
        }
    }
}

#[cfg(test)]
#[path = "types_tests.rs"]
mod tests;
