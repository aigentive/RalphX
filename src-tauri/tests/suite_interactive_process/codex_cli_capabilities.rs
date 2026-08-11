use std::path::PathBuf;

use ralphx_lib::commands::diagnostic_commands::{
    build_codex_cli_diagnostics_response, CodexCliProbeStatus,
};
use ralphx_lib::domain::agents::LogicalEffort;
use ralphx_lib::infrastructure::agents::{
    build_codex_exec_args, build_codex_exec_resume_args, build_spawnable_codex_exec_command,
    build_spawnable_codex_resume_command, compose_codex_prompt, normalize_codex_exec_output,
    parse_codex_cli_capabilities, parse_codex_version, CodexCliCapabilities, CodexExecCliConfig,
};
use ralphx_lib::utils::runtime_log_paths;

const ROOT_HELP: &str = r#"
Codex CLI

Commands:
  exec        Run Codex non-interactively [aliases: e]
  mcp         Manage external MCP servers for Codex
  resume      Resume a previous interactive session

Options:
  -c, --config <key=value>
  -m, --model <MODEL>
  -s, --sandbox <SANDBOX_MODE>
      --search
      --add-dir <DIR>
"#;

const EXEC_HELP: &str = r#"
Run Codex non-interactively

Usage: codex exec [OPTIONS] [PROMPT] [COMMAND]

Options:
  -c, --config <key=value>
  -m, --model <MODEL>
  -s, --sandbox <SANDBOX_MODE>
      --add-dir <DIR>
      --json
  -C, --cd <DIR>
      --skip-git-repo-check
"#;

#[test]
fn parse_codex_version_extracts_semver() {
    assert_eq!(
        parse_codex_version("codex-cli 0.116.0"),
        Some("0.116.0".to_string())
    );
    assert_eq!(parse_codex_version("codex 0.116.0"), None);
}

#[test]
fn parse_codex_cli_capabilities_detects_core_exec_surface() {
    let capabilities = parse_codex_cli_capabilities(
        ROOT_HELP,
        EXEC_HELP,
        Some("codex-cli 0.116.0"),
        None,
        None,
        None,
    );

    assert_eq!(capabilities.version.as_deref(), Some("0.116.0"));
    assert!(capabilities.supports_exec_subcommand);
    assert!(capabilities.supports_json_output);
    assert!(capabilities.supports_model_flag);
    assert!(capabilities.supports_config_override);
    assert!(capabilities.supports_sandbox_flag);
    assert!(capabilities.supports_add_dir);
    assert!(capabilities.supports_search_flag);
    assert!(capabilities.supports_resume_subcommand);
    assert!(capabilities.supports_mcp_subcommand);
    assert!(capabilities.has_core_exec_support());
}

#[test]
fn parse_codex_cli_capabilities_reports_missing_json_feature() {
    let capabilities = parse_codex_cli_capabilities(
        ROOT_HELP,
        &EXEC_HELP.replace("--json", ""),
        None,
        None,
        None,
        None,
    );

    assert!(!capabilities.supports_json_output);
    assert_eq!(
        capabilities.missing_core_exec_features(),
        vec!["json_output"]
    );
}

#[test]
fn build_codex_exec_args_maps_lane_settings_to_flags_and_overrides() {
    let capabilities = parse_codex_cli_capabilities(
        ROOT_HELP,
        EXEC_HELP,
        Some("codex-cli 0.116.0"),
        None,
        None,
        None,
    );
    let config = CodexExecCliConfig {
        model: Some("gpt-5.4".to_string()),
        reasoning_effort: Some(LogicalEffort::XHigh),
        approval_policy: Some("on_request".to_string()),
        sandbox_mode: Some("workspace_write".to_string()),
        service_tier: None,
        config_overrides: vec!["mcp_servers.ralphx.enabled=true".to_string()],
        cwd: Some(PathBuf::from("/tmp/work")),
        add_dirs: vec![PathBuf::from("/tmp/extra")],
        skip_git_repo_check: true,
        json_output: true,
        search: true,
        ultra_mode: false,
    };

    let args = build_codex_exec_args(&capabilities, &config).expect("args should build");

    assert_eq!(
        args,
        vec![
            "exec",
            "--json",
            "-m",
            "gpt-5.4",
            "-s",
            "danger-full-access",
            "-C",
            "/tmp/work",
            "--add-dir",
            "/tmp/extra",
            "--skip-git-repo-check",
            "--search",
            "-c",
            "mcp_servers.ralphx.enabled=true",
            "-c",
            "model_reasoning_effort=\"xhigh\"",
            "-c",
            "model_reasoning_summary=\"concise\"",
            "-c",
            "approval_policy=\"never\"",
        ]
    );
}

#[test]
fn build_codex_exec_args_rejects_missing_config_override_support() {
    let capabilities = parse_codex_cli_capabilities(
        &ROOT_HELP.replace("--config", ""),
        &EXEC_HELP.replace("--config", ""),
        Some("codex-cli 0.116.0"),
        None,
        None,
        None,
    );
    let config = CodexExecCliConfig {
        reasoning_effort: Some(LogicalEffort::Medium),
        ..Default::default()
    };

    let error = build_codex_exec_args(&capabilities, &config)
        .expect_err("missing config override support must fail");

    assert!(error.contains("config_override"));
}

#[test]
fn build_codex_exec_resume_args_maps_config_to_resume_surface() {
    let capabilities = parse_codex_cli_capabilities(
        ROOT_HELP,
        EXEC_HELP,
        Some("codex-cli 0.116.0"),
        None,
        None,
        None,
    );
    let config = CodexExecCliConfig {
        model: Some("gpt-5.4".to_string()),
        reasoning_effort: Some(LogicalEffort::XHigh),
        approval_policy: Some("on_request".to_string()),
        sandbox_mode: Some("workspace_write".to_string()),
        config_overrides: vec!["mcp_servers.ralphx.enabled=true".to_string()],
        cwd: Some(PathBuf::from("/tmp/work")),
        skip_git_repo_check: true,
        json_output: true,
        ..Default::default()
    };

    let args = build_codex_exec_resume_args(&capabilities, "session-123", &config)
        .expect("resume args should build");

    assert_eq!(
        args,
        vec![
            "exec",
            "resume",
            "session-123",
            "--json",
            "-m",
            "gpt-5.4",
            "--skip-git-repo-check",
            "-c",
            "mcp_servers.ralphx.enabled=true",
            "-c",
            "model_reasoning_effort=\"xhigh\"",
            "-c",
            "model_reasoning_summary=\"concise\"",
            "-c",
            "approval_policy=\"never\"",
            "-c",
            "sandbox_mode=\"danger-full-access\"",
        ]
    );
}

#[test]
fn build_spawnable_codex_exec_command_uses_prompt_arg_transport() {
    let capabilities = parse_codex_cli_capabilities(
        ROOT_HELP,
        EXEC_HELP,
        Some("codex-cli 0.116.0"),
        None,
        None,
        None,
    );
    let temp_dir = tempfile::tempdir().expect("temp dir");
    let config = CodexExecCliConfig {
        model: Some("gpt-5.4".to_string()),
        cwd: Some(temp_dir.path().to_path_buf()),
        ..Default::default()
    };

    let spawnable = build_spawnable_codex_exec_command(
        std::path::Path::new("/opt/homebrew/bin/codex"),
        "Plan the refactor",
        &capabilities,
        &config,
    )
    .expect("spawnable codex command should build");

    let cwd = temp_dir.path().to_string_lossy().into_owned();
    assert_eq!(
        spawnable.get_args_for_test(),
        vec![
            "exec".to_string(),
            "--json".to_string(),
            "-m".to_string(),
            "gpt-5.4".to_string(),
            "-s".to_string(),
            "danger-full-access".to_string(),
            "-C".to_string(),
            cwd,
            "-c".to_string(),
            "model_reasoning_summary=\"concise\"".to_string(),
            "-c".to_string(),
            "approval_policy=\"never\"".to_string(),
            "--".to_string(),
            "Plan the refactor".to_string(),
        ]
    );
}

#[test]
fn build_spawnable_codex_resume_command_uses_resume_subcommand_and_prompt_arg() {
    let capabilities = parse_codex_cli_capabilities(
        ROOT_HELP,
        EXEC_HELP,
        Some("codex-cli 0.116.0"),
        None,
        None,
        None,
    );
    let temp_dir = tempfile::tempdir().expect("temp dir");
    let config = CodexExecCliConfig {
        cwd: Some(temp_dir.path().to_path_buf()),
        json_output: true,
        ..Default::default()
    };

    let spawnable = build_spawnable_codex_resume_command(
        std::path::Path::new("/opt/homebrew/bin/codex"),
        "session-123",
        "Continue the plan",
        &capabilities,
        &config,
    )
    .expect("spawnable codex resume command should build");

    assert_eq!(
        spawnable.get_args_for_test(),
        vec![
            "exec".to_string(),
            "resume".to_string(),
            "session-123".to_string(),
            "--json".to_string(),
            "-c".to_string(),
            "model_reasoning_summary=\"concise\"".to_string(),
            "-c".to_string(),
            "approval_policy=\"never\"".to_string(),
            "-c".to_string(),
            "sandbox_mode=\"danger-full-access\"".to_string(),
            "--".to_string(),
            "Continue the plan".to_string(),
        ]
    );

    let debug = format!("{spawnable:?}");
    assert!(
        debug.contains(
            runtime_log_paths::codex_prompt_debug_dir()
                .to_string_lossy()
                .as_ref()
        ),
        "resume debug output should still point at the prompt artifact path: {debug}"
    );
}

#[test]
fn build_spawnable_codex_exec_command_logs_prompt_to_artifact_and_redacts_debug_output() {
    let capabilities = parse_codex_cli_capabilities(
        ROOT_HELP,
        EXEC_HELP,
        Some("codex-cli 0.116.0"),
        None,
        None,
        None,
    );
    let temp_dir = tempfile::tempdir().expect("temp dir");
    let config = CodexExecCliConfig {
        cwd: Some(temp_dir.path().to_path_buf()),
        ..Default::default()
    };
    let prompt = format!(
        "Plan the refactor {}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("system clock")
            .as_nanos()
    );

    let spawnable = build_spawnable_codex_exec_command(
        std::path::Path::new("/opt/homebrew/bin/codex"),
        &prompt,
        &capabilities,
        &config,
    )
    .expect("spawnable codex command should build");

    let debug = format!("{spawnable:?}");
    assert!(
        !debug.contains(&prompt),
        "debug output should redact the full prompt: {debug}"
    );
    let artifact_prefix = runtime_log_paths::codex_prompt_debug_dir()
        .to_string_lossy()
        .into_owned();
    assert!(
        debug.contains(&artifact_prefix),
        "debug output should point at the prompt artifact path: {debug}"
    );

    assert!(
        !temp_dir.path().join(".artifacts").exists(),
        "Codex prompt debug artifacts must not contaminate the target cwd"
    );

    let prompt_dir = runtime_log_paths::codex_prompt_debug_dir();
    let mut entries = std::fs::read_dir(&prompt_dir)
        .expect("prompt dir should exist")
        .collect::<Result<Vec<_>, _>>()
        .expect("prompt entries should be readable");
    let prompt_path = entries
        .drain(..)
        .map(|entry| entry.path())
        .find(|path| {
            std::fs::read_to_string(path)
                .map(|contents| contents == prompt)
                .unwrap_or(false)
        })
        .expect("expected logged prompt file under the app log root");
    assert_eq!(
        std::fs::read_to_string(&prompt_path).expect("read prompt artifact"),
        prompt
    );
}

#[test]
fn compose_codex_prompt_injects_agent_prompt_body_when_available() {
    let temp_dir = tempfile::tempdir().expect("temp dir");
    let plugin_dir = temp_dir.path().join("plugins/app");
    let agent_dir = temp_dir.path().join("agents/ralphx-execution-worker/codex");
    std::fs::create_dir_all(&agent_dir).expect("canonical agent dir");
    std::fs::write(
        temp_dir
            .path()
            .join("agents/ralphx-execution-worker/agent.yaml"),
        "name: ralphx-execution-worker\nrole: worker\n",
    )
    .expect("agent metadata");
    std::fs::write(agent_dir.join("prompt.md"), "You are the worker agent.\n").expect("agent file");

    let prompt = compose_codex_prompt(
        "Execute task task-123",
        Some(plugin_dir.as_path()),
        Some("ralphx:ralphx-execution-worker"),
    );

    assert!(prompt.contains("<ralphx_agent_instructions>"));
    assert!(prompt.contains("You are the worker agent."));
    assert!(prompt.contains("Execute task task-123"));
}

#[test]
fn compose_codex_prompt_returns_original_prompt_when_agent_prompt_missing() {
    let prompt = compose_codex_prompt("Execute task task-123", None, Some("ralphx:worker"));
    assert_eq!(prompt, "Execute task task-123");
}

#[test]
fn normalize_codex_exec_output_prefers_completed_agent_messages() {
    let stdout = concat!(
        "{\"type\":\"thread.started\",\"thread_id\":\"thread-1\"}\n",
        "{\"type\":\"item.completed\",\"item\":{\"type\":\"agent_message\",\"text\":\"First answer\"}}\n",
        "{\"type\":\"item.completed\",\"item\":{\"type\":\"agent_message\",\"text\":\"Second answer\"}}\n",
    );

    assert_eq!(
        normalize_codex_exec_output(stdout),
        "First answer\n\nSecond answer"
    );
}

#[test]
fn normalize_codex_exec_output_falls_back_to_structured_errors() {
    let stdout = concat!(
        "{\"type\":\"item.completed\",\"item\":{\"type\":\"command_execution\",\"exit_code\":1,\"aggregated_output\":\"npm test failed\"}}\n",
        "{\"type\":\"item.completed\",\"item\":{\"type\":\"error\",\"error\":{\"message\":\"agent crashed\"}}}\n",
    );

    assert_eq!(
        normalize_codex_exec_output(stdout),
        "npm test failed\n\nagent crashed"
    );
}

#[test]
fn normalize_codex_exec_output_keeps_raw_stdout_when_not_jsonl() {
    let stdout = "plain stdout from a non-json fallback";
    assert_eq!(normalize_codex_exec_output(stdout), stdout);
}

#[test]
fn build_codex_cli_diagnostics_response_handles_missing_binary() {
    let diagnostics = build_codex_cli_diagnostics_response(
        CodexCliProbeStatus {
            binary_path: None,
            binary_found: false,
            probe_succeeded: false,
            available: false,
            missing_core_exec_features: Vec::new(),
            error: Some("Codex CLI not found".to_string()),
        },
        None,
    );

    assert!(!diagnostics.binary_found);
    assert!(!diagnostics.probe_succeeded);
    assert_eq!(diagnostics.error.as_deref(), Some("Codex CLI not found"));
}

#[test]
fn build_codex_cli_diagnostics_response_handles_successful_probe() {
    let capabilities = CodexCliCapabilities {
        version: Some("0.116.0".to_string()),
        supports_exec_subcommand: true,
        supports_json_output: true,
        supports_model_flag: true,
        supports_config_override: true,
        supports_sandbox_flag: true,
        supports_add_dir: true,
        supports_search_flag: true,
        supports_resume_subcommand: true,
        supports_mcp_subcommand: true,
        supports_fast_mode_feature: false,
        fast_mode_supported_models: Vec::new(),
        ultra_supported_models: Vec::new(),
        supported_model_aliases: vec!["gpt-5.5".to_string()],
        supported_efforts: vec![
            "low".to_string(),
            "medium".to_string(),
            "high".to_string(),
            "xhigh".to_string(),
        ],
        model_supported_efforts: std::collections::BTreeMap::new(),
    };

    let diagnostics = build_codex_cli_diagnostics_response(
        CodexCliProbeStatus {
            binary_path: Some("/opt/homebrew/bin/codex".to_string()),
            binary_found: true,
            probe_succeeded: true,
            available: true,
            missing_core_exec_features: Vec::new(),
            error: None,
        },
        Some(capabilities),
    );

    assert!(diagnostics.binary_found);
    assert!(diagnostics.probe_succeeded);
    assert!(diagnostics.has_core_exec_support);
    assert_eq!(diagnostics.version.as_deref(), Some("0.116.0"));
    assert_eq!(
        diagnostics.binary_path.as_deref(),
        Some("/opt/homebrew/bin/codex")
    );
}

#[test]
fn build_codex_cli_diagnostics_response_handles_probe_errors() {
    let diagnostics = build_codex_cli_diagnostics_response(
        CodexCliProbeStatus {
            binary_path: Some("/opt/homebrew/bin/codex".to_string()),
            binary_found: true,
            probe_succeeded: false,
            available: false,
            missing_core_exec_features: Vec::new(),
            error: Some("help probe failed".to_string()),
        },
        None,
    );

    assert!(diagnostics.binary_found);
    assert!(!diagnostics.probe_succeeded);
    assert_eq!(diagnostics.error.as_deref(), Some("help probe failed"));
}
