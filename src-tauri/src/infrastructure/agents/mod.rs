// Agent implementations
// Infrastructure layer implementations of the AgenticClient trait

pub mod claude;
pub mod codex;
pub mod harness_agent_catalog;
pub mod internal_skills;
pub mod mcp_runtime_context;
pub mod mock;
pub mod spawner;

// Re-export commonly used items
pub use claude::ClaudeCodeClient;
pub use claude::{
    StreamEvent, StreamingSpawnResult, TeammateContext, TeammateSpawnConfig, TeammateSpawnResult,
};
pub use codex::stream_processor::{
    extract_codex_agent_message, extract_codex_command_execution, extract_codex_error,
    extract_codex_error_message, extract_codex_file_change_snapshot, extract_codex_thread_id,
    extract_codex_tool_call_snapshot, extract_codex_usage, parse_codex_event_line,
    CodexCommandExecution, CodexErrorMessage, CodexErrorSource, CodexFileChange,
    CodexFileChangeSnapshot, CodexItem, CodexItemError, CodexStreamEvent, CodexToolCallPhase,
    CodexToolCallSnapshot, CodexUsage,
};
pub use codex::{
    build_codex_exec_args, build_codex_exec_resume_args, build_codex_mcp_overrides,
    build_spawnable_codex_exec_command, build_spawnable_codex_resume_command, compose_codex_prompt,
    find_codex_cli, normalize_codex_exec_output, parse_codex_cli_capabilities, parse_codex_version,
    probe_codex_cli, resolve_codex_cli, CodexCliCapabilities, CodexCliClient, CodexExecCliConfig,
    CodexMcpRuntimeContext, ResolvedCodexCli,
};
pub use mock::{MockAgenticClient, MockCall, MockCallType};
pub use mcp_runtime_context::McpRuntimeContext;
pub use spawner::AgenticClientSpawner;
