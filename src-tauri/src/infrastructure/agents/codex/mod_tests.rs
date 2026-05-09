use super::{
    build_codex_exec_args, build_codex_exec_resume_args, build_codex_mcp_overrides,
    build_spawnable_codex_exec_command, compose_codex_prompt, configure_spawn, probe_codex_cli,
    resolve_codex_cli_from_candidates, CodexCliCapabilities, CodexExecCliConfig,
    CodexMcpRuntimeContext,
};
use crate::domain::agents::LogicalEffort;
use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};

struct EnvGuard {
    key: &'static str,
    original: Option<OsString>,
}

impl EnvGuard {
    fn set_os(key: &'static str, value: impl AsRef<OsStr>) -> Self {
        let original = std::env::var_os(key);
        std::env::set_var(key, value);
        Self { key, original }
    }

    fn unset(key: &'static str) -> Self {
        let original = std::env::var_os(key);
        std::env::remove_var(key);
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

fn write_executable(path: &Path, contents: &str) {
    std::fs::write(path, contents).expect("write executable");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut permissions = std::fs::metadata(path)
            .expect("executable metadata")
            .permissions();
        permissions.set_mode(0o755);
        std::fs::set_permissions(path, permissions).expect("mark executable");
    }
}

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

fn project_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .canonicalize()
        .expect("canonical repo root")
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
fn probe_codex_cli_prepends_resolved_node_for_env_shim() {
    let _lock = crate::infrastructure::tool_paths::TEST_ENV_MUTEX
        .lock()
        .expect("env mutex");
    let temp_dir = tempfile::tempdir().expect("temp dir");
    let empty_path = temp_dir.path().join("empty-path");
    std::fs::create_dir_all(&empty_path).expect("create empty path");
    let nvm_bin = temp_dir
        .path()
        .join(".nvm")
        .join("versions")
        .join("node")
        .join("v22.16.0")
        .join("bin");
    std::fs::create_dir_all(&nvm_bin).expect("create nvm bin");
    let node_path = nvm_bin.join("node");
    let codex_path = nvm_bin.join("codex");
    write_executable(
        &node_path,
        r#"#!/bin/sh
shift
if [ "$1" = "--version" ]; then
  printf 'codex-cli 0.124.0\n'
elif [ "$1" = "--help" ]; then
  printf '%s\n' 'Codex CLI' 'Commands:' '  exec' '  resume' '  mcp' 'Options:' '  -c, --config <key=value>' '  -m, --model <MODEL>' '  -s, --sandbox <SANDBOX>' '      --search' '      --add-dir <DIR>'
elif [ "$1" = "exec" ] && [ "$2" = "--help" ]; then
  printf '%s\n' 'Run Codex non-interactively' 'Options:' '  -c, --config <key=value>' '  -m, --model <MODEL>' '  -s, --sandbox <SANDBOX>' '      --add-dir <DIR>' '      --json'
else
  printf 'unexpected args: %s\n' "$*" >&2
  exit 64
fi
"#,
    );
    write_executable(&codex_path, "#!/usr/bin/env node\n");

    let _home = EnvGuard::set_os("HOME", temp_dir.path());
    let _path = EnvGuard::set_os("PATH", &empty_path);
    let _nvm_bin = EnvGuard::unset("NVM_BIN");
    let _volta_home = EnvGuard::unset("VOLTA_HOME");
    let _node_override = EnvGuard::unset("RALPHX_NODE_PATH");

    let capabilities =
        probe_codex_cli(&codex_path).expect("Codex probe should run npm shim with resolved node");

    assert_eq!(capabilities.version.as_deref(), Some("0.124.0"));
    assert!(capabilities.supports_exec_subcommand);
    assert!(capabilities.supports_json_output);
    assert!(capabilities.supports_model_flag);
    assert!(capabilities.supports_config_override);
    assert!(capabilities.supports_sandbox_flag);
    assert!(capabilities.supports_add_dir);
    assert!(capabilities.supports_search_flag);
    assert!(capabilities.supports_resume_subcommand);
    assert!(capabilities.supports_mcp_subcommand);
}

#[test]
fn probe_codex_cli_reports_legacy_cli_without_exec_as_incompatible() {
    let temp_dir = tempfile::tempdir().expect("temp dir");
    let codex_path = temp_dir.path().join("codex");
    write_executable(
        &codex_path,
        r#"#!/bin/sh
if [ "$1" = "--version" ]; then
  printf '0.1.2505172129\n'
elif [ "$1" = "--help" ]; then
  printf '%s\n' 'Usage' '  $ codex [options] <prompt>' '  $ codex completion <bash|zsh|fish>' 'Options:' '  --version'
elif [ "$1" = "exec" ] && [ "$2" = "--help" ]; then
  printf '%s\n' 'Usage' '  $ codex [options] <prompt>' 'Options:' '  --version'
  exit 2
else
  printf 'unexpected args: %s\n' "$*" >&2
  exit 64
fi
"#,
    );

    let capabilities =
        probe_codex_cli(&codex_path).expect("legacy Codex should probe as incompatible");

    assert!(!capabilities.supports_exec_subcommand);
    assert!(!capabilities.has_core_exec_support());
    assert_eq!(
        capabilities.missing_core_exec_features(),
        vec![
            "exec_subcommand",
            "json_output",
            "model_flag",
            "config_override",
            "sandbox_flag",
            "add_dir",
        ]
    );
}

#[test]
fn resolve_codex_cli_skips_legacy_candidate_without_exec_support() {
    let temp_dir = tempfile::tempdir().expect("temp dir");
    let legacy_path = temp_dir.path().join("legacy").join("codex");
    let modern_path = temp_dir.path().join("modern").join("codex");
    std::fs::create_dir_all(legacy_path.parent().expect("legacy parent")).expect("legacy dir");
    std::fs::create_dir_all(modern_path.parent().expect("modern parent")).expect("modern dir");
    write_executable(
        &legacy_path,
        r#"#!/bin/sh
if [ "$1" = "--version" ]; then
  printf '0.1.2505172129\n'
elif [ "$1" = "--help" ]; then
  printf '%s\n' 'Usage' '  $ codex [options] <prompt>' 'Options:' '  --version'
elif [ "$1" = "exec" ] && [ "$2" = "--help" ]; then
  printf '%s\n' 'Usage' '  $ codex [options] <prompt>'
  exit 2
else
  exit 64
fi
"#,
    );
    write_executable(
        &modern_path,
        r#"#!/bin/sh
if [ "$1" = "--version" ]; then
  printf 'codex-cli 0.124.0\n'
elif [ "$1" = "--help" ]; then
  printf '%s\n' 'Codex CLI' 'Commands:' '  exec' '  resume' '  mcp' 'Options:' '  -c, --config <key=value>' '  -m, --model <MODEL>' '  -s, --sandbox <SANDBOX>' '      --search' '      --add-dir <DIR>'
elif [ "$1" = "exec" ] && [ "$2" = "--help" ]; then
  printf '%s\n' 'Run Codex non-interactively' 'Options:' '  -c, --config <key=value>' '  -m, --model <MODEL>' '  -s, --sandbox <SANDBOX>' '      --add-dir <DIR>' '      --json'
else
  exit 64
fi
"#,
    );

    let resolved = resolve_codex_cli_from_candidates(vec![legacy_path, modern_path.clone()])
        .expect("resolver should select the compatible candidate");

    assert_eq!(resolved.path, modern_path);
    assert!(resolved.capabilities.has_core_exec_support());
}

#[test]
fn resolve_codex_cli_returns_first_incompatible_candidate_when_none_support_exec() {
    let temp_dir = tempfile::tempdir().expect("temp dir");
    let legacy_path = temp_dir.path().join("codex");
    write_executable(
        &legacy_path,
        r#"#!/bin/sh
if [ "$1" = "--version" ]; then
  printf '0.1.2505172129\n'
elif [ "$1" = "--help" ]; then
  printf '%s\n' 'Usage' '  $ codex [options] <prompt>' 'Options:' '  --version'
elif [ "$1" = "exec" ] && [ "$2" = "--help" ]; then
  exit 2
else
  exit 64
fi
"#,
    );

    let resolved = resolve_codex_cli_from_candidates(vec![legacy_path.clone()])
        .expect("incompatible candidate should still resolve for availability reporting");

    assert_eq!(resolved.path, legacy_path);
    assert!(!resolved.capabilities.has_core_exec_support());
    assert!(resolved
        .capabilities
        .missing_core_exec_features()
        .contains(&"exec_subcommand"));
}

#[test]
fn resolve_codex_cli_reports_probe_errors_when_candidates_cannot_be_probed() {
    let temp_dir = tempfile::tempdir().expect("temp dir");
    let broken_path = temp_dir.path().join("codex");
    write_executable(
        &broken_path,
        r#"#!/bin/sh
if [ "$1" = "--version" ]; then
  printf 'broken codex\n' >&2
  exit 70
fi
exit 64
"#,
    );

    let error = resolve_codex_cli_from_candidates(vec![broken_path.clone()])
        .expect_err("broken candidate should fail probing");

    assert!(error.contains("No launchable Codex CLI could be probed"));
    assert!(error.contains(&broken_path.to_string_lossy().to_string()));
}

#[test]
fn resolve_codex_cli_reports_not_found_when_candidate_list_is_empty() {
    let error = resolve_codex_cli_from_candidates(Vec::new())
        .expect_err("empty candidate list should be not found");

    assert_eq!(error, "Codex CLI not found");
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
fn build_codex_exec_args_defaults_to_mcp_safe_approval_and_sandbox() {
    let args = build_codex_exec_args(
        &full_codex_capabilities(),
        &CodexExecCliConfig::default(),
    )
    .expect("build codex exec args");

    assert!(args
        .windows(2)
        .any(|pair| pair[0] == "-s" && pair[1] == "danger-full-access"));
    assert!(args
        .windows(2)
        .any(|pair| pair[0] == "-c" && pair[1] == "approval_policy=\"never\""));
}

#[test]
fn build_codex_exec_resume_args_defaults_to_mcp_safe_approval_and_sandbox() {
    let args = build_codex_exec_resume_args(
        &full_codex_capabilities(),
        "session-123",
        &CodexExecCliConfig::default(),
    )
    .expect("build codex resume args");

    assert!(args
        .windows(2)
        .any(|pair| pair[0] == "-c" && pair[1] == "approval_policy=\"never\""));
    assert!(args
        .windows(2)
        .any(|pair| pair[0] == "-c" && pair[1] == "sandbox_mode=\"danger-full-access\""));
}

#[test]
fn build_codex_exec_args_enforces_mcp_safe_approval_and_sandbox_overrides() {
    let args = build_codex_exec_args(
        &full_codex_capabilities(),
        &CodexExecCliConfig {
            approval_policy: Some("on-request".to_string()),
            sandbox_mode: Some("workspace-write".to_string()),
            ..CodexExecCliConfig::default()
        },
    )
    .expect("build codex exec args");

    assert!(args
        .windows(2)
        .any(|pair| pair[0] == "-s" && pair[1] == "danger-full-access"));
    assert!(args
        .windows(2)
        .any(|pair| pair[0] == "-c" && pair[1] == "approval_policy=\"never\""));
    assert!(!args
        .windows(2)
        .any(|pair| pair[0] == "-s" && pair[1] == "workspace-write"));
    assert!(!args
        .windows(2)
        .any(|pair| pair[0] == "-c" && pair[1] == "approval_policy=\"on-request\""));
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
fn build_codex_mcp_overrides_pr_describer_enables_submit_tool_without_shell() {
    let root = project_root();
    let plugin_dir = root.join("plugins").join("app");

    let overrides = build_codex_mcp_overrides(
        &plugin_dir,
        "ralphx:ralphx-utility-pr-describer",
        false,
        None,
    )
    .expect("PR describer Codex MCP overrides");

    assert!(
        overrides
            .iter()
            .any(|entry| entry == "features.shell_tool=false"),
        "PR describer should disable Codex shell tool: {overrides:?}"
    );
    assert!(
        overrides.iter().any(|entry| entry
            == "mcp_servers.ralphx.enabled_tools=[\"submit_agent_workspace_pr_description\"]"),
        "PR describer enabled tools should be limited to its submit tool: {overrides:?}"
    );
    assert!(
        overrides
            .iter()
            .any(|entry| entry.starts_with("mcp_servers.ralphx.args=")
                && entry.contains("--allowed-tools=submit_agent_workspace_pr_description")),
        "PR describer stdio MCP args should pass the submit-tool allowlist: {overrides:?}"
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
