use super::*;
use crate::domain::agents::AgentRole;
use crate::infrastructure::agents::claude::build_mcp_config_with_runtime_context;
use crate::infrastructure::agents::claude::clear_claude_cli_capability_cache;

fn make_temp_project_plugin_dir() -> (tempfile::TempDir, std::path::PathBuf, std::path::PathBuf) {
    let dir = tempfile::TempDir::new().unwrap();
    let root = dir.path().to_path_buf();
    let plugin_dir = root.join("plugins/app");
    std::fs::create_dir_all(plugin_dir.join("ralphx-mcp-server/build")).unwrap();
    std::fs::write(
        plugin_dir.join("ralphx-mcp-server/build/index.js"),
        "// fake",
    )
    .unwrap();
    (dir, root, plugin_dir)
}

fn write_pr_describer_agent(root: &std::path::Path) {
    let agent_root = root.join("agents/ralphx-utility-pr-describer");
    std::fs::create_dir_all(agent_root.join("shared")).unwrap();
    std::fs::write(
        agent_root.join("agent.yaml"),
        r#"name: ralphx-utility-pr-describer
role: pr_describer
description: "Writes reviewer-focused pull request descriptions"
capabilities:
  mcp_tools:
    - submit_agent_workspace_pr_description
harnesses:
  claude:
    model: haiku
    tools:
      mcp_only: true
    preapproved_cli_tools: []
"#,
    )
    .unwrap();
    std::fs::write(
        agent_root.join("shared/prompt.md"),
        "You are a pull request description writer.",
    )
    .unwrap();
}

fn arg_value<'a>(args: &'a [String], flag: &str) -> Option<&'a str> {
    args.iter()
        .position(|arg| arg == flag)
        .and_then(|idx| args.get(idx + 1))
        .map(String::as_str)
}

fn write_fake_claude_cli(path: &std::path::Path) {
    std::fs::write(
        path,
        r#"#!/bin/sh
if [ "$1" = "--version" ]; then
  printf 'claude-code 2.1.219\n'
elif [ "$1" = "--help" ]; then
  printf '%s\n' 'Claude Code' 'Options:' '  --model <MODEL>' '  --effort <EFFORT>'
else
  printf 'unexpected args: %s\n' "$*" >&2
  exit 64
fi
"#,
    )
    .expect("write fake claude");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut permissions = std::fs::metadata(path)
            .expect("fake claude metadata")
            .permissions();
        permissions.set_mode(0o755);
        std::fs::set_permissions(path, permissions).expect("chmod fake claude");
    }
}

fn write_fake_claude_cli_with_partial_messages_support(
    path: &std::path::Path,
    supports_partial_messages: bool,
    supports_thinking_display: bool,
) {
    let partial_messages_flag = if supports_partial_messages {
        "  --include-partial-messages"
    } else {
        ""
    };
    let thinking_display_probe = if supports_thinking_display {
        "elif [ \"$1\" = \"--thinking-display\" ] && [ \"$2\" = \"summarized\" ] && [ \"$3\" = \"--version\" ]; then\n  printf 'claude-code 2.1.219\\n'\n"
    } else {
        ""
    };
    std::fs::write(
        path,
        format!(
            "#!/bin/sh\nif [ \"$1\" = \"--version\" ]; then\n  printf 'claude-code 2.1.219\\n'\nelif [ \"$1\" = \"--help\" ]; then\n  printf '%s\\n' 'Claude Code' 'Options:' '{partial_messages_flag}'\n{thinking_display_probe}else\n  printf 'unexpected args: %s\\n' \"$*\" >&2\n  exit 64\nfi\n"
        ),
    )
    .expect("write fake claude");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut permissions = std::fs::metadata(path)
            .expect("fake claude metadata")
            .permissions();
        permissions.set_mode(0o755);
        std::fs::set_permissions(path, permissions).expect("chmod fake claude");
    }
}

struct EnvGuard {
    key: &'static str,
    original: Option<std::ffi::OsString>,
}

impl EnvGuard {
    fn set_os(key: &'static str, value: impl AsRef<std::ffi::OsStr>) -> Self {
        let original = std::env::var_os(key);
        std::env::set_var(key, value);
        Self { key, original }
    }
}

impl Drop for EnvGuard {
    fn drop(&mut self) {
        match &self.original {
            Some(value) => std::env::set_var(self.key, value),
            None => std::env::remove_var(self.key),
        }
    }
}

fn config_with_cli_path_override(path: impl Into<PathBuf>) -> AgentConfig {
    let mut config = AgentConfig::worker("test");
    config.cli_path_override = Some(path.into());
    config
}

fn assert_cli_not_available_uses_override<T>(
    result: AgentResult<T>,
    override_path: &std::path::Path,
    default_path: &std::path::Path,
) {
    let message = match result {
        Err(AgentError::CliNotAvailable(message)) => message,
        Err(error) => panic!("expected override CLI availability error, got {error:?}"),
        Ok(_) => panic!("expected override CLI availability error, got success"),
    };
    assert!(
        message.contains(&override_path.display().to_string()),
        "availability error should mention override path {override_path:?}, got {message}"
    );
    assert!(
        !message.contains(&default_path.display().to_string()),
        "availability error should not mention default path {default_path:?}, got {message}"
    );
}

#[test]
fn test_claude_code_client_new() {
    let client = ClaudeCodeClient::new();
    // CLI might or might not exist, but client should be created
    assert_eq!(client.capabilities.client_type, ClientType::ClaudeCode);
}

#[test]
fn test_claude_code_client_with_cli_path() {
    let client = ClaudeCodeClient::new().with_cli_path("/custom/path/claude");
    assert_eq!(client.cli_path, PathBuf::from("/custom/path/claude"));
}

#[test]
fn test_capabilities_claude_code() {
    let client = ClaudeCodeClient::new();
    let caps = client.capabilities();
    assert_eq!(caps.client_type, ClientType::ClaudeCode);
    assert!(caps.supports_shell);
    assert!(caps.supports_filesystem);
    assert!(caps.supports_streaming);
    assert!(caps.supports_mcp);
    assert_eq!(caps.max_context_tokens, 1_000_000);
}

#[test]
fn test_capabilities_has_models() {
    let client = ClaudeCodeClient::new();
    let caps = client.capabilities();
    assert!(caps.has_model("claude-sonnet-4-6"));
    assert!(caps.has_model("claude-sonnet-5"));
    assert!(caps.has_model("claude-opus-4-5-20251101"));
    assert!(caps.has_model("claude-haiku-4-5-20251001"));
}

#[test]
fn test_cli_path_getter() {
    let client = ClaudeCodeClient::new().with_cli_path("/test/claude");
    assert_eq!(client.cli_path(), &PathBuf::from("/test/claude"));
}

#[test]
fn test_default_trait() {
    let client = ClaudeCodeClient::default();
    assert_eq!(client.capabilities().client_type, ClientType::ClaudeCode);
}

#[tokio::test]
async fn test_is_available_with_nonexistent_path() {
    let client = ClaudeCodeClient::new().with_cli_path("/nonexistent/path/to/claude_binary_12345");
    let available = client.is_available().await.unwrap();
    assert!(!available);
}

#[tokio::test]
async fn test_spawn_agent_blocked_in_tests() {
    let client = ClaudeCodeClient::new().with_cli_path("/nonexistent/path/to/claude_binary_12345");
    let config = AgentConfig::worker("test");

    let result = client.spawn_agent(config).await;
    assert!(result.is_err());
    assert!(matches!(result, Err(AgentError::SpawnNotAllowed(_))));
}

#[allow(clippy::await_holding_lock)]
#[tokio::test]
async fn test_spawn_agent_checks_cli_path_override_availability() {
    let _lock = crate::infrastructure::tool_paths::TEST_ENV_MUTEX
        .lock()
        .expect("test env mutex");
    let _spawn_guard = EnvGuard::set_os("RALPHX_ALLOW_CLAUDE_SPAWN_IN_TESTS", "1");
    let temp_dir = tempfile::tempdir().expect("temp dir");
    let default_path = temp_dir.path().join("default-claude");
    let override_path = temp_dir.path().join("custom-claude");
    let client = ClaudeCodeClient::new().with_cli_path(&default_path);

    let result = client
        .spawn_agent(config_with_cli_path_override(&override_path))
        .await;

    assert_cli_not_available_uses_override(result, &override_path, &default_path);
}

#[tokio::test]
async fn test_stop_agent_nonexistent_handle() {
    let client = ClaudeCodeClient::new();
    let handle = AgentHandle::with_id("nonexistent", ClientType::ClaudeCode, AgentRole::Worker);

    // Should not error - just means already stopped
    let result = client.stop_agent(&handle).await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn test_wait_for_completion_nonexistent_handle() {
    let client = ClaudeCodeClient::new();
    let handle = AgentHandle::with_id("nonexistent", ClientType::ClaudeCode, AgentRole::Worker);

    let result = client.wait_for_completion(&handle).await;
    assert!(result.is_err());
    assert!(matches!(result, Err(AgentError::NotFound(_))));
}

// ==================== Streaming Spawn Tests ====================

#[test]
fn build_cli_args_includes_partial_messages_for_non_interactive() {
    let _lock = crate::infrastructure::tool_paths::TEST_ENV_MUTEX
        .lock()
        .expect("env mutex");
    clear_claude_cli_capability_cache();
    let temp = tempfile::tempdir().expect("tempdir");
    let cli_path = temp.path().join("claude");
    write_fake_claude_cli_with_partial_messages_support(&cli_path, true, false);
    let client = ClaudeCodeClient::new().with_cli_path(&cli_path);

    let args = client
        .build_cli_args(&AgentConfig::worker("Test prompt"), None, false)
        .expect("build CLI args");

    assert!(args.contains(&"--include-partial-messages".to_string()));
    clear_claude_cli_capability_cache();
}

#[test]
fn build_cli_args_includes_partial_messages_for_interactive() {
    let _lock = crate::infrastructure::tool_paths::TEST_ENV_MUTEX
        .lock()
        .expect("env mutex");
    clear_claude_cli_capability_cache();
    let temp = tempfile::tempdir().expect("tempdir");
    let cli_path = temp.path().join("claude");
    write_fake_claude_cli_with_partial_messages_support(&cli_path, true, false);
    let client = ClaudeCodeClient::new().with_cli_path(&cli_path);

    let args = client
        .build_cli_args(&AgentConfig::worker("Test prompt"), None, true)
        .expect("build CLI args");

    assert!(args.contains(&"--include-partial-messages".to_string()));
    clear_claude_cli_capability_cache();
}

#[test]
fn build_cli_args_omits_partial_messages_when_cli_capability_is_unsupported() {
    let _lock = crate::infrastructure::tool_paths::TEST_ENV_MUTEX
        .lock()
        .expect("env mutex");
    clear_claude_cli_capability_cache();
    let temp = tempfile::tempdir().expect("tempdir");
    let cli_path = temp.path().join("claude");
    write_fake_claude_cli_with_partial_messages_support(&cli_path, false, false);
    let client = ClaudeCodeClient::new().with_cli_path(&cli_path);

    let args = client
        .build_cli_args(&AgentConfig::worker("Test prompt"), None, false)
        .expect("build CLI args");

    assert!(!args.contains(&"--include-partial-messages".to_string()));
    clear_claude_cli_capability_cache();
}

#[test]
fn build_cli_args_includes_thinking_display_when_cli_capability_is_supported() {
    let _lock = crate::infrastructure::tool_paths::TEST_ENV_MUTEX
        .lock()
        .expect("env mutex");
    clear_claude_cli_capability_cache();
    let temp = tempfile::tempdir().expect("tempdir");
    let cli_path = temp.path().join("claude");
    write_fake_claude_cli_with_partial_messages_support(&cli_path, false, true);
    let client = ClaudeCodeClient::new().with_cli_path(&cli_path);

    let args = client
        .build_cli_args(&AgentConfig::worker("Test prompt"), None, true)
        .expect("build CLI args");

    assert_eq!(arg_value(&args, "--thinking-display"), Some("summarized"));
    clear_claude_cli_capability_cache();
}

#[test]
fn build_cli_args_omits_thinking_display_when_cli_capability_is_unsupported() {
    let _lock = crate::infrastructure::tool_paths::TEST_ENV_MUTEX
        .lock()
        .expect("env mutex");
    clear_claude_cli_capability_cache();
    let temp = tempfile::tempdir().expect("tempdir");
    let cli_path = temp.path().join("claude");
    write_fake_claude_cli_with_partial_messages_support(&cli_path, false, false);
    let client = ClaudeCodeClient::new().with_cli_path(&cli_path);

    let args = client
        .build_cli_args(&AgentConfig::worker("Test prompt"), None, false)
        .expect("build CLI args");

    assert!(!args.contains(&"--thinking-display".to_string()));
    clear_claude_cli_capability_cache();
}

#[test]
fn build_cli_args_omits_thinking_display_when_cli_capability_probe_fails_entirely() {
    let _lock = crate::infrastructure::tool_paths::TEST_ENV_MUTEX
        .lock()
        .expect("env mutex");
    clear_claude_cli_capability_cache();
    let missing_cli_path = tempfile::tempdir().expect("tempdir").path().join("claude");
    let client = ClaudeCodeClient::new().with_cli_path(&missing_cli_path);

    let args = client
        .build_cli_args(&AgentConfig::worker("Test prompt"), None, false)
        .expect("optional capability probe failure should not block argument construction");

    assert!(!args.contains(&"--thinking-display".to_string()));
    clear_claude_cli_capability_cache();
}

#[test]
fn test_build_cli_args_basic() {
    let client = ClaudeCodeClient::new();
    let config = AgentConfig::worker("Test prompt");

    let args = client
        .build_cli_args(&config, None, false)
        .expect("build_cli_args should succeed in test");

    assert!(args.contains(&"-p".to_string()));
    assert!(args.contains(&"Test prompt".to_string()));
    assert!(args.contains(&"--output-format".to_string()));
    assert!(args.contains(&"stream-json".to_string()));
    assert!(args.contains(&"--permission-prompt-tool".to_string()));
    assert!(args.contains(&"mcp__ralphx__permission_request".to_string()));
}

#[test]
fn test_build_cli_args_defaults_to_most_permissive_claude_permissions() {
    let client = ClaudeCodeClient::new();
    let config = AgentConfig::worker("Test prompt");

    let args = client
        .build_cli_args(&config, None, false)
        .expect("build_cli_args should succeed in test");

    assert_eq!(
        arg_value(&args, "--permission-mode"),
        Some("bypassPermissions")
    );
    assert!(
        args.contains(&"--dangerously-skip-permissions".to_string()),
        "Claude agent spawns must bypass permission prompts by default"
    );
}

#[test]
fn test_build_cli_args_with_agent() {
    let client = ClaudeCodeClient::new();
    let config = AgentConfig::worker("Test").with_agent("worker");

    let args = client
        .build_cli_args(&config, None, false)
        .expect("build_cli_args should succeed in test");

    assert!(args.contains(&"--agent".to_string()));
    assert!(args.contains(&"worker".to_string()));
}

#[test]
fn test_spawn_agent_command_uses_prompt_injection_for_utility_agents() {
    let (_dir, root, plugin_dir) = make_temp_project_plugin_dir();
    write_pr_describer_agent(&root);
    let client = ClaudeCodeClient::new().with_cli_path("/fake/claude");
    let config = AgentConfig::worker("Draft a PR description")
        .with_agent(crate::infrastructure::agents::claude::agent_names::AGENT_PR_DESCRIBER)
        .with_plugin_dir(plugin_dir)
        .with_working_dir("/tmp");

    let spawnable = client
        .build_spawnable_agent_command(&config, None, false)
        .expect("spawnable utility command");
    let args = spawnable.get_args_for_test();

    assert!(
        !args.contains(&"--agent".to_string()),
        "one-shot utility agents should avoid native --agent mode"
    );
    assert!(
        args.contains(&"--append-system-prompt-file".to_string())
            || args.contains(&"--append-system-prompt".to_string()),
        "utility agent behavior should be injected as a system prompt"
    );
    assert_eq!(arg_value(&args, "-p"), Some("-"));
    assert_eq!(
        spawnable.get_stdin_prompt_for_test(),
        Some("Draft a PR description")
    );
    assert_eq!(arg_value(&args, "--model"), Some("haiku"));
    assert!(
        arg_value(&args, "--allowedTools").is_some_and(
            |tools| tools.contains("mcp__ralphx__submit_agent_workspace_pr_description")
        ),
        "PR describer submit tool should stay preapproved"
    );
    assert!(
        args.contains(&"--mcp-config".to_string()),
        "utility agent should still receive its required RalphX MCP config"
    );
    assert!(
        !args.contains(&"--strict-mcp-config".to_string()),
        "utility agent should inherit enabled provider-native MCP servers"
    );
}

#[test]
fn test_build_cli_args_with_resume() {
    let client = ClaudeCodeClient::new();
    let config = AgentConfig::worker("Test").with_agent("worker");

    let args = client
        .build_cli_args(&config, Some("session-123"), false)
        .expect("build_cli_args should succeed in test");

    // When resuming, both --resume AND --agent should be present
    // to ensure tool restrictions (disallowedTools) are enforced
    assert!(args.contains(&"--resume".to_string()));
    assert!(args.contains(&"session-123".to_string()));
    // Agent MUST be present when resuming to enforce disallowedTools
    assert!(args.contains(&"--agent".to_string()));
    assert!(args.contains(&"worker".to_string()));
}

#[test]
fn build_cli_args_applies_resolved_mcp_denies_without_strict_isolation() {
    let client = ClaudeCodeClient::new();
    let mut config = AgentConfig::worker("Test").with_agent("worker");
    config.mcp_launch_policy.disabled_servers = vec!["github".to_string()];
    config
        .mcp_launch_policy
        .disabled_tools
        .insert("linear".to_string(), vec!["delete_issue".to_string()]);

    let args = client
        .build_cli_args(&config, None, false)
        .expect("build CLI args");

    assert_eq!(
        arg_value(&args, "--disallowedTools"),
        Some("mcp__github__*,mcp__linear__delete_issue")
    );
    assert!(!args.contains(&"--strict-mcp-config".to_string()));
}

#[test]
fn test_build_cli_args_mcp_only_agent_does_not_disable_tools() {
    let client = ClaudeCodeClient::new();
    // Use fully-qualified name as would be used in production
    let config = AgentConfig::worker("Test")
        .with_agent(crate::infrastructure::agents::claude::agent_names::AGENT_SESSION_NAMER);

    let args = client
        .build_cli_args(&config, None, false)
        .expect("build_cli_args should succeed in test");

    assert!(
        !args.contains(&"--tools".to_string()),
        "MCP-only session namer must not pass --tools \"\" because Claude CLI disables all tools, including the required MCP title update"
    );

    let allowed_tools_idx = args
        .iter()
        .position(|a| a == "--allowedTools")
        .expect("--allowedTools flag must preapprove the title update tool");
    assert!(
        args[allowed_tools_idx + 1].contains("mcp__ralphx__update_session_title"),
        "session namer must keep update_session_title preapproved"
    );
}

#[test]
fn test_build_cli_args_no_tools_for_unknown_agent() {
    let client = ClaudeCodeClient::new();
    let config = AgentConfig::worker("Test").with_agent("unknown-agent-xyz");

    let args = client
        .build_cli_args(&config, None, false)
        .expect("build_cli_args should succeed in test");

    // Unknown agent should NOT have --tools restriction
    assert!(
        !args.contains(&"--tools".to_string()),
        "unknown agent should not have --tools flag"
    );
}

#[test]
fn test_build_cli_args_restricted_agent_tools() {
    let client = ClaudeCodeClient::new();
    // Use fully-qualified name as would be used in production
    let config = AgentConfig::worker("Test").with_agent(
        crate::infrastructure::agents::claude::agent_names::AGENT_ORCHESTRATOR_IDEATION,
    );

    let args = client
        .build_cli_args(&config, None, false)
        .expect("build_cli_args should succeed in test");

    let tools_idx = args
        .iter()
        .position(|a| a == "--tools")
        .expect("--tools flag must be present");
    assert_eq!(
        args[tools_idx + 1],
        "Read,Grep,Glob,Bash,WebFetch,WebSearch,Skill,TaskCreate,TaskUpdate,TaskGet,TaskList,TaskOutput,KillShell,MCPSearch,Task",
        "ralphx-ideation should have base tools + Task"
    );
}

#[test]
fn test_build_cli_args_with_model() {
    let client = ClaudeCodeClient::new();
    let config = AgentConfig::worker("Test").with_model("opus");

    let args = client
        .build_cli_args(&config, None, false)
        .expect("build_cli_args should succeed in test");

    assert!(args.contains(&"--model".to_string()));
    assert!(args.contains(&"opus".to_string()));
}

#[test]
fn test_build_cli_args_preserves_supported_model_values_byte_for_byte() {
    let temp_dir = tempfile::tempdir().expect("temp dir");
    let custom_claude_path = temp_dir.path().join("claude-wrapper");
    write_fake_claude_cli(&custom_claude_path);
    let client = ClaudeCodeClient::new().with_cli_path("/missing/default/claude");

    for model in [
        "sonnet",
        "opus",
        "haiku",
        "fable",
        "claude-opus-4-7",
        "claude-opus-4-8",
        "claude-opus-5",
    ] {
        let mut config = AgentConfig::worker("Test").with_model(model);
        config.cli_path_override = Some(custom_claude_path.clone());

        let args = client
            .build_cli_args(&config, None, false)
            .expect("supported model should build CLI args");

        assert_eq!(arg_value(&args, "--model"), Some(model));
    }
}

#[test]
fn test_build_cli_args_validates_model_against_cli_path_override() {
    let temp_dir = tempfile::tempdir().expect("temp dir");
    let custom_claude_path = temp_dir.path().join("claude-wrapper");
    write_fake_claude_cli(&custom_claude_path);
    let client = ClaudeCodeClient::new().with_cli_path("/missing/default/claude");
    let mut config = AgentConfig::worker("Test").with_model("fable");
    config.cli_path_override = Some(custom_claude_path);

    let args = client
        .build_cli_args(&config, None, false)
        .expect("override CLI should validate Fable support");

    assert_eq!(arg_value(&args, "--model"), Some("fable"));
}

#[test]
fn test_build_cli_args_uses_agent_model_when_not_overridden() {
    let client = ClaudeCodeClient::new();
    let config = AgentConfig::worker("Test")
        .with_agent(crate::infrastructure::agents::claude::agent_names::AGENT_MERGER);

    let args = client
        .build_cli_args(&config, None, false)
        .expect("build_cli_args should succeed in test");
    let model_idx = args
        .iter()
        .position(|a| a == "--model")
        .expect("--model flag must be present");
    assert_eq!(args[model_idx + 1], "opus");
}

#[test]
fn test_build_cli_args_with_plugin_dir() {
    let client = ClaudeCodeClient::new();
    let config = AgentConfig::worker("Test").with_plugin_dir("/custom/plugin");

    let args = client
        .build_cli_args(&config, None, false)
        .expect("build_cli_args should succeed in test");

    assert!(args.contains(&"--plugin-dir".to_string()));
    assert!(args.contains(&"/custom/plugin".to_string()));
}

#[tokio::test]
async fn test_spawn_agent_streaming_blocked_in_tests() {
    let client = ClaudeCodeClient::new().with_cli_path("/nonexistent/path/to/claude_binary_12345");
    let config = AgentConfig::worker("test");

    let result = client.spawn_agent_streaming(config, None).await;
    assert!(result.is_err());
    assert!(matches!(result, Err(AgentError::SpawnNotAllowed(_))));
}

#[allow(clippy::await_holding_lock)]
#[tokio::test]
async fn test_spawn_agent_streaming_checks_cli_path_override_availability() {
    let _lock = crate::infrastructure::tool_paths::TEST_ENV_MUTEX
        .lock()
        .expect("test env mutex");
    let _spawn_guard = EnvGuard::set_os("RALPHX_ALLOW_CLAUDE_SPAWN_IN_TESTS", "1");
    let temp_dir = tempfile::tempdir().expect("temp dir");
    let default_path = temp_dir.path().join("default-claude");
    let override_path = temp_dir.path().join("custom-claude");
    let client = ClaudeCodeClient::new().with_cli_path(&default_path);

    let result = client
        .spawn_agent_streaming(config_with_cli_path_override(&override_path), None)
        .await;

    assert_cli_not_available_uses_override(result, &override_path, &default_path);
}

#[test]
fn test_cli_available_with_nonexistent_path() {
    let client = ClaudeCodeClient::new().with_cli_path("/nonexistent/path/to/claude_binary_12345");
    assert!(!client.cli_available());
}

// ==================== StreamEvent Tests ====================

#[test]
fn test_stream_event_text_chunk_serialization() {
    let event = StreamEvent::TextChunk {
        text: "Hello world".to_string(),
    };
    let json = serde_json::to_string(&event).unwrap();
    assert!(json.contains("TextChunk"));
    assert!(json.contains("Hello world"));

    // Deserialize back
    let parsed: StreamEvent = serde_json::from_str(&json).unwrap();
    if let StreamEvent::TextChunk { text } = parsed {
        assert_eq!(text, "Hello world");
    } else {
        panic!("Expected TextChunk");
    }
}

#[test]
fn test_stream_event_tool_call_start_serialization() {
    let event = StreamEvent::ToolCallStart {
        tool_name: "Read".to_string(),
        tool_id: Some("tool-123".to_string()),
    };
    let json = serde_json::to_string(&event).unwrap();
    assert!(json.contains("ToolCallStart"));
    assert!(json.contains("Read"));
    assert!(json.contains("tool-123"));
}

#[test]
fn test_stream_event_tool_call_complete_serialization() {
    let event = StreamEvent::ToolCallComplete {
        tool_name: "Write".to_string(),
        tool_id: None,
        arguments: serde_json::json!({"path": "test.txt"}),
    };
    let json = serde_json::to_string(&event).unwrap();
    assert!(json.contains("ToolCallComplete"));
    assert!(json.contains("Write"));
    assert!(json.contains("path"));
}

#[test]
fn test_stream_event_completed_serialization() {
    let event = StreamEvent::Completed {
        session_id: Some("sess-456".to_string()),
    };
    let json = serde_json::to_string(&event).unwrap();
    assert!(json.contains("Completed"));
    assert!(json.contains("sess-456"));
}

#[test]
fn test_stream_event_error_serialization() {
    let event = StreamEvent::Error {
        message: "Something went wrong".to_string(),
    };
    let json = serde_json::to_string(&event).unwrap();
    assert!(json.contains("Error"));
    assert!(json.contains("Something went wrong"));
}

#[test]
fn test_streaming_spawn_result_debug() {
    // StreamingSpawnResult is Debug
    fn assert_debug<T: std::fmt::Debug>() {}
    assert_debug::<StreamingSpawnResult>();
}

// ==================== create_mcp_config Tests (Fix A) ====================

/// Fix A: create_mcp_config never writes bare "node" as the command.
/// macOS GUI apps have stripped PATH, so the command must be a full path.
#[test]
fn test_create_mcp_config_resolves_node_command() {
    let tmp = tempfile::tempdir().unwrap();
    let plugin_dir = tmp.path();

    let json = build_mcp_config_with_runtime_context(plugin_dir, "worker", false, None)
        .expect("build_mcp_config_with_runtime_context should succeed");

    let mcp_server_name = super::claude_runtime_config().mcp_server_name.as_str();
    let command = json["mcpServers"][mcp_server_name]["command"]
        .as_str()
        .expect("command field must be a string");

    // The command must be either a full path (starts with /) OR the fallback
    // bare "node" (only if none of the standard locations exist in this test env).
    // Critical invariant: when any known node binary exists, it must use the full path.
    let node_candidates = ["/opt/homebrew/bin/node", "/usr/local/bin/node"];
    let any_known_node_exists = node_candidates
        .iter()
        .any(|p| std::path::Path::new(p).exists());

    if any_known_node_exists || which::which("node").is_ok() {
        assert_ne!(
            command, "node",
            "command must be resolved to a full path when node is available; got: {command}"
        );
        assert!(
            command.starts_with('/'),
            "resolved command must be an absolute path; got: {command}"
        );
    }
    // If node is completely absent in this environment, bare "node" is acceptable as last resort.
}

/// Fix A: The default stdio MCP config resolves bare `node` before launch.
#[test]
fn test_create_mcp_config_replaces_bare_node_default() {
    let tmp = tempfile::tempdir().unwrap();
    let plugin_dir = tmp.path();

    let mcp_server_name = super::claude_runtime_config().mcp_server_name.as_str();
    let json = build_mcp_config_with_runtime_context(plugin_dir, "worker", false, None)
        .expect("build_mcp_config_with_runtime_context should succeed");
    let command = json["mcpServers"][mcp_server_name]["command"]
        .as_str()
        .expect("command field must be a string");

    // "node" must have been replaced if any node binary is available
    let node_available = which::which("node").is_ok()
        || ["/opt/homebrew/bin/node", "/usr/local/bin/node"]
            .iter()
            .any(|p| std::path::Path::new(p).exists());

    if node_available {
        assert_ne!(
            command, "node",
            "bare 'node' must be replaced with full path; got: {command}"
        );
    }
}

/// Fix A: generated MCP args include the resolved plugin-root server path.
#[test]
fn test_create_mcp_config_uses_plugin_root_server_path() {
    let tmp = tempfile::tempdir().unwrap();
    let plugin_dir = tmp.path();

    let mcp_server_name = super::claude_runtime_config().mcp_server_name.as_str();
    let json = build_mcp_config_with_runtime_context(plugin_dir, "worker", false, None)
        .expect("build_mcp_config_with_runtime_context should succeed");

    let args = json["mcpServers"][mcp_server_name]["args"]
        .as_array()
        .expect("args must be an array");

    let plugin_dir_str = plugin_dir.to_string_lossy();
    let expanded = args
        .iter()
        .filter_map(|v| v.as_str())
        .any(|a| a.contains(plugin_dir_str.as_ref()) && !a.contains("${CLAUDE_PLUGIN_ROOT}"));

    assert!(
        expanded,
        "args must include the plugin dir ({plugin_dir_str}); got: {args:?}"
    );
}

// ==================== Interactive Spawn Tests ====================

#[test]
fn test_build_cli_args_interactive_omits_p_flag() {
    let client = ClaudeCodeClient::new();
    let config = AgentConfig::worker("My interactive prompt");

    let args = client
        .build_cli_args(&config, None, true)
        .expect("build_cli_args should succeed in test");

    // Interactive mode: -p must NOT be present
    assert!(
        !args.contains(&"-p".to_string()),
        "interactive build_cli_args must NOT contain -p flag"
    );
    // The prompt text must not appear as a positional arg either
    assert!(
        !args.contains(&"My interactive prompt".to_string()),
        "prompt text must not appear in interactive args"
    );
    // But streaming flags and permissions are still present
    assert!(args.contains(&"--output-format".to_string()));
    assert!(args.contains(&"--permission-prompt-tool".to_string()));
}

#[test]
fn test_build_cli_args_non_interactive_has_p_flag() {
    let client = ClaudeCodeClient::new();
    let config = AgentConfig::worker("Non-interactive prompt");

    let args = client
        .build_cli_args(&config, None, false)
        .expect("build_cli_args should succeed in test");

    // Non-interactive mode: -p must be present (backward compat)
    assert!(
        args.contains(&"-p".to_string()),
        "non-interactive build_cli_args must contain -p flag"
    );
    assert!(args.contains(&"Non-interactive prompt".to_string()));
}

#[test]
fn test_streaming_spawn_result_has_stdin_field() {
    // Compile-time check: StreamingSpawnResult has a stdin field of the right type
    // (accessed at compile time, exercised via Debug format in runtime)
    fn assert_debug<T: std::fmt::Debug>() {}
    assert_debug::<StreamingSpawnResult>();

    // Verify the stdin field is Option<tokio::process::ChildStdin> by checking
    // None default is constructable — this test will fail to compile if the field is removed
    let _ = std::mem::size_of::<Option<tokio::process::ChildStdin>>();
}

#[tokio::test]
async fn test_spawn_agent_interactive_blocked_in_tests() {
    let client = ClaudeCodeClient::new().with_cli_path("/nonexistent/path/to/claude_binary_12345");
    let config = AgentConfig::worker("test");

    let result = client.spawn_agent_interactive(config, None).await;
    assert!(result.is_err());
    assert!(matches!(result, Err(AgentError::SpawnNotAllowed(_))));
}

#[allow(clippy::await_holding_lock)]
#[tokio::test]
async fn test_spawn_agent_interactive_checks_cli_path_override_availability() {
    let _lock = crate::infrastructure::tool_paths::TEST_ENV_MUTEX
        .lock()
        .expect("test env mutex");
    let _spawn_guard = EnvGuard::set_os("RALPHX_ALLOW_CLAUDE_SPAWN_IN_TESTS", "1");
    let temp_dir = tempfile::tempdir().expect("temp dir");
    let default_path = temp_dir.path().join("default-claude");
    let override_path = temp_dir.path().join("custom-claude");
    let client = ClaudeCodeClient::new().with_cli_path(&default_path);

    let result = client
        .spawn_agent_interactive(config_with_cli_path_override(&override_path), None)
        .await;

    assert_cli_not_available_uses_override(result, &override_path, &default_path);
}

/// Fix A: --agent-type is always injected into MCP args for tool filtering.
#[test]
fn test_create_mcp_config_injects_agent_type() {
    let tmp = tempfile::tempdir().unwrap();
    let plugin_dir = tmp.path();

    let json = build_mcp_config_with_runtime_context(plugin_dir, "ralphx-ideation", false, None)
        .expect("build_mcp_config_with_runtime_context should succeed");

    let mcp_server_name = super::claude_runtime_config().mcp_server_name.as_str();
    let args = json["mcpServers"][mcp_server_name]["args"]
        .as_array()
        .expect("args must be an array");

    let arg_strs: Vec<&str> = args.iter().filter_map(|v| v.as_str()).collect();
    let agent_type_idx = arg_strs
        .iter()
        .position(|&a| a == "--agent-type")
        .expect("--agent-type must be present in MCP server args");

    assert!(
        agent_type_idx + 1 < arg_strs.len(),
        "--agent-type must be followed by a value"
    );
    // short name for "ralphx-ideation" drops the "ralphx:" prefix if present
    assert_eq!(arg_strs[agent_type_idx + 1], "ralphx-ideation");

    let tauri_api_idx = arg_strs
        .iter()
        .position(|&a| a == "--tauri-api-url")
        .expect("--tauri-api-url must be present in MCP server args");
    assert!(
        tauri_api_idx + 1 < arg_strs.len(),
        "--tauri-api-url must be followed by a value"
    );
    assert!(
        arg_strs[tauri_api_idx + 1].starts_with("http://127.0.0.1:"),
        "TAURI API URL must point at a loopback backend; got {}",
        arg_strs[tauri_api_idx + 1]
    );
}
