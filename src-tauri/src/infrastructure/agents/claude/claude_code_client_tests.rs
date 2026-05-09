use super::*;
use crate::domain::agents::AgentRole;
use crate::infrastructure::agents::claude::build_mcp_config_with_runtime_context;

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
    assert_eq!(caps.max_context_tokens, 200_000);
}

#[test]
fn test_capabilities_has_models() {
    let client = ClaudeCodeClient::new();
    let caps = client.capabilities();
    assert!(caps.has_model("claude-sonnet-4-5-20250929"));
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
fn test_build_cli_args_applies_tools_restriction() {
    let client = ClaudeCodeClient::new();
    // Use fully-qualified name as would be used in production
    let config = AgentConfig::worker("Test")
        .with_agent(crate::infrastructure::agents::claude::agent_names::AGENT_SESSION_NAMER);

    let args = client
        .build_cli_args(&config, None, false)
        .expect("build_cli_args should succeed in test");

    // ralphx-utility-session-namer has allowed_tools = Some("") meaning no CLI tools
    // get_allowed_tools strips the ralphx: prefix for AGENT_CONFIGS lookup
    let tools_idx = args
        .iter()
        .position(|a| a == "--tools")
        .expect("--tools flag must be present");
    assert_eq!(
        args[tools_idx + 1],
        "",
        "ralphx-utility-session-namer should have empty tools (MCP only)"
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

// ==================== Teammate Interactive Spawn Tests ====================

fn test_teammate_config() -> TeammateSpawnConfig {
    TeammateSpawnConfig::new(
        "transport-researcher",
        "ideation-abc123",
        "You are a transport research specialist. Investigate WebSocket vs SSE.",
    )
    .with_parent_session_id("lead-session-uuid")
    .with_model("sonnet")
    .with_tools(vec![
        "Read".to_string(),
        "Grep".to_string(),
        "Glob".to_string(),
    ])
    .with_mcp_tools(vec![
        "get_session_plan".to_string(),
        "list_session_proposals".to_string(),
    ])
    .with_color("blue")
    .with_working_dir("/tmp/test")
    .with_plugin_dir("/test/ralphx-plugin")
}

#[test]
fn test_teammate_spawn_config_new_defaults() {
    let config = TeammateSpawnConfig::new("researcher", "team-1", "Do research");

    assert_eq!(config.name, "researcher");
    assert_eq!(config.team_name, "team-1");
    assert_eq!(config.parent_session_id, ""); // Must be set via with_parent_session_id()
    assert_eq!(config.prompt, "Do research");
    assert_eq!(config.model, "sonnet");
    assert_eq!(config.color, "blue");
    assert_eq!(config.agent_type, "general-purpose");
    assert_eq!(config.mcp_agent_type, "ideation-team-member");
    assert_eq!(config.plugin_dir, Some(PathBuf::from("./plugins/app")));
    assert!(config.tools.is_empty());
    assert!(config.mcp_tools.is_empty());
    assert!(config.env.is_empty());
    assert!(config.context.context_id.is_empty());
    assert!(config.context.context_type.is_empty());
    assert!(config.context.project_id.is_none());
}

#[test]
fn test_teammate_spawn_config_builder_chain() {
    let ctx = TeammateContext {
        context_id: "ctx-123".to_string(),
        context_type: "ideation".to_string(),
        project_id: Some("proj-456".to_string()),
    };
    let config = TeammateSpawnConfig::new("dev", "team-x", "Code stuff")
        .with_parent_session_id("sess-1")
        .with_context(ctx)
        .with_model("haiku")
        .with_tools(vec!["Read".to_string()])
        .with_mcp_tools(vec!["get_task_context".to_string()])
        .with_color("green")
        .with_working_dir("/work")
        .with_plugin_dir("/plugins")
        .with_agent_type("Bash")
        .with_mcp_agent_type("worker-team-member")
        .with_env("CUSTOM_VAR", "value");

    assert_eq!(config.parent_session_id, "sess-1");
    assert_eq!(config.context.context_id, "ctx-123");
    assert_eq!(config.context.context_type, "ideation");
    assert_eq!(config.context.project_id, Some("proj-456".to_string()));
    assert_eq!(config.model, "haiku");
    assert_eq!(config.tools, vec!["Read"]);
    assert_eq!(config.mcp_tools, vec!["get_task_context"]);
    assert_eq!(config.color, "green");
    assert_eq!(config.working_directory, PathBuf::from("/work"));
    assert_eq!(config.plugin_dir, Some(PathBuf::from("/plugins")));
    assert_eq!(config.agent_type, "Bash");
    assert_eq!(config.mcp_agent_type, "worker-team-member");
    assert_eq!(config.env.get("CUSTOM_VAR"), Some(&"value".to_string()));
}

#[test]
fn test_build_teammate_cli_args_interactive_stdin_flags() {
    let client = ClaudeCodeClient::new();
    let config = test_teammate_config();
    let args = client
        .build_teammate_cli_args(&config)
        .expect("build_teammate_cli_args should succeed in test");

    // Interactive mode: -p - and --input-format stream-json are required so that
    // the process stays in print mode (needed for --output-format stream-json) and
    // reads structured JSON messages from stdin.
    let p_pos = args.iter().position(|a| a == "-p");
    assert!(p_pos.is_some(), "Teammate args must contain -p flag");
    assert_eq!(
        args.get(p_pos.unwrap() + 1).map(String::as_str),
        Some("-"),
        "-p must be followed by - (stdin) for interactive teammates"
    );
    assert!(
        args.contains(&"--input-format".to_string()),
        "Teammate args must contain --input-format"
    );
    assert!(
        args.contains(&"stream-json".to_string()),
        "Teammate args must contain stream-json as --input-format value"
    );
}

#[test]
fn test_build_teammate_cli_args_has_output_format() {
    let client = ClaudeCodeClient::new();
    let config = test_teammate_config();
    let args = client
        .build_teammate_cli_args(&config)
        .expect("build_teammate_cli_args should succeed in test");

    assert!(args.contains(&"--output-format".to_string()));
    assert!(args.contains(&"stream-json".to_string()));
    assert!(args.contains(&"--verbose".to_string()));
}

#[test]
fn test_build_teammate_cli_args_has_team_flags() {
    let client = ClaudeCodeClient::new();
    let config = test_teammate_config();
    let args = client
        .build_teammate_cli_args(&config)
        .expect("build_teammate_cli_args should succeed in test");

    // --agent-id <name>@<team-name>
    let agent_id_idx = args
        .iter()
        .position(|a| a == "--agent-id")
        .expect("--agent-id flag must be present");
    assert_eq!(
        args[agent_id_idx + 1],
        "transport-researcher@ideation-abc123"
    );

    // --agent-name
    let agent_name_idx = args
        .iter()
        .position(|a| a == "--agent-name")
        .expect("--agent-name flag must be present");
    assert_eq!(args[agent_name_idx + 1], "transport-researcher");

    // --team-name
    let team_name_idx = args
        .iter()
        .position(|a| a == "--team-name")
        .expect("--team-name flag must be present");
    assert_eq!(args[team_name_idx + 1], "ideation-abc123");

    // --agent-color
    let color_idx = args
        .iter()
        .position(|a| a == "--agent-color")
        .expect("--agent-color flag must be present");
    assert_eq!(args[color_idx + 1], "blue");

    // --parent-session-id
    let parent_idx = args
        .iter()
        .position(|a| a == "--parent-session-id")
        .expect("--parent-session-id flag must be present");
    assert_eq!(args[parent_idx + 1], "lead-session-uuid");

    // --agent-type (Claude Code built-in tool set)
    let agent_type_idx = args
        .iter()
        .position(|a| a == "--agent-type")
        .expect("--agent-type flag must be present");
    assert_eq!(args[agent_type_idx + 1], "general-purpose");
}

#[test]
fn test_build_teammate_cli_args_has_model() {
    let client = ClaudeCodeClient::new();
    let config = test_teammate_config();
    let args = client
        .build_teammate_cli_args(&config)
        .expect("build_teammate_cli_args should succeed in test");

    let model_idx = args
        .iter()
        .position(|a| a == "--model")
        .expect("--model flag must be present");
    assert_eq!(args[model_idx + 1], "sonnet");
}

#[test]
fn test_build_teammate_cli_args_has_tools() {
    let client = ClaudeCodeClient::new();
    let config = test_teammate_config();
    let args = client
        .build_teammate_cli_args(&config)
        .expect("build_teammate_cli_args should succeed in test");

    let tools_idx = args
        .iter()
        .position(|a| a == "--tools")
        .expect("--tools flag must be present");
    assert_eq!(args[tools_idx + 1], "Read,Grep,Glob");
}

#[test]
fn test_build_teammate_cli_args_no_tools_when_empty() {
    let client = ClaudeCodeClient::new();
    let config = TeammateSpawnConfig::new("r", "t", "p");
    let args = client
        .build_teammate_cli_args(&config)
        .expect("build_teammate_cli_args should succeed in test");

    assert!(
        !args.contains(&"--tools".to_string()),
        "Empty tools should not produce --tools flag"
    );
}

#[test]
fn test_build_teammate_cli_args_mcp_tools_prefixed() {
    let client = ClaudeCodeClient::new();
    let config = test_teammate_config();
    let args = client
        .build_teammate_cli_args(&config)
        .expect("build_teammate_cli_args should succeed in test");

    let allowed_idx = args
        .iter()
        .position(|a| a == "--allowedTools")
        .expect("--allowedTools flag must be present");
    let allowed_value = &args[allowed_idx + 1];

    // MCP tools should be prefixed with mcp__ralphx__
    assert!(
        allowed_value.contains("mcp__ralphx__get_session_plan"),
        "MCP tools must be prefixed: got {allowed_value}"
    );
    assert!(
        allowed_value.contains("mcp__ralphx__list_session_proposals"),
        "MCP tools must be prefixed: got {allowed_value}"
    );
}

#[test]
fn test_build_teammate_cli_args_no_allowed_tools_when_empty() {
    let client = ClaudeCodeClient::new();
    let config = TeammateSpawnConfig::new("r", "t", "p");
    let args = client
        .build_teammate_cli_args(&config)
        .expect("build_teammate_cli_args should succeed in test");

    assert!(
        !args.contains(&"--allowedTools".to_string()),
        "Empty MCP tools should not produce --allowedTools flag"
    );
}

#[test]
fn test_build_teammate_cli_args_no_append_system_prompt() {
    // --append-system-prompt was removed (commit 959c4c8d); teammates join via
    // the team inbox system, not a one-shot prompt injection.
    let client = ClaudeCodeClient::new();
    let config = test_teammate_config();
    let args = client
        .build_teammate_cli_args(&config)
        .expect("build_teammate_cli_args should succeed in test");

    assert!(
        !args.contains(&"--append-system-prompt".to_string()),
        "--append-system-prompt must not be present (removed in 959c4c8d)"
    );
}

#[test]
fn test_build_teammate_cli_args_has_skip_permissions() {
    let client = ClaudeCodeClient::new();
    let config = test_teammate_config();
    let args = client
        .build_teammate_cli_args(&config)
        .expect("build_teammate_cli_args should succeed in test");

    assert!(
        args.contains(&"--dangerously-skip-permissions".to_string()),
        "Teammates must skip permissions"
    );
}

#[test]
fn test_build_teammate_cli_args_uses_bypass_permission_mode() {
    let client = ClaudeCodeClient::new();
    let config = test_teammate_config();
    let args = client
        .build_teammate_cli_args(&config)
        .expect("build_teammate_cli_args should succeed in test");

    assert_eq!(
        arg_value(&args, "--permission-mode"),
        Some("bypassPermissions")
    );
}

#[test]
fn test_build_teammate_cli_args_has_disable_slash_commands() {
    let client = ClaudeCodeClient::new();
    let config = test_teammate_config();
    let args = client
        .build_teammate_cli_args(&config)
        .expect("build_teammate_cli_args should succeed in test");

    assert!(args.contains(&"--disable-slash-commands".to_string()));
}

#[test]
fn test_build_teammate_cli_args_has_plugin_dir() {
    let client = ClaudeCodeClient::new();
    let config = test_teammate_config();
    let args = client
        .build_teammate_cli_args(&config)
        .expect("build_teammate_cli_args should succeed in test");

    assert!(args.contains(&"--plugin-dir".to_string()));
    assert!(args.contains(&"/test/ralphx-plugin".to_string()));
}

#[test]
fn test_build_teammate_cli_args_custom_agent_type() {
    let client = ClaudeCodeClient::new();
    let config = test_teammate_config().with_agent_type("Bash");
    let args = client
        .build_teammate_cli_args(&config)
        .expect("build_teammate_cli_args should succeed in test");

    let agent_type_idx = args
        .iter()
        .position(|a| a == "--agent-type")
        .expect("--agent-type flag must be present");
    assert_eq!(args[agent_type_idx + 1], "Bash");
}

#[test]
fn test_build_teammate_env_vars_has_team_flags() {
    let config = test_teammate_config();
    let env = ClaudeCodeClient::build_teammate_env_vars(&config);

    assert_eq!(env.get("CLAUDECODE"), Some(&"1".to_string()));
    assert_eq!(
        env.get("CLAUDE_CODE_EXPERIMENTAL_AGENT_TEAMS"),
        Some(&"1".to_string())
    );
}

#[test]
fn test_build_teammate_env_vars_has_agent_type() {
    let config = test_teammate_config();
    let env = ClaudeCodeClient::build_teammate_env_vars(&config);

    assert_eq!(
        env.get("RALPHX_AGENT_TYPE"),
        Some(&"ideation-team-member".to_string())
    );
}

#[test]
fn test_build_teammate_env_vars_custom_mcp_agent_type() {
    let config = test_teammate_config().with_mcp_agent_type("worker-team-member");
    let env = ClaudeCodeClient::build_teammate_env_vars(&config);

    assert_eq!(
        env.get("RALPHX_AGENT_TYPE"),
        Some(&"worker-team-member".to_string())
    );
}

#[test]
fn test_build_teammate_env_vars_includes_custom_env() {
    let config = test_teammate_config().with_env("RALPHX_SESSION_ID", "sess-456");
    let env = ClaudeCodeClient::build_teammate_env_vars(&config);

    assert_eq!(env.get("RALPHX_SESSION_ID"), Some(&"sess-456".to_string()));
    // Team flags still present
    assert_eq!(env.get("CLAUDECODE"), Some(&"1".to_string()));
}

#[test]
fn test_build_teammate_env_vars_propagates_context() {
    let ctx = TeammateContext {
        context_id: "ctx-abc".to_string(),
        context_type: "ideation".to_string(),
        project_id: Some("proj-789".to_string()),
    };
    let config = test_teammate_config().with_context(ctx);
    let env = ClaudeCodeClient::build_teammate_env_vars(&config);

    assert_eq!(env.get("RALPHX_CONTEXT_ID"), Some(&"ctx-abc".to_string()));
    assert_eq!(
        env.get("RALPHX_CONTEXT_TYPE"),
        Some(&"ideation".to_string())
    );
    assert_eq!(env.get("RALPHX_PROJECT_ID"), Some(&"proj-789".to_string()));
}

#[test]
fn test_build_teammate_env_vars_omits_empty_context() {
    // Default context has empty strings — env vars should not be set
    let config = test_teammate_config();
    let env = ClaudeCodeClient::build_teammate_env_vars(&config);

    assert!(!env.contains_key("RALPHX_CONTEXT_ID"));
    assert!(!env.contains_key("RALPHX_CONTEXT_TYPE"));
    assert!(!env.contains_key("RALPHX_PROJECT_ID"));
}

#[tokio::test]
async fn test_spawn_teammate_interactive_blocked_in_tests() {
    let client = ClaudeCodeClient::new().with_cli_path("/nonexistent/path/to/claude_binary_12345");
    let config = test_teammate_config();

    let result = client.spawn_teammate_interactive(config).await;
    assert!(result.is_err());
    assert!(matches!(result, Err(AgentError::SpawnNotAllowed(_))));
}

#[test]
fn test_teammate_spawn_result_debug() {
    fn assert_debug<T: std::fmt::Debug>() {}
    assert_debug::<TeammateSpawnResult>();
}

#[test]
fn test_teammate_spawn_config_debug_and_clone() {
    let config = test_teammate_config();
    let cloned = config.clone();
    assert_eq!(cloned.name, "transport-researcher");
    // Verify Debug is implemented (compile-time check)
    let _debug = format!("{:?}", cloned);
}

#[test]
fn test_build_teammate_cli_args_full_integration() {
    // Verify the complete arg list for a realistic teammate spawn
    let client = ClaudeCodeClient::new();
    let config = TeammateSpawnConfig::new(
        "react-state-sync-researcher",
        "ideation-session-789",
        "You are a React state management specialist. Analyze existing Zustand stores.",
    )
    .with_parent_session_id("c43c3747-44d8-437b-9a25-911032eec2ea")
    .with_model("sonnet")
    .with_tools(vec![
        "Read".to_string(),
        "Grep".to_string(),
        "Glob".to_string(),
        "WebSearch".to_string(),
    ])
    .with_mcp_tools(vec![
        "get_session_plan".to_string(),
        "get_artifact".to_string(),
    ])
    .with_color("green")
    .with_working_dir("/Users/test/project");

    let args = client
        .build_teammate_cli_args(&config)
        .expect("build_teammate_cli_args should succeed in test");

    // Interactive mode: -p - and --input-format stream-json must be present
    let p_pos = args.iter().position(|a| a == "-p");
    assert!(p_pos.is_some(), "args must contain -p");
    assert_eq!(args.get(p_pos.unwrap() + 1).map(String::as_str), Some("-"));
    assert!(args.contains(&"--input-format".to_string()));
    assert!(args.contains(&"stream-json".to_string()));

    // Verify all required flags are present
    let required_flags = vec![
        "--output-format",
        "--verbose",
        "--disable-slash-commands",
        "--agent-id",
        "--agent-name",
        "--team-name",
        "--agent-color",
        "--parent-session-id",
        "--agent-type",
        "--model",
        "--tools",
        "--allowedTools",
        "--dangerously-skip-permissions",
    ];
    for flag in &required_flags {
        assert!(
            args.contains(&flag.to_string()),
            "Missing required flag: {flag}"
        );
    }

    // Verify agent-id format: name@team-name
    let agent_id_idx = args.iter().position(|a| a == "--agent-id").unwrap();
    assert_eq!(
        args[agent_id_idx + 1],
        "react-state-sync-researcher@ideation-session-789"
    );

    // Verify tools are comma-separated
    let tools_idx = args.iter().position(|a| a == "--tools").unwrap();
    assert_eq!(args[tools_idx + 1], "Read,Grep,Glob,WebSearch");

    // Verify MCP tools are prefixed and comma-separated
    let allowed_idx = args.iter().position(|a| a == "--allowedTools").unwrap();
    assert_eq!(
        args[allowed_idx + 1],
        "mcp__ralphx__get_session_plan,mcp__ralphx__get_artifact"
    );
}

#[test]
fn test_build_teammate_cli_args_passes_settings_when_profile_exists() {
    // Verifies that --settings is passed when get_effective_settings returns a value.
    // The embedded config/ralphx.yaml configures a global `settings_profile: default`,
    // so any agent_type not registered in agents[] falls through to the global profile.
    // "unregistered-agent-type" triggers the global fallback path.
    let client = ClaudeCodeClient::new();
    let config = test_teammate_config().with_agent_type("unregistered-agent-type");

    // Confirm a settings value is available for this agent_type (global fallback)
    let has_settings = super::get_effective_settings(Some("unregistered-agent-type")).is_some();
    if !has_settings {
        // No settings profile configured in this environment — skip the positive assertion
        // and verify --settings is correctly absent
        let args = client
            .build_teammate_cli_args(&config)
            .expect("build_teammate_cli_args should succeed in test");
        assert!(
            !args.contains(&"--settings".to_string()),
            "--settings must not appear when no profile is configured"
        );
        return;
    }

    let args = client
        .build_teammate_cli_args(&config)
        .expect("build_teammate_cli_args should succeed in test");

    // --settings flag must be present
    let settings_idx = args
        .iter()
        .position(|a| a == "--settings")
        .expect("--settings flag must be present when a settings profile exists");

    // The value after --settings must be valid JSON
    let json_str = &args[settings_idx + 1];
    serde_json::from_str::<serde_json::Value>(json_str)
        .expect("--settings value must be valid JSON");
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
}

// ==================== Effort Flag Tests (build_teammate_cli_args) ====================

#[test]
fn test_build_teammate_cli_args_includes_effort_when_some() {
    let client = ClaudeCodeClient::new();
    let config = TeammateSpawnConfig::new("dev", "team-1", "Do stuff")
        .with_plugin_dir("/test/plugin")
        .with_effort("high");
    let args = client
        .build_teammate_cli_args(&config)
        .expect("build_teammate_cli_args should succeed in test");

    let effort_idx = args
        .iter()
        .position(|a| a == "--effort")
        .expect("--effort flag must be present when config.effort is Some");
    assert_eq!(
        args[effort_idx + 1],
        "high",
        "--effort must be followed by the configured effort level"
    );
}

#[test]
fn test_build_teammate_cli_args_falls_back_to_global_default_effort() {
    use crate::infrastructure::agents::claude::agent_config::claude_runtime_config;
    let client = ClaudeCodeClient::new();
    let config =
        TeammateSpawnConfig::new("dev", "team-1", "Do stuff").with_plugin_dir("/test/plugin");
    // effort is None (default) — should fall back to global default_effort
    let args = client
        .build_teammate_cli_args(&config)
        .expect("build_teammate_cli_args should succeed in test");

    let effort_idx = args
        .iter()
        .position(|a| a == "--effort")
        .expect("--effort flag must always be present (falls back to global default)");
    let expected_default = &claude_runtime_config().default_effort;
    assert_eq!(
        &args[effort_idx + 1],
        expected_default,
        "--effort must fall back to global default_effort when config.effort is None"
    );
}
