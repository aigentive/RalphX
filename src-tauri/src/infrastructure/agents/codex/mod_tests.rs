use super::{
    build_codex_exec_args, build_codex_exec_resume_args, build_codex_mcp_overrides,
    build_codex_mcp_overrides_for_profile, build_spawnable_codex_exec_command,
    compose_codex_prompt, compose_codex_prompt_for_profile,
    compose_codex_prompt_for_profile_with_learned_skills_and_outcome,
    compose_codex_prompt_for_profile_with_outcome,
    compose_codex_prompt_for_profile_with_runtime_context, configure_spawn,
    parse_codex_fast_mode_feature, parse_codex_fast_mode_supported_models,
    parse_codex_model_catalog_capabilities, probe_codex_cli, redact_persona_from_codex_prompt,
    resolve_codex_cli_from_candidates, CodexCliCapabilities, CodexExecCliConfig,
    CodexMcpRuntimeContext, CodexPromptTransport,
};

use crate::domain::agents::LogicalEffort;
use crate::domain::services::learned_skill_adapters::{
    LearnedSkillBucket, LearnedSkillMultiSelectionRequest, LearnedSkillRecord, LearnedSkillStage,
    LearnedSkillStatus,
};
use crate::infrastructure::agents::claude::{SpawnableCommand, SpawnableStdinTransport};
use crate::infrastructure::agents::internal_skills::PreExecutionLearnedSkillContext;
use std::collections::BTreeMap;
use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};

#[test]
fn persona_codex_prompt_outcome_is_reasoned_when_agent_prompt_is_unavailable() {
    let composition = compose_codex_prompt_for_profile_with_outcome(
        "User prompt",
        None,
        Some("ralphx-chat-project"),
        None,
        Some("<ralphx_agent_persona>secret</ralphx_agent_persona>"),
    );

    assert_eq!(composition.prompt, "User prompt");
    assert!(!composition.persona_injected);
    assert_eq!(
        composition.persona_injection_skipped_reason,
        Some("codex_plugin_dir_unavailable")
    );
}

#[test]
fn persona_codex_debug_prompt_redaction_removes_body() {
    let redacted = redact_persona_from_codex_prompt(
        "before<ralphx_agent_persona>SECRET_PERSONA_BODY</ralphx_agent_persona>after",
    );

    assert_eq!(
        redacted,
        "before<ralphx_agent_persona>[redacted]</ralphx_agent_persona>after"
    );
    assert!(!redacted.contains("SECRET_PERSONA_BODY"));
}

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

fn path_index(entries: &[PathBuf], path: impl AsRef<Path>) -> usize {
    entries
        .iter()
        .position(|entry| entry == path.as_ref())
        .unwrap_or_else(|| panic!("PATH entry missing: {}", path.as_ref().display()))
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
        supports_fast_mode_feature: true,
        fast_mode_supported_models: vec!["gpt-5.4".to_string(), "gpt-5.5".to_string()],
        supported_model_aliases: vec!["gpt-5.5".to_string()],
        supported_efforts: vec![
            "low".to_string(),
            "medium".to_string(),
            "high".to_string(),
            "xhigh".to_string(),
        ],
        model_supported_efforts: BTreeMap::from([(
            "gpt-5.5".to_string(),
            vec![
                "low".to_string(),
                "medium".to_string(),
                "high".to_string(),
                "xhigh".to_string(),
            ],
        )]),
        ultra_supported_models: Vec::new(),
    }
}

#[test]
fn parse_codex_fast_mode_feature_detects_enabled_feature() {
    assert!(parse_codex_fast_mode_feature(
        "name stage enabled\nfast_mode stable true\n"
    ));
    assert!(!parse_codex_fast_mode_feature(
        "name stage enabled\nfast_mode stable false\n"
    ));
}

#[test]
fn parse_codex_fast_mode_supported_models_reads_speed_tier_catalog() {
    let models = parse_codex_fast_mode_supported_models(
        r#"{
          "models": [
            {
              "slug": "gpt-5.5",
              "additional_speed_tiers": ["fast"],
              "service_tiers": [{"id": "priority", "name": "Fast"}]
            },
            {
              "slug": "gpt-5.4",
              "service_tiers": [{"id": "priority"}]
            },
            {
              "slug": "gpt-5.4-mini",
              "additional_speed_tiers": []
            }
          ]
        }"#,
    );

    assert_eq!(models, vec!["gpt-5.4".to_string(), "gpt-5.5".to_string()]);
}

#[test]
fn parse_codex_model_catalog_capabilities_reads_visible_aliases_and_efforts() {
    let catalog = parse_codex_model_catalog_capabilities(
        r#"{
          "models": [
            {
              "slug": "gpt-5.6-sol",
              "visibility": "list",
              "supported_reasoning_levels": [
                {"effort": "low"},
                {"effort": "medium"},
                {"effort": "high"},
                {"effort": "xhigh"},
                {"effort": "max"},
                {"effort": "ultra"},
                {"effort": "warp"}
              ]
            },
            {
              "slug": "gpt-5.6-luna",
              "visibility": "list",
              "supported_reasoning_levels": [
                {"effort": "medium"},
                {"effort": "low"},
                {"effort": "max"}
              ]
            },
            {
              "slug": "hidden-model",
              "visibility": "hidden",
              "supported_reasoning_levels": [{"effort": "ultra"}]
            }
          ]
        }"#,
    );

    assert_eq!(
        catalog.supported_model_aliases,
        vec!["gpt-5.6-luna".to_string(), "gpt-5.6-sol".to_string()]
    );
    assert_eq!(
        catalog.supported_efforts,
        vec![
            "low".to_string(),
            "medium".to_string(),
            "high".to_string(),
            "xhigh".to_string(),
            "max".to_string(),
        ]
    );
    assert_eq!(
        catalog.model_supported_efforts.get("gpt-5.6-sol"),
        Some(&vec![
            "low".to_string(),
            "medium".to_string(),
            "high".to_string(),
            "xhigh".to_string(),
            "max".to_string(),
        ])
    );
    assert_eq!(
        catalog.model_supported_efforts.get("gpt-5.6-luna"),
        Some(&vec![
            "low".to_string(),
            "medium".to_string(),
            "max".to_string(),
        ])
    );
    assert!(!catalog.model_supported_efforts.contains_key("hidden-model"));
    assert_eq!(
        catalog.ultra_supported_models,
        vec!["gpt-5.6-sol".to_string()]
    );
}

fn create_plugin_dir(root: &std::path::Path) -> PathBuf {
    let plugin_dir = root.join("plugins/app");
    std::fs::create_dir_all(plugin_dir.join("agents")).expect("create plugin agents dir");
    plugin_dir
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

fn codex_mcp_args_override(overrides: &[String]) -> &str {
    overrides
        .iter()
        .find_map(|entry| entry.strip_prefix("mcp_servers.ralphx.args="))
        .expect("Codex MCP args override")
}

fn override_keys(overrides: &[String]) -> Vec<&str> {
    overrides
        .iter()
        .map(|entry| entry.split_once('=').map_or(entry.as_str(), |(key, _)| key))
        .collect()
}

fn seed_live_agent_yaml(root: &Path, agent_name: &str) {
    let agent_dir = root.join("agents").join(agent_name);
    std::fs::create_dir_all(&agent_dir).expect("create agent fixture dir");
    std::fs::copy(
        project_root()
            .join("agents")
            .join(agent_name)
            .join("agent.yaml"),
        agent_dir.join("agent.yaml"),
    )
    .expect("copy live agent fixture");
}

fn project_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .canonicalize()
        .expect("canonical repo root")
}

#[test]
fn build_codex_exec_command_sets_agent_tool_path() {
    let _lock = crate::infrastructure::tool_paths::TEST_ENV_MUTEX
        .lock()
        .expect("env mutex");
    let seeded_path = dirs::home_dir()
        .map(|home| {
            std::env::join_paths([
                home.join(".cargo").join("bin"),
                PathBuf::from("/opt/homebrew/bin"),
                PathBuf::from("/usr/local/bin"),
                PathBuf::from("/usr/bin"),
                PathBuf::from("/bin"),
            ])
            .expect("seed test PATH")
        })
        .unwrap_or_else(|| OsString::from("/opt/homebrew/bin:/usr/local/bin:/usr/bin:/bin"));
    let _path = EnvGuard::set_os("PATH", seeded_path);
    let _disable_login_shell =
        EnvGuard::set_os(crate::infrastructure::login_shell_env::DISABLE_ENV_VAR, "1");

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
    if let Some(home) = dirs::home_dir() {
        let cargo_bin = home.join(".cargo").join("bin");
        let entries = std::env::split_paths(&path).collect::<Vec<_>>();
        assert!(
            path_index(&entries, &cargo_bin) < path_index(&entries, "/opt/homebrew/bin"),
            "user cargo shim should stay before Homebrew in Codex spawn PATH: {path}"
        );
    }
}

#[test]
fn probe_codex_cli_ensures_resolved_node_for_env_shim() {
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
elif [ "$1" = "features" ] && [ "$2" = "list" ]; then
  printf '%s\n' 'fast_mode stable true'
elif [ "$1" = "debug" ] && [ "$2" = "models" ] && [ -z "$3" ]; then
  printf '%s\n' '{"models":[{"slug":"gpt-5.6-sol","visibility":"list","supported_reasoning_levels":[{"effort":"low"},{"effort":"medium"},{"effort":"high"},{"effort":"xhigh"},{"effort":"max"},{"effort":"ultra"}]}]}'
elif [ "$1" = "debug" ] && [ "$2" = "models" ] && [ "$3" = "--bundled" ]; then
  printf '%s\n' '{"models":[{"slug":"gpt-5.5","visibility":"list","supported_reasoning_levels":[{"effort":"low"},{"effort":"medium"},{"effort":"high"},{"effort":"xhigh"}],"additional_speed_tiers":["fast"],"service_tiers":[{"id":"priority","name":"Fast"}]},{"slug":"gpt-5.4-mini","visibility":"list","supported_reasoning_levels":[{"effort":"low"},{"effort":"medium"},{"effort":"high"}],"additional_speed_tiers":[]}]}'
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
    assert!(capabilities.supports_fast_mode());
    assert_eq!(
        capabilities.fast_mode_supported_models(),
        vec!["gpt-5.5".to_string()]
    );
    assert_eq!(
        capabilities.supported_model_aliases,
        vec!["gpt-5.6-sol".to_string()]
    );
    assert_eq!(
        capabilities.supported_effort_labels(),
        vec![
            "low".to_string(),
            "medium".to_string(),
            "high".to_string(),
            "xhigh".to_string(),
            "max".to_string(),
        ]
    );
    assert!(capabilities.supports_ultra_for_model("gpt-5.6-sol"));
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
    assert!(!capabilities.supports_fast_mode());
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
    let args = build_codex_exec_args(&full_codex_capabilities(), &CodexExecCliConfig::default())
        .expect("build codex exec args");

    assert!(args
        .windows(2)
        .any(|pair| pair[0] == "-s" && pair[1] == "danger-full-access"));
    assert!(args
        .windows(2)
        .any(|pair| pair[0] == "-c" && pair[1] == "approval_policy=\"never\""));
}

#[test]
fn build_codex_exec_args_enables_fast_service_tier() {
    let args = build_codex_exec_args(
        &full_codex_capabilities(),
        &CodexExecCliConfig {
            service_tier: Some("fast".to_string()),
            ..CodexExecCliConfig::default()
        },
    )
    .expect("build codex exec args");

    assert!(args
        .windows(2)
        .any(|pair| pair[0] == "-c" && pair[1] == "service_tier=\"fast\""));
    assert!(args
        .windows(2)
        .any(|pair| pair[0] == "-c" && pair[1] == "features.fast_mode=true"));
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
fn build_codex_exec_resume_args_requires_resume_capability() {
    let mut capabilities = full_codex_capabilities();
    capabilities.supports_resume_subcommand = false;

    let error =
        build_codex_exec_resume_args(&capabilities, "session-123", &CodexExecCliConfig::default())
            .expect_err("old Codex CLIs without resume support must not build resume args");

    assert!(error.contains("resume subcommand"));
}

#[test]
fn build_codex_exec_resume_args_uses_resume_when_supported() {
    let args = build_codex_exec_resume_args(
        &full_codex_capabilities(),
        "session-123",
        &CodexExecCliConfig::default(),
    )
    .expect("build codex resume args");

    assert_eq!(&args[..3], ["exec", "resume", "session-123"]);
}

#[test]
fn build_codex_exec_resume_args_enables_fast_service_tier() {
    let args = build_codex_exec_resume_args(
        &full_codex_capabilities(),
        "session-123",
        &CodexExecCliConfig {
            service_tier: Some("fast".to_string()),
            ..CodexExecCliConfig::default()
        },
    )
    .expect("build codex resume args");

    assert!(args
        .windows(2)
        .any(|pair| pair[0] == "-c" && pair[1] == "service_tier=\"fast\""));
    assert!(args
        .windows(2)
        .any(|pair| pair[0] == "-c" && pair[1] == "features.fast_mode=true"));
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
        (LogicalEffort::Max, "max"),
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
fn build_codex_exec_args_only_emits_ultra_for_the_ultra_capability() {
    let legacy_ultra_args = build_codex_exec_args(
        &full_codex_capabilities(),
        &CodexExecCliConfig {
            reasoning_effort: Some(LogicalEffort::Ultra),
            ..CodexExecCliConfig::default()
        },
    )
    .expect("legacy Ultra effort should normalize");
    assert!(legacy_ultra_args
        .windows(2)
        .any(|pair| { pair[0] == "-c" && pair[1] == "model_reasoning_effort=\"max\"" }));

    let capability_args = build_codex_exec_args(
        &full_codex_capabilities(),
        &CodexExecCliConfig {
            reasoning_effort: Some(LogicalEffort::Max),
            ultra_mode: true,
            ..CodexExecCliConfig::default()
        },
    )
    .expect("Ultra capability should build");
    assert!(capability_args
        .windows(2)
        .any(|pair| { pair[0] == "-c" && pair[1] == "model_reasoning_effort=\"ultra\"" }));
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
fn compose_codex_prompt_injects_pre_execution_learned_skill_citations() {
    let temp_dir = tempfile::tempdir().expect("temp dir");
    let root = temp_dir.path();
    let plugin_dir = create_plugin_dir(root);

    std::fs::create_dir_all(root.join("agents/ralphx-chat-project/shared"))
        .expect("create shared prompt dir");
    std::fs::write(
        root.join("agents/ralphx-chat-project/agent.yaml"),
        r#"name: ralphx-chat-project
role: project_chat
"#,
    )
    .expect("write shared definition");
    std::fs::write(
        root.join("agents/ralphx-chat-project/shared/prompt.md"),
        "Project chat prompt",
    )
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

    let composition = compose_codex_prompt_for_profile_with_learned_skills_and_outcome(
        "Plan the domain change.",
        Some(&plugin_dir),
        Some("ralphx-chat-project"),
        None,
        Some("<ralphx_agent_persona>Methodical planner</ralphx_agent_persona>"),
        Some(&context),
    );
    let composed = composition.prompt;

    assert!(composed.contains("Project chat prompt"));
    assert!(composed.contains("Methodical planner"));
    assert!(composition.persona_injected);
    assert!(composed.contains("<ralphx_learned_skill_citations>"));
    assert!(composed.contains("skill-planning"));
    assert!(!composed.contains("skill-review"));
    assert_eq!(composition.injected_skill_names, ["learned:skill-planning"]);
}

#[test]
fn c2_codex_fallback_has_no_stale_injected_names() {
    let composition = compose_codex_prompt_for_profile_with_learned_skills_and_outcome(
        "Plan the domain change.",
        None,
        Some("ralphx-chat-project"),
        None,
        None,
        None,
    );

    assert!(composition.injected_skill_names.is_empty());
}

#[test]
fn c1_direct_codex_runtime_context_composer_does_not_inject_project_skills() {
    let temp_dir = tempfile::tempdir().expect("temp dir");
    let root = temp_dir.path();
    let plugin_dir = create_plugin_dir(root);

    std::fs::create_dir_all(root.join("agents/ralphx-chat-project/shared"))
        .expect("create shared prompt dir");
    std::fs::write(
        root.join("agents/ralphx-chat-project/agent.yaml"),
        r#"name: ralphx-chat-project
role: project_chat
"#,
    )
    .expect("write shared definition");
    std::fs::write(
        root.join("agents/ralphx-chat-project/shared/prompt.md"),
        "Project chat prompt",
    )
    .expect("write shared prompt");

    let runtime_context = CodexMcpRuntimeContext {
        context_type: Some("project".to_string()),
        context_id: Some("project-1".to_string()),
        conversation_id: Some("conversation-1".to_string()),
        coordination_mode: None,
        task_id: None,
        project_id: Some("project-1".to_string()),
        working_directory: Some(root.join("workspace")),
        filesystem_read_roots: Vec::new(),
        enforce_filesystem_roots: false,
        lead_session_id: None,
        parent_conversation_id: Some("conversation-1".to_string()),
        agent_run_id: None,
        task_state: None,
        pipeline_role: None,
        skill_distillation_batch_id: None,
        skill_distillation_claim_token: None,
        skill_distillation_fingerprint: None,
        skill_distillation_outcome_ids: None,
    };

    let composed = compose_codex_prompt_for_profile_with_runtime_context(
        "Plan the domain change.",
        Some(&plugin_dir),
        Some("ralphx-chat-project"),
        None,
        Some(&runtime_context),
    );

    assert!(composed.contains("Project chat prompt"));
    assert!(!composed.contains("<ralphx_learned_skill_citations>"));
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
        "Codex runtime feature flags should flow into config overrides; override keys: {:?}",
        override_keys(&overrides)
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
        "PR describer should disable Codex shell tool; override keys: {:?}",
        override_keys(&overrides)
    );
    assert!(
        overrides.iter().any(|entry| entry
            == "mcp_servers.ralphx.enabled_tools=[\"submit_agent_workspace_pr_description\"]"),
        "PR describer enabled tools should be limited to its submit tool; override keys: {:?}",
        override_keys(&overrides)
    );
    assert!(
        overrides
            .iter()
            .any(|entry| entry.starts_with("mcp_servers.ralphx.args=")
                && entry.contains("--allowed-tools=submit_agent_workspace_pr_description")),
        "PR describer stdio MCP args should pass the submit-tool allowlist; override keys: {:?}",
        override_keys(&overrides)
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
        conversation_id: Some("conversation-current".to_string()),
        coordination_mode: Some("rx_native_workflow".to_string()),
        task_id: None,
        task_state: Some("re_executing".to_string()),
        project_id: Some("project-456".to_string()),
        working_directory: Some(root.join("workspace")),
        filesystem_read_roots: vec![root.join("project-root")],
        enforce_filesystem_roots: false,
        lead_session_id: Some("lead-789".to_string()),
        parent_conversation_id: Some("conversation-abc".to_string()),
        agent_run_id: Some("run-123".to_string()),
        pipeline_role: Some("memory_capture".to_string()),
        skill_distillation_batch_id: None,
        skill_distillation_claim_token: None,
        skill_distillation_fingerprint: None,
        skill_distillation_outcome_ids: None,
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
        args_override.contains("--trace-dir"),
        "expected trace-dir CLI arg in overrides: {args_override}"
    );
    assert!(
        args_override.contains("mcp-proxy"),
        "expected app-owned MCP proxy trace dir in overrides: {args_override}"
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
        args_override.contains("--conversation-id"),
        "expected conversation-id CLI arg in overrides: {args_override}"
    );
    assert!(
        args_override.contains("conversation-current"),
        "expected conversation-id value in overrides: {args_override}"
    );
    assert!(
        args_override.contains("--project-id"),
        "expected project-id CLI arg in overrides: {args_override}"
    );
    assert!(
        args_override.contains("--task-state"),
        "expected task-state CLI arg in overrides: {args_override}"
    );
    assert!(
        args_override.contains("re_executing"),
        "expected task-state value in overrides: {args_override}"
    );
    assert!(
        args_override.contains("--filesystem-read-root"),
        "expected filesystem read-root CLI arg in overrides: {args_override}"
    );
    assert!(
        args_override.contains("project-root"),
        "expected filesystem read-root value in overrides: {args_override}"
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
    assert!(
        args_override.contains("--parent-conversation-id"),
        "expected parent-conversation-id CLI arg in overrides: {args_override}"
    );
    assert!(
        args_override.contains("conversation-abc"),
        "expected parent conversation id value in overrides: {args_override}"
    );
    assert!(
        args_override.contains("--agent-run-id"),
        "expected agent-run-id CLI arg in overrides: {args_override}"
    );
    assert!(
        args_override.contains("run-123"),
        "expected agent run id value in overrides: {args_override}"
    );
    assert!(
        args_override.contains("--pipeline-role") && args_override.contains("memory_capture"),
        "expected pipeline role CLI arg in overrides: {args_override}"
    );
}

#[test]
fn build_codex_mcp_overrides_emits_filesystem_enforcement_only_when_enabled() {
    let temp_dir = tempfile::tempdir().expect("temp dir");
    let plugin_dir = create_plugin_dir(temp_dir.path());
    std::fs::create_dir_all(plugin_dir.join("ralphx-mcp-server/build"))
        .expect("create fake mcp build dir");
    std::fs::write(
        plugin_dir.join("ralphx-mcp-server/build/index.js"),
        "// fake mcp server",
    )
    .expect("write fake mcp server");

    let enforced = CodexMcpRuntimeContext {
        enforce_filesystem_roots: true,
        ..Default::default()
    };
    let enforced_overrides =
        build_codex_mcp_overrides(&plugin_dir, "ralphx-plan-verifier", false, Some(&enforced))
            .expect("enforced overrides");
    let enforced_args = enforced_overrides
        .iter()
        .find(|entry| entry.starts_with("mcp_servers.") && entry.contains(".args="))
        .expect("enforced args override");
    assert!(
        enforced_args.contains("--filesystem-enforced") && enforced_args.contains("\"1\""),
        "enforced Codex MCP args must carry the CLI-only flag: {enforced_args}"
    );

    let unenforced = CodexMcpRuntimeContext::default();
    let unenforced_overrides = build_codex_mcp_overrides(
        &plugin_dir,
        "ralphx-plan-verifier",
        false,
        Some(&unenforced),
    )
    .expect("unenforced overrides");
    let unenforced_args = unenforced_overrides
        .iter()
        .find(|entry| entry.starts_with("mcp_servers.") && entry.contains(".args="))
        .expect("unenforced args override");
    assert!(
        !unenforced_args.contains("--filesystem-enforced"),
        "unenforced Codex MCP args must preserve the prior shape: {unenforced_args}"
    );
    assert!(
        enforced_overrides
            .iter()
            .chain(unenforced_overrides.iter())
            .all(|entry| !entry.contains("RALPHX_FILESYSTEM_ENFORCED")),
        "filesystem enforcement must never be delivered through process env"
    );
}

#[test]
fn configure_spawn_preserves_user_shims_while_ensuring_node_bin() {
    let _lock = crate::infrastructure::tool_paths::TEST_ENV_MUTEX
        .lock()
        .expect("env mutex");
    let _path = EnvGuard::set_os("PATH", "/usr/bin:/bin");
    let _disable_login_shell =
        EnvGuard::set_os(crate::infrastructure::login_shell_env::DISABLE_ENV_VAR, "1");
    let _node_override = EnvGuard::set_os("RALPHX_NODE_PATH", "/tmp/fake-node-bin/node");
    let expected_node_bin = PathBuf::from("/tmp/fake-node-bin");

    let mut cmd = tokio::process::Command::new("/usr/bin/env");
    cmd.env("GITHUB_TOKEN", "stale-secret");
    configure_spawn(&mut cmd, None, CodexPromptTransport::PositionalArg);

    assert!(cmd
        .as_std()
        .get_envs()
        .all(|(key, value)| { key != OsStr::new("GITHUB_TOKEN") || value.is_none() }));

    let path_value = cmd
        .as_std()
        .get_envs()
        .find_map(|(key, value)| {
            (key == OsStr::new("PATH")).then(|| value.map(|v| v.to_os_string()))?
        })
        .expect("PATH env");
    let path_entries = std::env::split_paths(&path_value).collect::<Vec<_>>();
    if let Some(home) = dirs::home_dir() {
        let cargo_bin = home.join(".cargo").join("bin");
        assert!(
            path_index(&path_entries, &cargo_bin) < path_index(&path_entries, &expected_node_bin),
            "user cargo shim should stay before inserted Node bin: {path_value:?}"
        );
    }
    assert!(path_index(&path_entries, &expected_node_bin) < path_index(&path_entries, "/usr/bin"));

    let screenshot_dir = cmd
        .as_std()
        .get_envs()
        .find_map(|(key, value)| {
            (key == OsStr::new("RALPHX_AGENT_SCREENSHOT_DIR"))
                .then(|| value.map(|v| v.to_os_string()))?
        })
        .expect("RALPHX_AGENT_SCREENSHOT_DIR env");
    assert!(screenshot_dir.to_string_lossy().contains("screenshots"));
}

#[tokio::test]
async fn positional_prompt_transport_exposes_immediate_stdin_eof() {
    let _disable_login_shell =
        EnvGuard::set_os(crate::infrastructure::login_shell_env::DISABLE_ENV_VAR, "1");
    let mut cmd = tokio::process::Command::new("/bin/sh");
    cmd.args([
        "-c",
        "payload=$(cat); if [ -n \"$payload\" ]; then printf 'data:%s' \"$payload\"; else printf eof; fi",
    ]);
    let transport = configure_spawn(&mut cmd, None, CodexPromptTransport::PositionalArg);
    let child = SpawnableCommand::new_with_stdin_transport(cmd, None, transport)
        .spawn()
        .await
        .expect("spawn positional transport fixture");
    let output = child.wait_with_output().await.expect("wait for fixture");

    assert_eq!(transport, SpawnableStdinTransport::Null);
    assert_eq!(String::from_utf8_lossy(&output.stdout), "eof");
}

#[tokio::test]
async fn explicit_stdin_prompt_transport_writes_prompt_then_closes_pipe() {
    let _disable_login_shell =
        EnvGuard::set_os(crate::infrastructure::login_shell_env::DISABLE_ENV_VAR, "1");
    let mut cmd = tokio::process::Command::new("/bin/sh");
    cmd.args([
        "-c",
        "payload=$(cat); if [ -n \"$payload\" ]; then printf 'data:%s' \"$payload\"; else printf eof; fi",
    ]);
    let transport = configure_spawn(&mut cmd, None, CodexPromptTransport::Stdin);
    let child = SpawnableCommand::new_with_stdin_transport(
        cmd,
        Some("prompt-through-stdin".to_string()),
        transport,
    )
    .spawn()
    .await
    .expect("spawn stdin transport fixture");
    let output = child.wait_with_output().await.expect("wait for fixture");

    assert_eq!(transport, SpawnableStdinTransport::Piped);
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        "data:prompt-through-stdin"
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
        "external MCP transport should use a streamable HTTP URL; override keys: {:?}",
        override_keys(&overrides)
    );
    assert!(overrides
        .iter()
        .any(|entry| entry == "mcp_servers.ralphx.required=true"));
    assert!(overrides
        .iter()
        .any(|entry| entry == "mcp_servers.ralphx.startup_timeout_sec=30"));
    assert!(
        overrides.iter().any(|entry| {
            entry == "mcp_servers.ralphx.bearer_token_env_var=\"RALPHX_TAURI_MCP_BYPASS_TOKEN\""
        }),
        "external MCP transport should use the Tauri bypass token env var; override keys: {:?}",
        override_keys(&overrides)
    );
    assert!(
        overrides
            .iter()
            .any(|entry| entry == "mcp_servers.ralphx.enabled_tools=[\"v1_start_ideation\",\"v1_get_ideation_status\"]"),
        "external MCP enabled tools should come from Codex metadata; override keys: {:?}",
        override_keys(&overrides)
    );
    assert!(
        !overrides.iter().any(|entry| entry.contains(".command=") || entry.contains(".args=")),
        "external MCP transport must not point Codex at the bundled stdio MCP server; override keys: {:?}",
        override_keys(&overrides)
    );
    assert!(
        overrides
            .iter()
            .any(|entry| entry == "features.shell_tool=false"),
        "runtime feature flags should still be preserved; override keys: {:?}",
        override_keys(&overrides)
    );
}

#[test]
fn build_codex_mcp_overrides_keeps_plan_question_tool_for_interactive_runs() {
    let temp_dir = tempfile::tempdir().expect("temp dir");
    let root = temp_dir.path();
    let plugin_dir = create_plugin_dir(root);
    seed_live_agent_yaml(root, "ralphx-ideation");

    let overrides = build_codex_mcp_overrides_for_profile(
        &plugin_dir,
        "ralphx-ideation",
        Some("plan"),
        false,
        None,
    )
    .expect("overrides");
    let args = codex_mcp_args_override(&overrides);
    let spawn_args = build_codex_exec_args(
        &full_codex_capabilities(),
        &CodexExecCliConfig {
            config_overrides: overrides.clone(),
            ..CodexExecCliConfig::default()
        },
    )
    .expect("Plan spawn args");

    assert!(
        args.contains("--allowed-tools=") && args.contains("ask_user_question"),
        "Codex Plan chat must keep ask_user_question for interactive Agent conversations; override keys: {:?}",
        override_keys(&overrides)
    );
    assert!(
        spawn_args.windows(2).any(|pair| {
            pair[0] == "-c" && pair[1] == "features.apply_patch_freeform=false"
        }),
        "Codex Plan profile must disable the legacy apply_patch feature if the CLI recognizes it; override keys: {:?}",
        override_keys(&overrides)
    );
    assert!(
        spawn_args.windows(2).any(|pair| {
            pair[0] == "-c" && pair[1] == "features.apply_patch_streaming_events=false"
        }),
        "Codex Plan profile must disable apply_patch streaming events if the CLI recognizes them; override keys: {:?}",
        override_keys(&overrides)
    );
    assert!(
        spawn_args.windows(2).any(|pair| {
            pair[0] == "-c" && pair[1] == "include_apply_patch_tool=false"
        }),
        "Codex Plan profile must disable the direct apply_patch tool config if the CLI recognizes it; override keys: {:?}",
        override_keys(&overrides)
    );
}

#[test]
fn build_codex_mcp_overrides_disables_apply_patch_for_persona_extractor() {
    let root = project_root();
    let plugin_dir = root.join("plugins").join("app");

    let overrides = build_codex_mcp_overrides(&plugin_dir, "ralphx-persona-extractor", false, None)
        .expect("persona extractor overrides");
    let spawn_args = build_codex_exec_args(
        &full_codex_capabilities(),
        &CodexExecCliConfig {
            config_overrides: overrides.clone(),
            ..CodexExecCliConfig::default()
        },
    )
    .expect("PersonaExtractor spawn args");

    assert!(
        spawn_args
            .windows(2)
            .any(|pair| pair[0] == "-c" && pair[1] == "features.shell_tool=false"),
        "Codex PersonaExtractor must disable the native shell; override keys: {:?}",
        override_keys(&overrides)
    );

    assert!(
        overrides.iter().any(|entry| entry
            == "mcp_servers.ralphx.enabled_tools=[\"fs_read_file\",\"fs_list_dir\",\"fs_grep\",\"fs_glob\",\"ask_user_question\",\"save_persona_draft\",\"get_persona_draft\"]"),
        "Codex PersonaExtractor must receive exactly its canonical MCP grants"
    );

    for expected in [
        "features.apply_patch_freeform=false",
        "features.apply_patch_streaming_events=false",
        "include_apply_patch_tool=false",
    ] {
        assert!(
            spawn_args
                .windows(2)
                .any(|pair| pair[0] == "-c" && pair[1] == expected),
            "Codex PersonaExtractor must disable {expected}; override keys: {:?}",
            override_keys(&overrides)
        );
    }
}

#[test]
fn build_codex_mcp_overrides_filters_plan_question_tool_for_external_runs() {
    let temp_dir = tempfile::tempdir().expect("temp dir");
    let root = temp_dir.path();
    let plugin_dir = create_plugin_dir(root);
    seed_live_agent_yaml(root, "ralphx-ideation");

    let overrides = build_codex_mcp_overrides_for_profile(
        &plugin_dir,
        "ralphx-ideation",
        Some("plan"),
        true,
        None,
    )
    .expect("overrides");
    let args = codex_mcp_args_override(&overrides);

    assert!(
        !args.contains("ask_user_question"),
        "External Codex Plan chat spawns must filter ask_user_question; override keys: {:?}",
        override_keys(&overrides)
    );
    assert!(
        args.contains("get_session_plan"),
        "Filtering interactive tools must preserve non-interactive Plan tools; override keys: {:?}",
        override_keys(&overrides)
    );
}

#[test]
fn compose_codex_prompt_includes_runtime_profile_context_for_profile() {
    let root = project_root();
    let plugin_dir = root.join("plugins/app");
    let prompt = compose_codex_prompt_for_profile(
        "Create a plan",
        Some(&plugin_dir),
        Some("ralphx-ideation"),
        Some("plan"),
        None,
    );

    assert!(prompt.contains("<agent_runtime_profile>"));
    assert!(prompt.contains("<agent_name>ralphx-ideation</agent_name>"));
    assert!(prompt.contains("<profile_slug>plan</profile_slug>"));
    assert!(prompt.contains("<profile_role>plan_chat</profile_role>"));

    let default_prompt =
        compose_codex_prompt("Create a plan", Some(&plugin_dir), Some("ralphx-ideation"));
    assert!(!default_prompt.contains("<agent_name>ralphx-ideation</agent_name>"));
    assert!(!default_prompt.contains("<profile_role>plan_chat</profile_role>"));
}

#[test]
fn codex_prompt_orders_persona_before_skills_and_runtime_profile_inside_ralphx_agent_instructions()
{
    let root = project_root();
    let plugin_dir = root.join("plugins/app");
    let persona = "<ralphx_agent_persona>Persona voice</ralphx_agent_persona>";
    let prompt = compose_codex_prompt_for_profile(
        "<!-- ralphx_internal_skill=ralphx-agent-workspace-swe -->",
        Some(&plugin_dir),
        Some("ralphx-ideation"),
        Some("plan"),
        Some(persona),
    );

    let instructions = prompt
        .find("<ralphx_agent_instructions>")
        .expect("agent instructions");
    let persona = prompt.find(persona).expect("persona block");
    // The live profile prompt mentions these tags in prose, so anchor on the
    // APPENDED blocks (last occurrence), not the first prose mention.
    let skills = prompt
        .rfind("<ralphx_internal_skills>")
        .expect("internal skills");
    let runtime_profile = prompt
        .rfind("<agent_runtime_profile>")
        .expect("runtime profile");

    assert!(instructions < persona && persona < skills && skills < runtime_profile);
}

#[test]
fn compose_codex_prompt_for_profile_without_persona_is_byte_identical_to_today() {
    let root = project_root();
    let plugin_dir = root.join("plugins/app");
    let wrapper = compose_codex_prompt("Create a plan", Some(&plugin_dir), Some("ralphx-ideation"));
    let threaded = compose_codex_prompt_for_profile(
        "Create a plan",
        Some(&plugin_dir),
        Some("ralphx-ideation"),
        None,
        None,
    );

    assert_eq!(threaded, wrapper);
    assert!(!threaded.contains("<ralphx_agent_persona>"));
}

#[test]
fn build_codex_mcp_overrides_mixes_external_transport_with_internal_sidecar_tools() {
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
    internal_mcp_tools:
      - create_agent_task
      - list_agent_tasks
"#,
    )
    .expect("write shared definition");

    let overrides = build_codex_mcp_overrides(&plugin_dir, "ralphx-chat-project", false, None)
        .expect("overrides");

    assert!(
        overrides
            .iter()
            .any(|entry| entry.starts_with("mcp_servers.ralphx.url=")),
        "external MCP transport should remain configured; override keys: {:?}",
        override_keys(&overrides)
    );
    assert!(
        overrides
            .iter()
            .any(|entry| entry.starts_with("mcp_servers.ralphx_internal.command=")),
        "internal MCP sidecar should launch bundled stdio server; override keys: {:?}",
        override_keys(&overrides)
    );
    assert!(overrides
        .iter()
        .any(|entry| entry == "mcp_servers.ralphx_internal.required=true"));
    assert!(overrides
        .iter()
        .any(|entry| entry == "mcp_servers.ralphx_internal.startup_timeout_sec=30"));
    assert!(
        overrides.iter().any(|entry| {
            entry
                == "mcp_servers.ralphx_internal.enabled_tools=[\"create_agent_task\",\"list_agent_tasks\"]"
        }),
        "internal sidecar enabled tools should come from internal_mcp_tools; override keys: {:?}",
        override_keys(&overrides)
    );
    assert!(
        overrides.iter().any(|entry| {
            entry.contains("mcp_servers.ralphx_internal.args=")
                && entry.contains("--allowed-tools=create_agent_task,list_agent_tasks")
        }),
        "internal sidecar args should pass narrowed --allowed-tools; override keys: {:?}",
        override_keys(&overrides)
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
        conversation_id: Some("conversation current".to_string()),
        coordination_mode: None,
        task_id: None,
        task_state: Some("reviewing".to_string()),
        project_id: Some("project-123".to_string()),
        working_directory: Some(root.join("workspace")),
        filesystem_read_roots: Vec::new(),
        enforce_filesystem_roots: false,
        lead_session_id: None,
        parent_conversation_id: Some("conversation 456".to_string()),
        agent_run_id: Some("run 789".to_string()),
        pipeline_role: None,
        skill_distillation_batch_id: None,
        skill_distillation_claim_token: None,
        skill_distillation_fingerprint: None,
        skill_distillation_outcome_ids: None,
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
        url_override.contains("conversation_id=conversation%20current"),
        "external MCP URL should include encoded conversation id: {url_override}"
    );
    assert!(
        url_override.contains("parent_conversation_id=conversation%20456"),
        "external MCP URL should include encoded parent conversation id: {url_override}"
    );
    assert!(
        url_override.contains("agent_run_id=run%20789"),
        "external MCP URL should include encoded agent run id: {url_override}"
    );
}
