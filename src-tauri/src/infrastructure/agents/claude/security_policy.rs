use super::ClaudePermissionCliOptions;

pub(crate) const CLAUDE_PROMPT_PERMISSION_MODE: &str = "default";

const PROMPT_GATED_NATIVE_READ_TOOLS: [&str; 3] = ["Read", "Grep", "Glob"];

/// Backend-selected permission behavior for one Claude CLI launch.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ClaudePermissionPolicy {
    /// Preserve the configured MCP compatibility contract.
    InheritConfigured,
    /// Require Claude's permission bridge for standalone native filesystem reads.
    StandalonePromptBoundary,
}

impl ClaudePermissionPolicy {
    pub(super) fn resolve_cli_options(
        self,
        configured: ClaudePermissionCliOptions,
    ) -> ClaudePermissionCliOptions {
        match self {
            Self::InheritConfigured => configured,
            Self::StandalonePromptBoundary => ClaudePermissionCliOptions {
                permission_prompt_tool: configured.permission_prompt_tool,
                permission_mode: CLAUDE_PROMPT_PERMISSION_MODE.to_string(),
                dangerously_skip_permissions: false,
                allow_dangerously_skip_permissions: false,
            },
        }
    }

    pub(super) fn filter_preapproved_tools(self, preapproved: String) -> Option<String> {
        match self {
            Self::InheritConfigured => Some(preapproved),
            Self::StandalonePromptBoundary => {
                let filtered = preapproved
                    .split(',')
                    .filter(|tool| !PROMPT_GATED_NATIVE_READ_TOOLS.contains(tool))
                    .collect::<Vec<_>>()
                    .join(",");
                (!filtered.is_empty()).then_some(filtered)
            }
        }
    }
}
