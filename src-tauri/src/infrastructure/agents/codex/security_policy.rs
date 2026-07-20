use crate::domain::agents::{CODEX_DEFAULT_APPROVAL_POLICY, CODEX_DEFAULT_SANDBOX_MODE};

/// Codex approval policy used only for projectless Standalone Chat conversations.
pub(crate) const CODEX_STANDALONE_APPROVAL_POLICY: &str = "on-request";

/// Codex sandbox used only for projectless Standalone Chat conversations.
pub(crate) const CODEX_STANDALONE_SANDBOX_MODE: &str = "workspace-write";

/// Backend-owned launch security policy for Codex CLI processes.
///
/// Persisted provider/lane values remain compatibility-locked. Callers may select
/// the contained exception only after resolving authoritative conversation context.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CodexLaunchSecurityPolicy {
    /// Existing RalphX launch behavior required by non-standalone MCP workflows.
    McpCompatibility,
    /// Contained private-workspace behavior for Standalone Chat conversations.
    StandaloneContained,
}

impl CodexLaunchSecurityPolicy {
    pub(crate) const fn approval_policy(self) -> &'static str {
        match self {
            Self::McpCompatibility => CODEX_DEFAULT_APPROVAL_POLICY,
            Self::StandaloneContained => CODEX_STANDALONE_APPROVAL_POLICY,
        }
    }

    pub(crate) const fn sandbox_mode(self) -> &'static str {
        match self {
            Self::McpCompatibility => CODEX_DEFAULT_SANDBOX_MODE,
            Self::StandaloneContained => CODEX_STANDALONE_SANDBOX_MODE,
        }
    }
}
