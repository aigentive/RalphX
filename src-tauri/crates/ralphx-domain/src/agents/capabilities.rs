// Client capabilities
// Information about what features an agentic client supports

use super::{harness::AgentHarnessKind, model_registry::built_in_agent_models, types::ClientType};

/// Information about a model available to a client
#[derive(Debug, Clone)]
pub struct ModelInfo {
    /// Model identifier (e.g., "claude-sonnet-4-5-20250929")
    pub id: String,
    /// Human-readable model name (e.g., "Claude Sonnet 4.5")
    pub name: String,
    /// Maximum tokens the model can generate
    pub max_tokens: u32,
}

impl ModelInfo {
    /// Create a new model info
    pub fn new(id: impl Into<String>, name: impl Into<String>, max_tokens: u32) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            max_tokens,
        }
    }
}

/// Capabilities of an agentic client
#[derive(Debug, Clone)]
pub struct ClientCapabilities {
    /// Type of the client
    pub client_type: ClientType,
    /// Whether the client can execute shell commands
    pub supports_shell: bool,
    /// Whether the client can access the filesystem
    pub supports_filesystem: bool,
    /// Whether the client supports streaming responses
    pub supports_streaming: bool,
    /// Whether the client supports MCP (Model Context Protocol)
    pub supports_mcp: bool,
    /// Maximum context tokens supported
    pub max_context_tokens: u32,
    /// Available models
    pub models: Vec<ModelInfo>,
}

impl ClientCapabilities {
    /// Create capabilities for Claude Code client
    pub fn claude_code() -> Self {
        Self {
            client_type: ClientType::ClaudeCode,
            supports_shell: true,
            supports_filesystem: true,
            supports_streaming: true,
            supports_mcp: true,
            max_context_tokens: 200_000,
            models: vec![
                ModelInfo::new("claude-sonnet-4-5-20250929", "Claude Sonnet 4.5", 64_000),
                ModelInfo::new("claude-opus-4-5-20251101", "Claude Opus 4.5", 32_000),
                ModelInfo::new("claude-haiku-4-5-20251001", "Claude Haiku 4.5", 32_000),
            ],
        }
    }

    /// Create capabilities for Codex CLI client.
    pub fn codex() -> Self {
        Self {
            client_type: ClientType::Codex,
            supports_shell: true,
            supports_filesystem: true,
            supports_streaming: true,
            supports_mcp: true,
            max_context_tokens: 1_000_000,
            models: built_in_agent_models()
                .into_iter()
                .filter(|model| model.provider == AgentHarnessKind::Codex)
                .map(|model| ModelInfo::new(model.model_id, model.menu_label, 128_000))
                .collect(),
        }
    }

    /// Create capabilities for mock client
    pub fn mock() -> Self {
        Self {
            client_type: ClientType::Mock,
            supports_shell: true,
            supports_filesystem: true,
            supports_streaming: true,
            supports_mcp: false,
            max_context_tokens: 200_000,
            models: vec![ModelInfo::new("mock", "Mock Model", 100_000)],
        }
    }

    /// Check if a specific model is available
    pub fn has_model(&self, model_id: &str) -> bool {
        self.models.iter().any(|m| m.id == model_id)
    }

    /// Get the default model (first in list)
    pub fn default_model(&self) -> Option<&ModelInfo> {
        self.models.first()
    }

    /// Get a model by ID
    pub fn get_model(&self, model_id: &str) -> Option<&ModelInfo> {
        self.models.iter().find(|m| m.id == model_id)
    }
}

#[cfg(test)]
#[path = "capabilities_tests.rs"]
mod tests;
