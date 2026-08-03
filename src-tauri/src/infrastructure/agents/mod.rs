// Agent implementations
// Infrastructure layer implementations of the AgenticClient trait

pub mod claude;
pub mod codex;
pub mod harness_agent_catalog;
pub mod internal_skills;
mod mcp_launch_policy;
pub mod mcp_runtime_context;
pub mod mock;
pub mod persona_overlay;
pub mod spawn_isolation;
pub mod spawner;

// Re-export commonly used items
pub use claude::ClaudeCodeClient;
// Provider-neutral runtime configuration facade. The implementation remains in
// the legacy Claude config module for persisted/config compatibility, while new
// shared application code imports these values from `infrastructure::agents`.
pub use claude::{
    agent_personas_enabled, limits_config, set_agent_personas_override,
    set_standalone_conversations_override, standalone_conversations_enabled, LimitsConfig,
};
#[cfg(any(test, feature = "test-utils"))]
pub use claude::{
    reset_agent_personas_override_for_test, reset_standalone_conversations_override_for_test,
    LiveFlagOverrideTestGuard,
};
pub use claude::{StreamEvent, StreamingSpawnResult};
pub use codex::stream_processor::{
    extract_codex_agent_message, extract_codex_command_execution, extract_codex_error,
    extract_codex_error_message, extract_codex_file_change_snapshot, extract_codex_reasoning,
    extract_codex_thread_id, extract_codex_tool_call_snapshot,
    extract_codex_turn_reasoning_tokens, extract_codex_usage, parse_codex_event_line,
    CodexCommandExecution, CodexErrorMessage, CodexErrorSource, CodexFileChange,
    CodexFileChangeSnapshot, CodexItem, CodexItemError, CodexStreamEvent, CodexToolCallPhase,
    CodexToolCallSnapshot, CodexUsage, CodexUsagePayload, CodexUsageSnapshot, CodexUsageSource,
};
pub use codex::{
    build_codex_exec_args, build_codex_exec_resume_args, build_codex_mcp_overrides,
    build_codex_mcp_overrides_for_profile, build_spawnable_codex_exec_command,
    build_spawnable_codex_resume_command, compose_codex_prompt, compose_codex_prompt_for_profile,
    find_codex_cli, normalize_codex_exec_output, parse_codex_cli_capabilities, parse_codex_version,
    probe_codex_cli, resolve_codex_cli, CodexCliCapabilities, CodexCliClient, CodexExecCliConfig,
    CodexMcpRuntimeContext, ResolvedCodexCli,
};
pub(crate) use codex::{
    build_spawnable_codex_exec_command_with_security_policy,
    build_spawnable_codex_resume_command_with_security_policy, CodexLaunchSecurityPolicy,
};
pub(crate) use harness_agent_catalog::escape_prompt_context_text;
pub use mcp_launch_policy::apply_mcp_launch_policy;
pub(crate) use mcp_launch_policy::ensure_no_reserved_native_mcp_collision_at;
pub use mcp_runtime_context::McpRuntimeContext;
pub use mock::{MockAgenticClient, MockCall, MockCallType};
pub use spawner::AgenticClientSpawner;

pub fn agent_requires_external_mcp(
    provider: crate::domain::agents::AgentHarnessKind,
    plugin_dir: &std::path::Path,
    agent_name: &str,
    agent_profile: Option<&str>,
) -> Result<bool, String> {
    let project_root = harness_agent_catalog::resolve_project_root_from_plugin_dir(plugin_dir);
    let short_name = claude::mcp_agent_type(agent_name);
    let transport = match provider {
        crate::domain::agents::AgentHarnessKind::Claude => {
            harness_agent_catalog::try_load_canonical_claude_metadata_for_profile(
                &project_root,
                short_name,
                agent_profile,
            )?
            .mcp_transport
        }
        crate::domain::agents::AgentHarnessKind::Codex => {
            harness_agent_catalog::try_load_canonical_codex_metadata_for_profile(
                &project_root,
                short_name,
                agent_profile,
            )?
            .mcp_transport
        }
    };
    Ok(transport.as_deref() == Some("external"))
}

#[cfg(test)]
mod mcp_launch_policy_tests;
