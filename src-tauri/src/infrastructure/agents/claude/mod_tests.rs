/// TDD tests for --allowed-tools CLI arg injection in create_mcp_config (Wave 1).
/// These tests FAIL until Wave 3 implementation is complete.
///
/// Covers:
/// - validate_mcp_tool_name(): rejects names with commas/spaces/uppercase/digits-first
/// - format_allowed_tools_arg_value(): None→None, Some([])→"__NONE__", Some([...])→"t1,t2"
/// - create_mcp_config(): injects --allowed-tools from agent's mcp_tools list
/// - create_mcp_config(): --agent-type still present alongside --allowed-tools
/// - create_mcp_config(): no --allowed-tools arg when agent has no mcp_tools config
use super::*;
use crate::domain::services::learned_skill_adapters::{
    LearnedSkillBucket, LearnedSkillMultiSelectionRequest, LearnedSkillRecord, LearnedSkillStage,
    LearnedSkillStatus,
};
use crate::infrastructure::agents::harness_agent_catalog::{
    internal_mcp_server_name, load_canonical_agent_definition, load_canonical_claude_metadata,
    load_harness_agent_prompt, AgentPromptHarness,
};
use crate::infrastructure::agents::internal_skills::PreExecutionLearnedSkillContext;
use crate::infrastructure::agents::mcp_runtime_context::McpRuntimeContext;
use crate::utils::path_safety::{
    checked_exists, checked_read_to_string, validate_absolute_non_root_path,
};
use serde_yaml::Value;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

// ─── Helpers ────────────────────────────────────────────────────────────────

/// Create a minimal plugin dir structure that create_mcp_config() can use.
fn make_temp_plugin_dir() -> (tempfile::TempDir, std::path::PathBuf) {
    let dir = tempfile::TempDir::new().unwrap();
    let plugin_dir = dir.path().to_path_buf();
    std::fs::create_dir_all(plugin_dir.join("ralphx-mcp-server/build")).unwrap();
    std::fs::write(
        plugin_dir.join("ralphx-mcp-server/build/index.js"),
        "// fake",
    )
    .unwrap();
    (dir, plugin_dir)
}

fn make_temp_project_plugin_dir() -> (tempfile::TempDir, std::path::PathBuf, std::path::PathBuf) {
    let dir = tempfile::TempDir::new().unwrap();
    let root = dir.path().to_path_buf();
    let plugin_dir = root.join("plugins/app");
    std::fs::create_dir_all(plugin_dir.join("agents")).unwrap();
    std::fs::create_dir_all(plugin_dir.join("ralphx-mcp-server/build")).unwrap();
    std::fs::write(
        plugin_dir.join("ralphx-mcp-server/build/index.js"),
        "// fake",
    )
    .unwrap();
    std::fs::write(
        plugin_dir.join(".mcp.json"),
        r#"{"mcpServers":{"ralphx":{"type":"stdio","command":"node","args":["${CLAUDE_PLUGIN_ROOT}/ralphx-mcp-server/build/index.js"]}}}"#,
    )
    .unwrap();
    (dir, root, plugin_dir)
}

fn learned_skill(id: &str, project_id: &str) -> LearnedSkillRecord {
    LearnedSkillRecord {
        id: id.to_string(),
        project_id: project_id.to_string(),
        title: format!("Skill {id}"),
        status: LearnedSkillStatus::Approved,
        pinned: false,
        caller_surfaces: Vec::new(),
        stages: Vec::new(),
        buckets: Vec::new(),
        path_scopes: Vec::new(),
        compact_guidance: format!("Follow {id}."),
        predicted_effect: format!("Improve {id}."),
        provenance_refs: vec![format!("outcome:{id}")],
    }
}

fn allowed_tools_arg_from_mcp_config(json: &serde_json::Value) -> Option<String> {
    json["mcpServers"]
        .as_object()
        .and_then(|servers| servers.values().next())
        .and_then(|server| server["args"].as_array())
        .and_then(|args| {
            args.iter()
                .filter_map(|arg| arg.as_str())
                .find(|arg| arg.starts_with("--allowed-tools="))
                .map(str::to_string)
        })
}

fn seed_live_agent_yaml(root: &Path, agent_name: &str) {
    let agent_dir = root.join("agents").join(agent_name);
    std::fs::create_dir_all(&agent_dir).expect("create agent fixture dir");
    std::fs::copy(
        repo_project_root()
            .join("agents")
            .join(agent_name)
            .join("agent.yaml"),
        agent_dir.join("agent.yaml"),
    )
    .expect("copy live agent fixture");
}

fn seed_runnable_mcp_runtime(plugin_dir: &Path, runtime_marker: &str) {
    std::fs::create_dir_all(plugin_dir.join("ralphx-mcp-server/build")).unwrap();
    std::fs::create_dir_all(
        plugin_dir.join("ralphx-mcp-server/node_modules/@modelcontextprotocol/sdk"),
    )
    .unwrap();
    std::fs::write(
        plugin_dir.join("ralphx-mcp-server/build/index.js"),
        runtime_marker,
    )
    .unwrap();
    std::fs::write(
        plugin_dir.join("ralphx-mcp-server/node_modules/@modelcontextprotocol/sdk/package.json"),
        "{}",
    )
    .unwrap();
}

fn make_isolated_live_project_plugin_dir() -> (
    tempfile::TempDir,
    std::path::PathBuf,
    std::path::PathBuf,
    super::RuntimePluginDirsOverrideGuard,
) {
    let dir = tempfile::TempDir::new().expect("create temp project dir");
    let root = dir.path().to_path_buf();
    let repo_root = repo_project_root();
    let plugin_dir = root.join("plugins/app");
    let generated_plugin_dir = root.join("generated/claude-plugin");

    std::fs::create_dir_all(root.join("plugins")).expect("create plugins parent");
    let plugin_copy_status = std::process::Command::new("cp")
        .arg("-R")
        .arg(repo_root.join("plugins/app"))
        .arg(&plugin_dir)
        .status()
        .expect("copy live plugin fixture");
    assert!(plugin_copy_status.success(), "copy live plugin fixture");
    let agents_copy_status = std::process::Command::new("cp")
        .arg("-R")
        .arg(repo_root.join("agents"))
        .arg(root.join("agents"))
        .status()
        .expect("copy live agents fixture");
    assert!(agents_copy_status.success(), "copy live agents fixture");
    let runtime_guard =
        override_runtime_plugin_dirs_for_tests(plugin_dir.clone(), generated_plugin_dir);

    (dir, root, plugin_dir, runtime_guard)
}

#[cfg(unix)]
fn symlink_dir(source: impl AsRef<Path>, target: impl AsRef<Path>) {
    std::os::unix::fs::symlink(source, target).expect("create directory symlink");
}

#[cfg(windows)]
fn symlink_dir(source: impl AsRef<Path>, target: impl AsRef<Path>) {
    std::os::windows::fs::symlink_dir(source, target).expect("create directory symlink");
}

fn read_test_file(path: impl AsRef<Path>) -> String {
    checked_read_to_string(path.as_ref(), "Claude plugin test fixture")
        .expect("read Claude plugin test fixture")
}

fn test_path_exists(path: impl AsRef<Path>) -> bool {
    checked_exists(path.as_ref(), "Claude plugin test fixture")
        .expect("inspect Claude plugin test fixture")
}

fn read_test_link(path: impl AsRef<Path>) -> PathBuf {
    let path = validate_absolute_non_root_path(path.as_ref(), "Claude plugin test symlink")
        .expect("validate Claude plugin test symlink");

    // codeql[rust/path-injection]
    std::fs::read_link(path).expect("read Claude plugin test symlink")
}

fn test_symlink_metadata_is_err(path: impl AsRef<Path>) -> bool {
    let path = validate_absolute_non_root_path(path.as_ref(), "Claude plugin test metadata")
        .expect("validate Claude plugin test metadata path");

    // codeql[rust/path-injection]
    std::fs::symlink_metadata(path).is_err()
}

fn remove_test_file_or_dir(path: impl AsRef<Path>) {
    let path = validate_absolute_non_root_path(path.as_ref(), "Claude plugin test removal")
        .expect("validate Claude plugin test removal path");

    // codeql[rust/path-injection]
    if std::fs::remove_file(&path).is_err() {
        // codeql[rust/path-injection]
        std::fs::remove_dir(&path).expect("remove Claude plugin test directory");
    }
}

/// Parse the JSON args array from an MCP config value without touching temp files.
fn get_json_args(config: &serde_json::Value) -> Vec<String> {
    config
        .get("mcpServers")
        .and_then(|s| s.as_object())
        .and_then(|m| m.values().next())
        .and_then(|server| server.get("args"))
        .and_then(|args| args.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(|s| s.to_string()))
                .collect()
        })
        .unwrap_or_default()
}

fn repo_project_root() -> std::path::PathBuf {
    std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..")
}

fn split_frontmatter(markdown: &str) -> (Value, String) {
    let after_start = markdown
        .strip_prefix("---\n")
        .expect("expected frontmatter start delimiter");
    let (frontmatter, body) = after_start
        .split_once("\n---\n")
        .expect("expected frontmatter end delimiter");
    let parsed = serde_yaml::from_str(frontmatter).expect("valid yaml frontmatter");
    (parsed, body.trim().to_string())
}

fn frontmatter_tools_set(frontmatter: &Value) -> BTreeSet<String> {
    frontmatter["tools"]
        .as_sequence()
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .map(ToString::to_string)
        .collect()
}

fn frontmatter_has_mcp_servers(frontmatter: &Value) -> bool {
    !matches!(frontmatter.get("mcpServers"), None | Some(Value::Null))
}

fn expected_frontmatter_tools(agent_name: &str) -> BTreeSet<String> {
    let agent_config = get_agent_config(agent_name)
        .unwrap_or_else(|| panic!("missing runtime config for {agent_name}"));
    let mcp_server_name = &claude_runtime_config().mcp_server_name;

    let mut tools = BTreeSet::new();
    if !agent_config.mcp_only {
        tools.extend(agent_config.resolved_cli_tools.iter().cloned());
    }
    let project_root = repo_project_root();
    let claude_metadata = load_canonical_claude_metadata(&project_root, agent_name);
    if claude_metadata.mcp_transport.as_deref() == Some("external") {
        tools.extend(claude_metadata.mcp_tools.iter().map(|tool| {
            if tool.starts_with("mcp__") {
                tool.to_string()
            } else {
                format!("mcp__{mcp_server_name}__{tool}")
            }
        }));
        let internal_server_name = internal_mcp_server_name(mcp_server_name);
        tools.extend(claude_metadata.internal_mcp_tools.iter().map(|tool| {
            if tool.starts_with("mcp__") {
                tool.to_string()
            } else {
                format!("mcp__{internal_server_name}__{tool}")
            }
        }));
    } else {
        tools.extend(agent_config.allowed_mcp_tools.iter().map(|tool| {
            if tool.starts_with("mcp__") {
                tool.to_string()
            } else {
                format!("mcp__{mcp_server_name}__{tool}")
            }
        }));
    }
    tools.extend(agent_config.preapproved_cli_tools.iter().cloned());
    tools
}

// ─── validate_mcp_tool_name ──────────────────────────────────────────────────

#[test]
fn test_validate_mcp_tool_name_accepts_lowercase_alphanumeric_underscore() {
    assert!(validate_mcp_tool_name("get_session_plan"));
    assert!(validate_mcp_tool_name("tool1"));
    assert!(validate_mcp_tool_name("a"));
    assert!(validate_mcp_tool_name("abc123_def"));
    assert!(validate_mcp_tool_name("start_step"));
}

#[test]
fn test_validate_mcp_tool_name_rejects_names_with_spaces() {
    assert!(!validate_mcp_tool_name("has space"));
    assert!(!validate_mcp_tool_name(" leading"));
    assert!(!validate_mcp_tool_name("trailing "));
}

#[test]
fn test_validate_mcp_tool_name_rejects_names_with_commas() {
    assert!(!validate_mcp_tool_name("has,comma"));
    assert!(!validate_mcp_tool_name(",starts_with_comma"));
}

#[test]
fn test_validate_mcp_tool_name_rejects_uppercase() {
    assert!(!validate_mcp_tool_name("UPPERCASE"));
    assert!(!validate_mcp_tool_name("Mixed_Case"));
    assert!(!validate_mcp_tool_name("camelCase"));
}

#[test]
fn test_validate_mcp_tool_name_rejects_starting_with_digit() {
    assert!(!validate_mcp_tool_name("1starts_digit"));
    assert!(!validate_mcp_tool_name("9tool"));
}

#[test]
fn test_validate_mcp_tool_name_rejects_special_characters() {
    assert!(!validate_mcp_tool_name("has-hyphen"));
    assert!(!validate_mcp_tool_name("has.dot"));
    assert!(!validate_mcp_tool_name("has@at"));
    assert!(!validate_mcp_tool_name(""));
}

// ─── format_allowed_tools_arg_value ─────────────────────────────────────────

#[test]
fn test_format_allowed_tools_arg_value_with_tools_list() {
    let tools = vec!["tool1".to_string(), "tool2".to_string()];
    let result = format_allowed_tools_arg_value(Some(&tools));
    assert_eq!(result, Some("tool1,tool2".to_string()));
}

#[test]
fn test_format_allowed_tools_arg_value_single_tool() {
    let tools = vec!["get_session_plan".to_string()];
    let result = format_allowed_tools_arg_value(Some(&tools));
    assert_eq!(result, Some("get_session_plan".to_string()));
}

#[test]
fn test_format_allowed_tools_arg_value_explicit_empty_returns_none_sentinel() {
    let result = format_allowed_tools_arg_value(Some(&[]));
    assert_eq!(result, Some("__NONE__".to_string()));
}

#[test]
fn test_format_allowed_tools_arg_value_absent_mcp_tools_returns_none() {
    let result = format_allowed_tools_arg_value(None);
    assert_eq!(result, None);
}

// ─── create_mcp_config integration ──────────────────────────────────────────

#[test]
fn test_create_mcp_config_injects_allowed_tools_for_agent_with_mcp_tools() {
    let (_dir, plugin_dir) = make_temp_plugin_dir();
    // ralphx-ideation has a non-empty mcp_tools list in config/ralphx.yaml
    let config = build_mcp_config_with_runtime_context(&plugin_dir, "ralphx-ideation", false, None)
        .expect("should create config");
    let args = get_json_args(&config);

    let allowed_tools_arg = args.iter().find(|a| a.starts_with("--allowed-tools="));
    assert!(
        allowed_tools_arg.is_some(),
        "--allowed-tools should be present for agent with mcp_tools; got args: {args:?}"
    );
    let value = allowed_tools_arg
        .unwrap()
        .strip_prefix("--allowed-tools=")
        .unwrap();
    assert!(
        !value.is_empty(),
        "--allowed-tools value should not be empty"
    );
    assert_ne!(
        value, "__NONE__",
        "--allowed-tools should contain real tools, not __NONE__"
    );
}

#[test]
fn test_create_mcp_config_injects_agent_type_alongside_allowed_tools() {
    let (_dir, plugin_dir) = make_temp_plugin_dir();
    let config = build_mcp_config_with_runtime_context(&plugin_dir, "ralphx-ideation", false, None)
        .expect("should create config");
    let args = get_json_args(&config);

    // Both --agent-type and --allowed-tools must be present
    assert!(
        args.contains(&"--agent-type".to_string()),
        "--agent-type should be present; got: {args:?}"
    );
    assert!(
        args.iter().any(|a| a.starts_with("--allowed-tools=")),
        "--allowed-tools should be present; got: {args:?}"
    );
}

#[test]
fn test_create_mcp_config_injects_app_owned_trace_dir() {
    let (_dir, plugin_dir) = make_temp_plugin_dir();
    let config = build_mcp_config_with_runtime_context(&plugin_dir, "ralphx-ideation", false, None)
        .expect("should create config");
    let args = get_json_args(&config);

    let trace_dir_index = args
        .iter()
        .position(|arg| arg == "--trace-dir")
        .expect("--trace-dir should be present in MCP args");
    let trace_dir = args
        .get(trace_dir_index + 1)
        .expect("--trace-dir should have a value");

    assert!(
        trace_dir.contains("mcp-proxy"),
        "trace dir should point at the MCP proxy log root: {trace_dir}"
    );
    assert!(
        !trace_dir.starts_with(plugin_dir.to_string_lossy().as_ref()),
        "trace dir must not be rooted under the generated plugin dir: {trace_dir}"
    );
}

#[test]
fn test_create_mcp_config_injects_runtime_context_args() {
    let (_dir, plugin_dir) = make_temp_plugin_dir();
    let workspace_dir = plugin_dir.join("workspace");
    let project_root = plugin_dir.join("project-root");
    let runtime_context = McpRuntimeContext {
        context_type: Some("project".to_string()),
        context_id: Some("project-123".to_string()),
        conversation_id: Some("conversation-current".to_string()),
        coordination_mode: Some("rx_native_workflow".to_string()),
        task_id: None,
        task_state: Some("executing".to_string()),
        project_id: Some("project-123".to_string()),
        working_directory: Some(workspace_dir.clone()),
        filesystem_read_roots: vec![project_root.clone()],
        enforce_filesystem_roots: false,
        lead_session_id: Some("lead-456".to_string()),
        parent_conversation_id: Some("conversation-789".to_string()),
        agent_run_id: Some("run-123".to_string()),
        pipeline_role: Some("memory_capture".to_string()),
        skill_distillation_batch_id: None,
        skill_distillation_claim_token: None,
        skill_distillation_fingerprint: None,
        skill_distillation_outcome_ids: None,
    };

    let json = build_mcp_config_with_runtime_context(
        &plugin_dir,
        "ralphx-general-worker",
        false,
        Some(&runtime_context),
    )
    .expect("should create config");
    let args: Vec<String> = json
        .get("mcpServers")
        .and_then(|s| s.as_object())
        .and_then(|m| m.values().next())
        .and_then(|server| server.get("args"))
        .and_then(|args| args.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|value| value.as_str().map(ToString::to_string))
                .collect()
        })
        .unwrap_or_default();

    assert!(
        args.contains(&"--context-type".to_string()),
        "args: {args:?}"
    );
    assert!(args.contains(&"project".to_string()), "args: {args:?}");
    assert!(args.contains(&"--context-id".to_string()), "args: {args:?}");
    assert!(args.contains(&"project-123".to_string()), "args: {args:?}");
    assert!(
        args.contains(&"--conversation-id".to_string()),
        "args: {args:?}"
    );
    assert!(
        args.contains(&"conversation-current".to_string()),
        "args: {args:?}"
    );
    assert!(args.contains(&"--project-id".to_string()), "args: {args:?}");
    assert!(args.contains(&"--task-state".to_string()), "args: {args:?}");
    assert!(args.contains(&"executing".to_string()), "args: {args:?}");
    assert!(
        args.contains(&"--working-directory".to_string()),
        "args: {args:?}"
    );
    assert!(
        args.contains(&workspace_dir.to_string_lossy().into_owned()),
        "args: {args:?}"
    );
    assert!(
        args.contains(&"--filesystem-read-root".to_string()),
        "args: {args:?}"
    );
    assert!(
        args.contains(&project_root.to_string_lossy().into_owned()),
        "args: {args:?}"
    );
    assert!(
        args.contains(&"--parent-conversation-id".to_string()),
        "args: {args:?}"
    );
    assert!(
        args.contains(&"conversation-789".to_string()),
        "args: {args:?}"
    );
    assert!(
        args.contains(&"--agent-run-id".to_string()),
        "args: {args:?}"
    );
    assert!(args.contains(&"run-123".to_string()), "args: {args:?}");
    assert!(
        args.contains(&"--pipeline-role".to_string()),
        "args: {args:?}"
    );
    assert!(
        args.contains(&"memory_capture".to_string()),
        "args: {args:?}"
    );
}

#[test]
fn test_create_mcp_config_emits_filesystem_enforcement_only_when_enabled() {
    let (_dir, plugin_dir) = make_temp_plugin_dir();
    let enforced = McpRuntimeContext {
        enforce_filesystem_roots: true,
        ..Default::default()
    };

    let enforced_config = build_mcp_config_with_runtime_context(
        &plugin_dir,
        "ralphx-general-worker",
        false,
        Some(&enforced),
    )
    .expect("should create enforced config");
    let enforced_args = get_json_args(&enforced_config);
    assert!(
        enforced_args
            .windows(2)
            .any(|pair| pair == ["--filesystem-enforced", "1"]),
        "enforced config must carry the CLI-only flag: {enforced_args:?}"
    );

    let unenforced = McpRuntimeContext::default();
    assert!(
        !unenforced.enforce_filesystem_roots,
        "default runtime contexts must remain unenforced"
    );
    let unenforced_config = build_mcp_config_with_runtime_context(
        &plugin_dir,
        "ralphx-general-worker",
        false,
        Some(&unenforced),
    )
    .expect("should create unenforced config");
    let unenforced_args = get_json_args(&unenforced_config);
    assert!(
        !unenforced_args
            .iter()
            .any(|arg| arg == "--filesystem-enforced"),
        "unenforced config must preserve the prior argument shape: {unenforced_args:?}"
    );

    for config in [&enforced_config, &unenforced_config] {
        let server_env = config
            .get("mcpServers")
            .and_then(|servers| servers.as_object())
            .and_then(|servers| servers.values().next())
            .and_then(|server| server.get("env"))
            .and_then(|env| env.as_object());
        assert!(
            server_env.is_none_or(|env| !env.contains_key("RALPHX_FILESYSTEM_ENFORCED")),
            "filesystem enforcement must never be delivered through process env"
        );
    }
}

#[test]
fn test_create_mcp_config_no_allowed_tools_arg_for_unknown_agent() {
    let (_dir, plugin_dir) = make_temp_plugin_dir();
    // Unknown agent has no config → mcp_tools absent → no --allowed-tools injected
    let config = build_mcp_config_with_runtime_context(
        &plugin_dir,
        "completely-unknown-agent-xyz",
        false,
        None,
    )
    .expect("should create config even for unknown agent");
    let args = get_json_args(&config);

    let has_allowed_tools = args.iter().any(|a| a.starts_with("--allowed-tools="));
    assert!(
        !has_allowed_tools,
        "--allowed-tools should NOT be present for agent with no mcp_tools config; got: {args:?}"
    );
    // --agent-type should still be present
    assert!(
        args.contains(&"--agent-type".to_string()),
        "--agent-type should still be present; got: {args:?}"
    );
}

#[test]
fn test_create_mcp_config_allowed_tools_value_matches_agent_mcp_tools() {
    let (_dir, plugin_dir) = make_temp_plugin_dir();
    // ralphx-utility-session-namer has a small mcp_tools list: [update_session_title]
    let config = build_mcp_config_with_runtime_context(
        &plugin_dir,
        "ralphx-utility-session-namer",
        false,
        None,
    )
    .expect("should create config");
    let args = get_json_args(&config);

    let allowed_arg = args
        .iter()
        .find(|a| a.starts_with("--allowed-tools="))
        .expect("--allowed-tools should be present for ralphx-utility-session-namer");
    let value = allowed_arg.strip_prefix("--allowed-tools=").unwrap();
    // ralphx-utility-session-namer has mcp_tools: [update_session_title]
    assert_eq!(
        value, "update_session_title",
        "ralphx-utility-session-namer should have exactly update_session_title"
    );
}

#[test]
fn persona_extractor_spawn_emits_tools_flag_and_mcp_grants() {
    let (_plugin_dir_guard, plugin_dir) = make_temp_plugin_dir();
    let working_directory = tempfile::tempdir().expect("working directory");
    let command = build_spawnable_command_with_mcp_runtime_context_for_test(
        Path::new("/fake/claude"),
        &plugin_dir,
        "Distill the selected context into a persona.",
        Some("ralphx:ralphx-persona-extractor"),
        None,
        working_directory.path(),
        None,
        None,
        None,
    )
    .expect("persona extractor command should build");
    let args = command.get_args_for_test();

    let tools_index = args
        .iter()
        .position(|arg| arg == "--tools")
        .expect("A7 containment requires --tools on the extractor command");
    assert!(
        !args[tools_index + 1].is_empty(),
        "A7 containment requires a non-empty --tools value"
    );
    assert_eq!(args[tools_index + 1], "TaskList");

    let mcp_config_index = args
        .iter()
        .position(|arg| arg == "--mcp-config")
        .expect("extractor command should carry a strict MCP config");
    let mcp_config: serde_json::Value =
        serde_json::from_str(&read_test_file(Path::new(&args[mcp_config_index + 1])))
            .expect("generated MCP config should be valid JSON");
    let mcp_args = get_json_args(&mcp_config);
    let allowed_tools = mcp_args
        .iter()
        .find_map(|arg| arg.strip_prefix("--allowed-tools="))
        .expect("extractor MCP config should explicitly restrict allowed tools");

    let expected_grants = BTreeSet::from([
        "fs_read_file",
        "fs_list_dir",
        "fs_grep",
        "fs_glob",
        "ask_user_question",
        "save_persona_draft",
        "get_persona_draft",
    ]);
    let actual_grants = allowed_tools.split(',').collect::<BTreeSet<_>>();
    assert_eq!(actual_grants, expected_grants);
}

#[test]
fn test_create_mcp_config_injects_no_tools_sentinel_for_automation_judge() {
    let (_dir, plugin_dir) = make_temp_plugin_dir();
    let config =
        build_mcp_config_with_runtime_context(&plugin_dir, "ralphx-automation-judge", false, None)
            .expect("should create config");
    let args = get_json_args(&config);

    let allowed_arg = args
        .iter()
        .find(|a| a.starts_with("--allowed-tools="))
        .expect("--allowed-tools should be present for zero-tool automation judge");
    assert_eq!(allowed_arg, "--allowed-tools=__NONE__");
}

// ─── validate_mcp_config_json ────────────────────────────────────────────────

#[test]
fn test_validate_mcp_config_json_accepts_valid_config() {
    let config = serde_json::json!({
        "mcpServers": {
            "ralphx": {
                "type": "stdio",
                "command": "/usr/local/bin/node",
                "args": ["/path/to/index.js", "--agent-type", "worker"]
            }
        }
    });
    assert!(validate_mcp_config_json(&config, "ralphx").is_ok());
}

#[test]
fn test_validate_mcp_config_json_rejects_missing_mcp_servers() {
    let config = serde_json::json!({
        "other": {}
    });
    let result = validate_mcp_config_json(&config, "ralphx");
    assert!(result.is_err());
    assert!(
        result.unwrap_err().contains("mcpServers"),
        "error should mention missing mcpServers"
    );
}

#[test]
fn test_validate_mcp_config_json_rejects_missing_server_entry() {
    let config = serde_json::json!({
        "mcpServers": {
            "other-server": {}
        }
    });
    let result = validate_mcp_config_json(&config, "ralphx");
    assert!(result.is_err());
    assert!(
        result.unwrap_err().contains("ralphx"),
        "error should mention missing server name"
    );
}

#[test]
fn test_validate_mcp_config_json_rejects_missing_command() {
    let config = serde_json::json!({
        "mcpServers": {
            "ralphx": {
                "args": ["/path/to/index.js"]
            }
        }
    });
    let result = validate_mcp_config_json(&config, "ralphx");
    assert!(result.is_err());
    assert!(
        result.unwrap_err().contains("command"),
        "error should mention missing command field"
    );
}

#[test]
fn test_validate_mcp_config_json_rejects_missing_args() {
    let config = serde_json::json!({
        "mcpServers": {
            "ralphx": {
                "command": "/usr/local/bin/node"
            }
        }
    });
    let result = validate_mcp_config_json(&config, "ralphx");
    assert!(result.is_err());
    assert!(
        result.unwrap_err().contains("args"),
        "error should mention missing args field"
    );
}

#[test]
fn test_create_mcp_config_returns_error_on_io_failure() {
    // Use a non-existent directory as plugin_dir — should fail gracefully
    let plugin_dir = std::path::Path::new("/nonexistent/path/that/does/not/exist");
    // create_mcp_config should return Err, not panic
    let result = create_mcp_config(plugin_dir, "worker", false);
    // May succeed (writing temp file doesn't need plugin_dir to exist) or fail on validation
    // The key invariant: it must not panic, regardless of outcome
    let _ = result; // just checking no panic
}

// ─── filter_interactive_tools tests ─────────────────────────────────────────

#[test]
fn test_filter_interactive_tools_removes_ask_user_question() {
    let tools = vec![
        "get_task_context".to_string(),
        "ask_user_question".to_string(),
        "complete_step".to_string(),
    ];
    let filtered = filter_interactive_tools(&tools);
    assert!(!filtered.contains(&"ask_user_question".to_string()));
    assert!(filtered.contains(&"get_task_context".to_string()));
    assert!(filtered.contains(&"complete_step".to_string()));
    assert_eq!(filtered.len(), 2);
}

#[test]
fn test_filter_interactive_tools_no_op_when_not_present() {
    let tools = vec!["get_task_context".to_string(), "complete_step".to_string()];
    let filtered = filter_interactive_tools(&tools);
    assert_eq!(filtered.len(), 2);
}

#[test]
fn test_filter_interactive_tools_empty_input() {
    let tools: Vec<String> = vec![];
    let filtered = filter_interactive_tools(&tools);
    assert!(filtered.is_empty());
}

// ─── create_mcp_config with is_external_mcp=true tests ───────────────────────

#[test]
fn test_create_mcp_config_external_mcp_filters_ask_user_question() {
    let (dir, plugin_dir) = make_temp_plugin_dir();
    // ralphx-ideation has ask_user_question in its mcp_tools
    let config = build_mcp_config_with_runtime_context(&plugin_dir, "ralphx-ideation", true, None)
        .expect("should create config");
    let args = get_json_args(&config);
    let allowed_tools_arg = args.iter().find(|a| a.starts_with("--allowed-tools="));
    if let Some(arg) = allowed_tools_arg {
        assert!(
            !arg.contains("ask_user_question"),
            "ask_user_question must not appear in --allowed-tools when is_external_mcp=true, got: {arg}"
        );
    }
    drop(dir);
}

#[test]
fn test_create_mcp_config_non_external_mcp_keeps_ask_user_question() {
    let (dir, plugin_dir) = make_temp_plugin_dir();
    // ralphx-ideation has ask_user_question in its mcp_tools — should be present when not external
    let config = build_mcp_config_with_runtime_context(&plugin_dir, "ralphx-ideation", false, None)
        .expect("should create config");
    let args = get_json_args(&config);
    let allowed_tools_arg = args.iter().find(|a| a.starts_with("--allowed-tools="));
    if let Some(arg) = allowed_tools_arg {
        assert!(
            arg.contains("ask_user_question"),
            "ask_user_question must appear in --allowed-tools when is_external_mcp=false, got: {arg}"
        );
    }
    drop(dir);
}

#[test]
fn test_plan_profile_mcp_config_non_external_keeps_ask_user_question() {
    let (dir, root, plugin_dir) = make_temp_project_plugin_dir();
    seed_live_agent_yaml(&root, "ralphx-ideation");
    let config = build_mcp_config_with_runtime_context_for_profile(
        &plugin_dir,
        "ralphx-ideation",
        Some("plan"),
        false,
        None,
    )
    .expect("should succeed");
    let allowed_tools_arg = allowed_tools_arg_from_mcp_config(&config).expect("allowed tools arg");

    assert!(
        allowed_tools_arg.contains("ask_user_question"),
        "Plan chat must keep ask_user_question for interactive Agent conversations, got: {allowed_tools_arg}"
    );
    drop(dir);
}

#[test]
fn test_plan_profile_mcp_config_external_filters_ask_user_question() {
    let (dir, root, plugin_dir) = make_temp_project_plugin_dir();
    seed_live_agent_yaml(&root, "ralphx-ideation");
    let config = build_mcp_config_with_runtime_context_for_profile(
        &plugin_dir,
        "ralphx-ideation",
        Some("plan"),
        true,
        None,
    )
    .expect("should succeed");
    let allowed_tools_arg = allowed_tools_arg_from_mcp_config(&config).expect("allowed tools arg");

    assert!(
        !allowed_tools_arg.contains("ask_user_question"),
        "External Plan chat spawns must filter ask_user_question to avoid unattended deadlocks, got: {allowed_tools_arg}"
    );
    assert!(
        allowed_tools_arg.contains("get_session_plan"),
        "Filtering interactive tools must preserve non-interactive Plan tools, got: {allowed_tools_arg}"
    );
    drop(dir);
}

#[test]
fn test_plan_profile_system_prompt_includes_runtime_profile_context() {
    let (_dir, _root, plugin_dir, _runtime_guard) = make_isolated_live_project_plugin_dir();
    let (system_prompt, _) = load_agent_system_prompt_with_internal_skills(
        &plugin_dir,
        "ralphx-ideation",
        Some("plan"),
        "Create a plan",
        None,
        None,
    )
    .expect("plan profile prompt");

    assert!(system_prompt.contains("<agent_runtime_profile>"));
    assert!(system_prompt.contains("<agent_name>ralphx-ideation</agent_name>"));
    assert!(system_prompt.contains("<profile_slug>plan</profile_slug>"));
    assert!(system_prompt.contains("<profile_role>plan_chat</profile_role>"));

    let (default_prompt, _) = load_agent_system_prompt_with_internal_skills(
        &plugin_dir,
        "ralphx-ideation",
        None,
        "Create a plan",
        None,
        None,
    )
    .expect("default profile prompt");
    assert!(!default_prompt.contains("<agent_name>ralphx-ideation</agent_name>"));
    assert!(!default_prompt.contains("<profile_role>plan_chat</profile_role>"));
}

#[test]
fn claude_prompt_orders_persona_after_base_and_appendices_before_skills_and_runtime_profile() {
    let (_dir, _root, plugin_dir, _runtime_guard) = make_isolated_live_project_plugin_dir();
    let persona = "<ralphx_agent_persona>Persona voice</ralphx_agent_persona>";
    let (system_prompt, _) = load_agent_system_prompt_with_internal_skills(
        &plugin_dir,
        "ralphx-ideation",
        Some("plan"),
        "<!-- ralphx_internal_skill=ralphx-agent-workspace-swe -->",
        Some(persona),
        None,
    )
    .expect("plan profile prompt");

    let base = system_prompt
        .find("## Agent Conversation Plan Mode")
        .expect("profile prompt");
    let persona = system_prompt.find(persona).expect("persona block");
    // The live profile prompt mentions these tags in prose, so anchor on the
    // APPENDED blocks (last occurrence), not the first prose mention.
    let skills = system_prompt
        .rfind("<ralphx_internal_skills>")
        .expect("internal skills");
    let runtime_profile = system_prompt
        .rfind("<agent_runtime_profile>")
        .expect("runtime profile");

    assert!(base < persona && persona < skills && skills < runtime_profile);
}

#[test]
fn persona_block_is_excluded_from_internal_skills_match_text() {
    let (_dir, root, plugin_dir) = make_temp_project_plugin_dir();
    let agent_root = root.join("agents/test-agent");
    std::fs::create_dir_all(agent_root.join("claude")).expect("create agent prompt dir");
    std::fs::write(
        agent_root.join("agent.yaml"),
        r#"name: test-agent
role: test
capabilities:
  internal_skills:
    auto_match: true
    allowed:
      - persona-only-skill
"#,
    )
    .expect("write agent definition");
    std::fs::write(agent_root.join("claude/prompt.md"), "Base prompt").expect("write prompt");
    std::fs::create_dir_all(root.join("plugins/app/skills/persona-only-skill"))
        .expect("create skill dir");
    std::fs::write(
        root.join("plugins/app/skills/persona-only-skill/SKILL.md"),
        r#"---
name: persona-only-skill
trigger: persona-only-trigger
---
This skill must not be selected from persona text.
"#,
    )
    .expect("write skill");

    let (system_prompt, injected_skills) = load_agent_system_prompt_with_internal_skills(
        &plugin_dir,
        "test-agent",
        None,
        "ordinary user request",
        Some("<ralphx_agent_persona>persona-only-trigger</ralphx_agent_persona>"),
        None,
    )
    .expect("system prompt");

    assert!(injected_skills.is_empty());
    assert!(!system_prompt.contains("<ralphx_internal_skills>"));
}

#[test]
fn persona_survives_skills_injector_error_fallback() {
    let (_dir, root, plugin_dir) = make_temp_project_plugin_dir();
    let agent_root = root.join("agents/test-agent");
    std::fs::create_dir_all(agent_root.join("claude")).expect("create agent prompt dir");
    std::fs::write(
        agent_root.join("agent.yaml"),
        r#"name: test-agent
role: test
capabilities:
  internal_skills:
    allowed:
      - missing-skill
"#,
    )
    .expect("write agent definition");
    std::fs::write(agent_root.join("claude/prompt.md"), "Base prompt").expect("write prompt");

    let persona = "<ralphx_agent_persona>Fallback persona</ralphx_agent_persona>";
    let (system_prompt, injected_skills) = load_agent_system_prompt_with_internal_skills(
        &plugin_dir,
        "test-agent",
        None,
        "ordinary user request",
        Some(persona),
        None,
    )
    .expect("system prompt");

    assert!(injected_skills.is_empty());
    assert!(system_prompt.contains(persona));
}

#[test]
fn add_prompt_args_with_persona_appends_block_in_append_system_prompt_mode() {
    let (_dir, root, plugin_dir) = make_temp_project_plugin_dir();
    let agent_root = root.join("agents/test-agent");
    std::fs::create_dir_all(agent_root.join("claude")).expect("create agent prompt dir");
    std::fs::write(
        agent_root.join("agent.yaml"),
        "name: test-agent\nrole: test\n",
    )
    .expect("write agent definition");
    std::fs::write(agent_root.join("claude/prompt.md"), "Base prompt").expect("write prompt");

    let persona = "<ralphx_agent_persona>CLI persona</ralphx_agent_persona>";
    let mut command = tokio::process::Command::new("/fake/claude");
    let outcome = add_prompt_args(
        &mut command,
        &plugin_dir,
        "ordinary user request",
        Some(persona),
        Some("test-agent"),
        None,
        None,
        false,
        None,
        None,
        ClaudePermissionPolicy::InheritConfigured,
    );
    assert!(outcome.persona_injected);
    assert_eq!(outcome.persona_injection_skipped_reason, None);
    let args = command
        .as_std()
        .get_args()
        .map(|arg| arg.to_string_lossy().into_owned())
        .collect::<Vec<_>>();
    let appended_prompt = args
        .iter()
        .position(|arg| arg == "--append-system-prompt-file")
        .map(|index| read_test_file(&args[index + 1]))
        .or_else(|| {
            args.iter()
                .position(|arg| arg == "--append-system-prompt")
                .map(|index| args[index + 1].clone())
        })
        .expect("appended system prompt");

    assert!(appended_prompt.contains(persona));
}

#[test]
fn fallback_prompt_without_persona_pins_none_metadata() {
    let (_dir, _root, plugin_dir) = make_temp_project_plugin_dir();
    let persona = "<ralphx_agent_persona>Fallback persona</ralphx_agent_persona>";
    let mut command = tokio::process::Command::new("/fake/claude");

    let outcome = add_prompt_args(
        &mut command,
        &plugin_dir,
        "ordinary user request",
        Some(persona),
        Some("missing-agent"),
        None,
        None,
        false,
        None,
        None,
        ClaudePermissionPolicy::InheritConfigured,
    );

    assert!(
        !outcome.persona_injected,
        "native fallback must not claim that it injected a persona"
    );
    assert_eq!(
        outcome.persona_injection_skipped_reason,
        Some("agent_prompt_not_found_native_agent")
    );
}

#[test]
fn all_inherit_families_resolve_through_add_prompt_args_seam() {
    let (_dir, root, plugin_dir) = make_temp_project_plugin_dir();
    let agent_root = root.join("agents/test-agent");
    std::fs::create_dir_all(agent_root.join("claude")).expect("create agent prompt dir");
    std::fs::write(
        agent_root.join("agent.yaml"),
        "name: test-agent\nrole: test\n",
    )
    .expect("write agent definition");
    std::fs::write(agent_root.join("claude/prompt.md"), "Base prompt").expect("write prompt");

    let working_directory = tempfile::tempdir().expect("working directory");
    let persona = "<ralphx_agent_persona>Inherited voice</ralphx_agent_persona>";
    let commands = [
        build_spawnable_command_with_mcp_runtime_context_and_profile_for_test(
            std::path::Path::new("/fake/claude"),
            &plugin_dir,
            "continue",
            Some("test-agent"),
            None,
            Some(persona),
            Some("resume-session"),
            working_directory.path(),
            false,
            None,
            None,
            None,
        )
        .expect("noninteractive inherit command"),
        build_spawnable_interactive_command_with_mcp_runtime_context_and_profile_for_test(
            std::path::Path::new("/fake/claude"),
            &plugin_dir,
            "continue",
            Some("test-agent"),
            None,
            Some(persona),
            Some("resume-session"),
            working_directory.path(),
            false,
            None,
            None,
            None,
        )
        .expect("interactive inherit command"),
    ];

    for command in commands {
        let args = command.get_args_for_test().into_iter().collect::<Vec<_>>();
        let prompt = args
            .iter()
            .position(|arg| arg == "--append-system-prompt-file")
            .map(|index| read_test_file(&args[index + 1]))
            .or_else(|| {
                args.iter()
                    .position(|arg| arg == "--append-system-prompt")
                    .map(|index| args[index + 1].clone())
            })
            .expect("append-system-prompt argument");
        assert!(
            prompt.contains(persona),
            "inherit family must route through add_prompt_args with the persona block"
        );
    }
}

#[test]
fn native_agent_flag_bypass_reports_injection_skipped() {
    assert_eq!(
        persona_injection_skipped_reason(true, true),
        Some("native_agent_flag")
    );
    assert_eq!(persona_injection_skipped_reason(false, true), None);
    assert_eq!(persona_injection_skipped_reason(false, false), None);
}

#[test]
fn test_create_mcp_config_uses_claude_external_mcp_transport() {
    let (_dir, root, plugin_dir) = make_temp_project_plugin_dir();
    std::fs::create_dir_all(root.join("agents/ralphx-chat-project"))
        .expect("create canonical agent dir");
    std::fs::write(
        root.join("agents/ralphx-chat-project/agent.yaml"),
        r#"name: ralphx-chat-project
role: project_chat
harnesses:
  claude:
    mcp_transport: external
    mcp_tools:
      - v1_start_ideation
"#,
    )
    .expect("write agent definition");

    let runtime_context = McpRuntimeContext {
        context_type: Some("project".to_string()),
        context_id: Some("project-123".to_string()),
        conversation_id: Some("conversation current".to_string()),
        project_id: Some("project-123".to_string()),
        parent_conversation_id: Some("conversation 456".to_string()),
        agent_run_id: Some("run 789".to_string()),
        ..Default::default()
    };
    let json = build_mcp_config_with_runtime_context(
        &plugin_dir,
        "ralphx-chat-project",
        false,
        Some(&runtime_context),
    )
    .expect("should create external MCP config");
    let server = &json["mcpServers"]["ralphx"];

    assert_eq!(server["type"].as_str(), Some("http"));
    assert!(
        server["url"]
            .as_str()
            .is_some_and(|url| url.contains("conversation_id=conversation%20current")
                && url.contains("parent_conversation_id=conversation%20456")
                && url.contains("agent_run_id=run%20789")),
        "external MCP URL should carry encoded runtime context"
    );
    assert!(
        server["headers"]["Authorization"]
            .as_str()
            .is_some_and(|header| header.starts_with("Bearer rx_tauri_")),
        "external MCP config should use the local Tauri bypass token"
    );
    assert!(
        server.get("args").is_none(),
        "external MCP config must not launch the bundled stdio server"
    );
}

#[test]
fn test_create_mcp_config_mixes_external_transport_with_internal_sidecar_tools() {
    let (_dir, root, plugin_dir) = make_temp_project_plugin_dir();
    std::fs::create_dir_all(root.join("agents/ralphx-chat-project"))
        .expect("create canonical agent dir");
    std::fs::write(
        root.join("agents/ralphx-chat-project/agent.yaml"),
        r#"name: ralphx-chat-project
role: project_chat
harnesses:
  claude:
    mcp_transport: external
    mcp_tools:
      - v1_start_ideation
    internal_mcp_tools:
      - create_agent_task
      - list_agent_tasks
"#,
    )
    .expect("write agent definition");

    let json =
        build_mcp_config_with_runtime_context(&plugin_dir, "ralphx-chat-project", false, None)
            .expect("should create mixed MCP config");
    let external_server = &json["mcpServers"]["ralphx"];
    let internal_server = &json["mcpServers"]["ralphx_internal"];

    assert_eq!(external_server["type"].as_str(), Some("http"));
    assert!(
        external_server.get("args").is_none(),
        "external MCP server should remain HTTP-only"
    );
    assert_eq!(internal_server["type"].as_str(), Some("stdio"));
    let args = internal_server["args"]
        .as_array()
        .expect("internal sidecar args")
        .iter()
        .filter_map(|arg| arg.as_str())
        .collect::<Vec<_>>();
    assert!(
        args.contains(&"--allowed-tools=create_agent_task,list_agent_tasks"),
        "internal sidecar should be narrowed to declared internal tools"
    );
}

#[test]
fn test_materialize_generated_plugin_dir_renders_canonical_claude_frontmatter_without_legacy_agent_file(
) {
    let (_dir, root, plugin_dir) = make_temp_project_plugin_dir();
    let agent_root = root.join("agents/ralphx-ideation");
    std::fs::create_dir_all(agent_root.join("claude")).expect("create canonical claude dir");
    std::fs::write(
        agent_root.join("agent.yaml"),
        r#"name: ralphx-ideation
role: ideation_orchestrator
description: Facilitates ideation sessions and generates task proposals for RalphX.
"#,
    )
    .expect("write shared definition");
    std::fs::write(
        agent_root.join("claude/agent.yaml"),
        r#"disallowed_tools:
  - Write
  - Edit
  - NotebookEdit
skills:
  - task-decomposition
  - priority-assessment
  - dependency-analysis
"#,
    )
    .expect("write claude metadata");
    std::fs::write(
        agent_root.join("claude/prompt.md"),
        "Canonical Claude ideation prompt",
    )
    .expect("write claude prompt");

    let generated_dir =
        materialize_generated_plugin_dir(&plugin_dir).expect("materialize generated plugin dir");
    let generated_prompt = read_test_file(generated_dir.join("agents/ralphx-ideation.md"));

    assert!(
        generated_prompt.contains("name: ralphx-ideation"),
        "expected generated frontmatter name"
    );
    assert!(
        generated_prompt.contains("description: Facilitates ideation sessions"),
        "expected generated description"
    );
    assert!(
        generated_prompt.contains("mcp__ralphx__create_task_proposal"),
        "expected MCP tool grants from runtime config"
    );
    assert!(
        generated_prompt.contains("Task(Plan)")
            && !generated_prompt.contains("Task(Explore)")
            && !generated_prompt.contains("Task(ralphx:ralphx-ideation-specialist-ux)"),
        "expected only the retained Task(Plan) variant in generated frontmatter"
    );
    assert!(
        generated_prompt.contains("disallowedTools:\n  - Write\n  - Edit\n  - NotebookEdit"),
        "expected canonical claude disallowed tools"
    );
    assert!(
        generated_prompt.contains("skills:\n  - task-decomposition"),
        "expected canonical claude skills"
    );
    assert!(
        generated_prompt.contains("model: opus"),
        "expected runtime-derived model in generated frontmatter"
    );
    assert!(
        generated_prompt.contains("Canonical Claude ideation prompt"),
        "expected canonical prompt body to be preserved"
    );
}

#[test]
fn test_materialize_generated_plugin_dir_supports_shared_prompt_without_legacy_frontmatter() {
    let (_dir, root, plugin_dir) = make_temp_project_plugin_dir();
    let agent_root = root.join("agents/ralphx-utility-session-namer");
    std::fs::create_dir_all(agent_root.join("shared")).expect("create shared prompt dir");
    std::fs::write(
        agent_root.join("agent.yaml"),
        r#"name: ralphx-utility-session-namer
role: session_namer
description: Generates concise ideation session titles from user or plan context.
"#,
    )
    .expect("write shared definition");
    std::fs::write(
        agent_root.join("shared/prompt.md"),
        "Shared session naming prompt",
    )
    .expect("write shared prompt");

    let generated_dir =
        materialize_generated_plugin_dir(&plugin_dir).expect("materialize generated plugin dir");
    let generated_prompt =
        read_test_file(generated_dir.join("agents/ralphx-utility-session-namer.md"));

    assert!(
        generated_prompt.contains("model: haiku"),
        "expected runtime-derived model in generated frontmatter"
    );
    assert!(
        generated_prompt.contains("mcp__ralphx__update_session_title"),
        "expected ralphx-utility-session-namer MCP tool in generated frontmatter"
    );
    assert!(
        generated_prompt.contains("Shared session naming prompt"),
        "expected shared canonical prompt body to be preserved"
    );
}

#[test]
fn build_spawnable_command_injects_internal_skill_context_for_claude_prompt() {
    let (_dir, root, plugin_dir) = make_temp_project_plugin_dir();
    let agent_root = root.join("agents/ralphx-chat-project");
    std::fs::create_dir_all(agent_root.join("shared")).expect("create shared prompt dir");
    std::fs::write(
        agent_root.join("agent.yaml"),
        r#"name: ralphx-chat-project
role: project_chat
capabilities:
  internal_skills:
    allowed:
      - workspace-swe
"#,
    )
    .expect("write shared definition");
    std::fs::write(agent_root.join("shared/prompt.md"), "Project chat prompt")
        .expect("write shared prompt");
    std::fs::create_dir_all(root.join("plugins/app/skills/workspace-swe"))
        .expect("create skill dir");
    std::fs::write(
        root.join("plugins/app/skills/workspace-swe/SKILL.md"),
        r#"---
name: workspace-swe
description: Workspace bridge guidance
disable-model-invocation: true
user-invocable: false
---
# Workspace SWE
Report only unless workspace intervention is explicit.
"#,
    )
    .expect("write skill");

    let spawnable = build_spawnable_command_with_mcp_runtime_context_for_test(
        Path::new("/fake/claude"),
        &plugin_dir,
        "Use /workspace-swe skill for this bridge wake-up.",
        Some("ralphx:ralphx-chat-project"),
        None,
        Path::new("/tmp"),
        None,
        None,
        None,
    )
    .expect("build spawnable");
    let args = spawnable.get_args_for_test();
    let prompt_index = args
        .iter()
        .position(|arg| arg == "--append-system-prompt-file")
        .expect("expected system prompt file with internal skill context");
    let generated_prompt = read_test_file(Path::new(&args[prompt_index + 1]));
    assert!(
        generated_prompt.contains("Report only unless workspace intervention is explicit."),
        "expected internal skill body in Claude system prompt file"
    );
    assert!(
        !args.contains(&"--append-system-prompt".to_string()),
        "Claude must not use an inline prompt when internal skill context is selected"
    );
}

#[test]
fn append_system_prompt_args_falls_back_to_inline_on_write_error() {
    let mut cmd = tokio::process::Command::new("/fake/claude");
    let prompt = "Full generated system prompt";

    append_system_prompt_args(&mut cmd, "ralphx-test", prompt, true, |_| {
        Err("simulated prompt write failure".to_string())
    });

    let args: Vec<String> = cmd
        .as_std()
        .get_args()
        .map(|value| value.to_string_lossy().into_owned())
        .collect();
    let prompt_index = args
        .iter()
        .position(|arg| arg == "--append-system-prompt")
        .expect("write failure should fall back to inline system prompt");
    assert_eq!(args[prompt_index + 1], prompt);
    assert!(
        !args.contains(&"--append-system-prompt-file".to_string()),
        "write failure must not leave a system prompt file argument"
    );
}

#[test]
fn append_system_prompt_args_inline_when_file_delivery_disabled() {
    let mut cmd = tokio::process::Command::new("/fake/claude");
    let prompt = "Full generated system prompt";

    append_system_prompt_args(&mut cmd, "ralphx-test", prompt, false, |_| {
        panic!("disabled file delivery must not invoke the prompt writer")
    });

    let args: Vec<String> = cmd
        .as_std()
        .get_args()
        .map(|value| value.to_string_lossy().into_owned())
        .collect();
    let prompt_index = args
        .iter()
        .position(|arg| arg == "--append-system-prompt")
        .expect("disabled file delivery should use inline system prompt");
    assert_eq!(args[prompt_index + 1], prompt);
    assert!(
        !args.contains(&"--append-system-prompt-file".to_string()),
        "disabled file delivery must not add a system prompt file argument"
    );
}

#[test]
fn load_claude_prompt_injects_pre_execution_learned_skill_citations() {
    let (_dir, root, plugin_dir) = make_temp_project_plugin_dir();
    let agent_root = root.join("agents/ralphx-chat-project");
    std::fs::create_dir_all(agent_root.join("shared")).expect("create shared prompt dir");
    std::fs::write(
        agent_root.join("agent.yaml"),
        r#"name: ralphx-chat-project
role: project_chat
"#,
    )
    .expect("write shared definition");
    std::fs::write(agent_root.join("shared/prompt.md"), "Project chat prompt")
        .expect("write shared prompt");

    let context = PreExecutionLearnedSkillContext {
        request: LearnedSkillMultiSelectionRequest {
            project_id: "project-1".to_string(),
            caller_surface: "ralphx-chat-project".to_string(),
            stages: vec![LearnedSkillStage::Planning],
            buckets: vec![LearnedSkillBucket::Planning],
            touched_paths: vec!["src-tauri/src/domain/services/mod.rs".to_string()],
            max_skills: 4,
        },
        available_skills: vec![
            learned_skill("skill-planning", "project-1")
                .with_caller_surfaces(vec!["ralphx-chat-project"])
                .with_stages(vec![LearnedSkillStage::Planning])
                .with_buckets(vec![LearnedSkillBucket::Planning])
                .with_path_scopes(vec!["src-tauri/src/domain"]),
            learned_skill("skill-review", "project-1")
                .with_caller_surfaces(vec!["ralphx-review-history"])
                .with_stages(vec![LearnedSkillStage::Planning])
                .with_buckets(vec![LearnedSkillBucket::Planning])
                .with_path_scopes(vec!["src-tauri/src/domain"]),
        ],
        max_total_chars: 6_000,
        max_guidance_chars: 400,
    };

    let (system_prompt, injected_names) = load_agent_system_prompt_with_internal_skills(
        &plugin_dir,
        "ralphx:ralphx-chat-project",
        None,
        "Plan the domain change.",
        Some("<ralphx_agent_persona>Methodical planner</ralphx_agent_persona>"),
        Some(&context),
    )
    .expect("load prompt");

    assert!(system_prompt.contains("Project chat prompt"));
    assert!(system_prompt.contains("Methodical planner"));
    assert!(system_prompt.contains("<ralphx_learned_skill_citations>"));
    assert!(system_prompt.contains("skill-planning"));
    assert!(!system_prompt.contains("skill-review"));
    assert_eq!(injected_names, vec!["learned:skill-planning"]);
}

#[test]
fn build_spawnable_command_prompt_file_uses_generated_claude_prompt() {
    let (_dir, root, plugin_dir) = make_temp_project_plugin_dir();
    let agent_root = root.join("agents/ralphx-general-worker");
    std::fs::create_dir_all(agent_root.join("shared")).expect("create shared prompt dir");
    std::fs::write(
        agent_root.join("agent.yaml"),
        r#"name: ralphx-general-worker
role: general_worker
capabilities:
  mcp_tools:
    - list_agent_tasks
    - create_agent_task
    - claim_agent_task
    - update_agent_task
    - complete_agent_task
"#,
    )
    .expect("write agent definition");
    let raw_prompt_path = agent_root.join("shared/prompt.md");
    std::fs::write(&raw_prompt_path, "General worker prompt").expect("write shared prompt");
    let raw_prompt_path_str = raw_prompt_path.to_string_lossy().into_owned();

    let spawnable = build_spawnable_command_with_mcp_runtime_context_for_test(
        Path::new("/fake/claude"),
        &plugin_dir,
        "Implement a scoped change.",
        Some("ralphx:ralphx-general-worker"),
        None,
        Path::new("/tmp"),
        None,
        None,
        None,
    )
    .expect("build spawnable");
    let args = spawnable.get_args_for_test();
    let prompt_index = args
        .iter()
        .position(|arg| arg == "--append-system-prompt-file")
        .expect("expected generated system prompt file");
    let generated_prompt_path = &args[prompt_index + 1];
    assert_ne!(
        generated_prompt_path.as_str(),
        raw_prompt_path_str.as_str(),
        "Claude must not use the raw canonical prompt file when generated appendices are present"
    );
    let generated_prompt = read_test_file(Path::new(generated_prompt_path.as_str()));
    assert!(
        generated_prompt.contains("General worker prompt"),
        "generated prompt should preserve the canonical prompt body"
    );
    assert!(
        generated_prompt.contains("<agent_task_ledger_contract>"),
        "generated prompt file should include the task ledger appendix"
    );
    assert!(
        generated_prompt.contains(
            "For two or more requested fixes, checks, audit items, or investigation streams"
        ),
        "generated prompt file should include the strengthened task-ledger breakdown rule"
    );
}

#[test]
fn test_materialize_generated_plugin_dir_skips_canonical_agent_symlinks_outside_project_root() {
    let (_dir, root, plugin_dir) = make_temp_project_plugin_dir();
    let outside_dir = tempfile::TempDir::new().expect("create outside dir");
    let outside_agent_root = outside_dir.path().join("ralphx-escape");
    std::fs::create_dir_all(outside_agent_root.join("claude")).expect("create outside claude dir");
    std::fs::write(
        outside_agent_root.join("agent.yaml"),
        "name: ralphx-escape\nrole: test_agent\n",
    )
    .expect("write outside agent definition");
    std::fs::write(
        outside_agent_root.join("claude/prompt.md"),
        "escaped canonical prompt",
    )
    .expect("write outside claude prompt");
    std::fs::create_dir_all(root.join("agents")).expect("create project agents dir");
    symlink_dir(&outside_agent_root, root.join("agents/ralphx-escape"));

    let generated_dir =
        materialize_generated_plugin_dir(&plugin_dir).expect("materialize generated plugin dir");

    assert!(
        !test_path_exists(generated_dir.join("agents/ralphx-escape.md")),
        "generated plugin materialization must ignore canonical agent directories that resolve outside the project root"
    );
}

#[test]
fn test_materialize_generated_plugin_dir_renders_canonical_claude_max_turns() {
    let (_dir, root, plugin_dir) = make_temp_project_plugin_dir();
    let agent_root = root.join("agents/ralphx-utility-session-namer");
    std::fs::create_dir_all(agent_root.join("claude")).expect("create canonical claude dir");
    std::fs::write(
        agent_root.join("agent.yaml"),
        r#"name: ralphx-utility-session-namer
role: session_namer
description: Test-only canonical agent for generated plugin coverage.
"#,
    )
    .expect("write shared definition");
    std::fs::write(
        agent_root.join("claude/agent.yaml"),
        r#"disallowed_tools:
  - Write
  - Edit
  - NotebookEdit
max_turns: 80
"#,
    )
    .expect("write claude metadata");
    std::fs::write(
        agent_root.join("claude/prompt.md"),
        "Canonical test agent prompt",
    )
    .expect("write claude prompt");

    let generated_dir =
        materialize_generated_plugin_dir(&plugin_dir).expect("materialize generated plugin dir");
    let generated_prompt =
        read_test_file(generated_dir.join("agents/ralphx-utility-session-namer.md"));

    assert!(
        generated_prompt.contains("maxTurns: 80"),
        "expected canonical claude maxTurns in generated frontmatter"
    );
    assert!(
        generated_prompt.contains("Canonical test agent prompt"),
        "expected canonical prompt body to be preserved"
    );
}

#[test]
fn test_materialize_generated_plugin_dir_reuses_first_materialization_within_process() {
    let (_dir, root, plugin_dir) = make_temp_project_plugin_dir();
    let agent_root = root.join("agents/ralphx-utility-session-namer");
    std::fs::create_dir_all(agent_root.join("shared")).expect("create shared prompt dir");
    std::fs::write(
        agent_root.join("agent.yaml"),
        r#"name: ralphx-utility-session-namer
role: session_namer
description: Generates concise ideation session titles from user or plan context.
"#,
    )
    .expect("write shared definition");
    std::fs::write(
        agent_root.join("shared/prompt.md"),
        "Initial generated prompt",
    )
    .expect("write initial shared prompt");

    let generated_dir =
        materialize_generated_plugin_dir(&plugin_dir).expect("first generated plugin dir");
    let generated_prompt_path = generated_dir.join("agents/ralphx-utility-session-namer.md");
    let first_prompt = read_test_file(&generated_prompt_path);
    assert!(
        first_prompt.contains("Initial generated prompt"),
        "first materialization should render the initial prompt body"
    );

    std::fs::write(
        agent_root.join("shared/prompt.md"),
        "Updated prompt that should require an app restart",
    )
    .expect("write updated shared prompt");

    let reused_dir =
        materialize_generated_plugin_dir(&plugin_dir).expect("reused generated plugin dir");
    assert_eq!(
        reused_dir, generated_dir,
        "generated plugin path should be stable within the same process"
    );

    let reused_prompt = read_test_file(&generated_prompt_path);
    assert!(
        reused_prompt.contains("Initial generated prompt"),
        "later materialize calls in the same process must reuse the first generated prompt"
    );
    assert!(
        !reused_prompt.contains("Updated prompt that should require an app restart"),
        "later materialize calls must not rewrite generated prompts mid-process"
    );
}

#[test]
fn test_materialize_generated_plugin_dir_repairs_cached_runtime_entries_after_external_mutation() {
    let (_dir, root, plugin_dir) = make_temp_project_plugin_dir();
    let agent_root = root.join("agents/ralphx-utility-session-namer");
    std::fs::create_dir_all(agent_root.join("shared")).expect("create shared prompt dir");
    std::fs::write(
        agent_root.join("agent.yaml"),
        "name: ralphx-utility-session-namer\nrole: session_namer\n",
    )
    .expect("write shared definition");
    std::fs::write(
        agent_root.join("shared/prompt.md"),
        "Prompt before runtime contamination",
    )
    .expect("write shared prompt");

    let generated_dir =
        materialize_generated_plugin_dir(&plugin_dir).expect("first generated plugin dir");
    let expected_mcp_source = read_test_link(generated_dir.join("ralphx-mcp-server"));
    let stale_runtime = tempfile::TempDir::new().expect("create stale runtime dir");
    let stale_mcp_dir = stale_runtime.path().join("plugins/app/ralphx-mcp-server");
    std::fs::create_dir_all(&stale_mcp_dir).expect("create stale mcp dir");
    let generated_mcp_dir = generated_dir.join("ralphx-mcp-server");
    remove_test_file_or_dir(&generated_mcp_dir);
    symlink_dir(&stale_mcp_dir, &generated_mcp_dir);
    symlink_dir(&stale_mcp_dir, generated_dir.join(".cache"));

    let repaired_dir =
        materialize_generated_plugin_dir(&plugin_dir).expect("repair generated plugin dir");

    assert_eq!(repaired_dir, generated_dir);
    assert_eq!(
        read_test_link(repaired_dir.join("ralphx-mcp-server")),
        expected_mcp_source,
        "cached materialization should repair externally mutated managed runtime symlinks"
    );
    assert!(
        test_symlink_metadata_is_err(repaired_dir.join(".cache")),
        "cached materialization should remove unmanaged top-level entries"
    );
    assert!(
        read_test_file(repaired_dir.join("agents/ralphx-utility-session-namer.md"))
            .contains("Prompt before runtime contamination"),
        "repair should preserve generated canonical prompt materialization"
    );
}

#[test]
fn test_materialize_generated_plugin_dir_prefers_root_canonical_claude_disallowed_tools() {
    let (_dir, _root, plugin_dir, _runtime_guard) = make_isolated_live_project_plugin_dir();
    let generated_dir =
        materialize_generated_plugin_dir(&plugin_dir).expect("materialize generated plugin dir");
    let generated_prompt = read_test_file(generated_dir.join("agents/ralphx-ideation.md"));
    let (frontmatter, _) = split_frontmatter(&generated_prompt);
    let disallowed_tools = frontmatter["disallowedTools"]
        .as_sequence()
        .expect("generated frontmatter should include disallowedTools")
        .iter()
        .filter_map(Value::as_str)
        .collect::<Vec<_>>();

    assert_eq!(
        disallowed_tools,
        vec!["Write", "Edit", "NotebookEdit", "Task(ralphx:*)"],
        "expected root canonical Claude disallowedTools in generated frontmatter"
    );
}

#[test]
fn test_materialize_generated_plugin_dir_omits_removed_supervisor_agent() {
    let (_dir, _root, plugin_dir, _runtime_guard) = make_isolated_live_project_plugin_dir();
    let generated_dir =
        materialize_generated_plugin_dir(&plugin_dir).expect("materialize generated plugin dir");
    let generated_prompt_path = generated_dir.join("agents/ralphx-execution-supervisor.md");

    assert!(
        !test_path_exists(&generated_prompt_path),
        "removed supervisor agent should not be materialized into generated Claude assets"
    );
}

#[test]
fn test_materialize_generated_plugin_dir_matches_canonical_and_runtime_semantics_for_live_agents() {
    let (_dir, root, plugin_dir, _runtime_guard) = make_isolated_live_project_plugin_dir();
    let generated_dir =
        materialize_generated_plugin_dir(&plugin_dir).expect("materialize generated plugin dir");
    let agent_names =
        crate::infrastructure::agents::harness_agent_catalog::list_canonical_prompt_backed_agents(
            &root,
            crate::infrastructure::agents::harness_agent_catalog::AgentPromptHarness::Claude,
        );

    for agent_name in agent_names {
        let generated_path = generated_dir
            .join("agents")
            .join(format!("{agent_name}.md"));
        let generated_markdown = read_test_file(&generated_path);
        let definition = load_canonical_agent_definition(&root, &agent_name)
            .unwrap_or_else(|| panic!("missing canonical definition for {agent_name}"));
        let canonical_body =
            load_harness_agent_prompt(&root, &agent_name, AgentPromptHarness::Claude)
                .unwrap_or_else(|| panic!("missing canonical Claude prompt for {agent_name}"));
        let (generated_frontmatter, generated_body) = split_frontmatter(&generated_markdown);

        assert_eq!(
            Some(definition.name.as_str()),
            generated_frontmatter["name"].as_str(),
            "generated Claude name drifted from canonical definition for {agent_name}"
        );
        assert_eq!(
            definition.description.as_deref(),
            generated_frontmatter["description"].as_str(),
            "generated Claude description drifted from canonical definition for {agent_name}"
        );
        assert_eq!(
            get_agent_config(&agent_name)
                .unwrap_or_else(|| panic!("missing runtime config for {agent_name}"))
                .model
                .as_deref(),
            generated_frontmatter["model"].as_str(),
            "generated Claude model drifted from runtime config for {agent_name}"
        );
        assert_eq!(
            expected_frontmatter_tools(&agent_name),
            frontmatter_tools_set(&generated_frontmatter),
            "generated Claude tools drifted from runtime config for {agent_name}"
        );
        assert_eq!(
            !get_agent_config(&agent_name)
                .unwrap_or_else(|| panic!("missing runtime config for {agent_name}"))
                .allowed_mcp_tools
                .is_empty(),
            frontmatter_has_mcp_servers(&generated_frontmatter),
            "generated Claude mcpServers presence drifted from runtime config for {agent_name}"
        );
        assert_eq!(
            canonical_body, generated_body,
            "generated Claude prompt body drifted from canonical source for {agent_name}"
        );
    }
}

#[test]
fn generated_plugin_agents_contain_no_persona_content() {
    let (_dir, root, plugin_dir, _runtime_guard) = make_isolated_live_project_plugin_dir();
    let generated_dir =
        materialize_generated_plugin_dir(&plugin_dir).expect("materialize generated plugin dir");
    let agent_names =
        crate::infrastructure::agents::harness_agent_catalog::list_canonical_prompt_backed_agents(
            &root,
            crate::infrastructure::agents::harness_agent_catalog::AgentPromptHarness::Claude,
        );

    for agent_name in agent_names {
        let generated_markdown = read_test_file(
            generated_dir
                .join("agents")
                .join(format!("{agent_name}.md")),
        );
        assert!(
            !generated_markdown.contains("<ralphx_agent_persona>"),
            "generated agent {agent_name} must not contain conversation persona content"
        );
    }
}

#[test]
fn test_materialize_generated_plugin_dir_uses_fallback_runtime_entries_when_local_bundle_is_incomplete(
) {
    let (_dir, root, plugin_dir) = make_temp_project_plugin_dir();
    std::fs::create_dir_all(root.join("agents/ralphx-utility-session-namer/claude"))
        .expect("create canonical claude dir");
    std::fs::write(
        root.join("agents/ralphx-utility-session-namer/agent.yaml"),
        "name: ralphx-utility-session-namer\nrole: session_namer\n",
    )
    .expect("write shared definition");
    std::fs::write(
        root.join("agents/ralphx-utility-session-namer/claude/prompt.md"),
        "Local canonical test prompt",
    )
    .expect("write local canonical prompt");
    std::fs::write(
        plugin_dir.join(".mcp.json"),
        r#"{"mcpServers":{"ralphx":{"type":"stdio","command":"node","args":["local-config"]}}}"#,
    )
    .expect("write local mcp config");
    std::fs::write(
        plugin_dir.join("ralphx-mcp-server/build/index.js"),
        "// incomplete local runtime",
    )
    .expect("write incomplete local runtime");

    let fallback_dir = tempfile::TempDir::new().expect("create fallback runtime dir");
    let fallback_plugin_dir = fallback_dir.path().join("plugins/app");
    std::fs::create_dir_all(&fallback_plugin_dir).expect("create fallback plugin dir");
    std::fs::write(
        fallback_plugin_dir.join(".mcp.json"),
        r#"{"mcpServers":{"ralphx":{"type":"stdio","command":"node","args":["fallback-config"]}}}"#,
    )
    .expect("write fallback mcp config");
    seed_runnable_mcp_runtime(&fallback_plugin_dir, "// fallback runtime");

    let generated_dir = materialize_generated_plugin_dir_with_runtime_source(
        &plugin_dir,
        Some(&fallback_plugin_dir),
    )
    .expect("materialize generated plugin dir");

    assert_eq!(
        read_test_file(generated_dir.join(".mcp.json")),
        r#"{"mcpServers":{"ralphx":{"type":"stdio","command":"node","args":["local-config"]}}}"#,
        "generated plugin should preserve the local config surface"
    );
    assert_eq!(
        read_test_file(generated_dir.join("ralphx-mcp-server/build/index.js")),
        "// fallback runtime",
        "generated plugin should link the runnable fallback runtime bundle"
    );
    assert!(
        read_test_file(generated_dir.join("agents/ralphx-utility-session-namer.md"))
            .contains("Local canonical test prompt"),
        "generated plugin should keep canonical prompts from the local RalphX checkout"
    );
}
