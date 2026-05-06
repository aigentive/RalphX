use super::{
    build_codex_exec_args, build_codex_mcp_overrides, build_spawnable_codex_exec_command,
    compose_codex_prompt, configure_spawn, CodexCliCapabilities, CodexExecCliConfig,
    CodexMcpRuntimeContext,
};
use crate::domain::agents::LogicalEffort;
use std::ffi::OsStr;
use std::path::PathBuf;

fn full_codex_capabilities() -> CodexCliCapabilities {
    CodexCliCapabilities {
        version: Some("codex-cli 1.0.0".to_string()),
        supports_exec_subcommand: true,
        supports_json_output: true,
        supports_model_flag: true,
        supports_config_override: true,
        supports_sandbox_flag: true,
        supports_add_dir: true,
        supports_search_flag: true,
        supports_resume_subcommand: true,
        supports_mcp_subcommand: true,
    }
}

fn create_plugin_dir(root: &std::path::Path) -> PathBuf {
    let plugin_dir = root.join("plugins/app");
    std::fs::create_dir_all(plugin_dir.join("agents")).expect("create plugin agents dir");
    plugin_dir
}

#[test]
fn build_codex_exec_command_sets_agent_tool_path() {
    let spawnable = build_spawnable_codex_exec_command(
        std::path::Path::new("/fake/codex"),
        "Prompt",
        &full_codex_capabilities(),
        &CodexExecCliConfig::default(),
    )
    .expect("build codex exec command");

    let path = spawnable
        .get_envs_for_test()
        .into_iter()
        .find_map(|(key, value)| (key == "PATH").then(|| value.to_string_lossy().into_owned()))
        .expect("PATH should be explicitly set for Codex agent subprocesses");

    assert!(path.contains("/opt/homebrew/bin"));
    assert!(path.contains("/usr/local/bin"));
}

#[test]
fn build_codex_exec_args_preserves_gpt55_xhigh_selection() {
    let args = build_codex_exec_args(
        &full_codex_capabilities(),
        &CodexExecCliConfig {
            model: Some("gpt-5.5".to_string()),
            reasoning_effort: Some(LogicalEffort::XHigh),
            ..CodexExecCliConfig::default()
        },
    )
    .expect("build codex exec args");

    assert!(args
        .windows(2)
        .any(|pair| pair[0] == "-m" && pair[1] == "gpt-5.5"));
    assert!(args
        .windows(2)
        .any(|pair| pair[0] == "-c" && pair[1] == "model_reasoning_effort=\"xhigh\""));
}

#[test]
fn build_codex_exec_args_passes_each_supported_reasoning_effort() {
    for (effort, expected) in [
        (LogicalEffort::Low, "low"),
        (LogicalEffort::Medium, "medium"),
        (LogicalEffort::High, "high"),
        (LogicalEffort::XHigh, "xhigh"),
    ] {
        let args = build_codex_exec_args(
            &full_codex_capabilities(),
            &CodexExecCliConfig {
                model: Some("gpt-5.5".to_string()),
                reasoning_effort: Some(effort),
                ..CodexExecCliConfig::default()
            },
        )
        .expect("build codex exec args");

        assert!(args
            .windows(2)
            .any(|pair| pair[0] == "-c"
                && pair[1] == format!("model_reasoning_effort=\"{expected}\"")));
    }
}

#[test]
fn compose_codex_prompt_prefers_canonical_codex_prompt_when_available() {
    let temp_dir = tempfile::tempdir().expect("temp dir");
    let root = temp_dir.path();
    let plugin_dir = create_plugin_dir(root);

    std::fs::create_dir_all(root.join("agents/ralphx-utility-session-namer/codex"))
        .expect("create canonical codex dir");
    std::fs::write(
        root.join("agents/ralphx-utility-session-namer/agent.yaml"),
        "name: ralphx-utility-session-namer\nrole: session_namer\n",
    )
    .expect("write shared definition");
    std::fs::write(
        root.join("agents/ralphx-utility-session-namer/codex/prompt.md"),
        "Canonical Codex Prompt",
    )
    .expect("write canonical codex prompt");
    std::fs::write(
        plugin_dir.join("agents/ralphx-utility-session-namer.md"),
        "---\nname: ralphx-utility-session-namer\n---\nLegacy Claude Prompt",
    )
    .expect("write legacy prompt");

    let composed = compose_codex_prompt(
        "User prompt",
        Some(&plugin_dir),
        Some("ralphx-utility-session-namer"),
    );

    assert!(
        composed.contains("Canonical Codex Prompt"),
        "expected canonical codex prompt to be injected"
    );
    assert!(
        !composed.contains("Legacy Claude Prompt"),
        "expected legacy claude prompt to be ignored when canonical codex prompt exists"
    );
}

#[test]
fn compose_codex_prompt_ignores_legacy_claude_prompt_when_canonical_prompt_missing() {
    let temp_dir = tempfile::tempdir().expect("temp dir");
    let root = temp_dir.path();
    let plugin_dir = create_plugin_dir(root);

    std::fs::write(
        plugin_dir.join("agents/ralphx-utility-session-namer.md"),
        "---\nname: ralphx-utility-session-namer\n---\nLegacy Claude Prompt",
    )
    .expect("write legacy prompt");

    let composed = compose_codex_prompt(
        "User prompt",
        Some(&plugin_dir),
        Some("ralphx-utility-session-namer"),
    );

    assert_eq!(
        composed, "User prompt",
        "Codex should not inherit deleted legacy Claude plugin prompt files"
    );
}

#[test]
fn compose_codex_prompt_uses_shared_prompt_when_harness_is_explicitly_allowed() {
    let temp_dir = tempfile::tempdir().expect("temp dir");
    let root = temp_dir.path();
    let plugin_dir = create_plugin_dir(root);

    std::fs::create_dir_all(root.join("agents/ralphx-utility-session-namer/shared"))
        .expect("create shared prompt dir");
    std::fs::write(
        root.join("agents/ralphx-utility-session-namer/agent.yaml"),
        "name: ralphx-utility-session-namer\nrole: session_namer\n",
    )
    .expect("write shared definition");
    std::fs::write(
        root.join("agents/ralphx-utility-session-namer/shared/prompt.md"),
        "Shared Session Namer Prompt",
    )
    .expect("write shared prompt");
    std::fs::write(
        plugin_dir.join("agents/ralphx-utility-session-namer.md"),
        "---\nname: ralphx-utility-session-namer\n---\nLegacy Claude Prompt",
    )
    .expect("write legacy prompt");

    let composed = compose_codex_prompt(
        "User prompt",
        Some(&plugin_dir),
        Some("ralphx-utility-session-namer"),
    );

    assert!(
        composed.contains("Shared Session Namer Prompt"),
        "expected shared prompt to be injected for supported codex harnesses"
    );
    assert!(
        !composed.contains("Legacy Claude Prompt"),
        "expected shared canonical prompt to ignore deleted legacy Claude plugin prompt files"
    );
}

#[test]
fn compose_codex_prompt_injects_directed_internal_skill_context() {
    let temp_dir = tempfile::tempdir().expect("temp dir");
    let root = temp_dir.path();
    let plugin_dir = create_plugin_dir(root);

    std::fs::create_dir_all(root.join("agents/ralphx-chat-project/shared"))
        .expect("create shared prompt dir");
    std::fs::write(
        root.join("agents/ralphx-chat-project/agent.yaml"),
        r#"name: ralphx-chat-project
role: project_chat
capabilities:
  internal_skills:
    allowed:
      - workspace-swe
"#,
    )
    .expect("write shared definition");
    std::fs::write(
        root.join("agents/ralphx-chat-project/shared/prompt.md"),
        "Project chat prompt",
    )
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

    let composed = compose_codex_prompt(
        "<!-- ralphx_internal_skill=workspace-swe -->\nBridge payload",
        Some(&plugin_dir),
        Some("ralphx-chat-project"),
    );

    assert!(composed.contains("Project chat prompt"));
    assert!(
        composed.contains("<ralphx_internal_skills>"),
        "expected internal skill context to be injected"
    );
    assert!(
        composed.contains("Report only unless workspace intervention is explicit."),
        "expected directed skill body to be injected"
    );
}

#[test]
fn compose_codex_prompt_does_not_fall_back_to_legacy_prompt_when_canonical_agent_lacks_codex_prompt(
) {
    let temp_dir = tempfile::tempdir().expect("temp dir");
    let root = temp_dir.path();
    let plugin_dir = create_plugin_dir(root);

    std::fs::create_dir_all(root.join("agents/ralphx-ideation-team-lead/claude"))
        .expect("create canonical claude dir");
    std::fs::write(
        root.join("agents/ralphx-ideation-team-lead/agent.yaml"),
        "name: ralphx-ideation-team-lead\nrole: ideation_team_lead\n",
    )
    .expect("write shared definition");
    std::fs::write(
        root.join("agents/ralphx-ideation-team-lead/claude/prompt.md"),
        "Canonical Claude Prompt",
    )
    .expect("write canonical claude prompt");
    std::fs::write(
        plugin_dir.join("agents/ralphx-ideation-team-lead.md"),
        "---\nname: ralphx-ideation-team-lead\n---\nLegacy Claude Prompt",
    )
    .expect("write legacy prompt");

    let composed = compose_codex_prompt(
        "User prompt",
        Some(&plugin_dir),
        Some("ralphx-ideation-team-lead"),
    );

    assert_eq!(
        composed, "User prompt",
        "canonical agents without a codex prompt should not silently inherit the legacy claude prompt"
    );
}

#[test]
fn build_codex_mcp_overrides_includes_runtime_feature_flags_from_agent_metadata() {
    let temp_dir = tempfile::tempdir().expect("temp dir");
    let root = temp_dir.path();
    let plugin_dir = create_plugin_dir(root);
    std::fs::create_dir_all(root.join("agents/ralphx-plan-verifier/codex"))
        .expect("create canonical codex dir");
    std::fs::write(
        root.join("agents/ralphx-plan-verifier/agent.yaml"),
        "name: ralphx-plan-verifier\nrole: plan_verifier\n",
    )
    .expect("write shared definition");
    std::fs::write(
        root.join("agents/ralphx-plan-verifier/codex/agent.yaml"),
        "runtime_features:\n  shell_tool: false\n",
    )
    .expect("write codex metadata");
    std::fs::create_dir_all(plugin_dir.join("ralphx-mcp-server/build"))
        .expect("create fake mcp build dir");
    std::fs::write(
        plugin_dir.join("ralphx-mcp-server/build/index.js"),
        "// fake mcp server",
    )
    .expect("write fake mcp server");

    let overrides = build_codex_mcp_overrides(&plugin_dir, "ralphx-plan-verifier", false, None)
        .expect("overrides");

    assert!(
        overrides
            .iter()
            .any(|entry| entry == "features.shell_tool=false"),
        "Codex runtime feature flags should flow into config overrides: {overrides:?}"
    );
}

#[test]
fn build_codex_mcp_overrides_passes_runtime_context_over_cli_args() {
    let temp_dir = tempfile::tempdir().expect("temp dir");
    let root = temp_dir.path();
    let plugin_dir = create_plugin_dir(root);
    std::fs::create_dir_all(plugin_dir.join("ralphx-mcp-server/build"))
        .expect("create fake mcp build dir");
    std::fs::write(
        plugin_dir.join("ralphx-mcp-server/build/index.js"),
        "// fake mcp server",
    )
    .expect("write fake mcp server");

    let runtime_context = CodexMcpRuntimeContext {
        context_type: Some("ideation".to_string()),
        context_id: Some("session-123".to_string()),
        task_id: None,
        project_id: Some("project-456".to_string()),
        working_directory: Some(root.join("workspace")),
        lead_session_id: Some("lead-789".to_string()),
        parent_conversation_id: None,
    };

    let overrides = build_codex_mcp_overrides(
        &plugin_dir,
        "ralphx-plan-verifier",
        false,
        Some(&runtime_context),
    )
    .expect("overrides");

    let args_override = overrides
        .iter()
        .find(|entry| entry.starts_with("mcp_servers.") && entry.contains(".args="))
        .expect("args override");

    assert!(
        args_override.contains("--context-type"),
        "expected context-type CLI arg in overrides: {args_override}"
    );
    assert!(
        args_override.contains("--tauri-api-url"),
        "expected tauri-api-url CLI arg in overrides: {args_override}"
    );
    assert!(
        args_override.contains("http://127.0.0.1:"),
        "expected loopback Tauri API URL value in overrides: {args_override}"
    );
    assert!(
        args_override.contains("ideation"),
        "expected context-type value in overrides: {args_override}"
    );
    assert!(
        args_override.contains("--context-id"),
        "expected context-id CLI arg in overrides: {args_override}"
    );
    assert!(
        args_override.contains("session-123"),
        "expected context-id value in overrides: {args_override}"
    );
    assert!(
        args_override.contains("--project-id"),
        "expected project-id CLI arg in overrides: {args_override}"
    );
    assert!(
        args_override.contains("project-456"),
        "expected project-id value in overrides: {args_override}"
    );
    assert!(
        args_override.contains("--working-directory"),
        "expected working-directory CLI arg in overrides: {args_override}"
    );
    assert!(
        args_override.contains("--lead-session-id"),
        "expected lead-session-id CLI arg in overrides: {args_override}"
    );
}

#[test]
fn configure_spawn_prepends_resolved_node_bin_to_path() {
    let expected_node_bin = crate::infrastructure::tool_paths::resolve_node_cli_path()
        .parent()
        .map(PathBuf::from)
        .expect("resolved node bin");

    let mut cmd = tokio::process::Command::new("/usr/bin/env");
    cmd.env("PATH", "/usr/bin:/bin");
    configure_spawn(&mut cmd, None);

    let path_value = cmd
        .as_std()
        .get_envs()
        .find_map(|(key, value)| {
            (key == OsStr::new("PATH")).then(|| value.map(|v| v.to_os_string()))?
        })
        .expect("PATH env");
    let path_entries = std::env::split_paths(&path_value).collect::<Vec<_>>();
    assert_eq!(path_entries.first(), Some(&expected_node_bin));
}

#[test]
fn build_codex_mcp_overrides_uses_external_mcp_transport_when_declared() {
    let temp_dir = tempfile::tempdir().expect("temp dir");
    let root = temp_dir.path();
    let plugin_dir = create_plugin_dir(root);
    std::fs::create_dir_all(root.join("agents/ralphx-chat-project"))
        .expect("create canonical agent dir");
    std::fs::write(
        root.join("agents/ralphx-chat-project/agent.yaml"),
        r#"name: ralphx-chat-project
role: project_chat
harnesses:
  codex:
    mcp_transport: external
    mcp_tools:
      - v1_start_ideation
      - v1_get_ideation_status
    runtime_features:
      shell_tool: false
"#,
    )
    .expect("write shared definition");

    let overrides = build_codex_mcp_overrides(&plugin_dir, "ralphx-chat-project", false, None)
        .expect("overrides");

    assert!(
        overrides
            .iter()
            .any(|entry| entry.starts_with("mcp_servers.ralphx.url=")),
        "external MCP transport should use a streamable HTTP URL: {overrides:?}"
    );
    assert!(
        overrides.iter().any(|entry| {
            entry == "mcp_servers.ralphx.bearer_token_env_var=\"RALPHX_TAURI_MCP_BYPASS_TOKEN\""
        }),
        "external MCP transport should use the Tauri bypass token env var: {overrides:?}"
    );
    assert!(
        overrides
            .iter()
            .any(|entry| entry == "mcp_servers.ralphx.enabled_tools=[\"v1_start_ideation\",\"v1_get_ideation_status\"]"),
        "external MCP enabled tools should come from Codex metadata: {overrides:?}"
    );
    assert!(
        !overrides.iter().any(|entry| entry.contains(".command=") || entry.contains(".args=")),
        "external MCP transport must not point Codex at the bundled stdio MCP server: {overrides:?}"
    );
    assert!(
        overrides
            .iter()
            .any(|entry| entry == "features.shell_tool=false"),
        "runtime feature flags should still be preserved: {overrides:?}"
    );
}

#[test]
fn build_codex_mcp_overrides_threads_runtime_context_into_external_mcp_url() {
    let temp_dir = tempfile::tempdir().expect("temp dir");
    let root = temp_dir.path();
    let plugin_dir = create_plugin_dir(root);
    std::fs::create_dir_all(root.join("agents/ralphx-chat-project"))
        .expect("create canonical agent dir");
    std::fs::write(
        root.join("agents/ralphx-chat-project/agent.yaml"),
        r#"name: ralphx-chat-project
role: project_chat
harnesses:
  codex:
    mcp_transport: external
    mcp_tools:
      - v1_start_ideation
"#,
    )
    .expect("write shared definition");

    let runtime_context = CodexMcpRuntimeContext {
        context_type: Some("project".to_string()),
        context_id: Some("project-123".to_string()),
        task_id: None,
        project_id: Some("project-123".to_string()),
        working_directory: Some(root.join("workspace")),
        lead_session_id: None,
        parent_conversation_id: Some("conversation 456".to_string()),
    };

    let overrides = build_codex_mcp_overrides(
        &plugin_dir,
        "ralphx-chat-project",
        false,
        Some(&runtime_context),
    )
    .expect("overrides");

    let url_override = overrides
        .iter()
        .find(|entry| entry.starts_with("mcp_servers.ralphx.url="))
        .expect("external MCP URL override");

    assert!(
        url_override.contains("context_type=project"),
        "external MCP URL should include context type: {url_override}"
    );
    assert!(
        url_override.contains("project_id=project-123"),
        "external MCP URL should include project id: {url_override}"
    );
    assert!(
        url_override.contains("parent_conversation_id=conversation%20456"),
        "external MCP URL should include encoded parent conversation id: {url_override}"
    );
}
