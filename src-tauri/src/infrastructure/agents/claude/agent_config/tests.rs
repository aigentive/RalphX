use super::*;
use crate::domain::agents::{AgentHarnessKind, AgentLane, LogicalEffort};
use crate::infrastructure::agents::claude::agent_config::process_config::canonical_process_mapping;
use crate::infrastructure::agents::claude::agent_names::{
    SHORT_AGENT_WORKSPACE_PR_FIXER, SHORT_AGENT_WORKSPACE_REPAIR,
    SHORT_AUTOMATION_DECOMPOSITION_VERIFIER, SHORT_AUTOMATION_JUDGE, SHORT_AUTOMATION_PLAN_JUDGE,
    SHORT_AUTOMATION_SETUP, SHORT_BRANCH_UPDATER, SHORT_CHAT_PROJECT, SHORT_CHAT_TASK, SHORT_CODER,
    SHORT_DEEP_RESEARCHER, SHORT_GENERAL_EXPLORER, SHORT_GENERAL_WORKER, SHORT_IDEATION_ADVOCATE,
    SHORT_IDEATION_CRITIC, SHORT_IDEATION_SPECIALIST_BACKEND, SHORT_IDEATION_SPECIALIST_FRONTEND,
    SHORT_IDEATION_SPECIALIST_INFRA, SHORT_MEMORY_CAPTURE, SHORT_MEMORY_MAINTAINER, SHORT_MERGER,
    SHORT_ORCHESTRATOR, SHORT_ORCHESTRATOR_IDEATION, SHORT_ORCHESTRATOR_IDEATION_READONLY,
    SHORT_PERSONA_EXTRACTOR, SHORT_PROJECT_ANALYZER, SHORT_PR_DESCRIBER, SHORT_PR_REVIEWER,
    SHORT_QA_EXECUTOR, SHORT_QA_PREP, SHORT_REVIEWER, SHORT_REVIEW_CHAT, SHORT_REVIEW_HISTORY,
    SHORT_SESSION_NAMER, SHORT_TASK_MANAGER, SHORT_WORKER, SHORT_WORKSPACE_ANNOTATOR,
    SHORT_WORKSPACE_REVIEWER,
};
use crate::infrastructure::agents::harness_agent_catalog::{
    has_canonical_agent_definition, list_canonical_prompt_backed_agents, load_harness_agent_prompt,
    AgentPromptHarness,
};
use std::collections::HashSet;
use std::ffi::OsString;
use std::path::PathBuf;

struct EnvGuard {
    key: &'static str,
    original: Option<OsString>,
}

impl EnvGuard {
    fn set(key: &'static str, value: &str) -> Self {
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

#[test]
fn test_yaml_loaded_has_unique_names() {
    let mut names: Vec<String> = agent_configs().iter().map(|c| c.name.clone()).collect();
    let original_len = names.len();
    names.sort();
    names.dedup();
    assert_eq!(names.len(), original_len);
}

#[test]
fn test_canonical_agent_project_root_resolves_live_claude_agents() {
    let project_root = canonical_agent_project_root();
    let expected_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .canonicalize()
        .expect("canonical repo root");

    assert_eq!(project_root, expected_root);

    let live_agents =
        list_canonical_prompt_backed_agents(&project_root, AgentPromptHarness::Claude);
    assert!(
        live_agents.contains(&SHORT_ORCHESTRATOR_IDEATION.to_string()),
        "canonical project root should expose live Claude agents for runtime config synthesis"
    );
}

#[test]
fn test_canonical_agent_project_root_falls_back_to_runtime_plugin_dir() {
    let temp_dir = tempfile::TempDir::new().expect("temp dir");
    let runtime_root = temp_dir.path().join("runtime-root");
    let agent_root = runtime_root.join("agents").join("ralphx-test-agent");
    let plugin_dir = runtime_root.join("plugins").join("app");
    std::fs::create_dir_all(&agent_root).expect("agent root");
    std::fs::create_dir_all(&plugin_dir).expect("plugin dir");
    std::fs::write(
        agent_root.join("agent.yaml"),
        "name: ralphx-test-agent\nrole: test\n",
    )
    .expect("agent definition");

    let missing_config_path = temp_dir
        .path()
        .join("missing")
        .join("config")
        .join("ralphx.yaml");

    assert_eq!(
        canonical_agent_project_root_from_config_path(&missing_config_path, Some(&plugin_dir)),
        runtime_root.canonicalize().expect("canonical runtime root")
    );
}

#[test]
fn test_canonical_agent_project_root_ignores_runtime_plugin_dir_without_agents() {
    let temp_dir = tempfile::TempDir::new().expect("temp dir");
    let plugin_dir = temp_dir
        .path()
        .join("runtime-root")
        .join("plugins")
        .join("app");
    std::fs::create_dir_all(&plugin_dir).expect("plugin dir");

    let missing_config_path = temp_dir
        .path()
        .join("missing")
        .join("config")
        .join("ralphx.yaml");

    assert_eq!(
        canonical_agent_project_root_from_config_path(&missing_config_path, Some(&plugin_dir)),
        temp_dir.path().join("missing")
    );
}

#[test]
fn test_get_allowed_tools_worker_agent() {
    let tools = get_allowed_tools("ralphx-execution-worker").unwrap();
    let tool_list: HashSet<_> = tools.split(',').collect();
    assert!(tool_list.contains("Read"));
    assert!(tool_list.contains("Write"));
    assert!(tool_list.contains("Edit"));
    assert!(!tool_list.contains("Task"));
}

#[test]
fn test_get_allowed_tools_mcp_only_agent() {
    assert_eq!(
        get_allowed_tools("ralphx-utility-session-namer"),
        Some(String::new())
    );
}

#[test]
fn persona_extractor_resolved_cli_tools_exclude_native_fs_exec_and_are_nonempty() {
    let config = get_agent_config(SHORT_PERSONA_EXTRACTOR)
        .expect("persona extractor should resolve through the production config path");

    assert!(
        !config.resolved_cli_tools.is_empty(),
        "A7 containment requires a non-empty --tools value so Claude cannot fall back to its native defaults"
    );
    for forbidden in [
        "Read",
        "Grep",
        "Glob",
        "Bash",
        "Write",
        "Edit",
        "NotebookEdit",
    ] {
        assert!(
            !config
                .resolved_cli_tools
                .iter()
                .any(|tool| tool == forbidden),
            "persona extractor must exclude native {forbidden}; got {:?}",
            config.resolved_cli_tools
        );
    }
}

#[test]
fn test_get_preapproved_tools_worker_contains_expected() {
    let tools = get_preapproved_tools("ralphx-execution-worker").unwrap();
    let tool_list: HashSet<_> = tools.split(',').collect();
    assert!(tool_list.contains("mcp__ralphx__get_task_context"));
    assert!(tool_list.contains("mcp__ralphx__get_project_analysis"));
    assert!(tool_list.contains("Write"));
    assert!(!tool_list.contains("Task"));
    assert!(!tool_list.contains("Task(Explore)"));
    // Workers should NOT have memory skills - only dedicated memory agents
    assert!(!tool_list.contains("Skill(ralphx:rule-manager)"));
}

#[test]
fn test_get_preapproved_tools_project_chat_mixes_external_and_internal_mcp_prefixes() {
    let tools = get_preapproved_tools("ralphx-chat-project").unwrap();
    let tool_list: HashSet<_> = tools.split(',').collect();

    assert!(tool_list.contains("mcp__ralphx__v1_start_ideation"));
    assert!(tool_list.contains("mcp__ralphx_internal__create_agent_task"));
    assert!(tool_list.contains("mcp__ralphx_internal__delegate_start"));
    assert!(tool_list.contains("mcp__ralphx_internal__permission_request"));
    assert!(!tool_list.contains("mcp__ralphx__create_agent_task"));
}

#[test]
fn test_internal_sidecar_permission_tool_external_with_internal_uses_internal_server() {
    assert_eq!(
        internal_sidecar_permission_request_tool(true, true, "ralphx"),
        Some("mcp__ralphx_internal__permission_request".to_string())
    );
}

#[test]
fn test_internal_sidecar_permission_tool_external_without_internal_is_none() {
    // No permission_request tool is injected, so there is nothing for the flag to name.
    assert_eq!(
        internal_sidecar_permission_request_tool(true, false, "ralphx"),
        None
    );
}

#[test]
fn test_internal_sidecar_permission_tool_non_external_is_none() {
    // Non-external agents expose permission_request on the primary server, not the
    // sidecar; the shared helper returns None so the flag keeps its configured default.
    assert_eq!(
        internal_sidecar_permission_request_tool(false, false, "ralphx"),
        None
    );
}

#[test]
fn test_resolve_permission_prompt_tool_external_agent_matches_injected_tool() {
    // ralphx-chat-project uses external transport with an internal sidecar, so the
    // permission-prompt tool must point at the internal server — and must equal the
    // permission_request tool injected into its pre-approved tool surface.
    let resolved = resolve_permission_prompt_tool(
        Some("ralphx-chat-project"),
        None,
        "mcp__ralphx__permission_request",
    );
    assert_eq!(resolved, "mcp__ralphx_internal__permission_request");

    let preapproved = get_preapproved_tools("ralphx-chat-project").unwrap();
    let tool_list: HashSet<_> = preapproved.split(',').collect();
    assert!(tool_list.contains(resolved.as_str()));
}

#[test]
fn test_resolve_permission_prompt_tool_non_external_agent_preserves_custom_default() {
    // Non-external agents must NOT have a transport-local tool inferred over an explicit
    // configured default — including fully-qualified custom values the config preserves.
    let custom_default = "mcp__custom_server__custom_permission_request";
    let resolved =
        resolve_permission_prompt_tool(Some("ralphx-execution-worker"), None, custom_default);
    assert_eq!(resolved, custom_default);
}

#[test]
fn test_resolve_permission_prompt_tool_no_agent_returns_default() {
    let custom_default = "mcp__custom_server__custom_permission_request";
    assert_eq!(
        resolve_permission_prompt_tool(None, None, custom_default),
        custom_default
    );
}

#[test]
fn test_resolve_permission_prompt_tool_is_profile_aware_and_matches_surface() {
    // The resolver loads transport metadata through the same profile-aware path as
    // get_preapproved_tools_for_profile, so the flag stays consistent with the
    // profile's actual MCP surface for each profile (base + the `plan` profile).
    for profile in [None, Some("plan")] {
        let resolved = resolve_permission_prompt_tool(
            Some("ralphx-ideation"),
            profile,
            "mcp__ralphx__permission_request",
        );
        let preapproved = get_preapproved_tools_for_profile("ralphx-ideation", profile).unwrap();
        let tool_list: HashSet<_> = preapproved.split(',').collect();
        assert!(
            tool_list.contains(resolved.as_str()),
            "profile {profile:?}: resolved {resolved} not in pre-approved surface"
        );
    }
}

#[test]
fn test_default_base_tool_set_present_in_worker() {
    let tools = get_allowed_tools("ralphx-execution-worker").unwrap();
    for t in super::tool_sets::canonical_claude_tool_sets()
        .get("base_tools")
        .expect("embedded canonical tool sets should include base_tools")
    {
        assert!(tools.contains(t), "worker missing base tool {}", t);
    }
}

#[test]
fn test_agents_screen_claude_modes_include_web_search_tools() {
    let mode_agents = [
        ("agent", SHORT_GENERAL_WORKER),
        ("chat", SHORT_GENERAL_EXPLORER),
        ("plan", SHORT_GENERAL_EXPLORER),
        ("ideation", SHORT_CHAT_PROJECT),
    ];

    for (mode, agent_name) in mode_agents {
        let tools = get_allowed_tools(agent_name)
            .unwrap_or_else(|| panic!("{mode} mode agent {agent_name} should resolve tools"));
        let tool_list: HashSet<_> = tools.split(',').collect();
        assert!(
            tool_list.contains("WebFetch"),
            "{mode} mode agent {agent_name} missing WebFetch"
        );
        assert!(
            tool_list.contains("WebSearch"),
            "{mode} mode agent {agent_name} missing WebSearch"
        );
    }
}

#[test]
fn test_all_agent_names_are_known() {
    let known: HashSet<&str> = HashSet::from([
        SHORT_ORCHESTRATOR_IDEATION,
        SHORT_ORCHESTRATOR_IDEATION_READONLY,
        SHORT_SESSION_NAMER,
        SHORT_PR_DESCRIBER,
        SHORT_CHAT_TASK,
        SHORT_CHAT_PROJECT,
        SHORT_REVIEW_CHAT,
        SHORT_REVIEW_HISTORY,
        SHORT_GENERAL_EXPLORER,
        SHORT_GENERAL_WORKER,
        SHORT_TASK_MANAGER,
        SHORT_AGENT_WORKSPACE_REPAIR,
        SHORT_AGENT_WORKSPACE_PR_FIXER,
        SHORT_PR_REVIEWER,
        SHORT_WORKSPACE_REVIEWER,
        SHORT_WORKSPACE_ANNOTATOR,
        SHORT_WORKER,
        SHORT_CODER,
        SHORT_REVIEWER,
        SHORT_QA_PREP,
        SHORT_QA_EXECUTOR,
        SHORT_ORCHESTRATOR,
        SHORT_DEEP_RESEARCHER,
        SHORT_PROJECT_ANALYZER,
        SHORT_MERGER,
        SHORT_BRANCH_UPDATER,
        SHORT_MEMORY_MAINTAINER,
        SHORT_MEMORY_CAPTURE,
        // Ideation specialist agents
        SHORT_IDEATION_SPECIALIST_BACKEND,
        SHORT_IDEATION_SPECIALIST_FRONTEND,
        SHORT_IDEATION_SPECIALIST_INFRA,
        SHORT_IDEATION_ADVOCATE,
        SHORT_IDEATION_CRITIC,
        // Utility agent used for plan complexity checks.
        "ralphx-utility-plan-complexity",
        SHORT_AUTOMATION_SETUP,
        SHORT_AUTOMATION_JUDGE,
        SHORT_AUTOMATION_PLAN_JUDGE,
        SHORT_AUTOMATION_DECOMPOSITION_VERIFIER,
        // Persona extractor (agent persona system, PR-14)
        "ralphx-persona-extractor",
    ]);

    for agent in agent_configs() {
        assert!(
            known.contains(agent.name.as_str()),
            "Unknown agent name in resolved Claude runtime roster: {}",
            agent.name
        );
    }
}

#[test]
fn test_all_live_runtime_agents_have_canonical_claude_prompts() {
    let project_root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..");

    for agent in agent_configs() {
        assert!(
            has_canonical_agent_definition(&project_root, &agent.name),
            "Missing canonical agent definition for {}",
            agent.name
        );
        assert!(
            load_harness_agent_prompt(&project_root, &agent.name, AgentPromptHarness::Claude)
                .is_some(),
            "Missing canonical Claude prompt for {}",
            agent.name
        );
    }
}

#[test]
fn test_default_config_paths_use_config_directory_layout() {
    let expected_config = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../config/ralphx.yaml");
    let expected_processes =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../config/processes.yaml");
    let expected_claude =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../config/harnesses/claude.yaml");
    let expected_codex =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../config/harnesses/codex.yaml");
    let expected_external_mcp =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../config/external-mcp.yaml");

    assert_eq!(config_path(), expected_config);
    assert_eq!(process_config_path(), expected_processes);
    assert_eq!(claude_config_path(), expected_claude);
    assert_eq!(codex_config_path(), expected_codex);
    assert_eq!(external_mcp_config_path(), expected_external_mcp);
}

#[test]
fn test_runtime_config_dir_path_resolution_uses_bundled_config_dir() {
    let temp_dir = tempfile::TempDir::new().expect("temp dir");
    let config_dir = temp_dir.path().join("Resources").join("config");

    assert_eq!(
        config_path_for_runtime_config_dir(Some(&config_dir)),
        config_dir.join("ralphx.yaml")
    );

    assert_eq!(
        config_path_for_runtime_config_dir(None),
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../config/ralphx.yaml")
    );
}

#[test]
fn early_file_logging_environment_override_has_highest_precedence() {
    assert!(resolve_file_logging_from_sources(
        Some("YES"),
        None,
        "file_logging: false"
    ));
    assert!(!resolve_file_logging_from_sources(
        Some("0"),
        None,
        "file_logging: true"
    ));
}

#[test]
fn early_file_logging_public_resolver_reads_environment_override() {
    let _env = EnvGuard::set("RALPHX_FILE_LOGGING", "0");

    assert!(!resolve_file_logging_early());
}

#[test]
fn early_file_logging_prefers_configured_runtime_file() {
    let temp_dir = tempfile::TempDir::new().expect("temp dir");
    let config_path = temp_dir.path().join("ralphx.yaml");
    std::fs::write(&config_path, "file_logging: false\n").expect("write runtime config");

    assert!(!resolve_file_logging_from_sources(
        None,
        Some(&config_path),
        "file_logging: true"
    ));
}

#[test]
fn early_file_logging_uses_embedded_config_before_runtime_setup() {
    assert!(!resolve_file_logging_from_sources(
        None,
        None,
        "file_logging: false"
    ));
}

#[test]
fn early_file_logging_falls_back_from_missing_or_malformed_runtime_config() {
    let temp_dir = tempfile::TempDir::new().expect("temp dir");
    let missing_path = temp_dir.path().join("missing.yaml");
    let malformed_path = temp_dir.path().join("malformed.yaml");
    std::fs::write(&malformed_path, "file_logging: [\n").expect("write malformed config");

    assert!(!resolve_file_logging_from_sources(
        None,
        Some(&missing_path),
        "file_logging: false"
    ));
    assert!(!resolve_file_logging_from_sources(
        None,
        Some(&malformed_path),
        "file_logging: false"
    ));
    assert!(resolve_file_logging_from_sources(
        None,
        Some(&malformed_path),
        "file_logging: ["
    ));
}

#[test]
fn early_file_logging_limits_prefer_environment_then_runtime_config() {
    assert_eq!(
        resolve_file_logging_limits_from_sources(
            Some("42"),
            Some("7"),
            None,
            "file_logging_max_bytes: 9\nfile_logging_keep_files: 3"
        ),
        (42, 7)
    );

    let temp_dir = tempfile::TempDir::new().expect("temp dir");
    let config_path = temp_dir.path().join("ralphx.yaml");
    std::fs::write(
        &config_path,
        "file_logging_max_bytes: 12\nfile_logging_keep_files: 4\n",
    )
    .expect("write runtime config");

    assert_eq!(
        resolve_file_logging_limits_from_sources(
            None,
            None,
            Some(&config_path),
            "file_logging_max_bytes: 9\nfile_logging_keep_files: 3"
        ),
        (12, 4)
    );
}

#[test]
fn early_file_logging_zero_max_bytes_falls_back_to_default_from_any_source() {
    let temp_dir = tempfile::TempDir::new().expect("temp dir");
    let config_path = temp_dir.path().join("ralphx.yaml");
    std::fs::write(
        &config_path,
        "file_logging_max_bytes: 0\nfile_logging_keep_files: 4\n",
    )
    .expect("write runtime config");

    // YAML zero must not produce a writer that records nothing.
    let (max_bytes, keep_files) = resolve_file_logging_limits_from_sources(
        None,
        None,
        Some(&config_path),
        "file_logging_max_bytes: 9\nfile_logging_keep_files: 3",
    );
    assert_eq!(max_bytes, 1024 * 1024 * 1024);
    assert_eq!(keep_files, 4);

    // Environment zero was already rejected; it must fall through to config.
    assert_eq!(
        resolve_file_logging_limits_from_sources(
            Some("0"),
            None,
            None,
            "file_logging_max_bytes: 9\nfile_logging_keep_files: 3"
        ),
        (9, 3)
    );
}

#[test]
fn early_file_logging_limits_default_on_malformed_yaml() {
    let defaults = (
        default_file_logging_max_bytes(),
        default_file_logging_keep_files(),
    );
    assert_eq!(
        resolve_file_logging_limits_from_sources(None, None, None, "totally: [broken"),
        defaults,
    );
}

#[test]
fn early_file_logging_limits_missing_runtime_config_uses_embedded() {
    let temp_dir = tempfile::TempDir::new().expect("temp dir");
    let missing = temp_dir.path().join("does-not-exist.yaml");
    assert_eq!(
        resolve_file_logging_limits_from_sources(
            None,
            None,
            Some(&missing),
            "file_logging_max_bytes: 42\nfile_logging_keep_files: 2"
        ),
        (42, 2)
    );
}

#[test]
fn early_file_logging_limits_keep_files_env_overrides_config() {
    assert_eq!(
        resolve_file_logging_limits_from_sources(
            None,
            Some("11"),
            None,
            "file_logging_max_bytes: 42\nfile_logging_keep_files: 2"
        ),
        (42, 11)
    );
}

#[test]
fn early_file_logging_limits_non_numeric_env_falls_through() {
    assert_eq!(
        resolve_file_logging_limits_from_sources(
            Some("abc"),
            Some("xyz"),
            None,
            "file_logging_max_bytes: 42\nfile_logging_keep_files: 2"
        ),
        (42, 2)
    );
}

#[test]
fn database_maintenance_config_yaml_deserializes_with_defaults() {
    let yaml = "database_maintenance:\n  db_auto_compact_enabled: false\n";
    let cfg: RalphxConfig = serde_yaml::from_str(yaml).unwrap();
    assert!(!cfg.database_maintenance.db_auto_compact_enabled);
    assert_eq!(
        cfg.database_maintenance.db_auto_compact_max_db_bytes,
        2_147_483_648
    );
}

#[test]
fn test_live_runtime_agents_no_longer_reference_deprecated_plugin_prompt_paths() {
    for agent in agent_configs() {
        assert!(
            !agent.system_prompt_file.starts_with("plugins/app/agents/"),
            "live runtime agent {} still points at deleted legacy prompt path {}",
            agent.name,
            agent.system_prompt_file
        );
        assert!(
            agent.system_prompt_file.starts_with("agents/"),
            "live runtime agent {} should point at canonical prompt paths, got {}",
            agent.name,
            agent.system_prompt_file
        );
    }
}

#[test]
fn test_ideation_claude_prompt_keeps_provider_resume_silent_by_default() {
    let project_root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..");
    let prompt =
        load_harness_agent_prompt(&project_root, "ralphx-ideation", AgentPromptHarness::Claude)
            .expect("failed to load canonical ralphx-ideation prompt");

    assert!(
        prompt.contains("Do not behave like recovery mode on normal follow-up turns"),
        "ralphx-ideation Claude prompt must keep provider_resume turns conversational by default"
    );
    assert!(
        prompt.contains("do not narrate routine refreshes unless the check changes the answer"),
        "ralphx-ideation Claude prompt must avoid user-facing recovery chatter on ordinary resumed follow-ups"
    );
}

#[test]
fn test_permission_prompt_tool_accepts_shorthand() {
    let yaml = r#"
claude:
  mcp_server_name: ralphx
  permission_mode: default
  dangerously_skip_permissions: false
  permission_prompt_tool: permission_request
agents:
  - name: ralphx-execution-worker
tools:
  extends: base_tools
  include: [Write]
mcp_tools: [get_task_context]
preapproved_cli_tools: []
system_prompt_file: plugins/app/agents/worker.md
"#;
    let parsed = parse_config(yaml).expect("config should parse");
    assert_eq!(
        parsed.claude.permission_prompt_tool,
        "mcp__ralphx__permission_request"
    );
}

#[test]
fn test_settings_profile_selection_uses_default_profile_payload() {
    let yaml = r#"
claude:
  mcp_server_name: ralphx
  permission_mode: default
  dangerously_skip_permissions: false
  permission_prompt_tool: permission_request
  settings_profile: default
  settings_profiles:
    default:
      sandbox:
        enabled: false
    z_ai:
      env:
        ANTHROPIC_BASE_URL: https://api.z.ai/api/anthropic
agents:
  - name: ralphx-execution-worker
    tools:
      extends: base_tools
      include: [Write]
    mcp_tools: [get_task_context]
    preapproved_cli_tools: []
    system_prompt_file: plugins/app/agents/worker.md
"#;
    let parsed = parse_config_no_env_overrides(yaml).expect("config should parse");
    assert_eq!(
        parsed.claude.settings,
        Some(serde_json::json!({
            "sandbox": { "enabled": false }
        }))
    );
}

#[test]
fn test_automations_config_parses_top_level_block() {
    let yaml = r#"
automations:
  scheduler_poll_secs: 45
  signal_failure_pause_threshold: 7
  judge_timeout_secs: 240
  publish_grace_secs: 90
  max_run_duration_secs: 7200
  plan_max_revision_rounds: 5
  plan_judge_model:
    claude: claude-sonnet-5
    codex: gpt-5.4
"#;
    let parsed = parse_config_no_env_overrides(yaml).expect("config should parse");

    assert_eq!(parsed.automations.scheduler_poll_secs, 45);
    assert_eq!(parsed.automations.signal_failure_pause_threshold, 7);
    assert_eq!(parsed.automations.judge_timeout_secs, 240);
    assert_eq!(parsed.automations.publish_grace_secs, 90);
    assert_eq!(parsed.automations.max_run_duration_secs, 7200);
    assert_eq!(parsed.automations.plan_max_revision_rounds, 5);
    assert_eq!(
        parsed
            .automations
            .plan_judge_model
            .get(&AgentHarnessKind::Claude)
            .map(String::as_str),
        Some("claude-sonnet-5")
    );
    assert_eq!(
        parsed
            .automations
            .plan_judge_model
            .get(&AgentHarnessKind::Codex)
            .map(String::as_str),
        Some("gpt-5.4")
    );
}

#[test]
fn test_automations_config_env_overrides() {
    let parsed = parse_config_with_lookup("", &|name| match name {
        "RALPHX_AUTOMATIONS_SCHEDULER_POLL_SECS" => Some("45".to_string()),
        "RALPHX_AUTOMATIONS_SIGNAL_FAILURE_PAUSE_THRESHOLD" => Some("7".to_string()),
        "RALPHX_AUTOMATIONS_JUDGE_TIMEOUT_SECS" => Some("240".to_string()),
        "RALPHX_AUTOMATIONS_PUBLISH_GRACE_SECS" => Some("90".to_string()),
        "RALPHX_AUTOMATIONS_MAX_RUN_DURATION_SECS" => Some("7200".to_string()),
        "RALPHX_AUTOMATIONS_PLAN_MAX_REVISION_ROUNDS" => Some("6".to_string()),
        "RALPHX_AUTOMATIONS_PLAN_JUDGE_MODEL_CLAUDE" => Some("claude-sonnet-5".to_string()),
        "RALPHX_AUTOMATIONS_PLAN_JUDGE_MODEL_CODEX" => Some("gpt-5.4".to_string()),
        _ => None,
    })
    .expect("config should parse");

    assert_eq!(parsed.automations.scheduler_poll_secs, 45);
    assert_eq!(parsed.automations.signal_failure_pause_threshold, 7);
    assert_eq!(parsed.automations.judge_timeout_secs, 240);
    assert_eq!(parsed.automations.publish_grace_secs, 90);
    assert_eq!(parsed.automations.max_run_duration_secs, 7200);
    assert_eq!(parsed.automations.plan_max_revision_rounds, 6);
    assert_eq!(
        parsed
            .automations
            .plan_judge_model
            .get(&AgentHarnessKind::Claude)
            .map(String::as_str),
        Some("claude-sonnet-5")
    );
    assert_eq!(
        parsed
            .automations
            .plan_judge_model
            .get(&AgentHarnessKind::Codex)
            .map(String::as_str),
        Some("gpt-5.4")
    );
}

#[test]
fn resolve_file_logging_limits_early_returns_positive_defaults() {
    let (max_bytes, keep_files) = resolve_file_logging_limits_early();
    assert!(
        max_bytes > 0,
        "default max_bytes must be positive, got {max_bytes}"
    );
    assert!(
        keep_files > 0,
        "default keep_files must be positive, got {keep_files}"
    );
}

#[test]
fn test_openrouter_settings_profile_supports_blank_api_key_and_timeout() {
    let yaml = r#"
claude:
  mcp_server_name: ralphx
  permission_mode: default
  dangerously_skip_permissions: false
  permission_prompt_tool: permission_request
  settings_profile: openrouter
  settings_profiles:
    default:
      sandbox:
        enabled: false
    openrouter:
      extends: default
      env:
        ANTHROPIC_AUTH_TOKEN: your_openrouter_api_key
        ANTHROPIC_BASE_URL: https://openrouter.ai/api
        ANTHROPIC_API_KEY: ""
        API_TIMEOUT_MS: "3000000"
agents:
  - name: ralphx-execution-worker
    tools:
      extends: base_tools
      include: [Write]
    mcp_tools: [get_task_context]
    preapproved_cli_tools: []
    system_prompt_file: plugins/app/agents/worker.md
"#;
    let parsed: RalphxConfig = serde_yaml::from_str(yaml).expect("config should parse");
    let mut selected =
        resolve_profile_settings(&parsed.claude, "openrouter").expect("profile should resolve");
    if let Some(defaults) = parsed.claude.settings_profile_defaults.clone() {
        selected = merge_settings(defaults, selected);
    }
    apply_prefixed_env_overrides_with(&mut selected, Some("openrouter"), &|_| None);
    assert_eq!(
        Some(selected),
        Some(serde_json::json!({
            "sandbox": { "enabled": false },
            "env": {
                "ANTHROPIC_AUTH_TOKEN": "your_openrouter_api_key",
                "ANTHROPIC_BASE_URL": "https://openrouter.ai/api",
                "ANTHROPIC_API_KEY": "",
                "API_TIMEOUT_MS": "3000000"
            }
        }))
    );
}

#[test]
fn test_execution_defaults_parse_custom_values() {
    let yaml = r#"
execution_defaults:
  project:
    max_concurrent_tasks: 14
    project_ideation_max: 3
    auto_commit: false
    pause_on_failure: false
    agent_workspace_pr_autofix_default: true
    agent_workspace_pr_auto_merge_default: true
  global:
    global_max_concurrent: 28
    workspace_max_concurrent: 9
    global_ideation_max: 5
    allow_ideation_borrow_idle_execution: true
"#;
    let parsed = parse_config_no_env_overrides(yaml).expect("config should parse");

    assert_eq!(parsed.execution_defaults.project.max_concurrent_tasks, 14);
    assert_eq!(parsed.execution_defaults.project.project_ideation_max, 3);
    assert!(!parsed.execution_defaults.project.auto_commit);
    assert!(!parsed.execution_defaults.project.pause_on_failure);
    assert!(
        parsed
            .execution_defaults
            .project
            .agent_workspace_pr_autofix_default
    );
    assert!(
        parsed
            .execution_defaults
            .project
            .agent_workspace_pr_auto_merge_default
    );
    assert_eq!(parsed.execution_defaults.global.global_max_concurrent, 28);
    assert_eq!(parsed.execution_defaults.global.workspace_max_concurrent, 9);
    assert_eq!(parsed.execution_defaults.global.global_ideation_max, 5);
    assert!(
        parsed
            .execution_defaults
            .global
            .allow_ideation_borrow_idle_execution
    );
}

#[test]
fn test_execution_defaults_fallback_when_section_missing() {
    let parsed = parse_config_no_env_overrides("").expect("config should parse");

    assert_eq!(
        parsed.execution_defaults,
        ExecutionDefaultsConfig::default()
    );
}

#[test]
fn test_agent_harness_defaults_parse_custom_values() {
    let yaml = r#"
agent_harness_defaults:
  ideation_primary:
    harness: codex
    model: gpt-5.4
    effort: xhigh
    approval_policy: on-request
    sandbox_mode: workspace-write
  execution_worker:
    harness: claude
    model: sonnet
"#;
    let parsed = parse_config_no_env_overrides(yaml).expect("config should parse");

    let ideation_primary = parsed
        .agent_harness_defaults
        .get(&AgentLane::IdeationPrimary)
        .expect("ideation primary defaults should exist");
    assert_eq!(ideation_primary.harness, AgentHarnessKind::Codex);
    assert_eq!(ideation_primary.model.as_deref(), Some("gpt-5.4"));
    assert_eq!(ideation_primary.effort, Some(LogicalEffort::XHigh));
    assert_eq!(
        ideation_primary.approval_policy.as_deref(),
        Some("on-request")
    );
    assert_eq!(
        ideation_primary.sandbox_mode.as_deref(),
        Some("workspace-write")
    );

    let execution_worker = parsed
        .agent_harness_defaults
        .get(&AgentLane::ExecutionWorker)
        .expect("execution worker defaults should exist");
    assert_eq!(execution_worker.harness, AgentHarnessKind::Claude);
    assert_eq!(execution_worker.model.as_deref(), Some("sonnet"));
}

#[test]
fn test_agent_harness_defaults_fallback_when_section_missing() {
    let parsed = parse_config_no_env_overrides("").expect("config should parse");

    assert_eq!(
        parsed.agent_harness_defaults,
        default_agent_harness_defaults()
    );
}

#[test]
fn test_embedded_config_keeps_explicit_execution_defaults_aligned_with_fallback() {
    let parsed =
        parse_config_no_env_overrides(EMBEDDED_CONFIG).expect("embedded config should parse");

    assert_eq!(
        parsed.execution_defaults,
        ExecutionDefaultsConfig::default(),
        "config/ralphx.yaml should keep explicit execution_defaults aligned with the Rust fallback \
         defaults so YAML remains the human-edited source of truth and the code default stays \
         only a last-resort safety net"
    );
}

#[test]
fn test_embedded_config_parses_agent_completion_delivery_limits() {
    let parsed =
        parse_config_no_env_overrides(EMBEDDED_CONFIG).expect("embedded config should parse");

    assert_eq!(
        parsed.runtime.stream.agent_completion_correlation_ttl_secs,
        60
    );
    assert_eq!(
        parsed.runtime.stream.agent_completion_correlation_capacity,
        1_024
    );
    assert_eq!(
        parsed.runtime.stream.agent_completion_processed_ttl_secs,
        900
    );
    assert_eq!(
        parsed.runtime.stream.agent_completion_processed_capacity,
        4_096
    );
}

#[test]
fn test_embedded_config_omits_agent_harness_defaults_and_uses_fallback() {
    let parsed =
        parse_config_no_env_overrides(EMBEDDED_CONFIG).expect("embedded config should parse");

    assert_eq!(
        parsed.agent_harness_defaults,
        default_agent_harness_defaults(),
        "embedded config/ralphx.yaml should be able to omit agent_harness_defaults entirely while the \
         runtime still resolves the standard fallback defaults"
    );
}

#[test]
fn test_embedded_config_omits_runtime_system_prompt_paths_and_uses_canonical_prompts() {
    let parsed =
        parse_config_no_env_overrides(EMBEDDED_CONFIG).expect("embedded config should parse");

    for agent in &parsed.agents {
        assert!(
            agent.system_prompt_file.starts_with("agents/"),
            "live runtime agent {} should resolve a canonical prompt path when config/ralphx.yaml omits system_prompt_file, got {}",
            agent.name,
            agent.system_prompt_file
        );
    }
}

#[test]
fn test_embedded_config_omits_live_agent_runtime_mirrors_and_uses_canonical_metadata() {
    let parsed =
        parse_config_no_env_overrides(EMBEDDED_CONFIG).expect("embedded config should parse");

    let ideation = parsed
        .agents
        .iter()
        .find(|a| a.name == "ralphx-ideation")
        .expect("ideation agent should exist");
    assert_eq!(ideation.model.as_deref(), Some("opus"));
    assert_eq!(ideation.effort.as_deref(), Some("max"));
    assert!(ideation.resolved_cli_tools.contains(&"Task".to_string()));
    assert!(ideation
        .allowed_mcp_tools
        .contains(&"create_task_proposal".to_string()));
    assert!(ideation
        .preapproved_cli_tools
        .contains(&"Task(Plan)".to_string()));
    assert!(!ideation
        .preapproved_cli_tools
        .contains(&"Task(Explore)".to_string()));

    let worker = parsed
        .agents
        .iter()
        .find(|a| a.name == "ralphx-execution-worker")
        .expect("execution worker should exist");
    assert_eq!(worker.model.as_deref(), Some("sonnet"));
    assert_eq!(worker.permission_mode.as_deref(), Some("acceptEdits"));
    assert!(worker.resolved_cli_tools.contains(&"Write".to_string()));
    assert!(worker.resolved_cli_tools.contains(&"LSP".to_string()));
    assert!(!worker.resolved_cli_tools.contains(&"Task".to_string()));
    assert!(worker.allowed_mcp_tools.contains(&"start_step".to_string()));
    assert!(!worker.preapproved_cli_tools.contains(&"Task".to_string()));

    let qa_executor = parsed
        .agents
        .iter()
        .find(|a| a.name == "ralphx-qa-executor")
        .expect("qa executor should exist");
    assert_eq!(qa_executor.model.as_deref(), Some("sonnet"));
    assert_eq!(qa_executor.permission_mode.as_deref(), Some("acceptEdits"));
    assert!(qa_executor
        .resolved_cli_tools
        .contains(&"Write".to_string()));
    assert_eq!(
        qa_executor.allowed_mcp_tools,
        vec![
            "delegate_start",
            "delegate_wait",
            "delegate_cancel",
            "delegate_park",
        ]
    );
}

#[test]
fn test_codex_config_overlay_overrides_agent_harness_defaults_from_main_config() {
    let yaml = r#"
agent_harness_defaults:
  ideation_primary:
    harness: codex
    model: gpt-5.4
    effort: xhigh
    approval_policy: never
    sandbox_mode: danger-full-access
"#;
    let mut parsed = parse_raw_config(yaml).expect("config should parse");
    let overlay = parse_codex_config_overlay(
        r#"
agent_harness_defaults:
  ideation_primary:
    harness: codex
    model: gpt-5.4-mini
    effort: medium
    approval_policy: on-request
    sandbox_mode: workspace-write
"#,
    )
    .expect("overlay should parse");

    apply_codex_config_overlay(&mut parsed, overlay);
    let parsed = resolve_loaded_config_with_lookup(parsed, &|_| None).expect("config should load");

    let ideation_primary = parsed
        .agent_harness_defaults
        .get(&AgentLane::IdeationPrimary)
        .expect("ideation primary defaults should exist");
    assert_eq!(ideation_primary.harness, AgentHarnessKind::Codex);
    assert_eq!(ideation_primary.model.as_deref(), Some("gpt-5.4-mini"));
    assert_eq!(ideation_primary.effort, Some(LogicalEffort::Medium));
    assert_eq!(
        ideation_primary.approval_policy.as_deref(),
        Some("on-request")
    );
    assert_eq!(
        ideation_primary.sandbox_mode.as_deref(),
        Some("workspace-write")
    );
}

#[test]
fn test_codex_config_overlay_partial_lanes_do_not_clobber_other_agent_harness_defaults() {
    let yaml = r#"
agent_harness_defaults:
  ideation_primary:
    harness: codex
    model: gpt-5.4
    effort: xhigh
    approval_policy: never
    sandbox_mode: danger-full-access
  execution_worker:
    harness: claude
    model: sonnet
"#;
    let mut parsed = parse_raw_config(yaml).expect("config should parse");
    let overlay = parse_codex_config_overlay(
        r#"
agent_harness_defaults:
  ideation_primary:
    harness: codex
    model: gpt-5.4-mini
    effort: medium
    approval_policy: never
    sandbox_mode: danger-full-access
"#,
    )
    .expect("overlay should parse");

    apply_codex_config_overlay(&mut parsed, overlay);
    let parsed = resolve_loaded_config_with_lookup(parsed, &|_| None).expect("config should load");

    let ideation_primary = parsed
        .agent_harness_defaults
        .get(&AgentLane::IdeationPrimary)
        .expect("ideation primary defaults should exist");
    let execution_worker = parsed
        .agent_harness_defaults
        .get(&AgentLane::ExecutionWorker)
        .expect("execution worker defaults should exist");

    assert_eq!(ideation_primary.model.as_deref(), Some("gpt-5.4-mini"));
    assert_eq!(execution_worker.harness, AgentHarnessKind::Claude);
    assert_eq!(execution_worker.model.as_deref(), Some("sonnet"));
}

#[test]
fn test_external_mcp_config_overlay_overrides_main_config_section() {
    let yaml = r#"
external_mcp:
  enabled: false
  port: 3848
  host: "127.0.0.1"
  max_restart_attempts: 3
  restart_delay_ms: 2000
  human_wait_timeout_secs: 285
"#;
    let mut parsed = parse_raw_config(yaml).expect("config should parse");
    let overlay = parse_external_mcp_config_overlay(
        r#"
external_mcp:
  enabled: true
  port: 4949
  host: "0.0.0.0"
  shutdown_grace_ms: 750
"#,
    )
    .expect("overlay should parse");

    apply_external_mcp_config_overlay(&mut parsed, overlay);

    assert!(parsed.external_mcp.enabled);
    assert_eq!(parsed.external_mcp.port, 4949);
    assert_eq!(parsed.external_mcp.host, "0.0.0.0");
    assert_eq!(parsed.external_mcp.max_restart_attempts, 3);
    assert_eq!(parsed.external_mcp.shutdown_grace_ms, 750);
}

#[test]
fn shutdown_watchdog_config_loads_yaml_and_env_override() {
    let loaded = parse_config_with_lookup(
        r#"
shutdown:
  watchdog_deadline_secs: 25
"#,
        &|name| match name {
            "RALPHX_SHUTDOWN_WATCHDOG_DEADLINE_SECS" => Some("35".to_string()),
            _ => None,
        },
    )
    .expect("shutdown config should load");

    assert_eq!(loaded.shutdown.watchdog_deadline_secs, 35);
}

#[test]
fn test_external_mcp_config_overlay_partial_section_does_not_clobber_other_fields() {
    let yaml = r#"
external_mcp:
  enabled: false
  port: 3848
  host: "127.0.0.1"
  max_restart_attempts: 7
  restart_delay_ms: 9000
  human_wait_timeout_secs: 120
  external_message_queue_cap: 25
"#;
    let mut parsed = parse_raw_config(yaml).expect("config should parse");
    let overlay = parse_external_mcp_config_overlay(
        r#"
external_mcp:
  enabled: true
  port: 4949
"#,
    )
    .expect("overlay should parse");

    apply_external_mcp_config_overlay(&mut parsed, overlay);

    assert!(parsed.external_mcp.enabled);
    assert_eq!(parsed.external_mcp.port, 4949);
    assert_eq!(parsed.external_mcp.host, "127.0.0.1");
    assert_eq!(parsed.external_mcp.max_restart_attempts, 7);
    assert_eq!(parsed.external_mcp.restart_delay_ms, 9000);
    assert_eq!(parsed.external_mcp.human_wait_timeout_secs, 120);
    assert_eq!(parsed.external_mcp.external_message_queue_cap, 25);
}

#[test]
fn test_config_harnesses_codex_file_agent_harness_defaults_align_for_codex_lanes() {
    #[derive(Deserialize)]
    struct CodexHarnessConfigMirror {
        #[serde(default)]
        agent_harness_defaults: AgentHarnessDefaultsConfigRaw,
    }

    let yaml_path =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../config/harnesses/codex.yaml");
    let contents =
        std::fs::read_to_string(&yaml_path).expect("should read config/harnesses/codex.yaml");
    let parsed: CodexHarnessConfigMirror =
        serde_yaml::from_str(&contents).expect("should parse config/harnesses/codex.yaml");

    let defaults = default_agent_harness_defaults();
    for lane in [
        AgentLane::IdeationPrimary,
        AgentLane::IdeationVerifier,
        AgentLane::IdeationSubagent,
        AgentLane::IdeationVerifierSubagent,
    ] {
        let expected = defaults
            .get(&lane)
            .cloned()
            .expect("fallback defaults should contain codex lane");
        let actual = parsed
            .agent_harness_defaults
            .get(&lane)
            .cloned()
            .map(AgentLaneSettings::from)
            .expect("config/harnesses/codex.yaml should contain codex lane");
        assert_eq!(
            actual, expected,
            "codex harness config should stay aligned for {lane:?}"
        );
    }
}

#[test]
fn test_agent_harness_defaults_env_overrides_create_and_override_rows() {
    let parsed = parse_config_with_lookup("", &|name| match name {
        "RALPHX_AGENT_HARNESS_EXECUTION_WORKER" => Some("codex".to_string()),
        "RALPHX_AGENT_MODEL_EXECUTION_WORKER" => Some("gpt-5.4".to_string()),
        "RALPHX_AGENT_EFFORT_EXECUTION_WORKER" => Some("xhigh".to_string()),
        "RALPHX_AGENT_APPROVAL_POLICY_EXECUTION_WORKER" => Some("on-request".to_string()),
        "RALPHX_AGENT_SANDBOX_MODE_EXECUTION_WORKER" => Some("workspace-write".to_string()),
        "RALPHX_AGENT_MODEL_IDEATION_VERIFIER" => Some("gpt-5.4-nano".to_string()),
        _ => None,
    })
    .expect("config should parse");

    let execution_worker = parsed
        .agent_harness_defaults
        .get(&AgentLane::ExecutionWorker)
        .expect("execution worker defaults should be created from env");
    assert_eq!(execution_worker.harness, AgentHarnessKind::Codex);
    assert_eq!(execution_worker.model.as_deref(), Some("gpt-5.4"));
    assert_eq!(execution_worker.effort, Some(LogicalEffort::XHigh));
    assert_eq!(
        execution_worker.approval_policy.as_deref(),
        Some("on-request")
    );
    assert_eq!(
        execution_worker.sandbox_mode.as_deref(),
        Some("workspace-write")
    );

    let ideation_verifier = parsed
        .agent_harness_defaults
        .get(&AgentLane::IdeationVerifier)
        .expect("ideation verifier defaults should remain present");
    assert_eq!(ideation_verifier.harness, AgentHarnessKind::Codex);
    assert_eq!(ideation_verifier.model.as_deref(), Some("gpt-5.4-nano"));
}

#[test]
fn test_settings_profile_resolves_prefixed_env_overrides() {
    let mut settings = serde_json::json!({
        "env": {
            "ANTHROPIC_DEFAULT_HAIKU_MODEL": "glm-4.5-air",
            "ANTHROPIC_DEFAULT_SONNET_MODEL": "glm-5",
            "ANTHROPIC_DEFAULT_OPUS_MODEL": "glm-5",
        }
    });

    apply_prefixed_env_overrides_with(&mut settings, None, &|name| match name {
        "RALPHX_ANTHROPIC_DEFAULT_HAIKU_MODEL" => Some("custom-haiku".to_string()),
        "RALPHX_ANTHROPIC_DEFAULT_SONNET_MODEL" => Some("custom-sonnet".to_string()),
        _ => None,
    });

    assert_eq!(
        settings
            .get("env")
            .and_then(|v| v.get("ANTHROPIC_DEFAULT_HAIKU_MODEL"))
            .and_then(|v| v.as_str()),
        Some("custom-haiku")
    );
    assert_eq!(
        settings
            .get("env")
            .and_then(|v| v.get("ANTHROPIC_DEFAULT_SONNET_MODEL"))
            .and_then(|v| v.as_str()),
        Some("custom-sonnet")
    );
    assert_eq!(
        settings
            .get("env")
            .and_then(|v| v.get("ANTHROPIC_DEFAULT_OPUS_MODEL"))
            .and_then(|v| v.as_str()),
        Some("glm-5")
    );
}

#[test]
fn test_profile_specific_env_override_takes_precedence_over_generic() {
    let mut settings = serde_json::json!({
        "env": {
            "ANTHROPIC_AUTH_TOKEN": "yaml-token",
            "ANTHROPIC_BASE_URL": "https://openrouter.ai/api"
        }
    });

    apply_prefixed_env_overrides_with(&mut settings, Some("openrouter"), &|name| match name {
        "RALPHX_OPENROUTER_ANTHROPIC_AUTH_TOKEN" => Some("profile-token".to_string()),
        "RALPHX_ANTHROPIC_AUTH_TOKEN" => Some("generic-token".to_string()),
        _ => None,
    });

    assert_eq!(
        settings
            .get("env")
            .and_then(|v| v.get("ANTHROPIC_AUTH_TOKEN"))
            .and_then(|v| v.as_str()),
        Some("profile-token")
    );
}

#[test]
fn test_profile_specific_env_override_uses_normalized_profile_name() {
    let mut settings = serde_json::json!({
        "env": {
            "ANTHROPIC_AUTH_TOKEN": "yaml-token"
        }
    });

    apply_prefixed_env_overrides_with(&mut settings, Some("z_ai"), &|name| match name {
        "RALPHX_Z_AI_ANTHROPIC_AUTH_TOKEN" => Some("zai-token".to_string()),
        _ => None,
    });

    assert_eq!(
        settings
            .get("env")
            .and_then(|v| v.get("ANTHROPIC_AUTH_TOKEN"))
            .and_then(|v| v.as_str()),
        Some("zai-token")
    );
}

#[test]
fn test_agent_settings_profile_overrides_global_profile() {
    let yaml = r#"
claude:
  mcp_server_name: ralphx
  permission_mode: default
  dangerously_skip_permissions: false
  permission_prompt_tool: permission_request
  settings_profile: z_ai
  settings_profiles:
    default:
      sandbox:
        enabled: false
    z_ai:
      env:
        ANTHROPIC_BASE_URL: https://api.z.ai/api/anthropic
agents:
  - name: ralphx-execution-worker
    settings_profile: default
    tools:
      extends: base_tools
      include: [Write]
    mcp_tools: [get_task_context]
    preapproved_cli_tools: []
    system_prompt_file: plugins/app/agents/worker.md
  - name: ralphx-execution-coder
    tools:
      extends: base_tools
      include: [Write]
    mcp_tools: [get_task_context]
    preapproved_cli_tools: []
    system_prompt_file: plugins/app/agents/coder.md
"#;
    let parsed = parse_config(yaml).expect("config should parse");

    assert!(
        parsed.claude.settings.is_some(),
        "global z_ai should be active"
    );

    let worker = parsed
        .agents
        .iter()
        .find(|a| a.name == "ralphx-execution-worker")
        .expect("worker should exist");
    assert_eq!(
        worker.settings,
        Some(serde_json::json!({
            "sandbox": { "enabled": false }
        })),
        "worker should override to default profile"
    );

    let coder = parsed
        .agents
        .iter()
        .find(|a| a.name == "ralphx-execution-coder")
        .expect("coder should exist");
    assert!(
        coder.settings.is_some(),
        "coder should inherit global z_ai profile"
    );
}

#[test]
fn test_unknown_agent_settings_profile_falls_back_to_global() {
    let yaml = r#"
claude:
  mcp_server_name: ralphx
  permission_mode: default
  dangerously_skip_permissions: false
  permission_prompt_tool: permission_request
  settings_profile: z_ai
  settings_profiles:
z_ai:
  env:
    ANTHROPIC_BASE_URL: https://api.z.ai/api/anthropic
agents:
  - name: ralphx-execution-worker
settings_profile: missing_profile
tools:
  extends: base_tools
  include: [Write]
mcp_tools: [get_task_context]
preapproved_cli_tools: []
system_prompt_file: plugins/app/agents/worker.md
"#;
    let parsed = parse_config(yaml).expect("config should parse");
    let worker = parsed
        .agents
        .iter()
        .find(|a| a.name == "ralphx-execution-worker")
        .expect("worker should exist");
    assert_eq!(
        worker.settings, parsed.claude.settings,
        "unknown agent profile should inherit global settings"
    );
}

#[test]
fn test_runtime_settings_profile_override_reads_env_value() {
    let selection = runtime_settings_profile_override_with(&|name| match name {
        "RALPHX_CLAUDE_SETTINGS_PROFILE" => Some("z_ai".to_string()),
        _ => None,
    });
    assert_eq!(selection.as_deref(), Some("z_ai"));
}

#[test]
fn test_runtime_settings_profile_override_ignores_blank_value() {
    let selection = runtime_settings_profile_override_with(&|name| match name {
        "RALPHX_CLAUDE_SETTINGS_PROFILE" => Some("   ".to_string()),
        _ => None,
    });
    assert_eq!(selection, None);
}

#[test]
fn test_runtime_settings_profile_override_for_agent_uses_normalized_key() {
    let selection =
        runtime_settings_profile_override_for_agent_with("ralphx-ideation", &|name| match name {
            "RALPHX_CLAUDE_SETTINGS_PROFILE_RALPHX_IDEATION" => Some("default".to_string()),
            _ => None,
        });
    assert_eq!(selection.as_deref(), Some("default"));
}

#[test]
fn test_normalize_agent_name_for_env_replaces_symbols() {
    assert_eq!(
        normalize_agent_name_for_env("ralphx:ralphx-utility-session-namer"),
        "RALPHX_RALPHX_UTILITY_SESSION_NAMER"
    );
}

#[test]
fn test_normalize_profile_name_for_env_replaces_symbols() {
    assert_eq!(normalize_profile_name_for_env("z_ai"), "Z_AI");
    assert_eq!(normalize_profile_name_for_env("openrouter"), "OPENROUTER");
}

#[test]
fn test_settings_profile_defaults_apply_to_selected_profile() {
    let yaml = r#"
claude:
  mcp_server_name: ralphx
  permission_mode: default
  dangerously_skip_permissions: false
  permission_prompt_tool: permission_request
  settings_profile: z_ai
  settings_profile_defaults:
    permissions:
      deny:
        - Read(./.env)
  settings_profiles:
    z_ai:
      env:
        ANTHROPIC_BASE_URL: https://api.z.ai/api/anthropic
agents:
  - name: ralphx-execution-worker
    tools:
      extends: base_tools
      include: [Write]
    mcp_tools: [get_task_context]
    preapproved_cli_tools: []
    system_prompt_file: plugins/app/agents/worker.md
"#;
    let parsed = parse_config(yaml).expect("config should parse");
    assert_eq!(
        parsed.claude.settings,
        Some(serde_json::json!({
            "permissions": { "deny": ["Read(./.env)"] },
            "env": { "ANTHROPIC_BASE_URL": "https://api.z.ai/api/anthropic" }
        }))
    );
}

#[test]
fn test_settings_profile_extends_supports_base_profile() {
    let yaml = r#"
claude:
  mcp_server_name: ralphx
  permission_mode: default
  dangerously_skip_permissions: false
  permission_prompt_tool: permission_request
  settings_profile: z_ai
  settings_profiles:
    locked_down:
      permissions:
        deny:
          - Read(./.env)
          - Edit(./.env)
    z_ai:
      extends: locked_down
      env:
        ANTHROPIC_BASE_URL: https://api.z.ai/api/anthropic
agents:
  - name: ralphx-execution-worker
    tools:
      extends: base_tools
      include: [Write]
    mcp_tools: [get_task_context]
    preapproved_cli_tools: []
    system_prompt_file: plugins/app/agents/worker.md
"#;
    let parsed = parse_config(yaml).expect("config should parse");
    assert_eq!(
        parsed.claude.settings,
        Some(serde_json::json!({
            "permissions": {
                "deny": ["Read(./.env)", "Edit(./.env)"]
            },
            "env": {
                "ANTHROPIC_BASE_URL": "https://api.z.ai/api/anthropic"
            }
        }))
    );
}

#[test]
fn test_permission_prompt_tool_keeps_fully_qualified_name() {
    let yaml = r#"
claude:
  mcp_server_name: ralphx
  permission_mode: default
  dangerously_skip_permissions: false
  permission_prompt_tool: mcp__external__permission_prompt
agents:
  - name: ralphx-execution-worker
tools:
  extends: base_tools
  include: [Write]
mcp_tools: [get_task_context]
preapproved_cli_tools: []
system_prompt_file: plugins/app/agents/worker.md
"#;
    let parsed = parse_config(yaml).expect("config should parse");
    assert_eq!(
        parsed.claude.permission_prompt_tool,
        "mcp__external__permission_prompt"
    );
}

#[test]
fn test_mcp_server_name_changes_shorthand_prefix() {
    let yaml = r#"
claude:
  mcp_server_name: acme
  permission_mode: default
  dangerously_skip_permissions: false
  permission_prompt_tool: permission_request
agents:
  - name: ralphx-execution-worker
tools:
  extends: base_tools
  include: [Write]
mcp_tools: [get_task_context]
preapproved_cli_tools: []
system_prompt_file: plugins/app/agents/worker.md
"#;
    let parsed = parse_config(yaml).expect("config should parse");
    assert_eq!(parsed.claude.mcp_server_name, "acme");
    assert_eq!(
        parsed.claude.permission_prompt_tool,
        "mcp__acme__permission_request"
    );
}

#[test]
fn test_memory_maintainer_has_memory_skills() {
    let tools = get_preapproved_tools("ralphx:ralphx-memory-maintainer").unwrap();
    assert!(tools.contains("Skill(ralphx:rule-manager)"));
    assert!(tools.contains("Skill(ralphx:knowledge-capture)"));
}

#[test]
fn test_memory_capture_has_memory_skills() {
    let tools = get_preapproved_tools("ralphx:ralphx-memory-capture").unwrap();
    assert!(tools.contains("Skill(ralphx:rule-manager)"));
    assert!(tools.contains("Skill(ralphx:knowledge-capture)"));
}

#[test]
fn test_non_memory_agents_lack_memory_skills() {
    let agents_to_test = vec![
        "ralphx-execution-worker",
        "ralphx-execution-reviewer",
        "ralphx-execution-orchestrator",
        "ralphx-chat-task",
        "ralphx-chat-project",
    ];
    for agent_name in agents_to_test {
        if let Some(tools) = get_preapproved_tools(agent_name) {
            assert!(
                !tools.contains("Skill(ralphx:rule-manager)"),
                "Agent {} should not have rule-manager skill",
                agent_name
            );
            assert!(
                !tools.contains("Skill(ralphx:knowledge-capture)"),
                "Agent {} should not have knowledge-capture skill",
                agent_name
            );
        }
    }
}

#[test]
fn test_non_memory_agents_lack_memory_write_mcp_tools() {
    // Memory write tools per spec section 11.2
    let memory_write_tools = vec![
        "upsert_memories",
        "mark_memory_obsolete",
        "refresh_memory_rule_index",
        "ingest_rule_file",
        "rebuild_archive_snapshots",
    ];

    let agents_to_test = vec![
        "ralphx-execution-worker",
        "ralphx-execution-reviewer",
        "ralphx-execution-orchestrator",
        "ralphx-chat-task",
        "ralphx-chat-project",
    ];

    for agent_name in agents_to_test {
        if let Some(config) = get_agent_config(agent_name) {
            for write_tool in &memory_write_tools {
                assert!(
                    !config.allowed_mcp_tools.contains(&write_tool.to_string()),
                    "Agent {} should not have write memory tool: {}",
                    agent_name,
                    write_tool
                );
            }
        }
    }
}

#[test]
fn test_memory_agents_have_write_mcp_tools() {
    // Memory maintainer should have write tools
    if let Some(config) = get_agent_config("ralphx-memory-maintainer") {
        assert!(config
            .allowed_mcp_tools
            .contains(&"upsert_memories".to_string()));
        assert!(config
            .allowed_mcp_tools
            .contains(&"mark_memory_obsolete".to_string()));
        assert!(config
            .allowed_mcp_tools
            .contains(&"refresh_memory_rule_index".to_string()));
        assert!(config
            .allowed_mcp_tools
            .contains(&"ingest_rule_file".to_string()));
        assert!(config
            .allowed_mcp_tools
            .contains(&"rebuild_archive_snapshots".to_string()));
    }

    // Memory capture should have upsert_memories
    if let Some(config) = get_agent_config("ralphx-memory-capture") {
        assert!(config
            .allowed_mcp_tools
            .contains(&"upsert_memories".to_string()));
    }
}

#[test]
#[ignore = "memory read tools not yet added to worker/reviewer/orchestrator configs"]
fn test_read_only_agents_have_read_memory_tools() {
    let read_memory_tools = vec!["search_memories", "get_memory", "get_memories_for_paths"];

    let agents_to_test = vec![
        "ralphx-execution-worker",
        "ralphx-execution-reviewer",
        "ralphx-execution-orchestrator",
    ];

    for agent_name in agents_to_test {
        if let Some(config) = get_agent_config(agent_name) {
            // Each of these should have at least one of the read memory tools
            let has_read_tool = read_memory_tools
                .iter()
                .any(|t| config.allowed_mcp_tools.contains(&t.to_string()));
            assert!(
                has_read_tool,
                "Agent {} should have at least one read memory tool",
                agent_name
            );
        }
    }
}

#[test]
fn test_memory_maintainer_has_cli_write_tools() {
    // Memory maintainer must have Write and Edit to update rule files and archives
    if let Some(config) = get_agent_config("ralphx-memory-maintainer") {
        assert!(
            config.preapproved_cli_tools.contains(&"Write".to_string()),
            "ralphx-memory-maintainer must have Write tool"
        );
        assert!(
            config.preapproved_cli_tools.contains(&"Edit".to_string()),
            "ralphx-memory-maintainer must have Edit tool"
        );
        assert!(
            config.preapproved_cli_tools.contains(&"Bash".to_string()),
            "ralphx-memory-maintainer must have Bash tool for file operations"
        );
    }

    // Verify it's not MCP-only
    if let Some(config) = get_agent_config("ralphx-memory-maintainer") {
        assert!(
            !config.mcp_only,
            "ralphx-memory-maintainer should have CLI tools"
        );
    }
}

#[test]
fn test_memory_capture_has_read_cli_tools() {
    // Memory capture needs read tools to analyze conversations and extract memory
    if let Some(config) = get_agent_config("ralphx-memory-capture") {
        assert!(
            config.preapproved_cli_tools.contains(&"Read".to_string()),
            "ralphx-memory-capture must have Read tool"
        );
        assert!(
            config.preapproved_cli_tools.contains(&"Grep".to_string()),
            "ralphx-memory-capture must have Grep tool"
        );
    }

    // Verify it's not MCP-only
    if let Some(config) = get_agent_config("ralphx-memory-capture") {
        assert!(
            !config.mcp_only,
            "ralphx-memory-capture should have CLI tools"
        );
    }
}

#[test]
fn test_agent_workspace_repair_can_fetch_review_artifacts() {
    let config = get_agent_config(SHORT_AGENT_WORKSPACE_REPAIR)
        .expect("ralphx-agent-workspace-repair should exist");

    assert!(
        config
            .allowed_mcp_tools
            .contains(&"complete_agent_workspace_repair".to_string()),
        "repair agent must be able to signal completion"
    );
    assert!(
        config
            .allowed_mcp_tools
            .contains(&"get_artifact".to_string()),
        "repair agent must be able to fetch blocking Review artifacts"
    );
}

// ── Verification tool allowlist tests ───────────────────────────

#[test]
fn test_readonly_agent_has_get_plan_verification_not_update() {
    let config = get_agent_config("ralphx-ideation-readonly")
        .expect("ralphx-ideation-readonly should exist");
    assert!(
        config
            .allowed_mcp_tools
            .contains(&"get_plan_verification".to_string()),
        "readonly agent must include get_plan_verification"
    );
    assert!(
        !config
            .allowed_mcp_tools
            .contains(&"update_plan_verification".to_string()),
        "readonly agent must NOT include update_plan_verification"
    );
}

#[test]
fn test_canonical_agent_capabilities_override_runtime_yaml_mcp_tools_when_present() {
    let yaml = r#"
claude:
  mcp_server_name: ralphx
  permission_mode: default
  dangerously_skip_permissions: false
  permission_prompt_tool: permission_request
agents:
  - name: ralphx-qa-prep
    system_prompt_file: plugins/app/agents/qa-prep.md
    mcp_tools: [wrong_tool]
"#;
    let parsed = parse_config_no_env_overrides(yaml).expect("config should parse");
    let qa_prep = parsed
        .agents
        .iter()
        .find(|a| a.name == "ralphx-qa-prep")
        .expect("qa-prep should exist");

    assert_eq!(
        qa_prep.allowed_mcp_tools,
        vec![
            "fs_read_file",
            "fs_list_dir",
            "fs_grep",
            "fs_glob",
            "delegate_start",
            "delegate_wait",
            "delegate_cancel",
            "delegate_park",
        ]
    );
}

#[test]
fn test_canonical_claude_metadata_overrides_runtime_yaml_preapproved_cli_tools_when_present() {
    let yaml = r#"
claude:
  mcp_server_name: ralphx
  permission_mode: default
  dangerously_skip_permissions: false
  permission_prompt_tool: permission_request
agents:
  - name: ralphx-qa-prep
    system_prompt_file: plugins/app/agents/qa-prep.md
    tools: { extends: base_tools, include: [Task] }
    preapproved_cli_tools: [wrong_tool]
"#;
    let parsed = parse_config_no_env_overrides(yaml).expect("config should parse");
    let qa_prep = parsed
        .agents
        .iter()
        .find(|a| a.name == "ralphx-qa-prep")
        .expect("qa-prep should exist");

    assert_eq!(qa_prep.preapproved_cli_tools, vec!["Task(Plan)"]);
}

#[test]
fn test_canonical_claude_metadata_overrides_runtime_yaml_permission_mode_when_present() {
    let yaml = r#"
claude:
  mcp_server_name: ralphx
  permission_mode: default
  dangerously_skip_permissions: false
  permission_prompt_tool: permission_request
agents:
  - name: ralphx-qa-executor
    system_prompt_file: agents/ralphx-qa-executor/shared/prompt.md
    tools: { extends: base_tools, include: [Write, Edit, Task] }
    permission_mode: default
"#;
    let parsed = parse_config_no_env_overrides(yaml).expect("config should parse");
    let qa_executor = parsed
        .agents
        .iter()
        .find(|a| a.name == "ralphx-qa-executor")
        .expect("qa-executor should exist");

    assert_eq!(qa_executor.permission_mode.as_deref(), Some("acceptEdits"));
}

#[test]
fn test_canonical_claude_metadata_overrides_runtime_yaml_model_when_present() {
    let yaml = r#"
claude:
  mcp_server_name: ralphx
  permission_mode: default
  dangerously_skip_permissions: false
  permission_prompt_tool: permission_request
agents:
  - name: ralphx-qa-prep
    system_prompt_file: agents/ralphx-qa-prep/shared/prompt.md
    tools: { extends: base_tools, include: [Task] }
    model: opus
"#;
    let parsed = parse_config_no_env_overrides(yaml).expect("config should parse");
    let qa_prep = parsed
        .agents
        .iter()
        .find(|a| a.name == "ralphx-qa-prep")
        .expect("qa-prep should exist");

    assert_eq!(qa_prep.model.as_deref(), Some("sonnet"));
}

#[test]
fn test_canonical_claude_metadata_overrides_runtime_yaml_effort_when_present() {
    let yaml = r#"
claude:
  mcp_server_name: ralphx
  permission_mode: default
  dangerously_skip_permissions: false
  permission_prompt_tool: permission_request
agents:
  - name: ralphx-ideation
    system_prompt_file: agents/ralphx-ideation/claude/prompt.md
    tools: { extends: base_tools, include: [Task] }
    effort: high
"#;
    let parsed = parse_config_no_env_overrides(yaml).expect("config should parse");
    let ideation = parsed
        .agents
        .iter()
        .find(|a| a.name == "ralphx-ideation")
        .expect("ralphx-ideation should exist");

    assert_eq!(ideation.effort.as_deref(), Some("max"));
}

#[test]
fn test_canonical_claude_metadata_overrides_runtime_yaml_tools_when_present() {
    let yaml = r#"
claude:
  mcp_server_name: ralphx
  permission_mode: default
  dangerously_skip_permissions: false
  permission_prompt_tool: permission_request
tool_sets:
  base_tools: [Read, Grep, Glob, Bash]
agents:
  - name: ralphx-qa-prep
    system_prompt_file: agents/ralphx-qa-prep/shared/prompt.md
    tools: { mcp_only: true }
"#;
    let parsed = parse_config_no_env_overrides(yaml).expect("config should parse");
    let qa_prep = parsed
        .agents
        .iter()
        .find(|a| a.name == "ralphx-qa-prep")
        .expect("qa-prep should exist");

    assert!(!qa_prep.mcp_only);
    let expected = vec![
        "Read".to_string(),
        "Grep".to_string(),
        "Glob".to_string(),
        "Bash".to_string(),
        "Task".to_string(),
    ];
    assert_eq!(qa_prep.resolved_cli_tools, expected);
}

#[test]
fn test_claude_config_overlay_overrides_unknown_tool_sets_from_main_config() {
    let yaml = r#"
claude:
  mcp_server_name: ralphx
  permission_mode: default
  dangerously_skip_permissions: false
  permission_prompt_tool: permission_request
tool_sets:
  custom_tools: [Read]
agents:
  - name: custom-agent
    system_prompt_file: custom-agent.md
    tools: { extends: custom_tools, include: [Task] }
"#;
    let mut parsed = parse_raw_config(yaml).expect("config should parse");
    let overlay = parse_claude_config_overlay(
        r#"
tool_sets:
  custom_tools: [Write]
"#,
    )
    .expect("overlay should parse");

    apply_claude_config_overlay(&mut parsed, overlay);
    let parsed = resolve_loaded_config_with_lookup(parsed, &|_| None).expect("config should load");
    let custom = parsed
        .agents
        .iter()
        .find(|a| a.name == "custom-agent")
        .expect("custom agent should exist");

    assert_eq!(
        custom.resolved_cli_tools,
        vec!["Write".to_string(), "Task".to_string()]
    );
}

#[test]
fn test_claude_config_overlay_partial_sections_do_not_clobber_other_main_config_sections() {
    let yaml = r#"
claude:
  mcp_server_name: ralphx
  permission_mode: default
  default_effort: high
  dangerously_skip_permissions: false
  permission_prompt_tool: permission_request
tool_sets:
  custom_tools: [Read]
agents:
  - name: custom-agent
    system_prompt_file: custom-agent.md
    tools: { extends: custom_tools, include: [Task] }
"#;
    let mut parsed = parse_raw_config(yaml).expect("config should parse");
    let overlay = parse_claude_config_overlay(
        r#"
claude:
  permission_mode: acceptEdits
"#,
    )
    .expect("overlay should parse");

    apply_claude_config_overlay(&mut parsed, overlay);
    let parsed = resolve_loaded_config_with_lookup(parsed, &|_| None).expect("config should load");

    let custom = parsed
        .agents
        .iter()
        .find(|a| a.name == "custom-agent")
        .expect("custom agent should exist");

    assert_eq!(
        custom.resolved_cli_tools,
        vec!["Read".to_string(), "Task".to_string()]
    );
    assert_eq!(parsed.claude.permission_mode, "acceptEdits");
    assert_eq!(parsed.claude.default_effort, "high");
}

#[test]
fn test_config_harnesses_claude_file_tool_sets_match_embedded_canonical_registry() {
    #[derive(Deserialize)]
    struct ClaudeHarnessConfigMirror {
        #[serde(default)]
        tool_sets: std::collections::HashMap<String, Vec<String>>,
    }

    let yaml_path =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../config/harnesses/claude.yaml");
    let contents =
        std::fs::read_to_string(&yaml_path).expect("should read config/harnesses/claude.yaml");
    let parsed: ClaudeHarnessConfigMirror =
        serde_yaml::from_str(&contents).expect("should parse config/harnesses/claude.yaml");

    for (name, tools) in super::tool_sets::canonical_claude_tool_sets() {
        assert_eq!(
            parsed.tool_sets.get(name),
            Some(tools),
            "config/harnesses/claude.yaml tool_sets.{name} should stay aligned with the embedded canonical Claude tool-set registry"
        );
    }
}

#[test]
fn test_config_harnesses_claude_file_contains_expected_runtime_defaults() {
    #[derive(Deserialize)]
    struct ClaudeHarnessConfigMirror {
        claude: ClaudeRuntimeConfigRaw,
    }

    let yaml_path =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../config/harnesses/claude.yaml");
    let contents =
        std::fs::read_to_string(&yaml_path).expect("should read config/harnesses/claude.yaml");
    let parsed: ClaudeHarnessConfigMirror =
        serde_yaml::from_str(&contents).expect("should parse config/harnesses/claude.yaml");

    assert_eq!(parsed.claude.mcp_server_name, "ralphx");
    assert_eq!(
        parsed.claude.setting_sources,
        Some(vec![
            "user".to_string(),
            "project".to_string(),
            "local".to_string()
        ])
    );
    assert_eq!(parsed.claude.permission_mode, "bypassPermissions");
    assert!(parsed.claude.dangerously_skip_permissions);
    assert!(!parsed.claude.allow_dangerously_skip_permissions);
    assert_eq!(parsed.claude.permission_prompt_tool, "permission_request");
    assert!(parsed.claude.append_system_prompt_file);
    assert_eq!(parsed.claude.settings_profile.as_deref(), Some("default"));
    assert_eq!(parsed.claude.default_effort.as_deref(), Some("medium"));
    assert!(parsed.claude.settings_profiles.contains_key("default"));
}

#[test]
fn test_embedded_config_omits_claude_globals_and_overlay_restores_expected_defaults() {
    let mut parsed = parse_raw_config(EMBEDDED_CONFIG).expect("embedded config should parse");
    let overlay_path =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../config/harnesses/claude.yaml");
    let overlay_contents =
        std::fs::read_to_string(&overlay_path).expect("should read config/harnesses/claude.yaml");
    let overlay =
        parse_claude_config_overlay(&overlay_contents).expect("claude overlay should parse");

    apply_claude_config_overlay(&mut parsed, overlay);
    let parsed = resolve_loaded_config_with_lookup(parsed, &|_| None).expect("config should load");

    assert_eq!(parsed.claude.mcp_server_name, "ralphx");
    assert_eq!(
        parsed.claude.setting_sources,
        Some(vec![
            "user".to_string(),
            "project".to_string(),
            "local".to_string()
        ])
    );
    assert_eq!(parsed.claude.permission_mode, "bypassPermissions");
    assert!(parsed.claude.dangerously_skip_permissions);
    assert!(!parsed.claude.allow_dangerously_skip_permissions);
    assert_eq!(
        parsed.claude.permission_prompt_tool,
        "mcp__ralphx__permission_request"
    );
    assert_eq!(parsed.claude.default_effort, "medium");

    let qa_prep = parsed
        .agents
        .iter()
        .find(|a| a.name == "ralphx-qa-prep")
        .expect("qa-prep should exist");
    assert!(qa_prep.resolved_cli_tools.contains(&"Read".to_string()));
    assert!(qa_prep.resolved_cli_tools.contains(&"Grep".to_string()));
    assert!(qa_prep.resolved_cli_tools.contains(&"Glob".to_string()));
}

#[test]
fn test_config_external_mcp_file_contains_expected_runtime_defaults() {
    #[derive(Deserialize)]
    struct ExternalMcpConfigMirror {
        external_mcp: ExternalMcpConfig,
    }

    let yaml_path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../config/external-mcp.yaml");
    let contents =
        std::fs::read_to_string(&yaml_path).expect("should read config/external-mcp.yaml");
    let parsed: ExternalMcpConfigMirror =
        serde_yaml::from_str(&contents).expect("should parse config/external-mcp.yaml");

    assert!(parsed.external_mcp.enabled);
    assert_eq!(parsed.external_mcp.port, 3848);
    assert_eq!(parsed.external_mcp.host, "127.0.0.1");
    assert_eq!(parsed.external_mcp.max_restart_attempts, 3);
    assert_eq!(parsed.external_mcp.restart_delay_ms, 2000);
    assert_eq!(parsed.external_mcp.human_wait_timeout_secs, 285);
}

#[test]
fn test_embedded_config_omits_external_mcp_and_overlay_restores_expected_defaults() {
    let mut parsed = parse_raw_config(EMBEDDED_CONFIG).expect("embedded config should parse");
    let overlay_path =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../config/external-mcp.yaml");
    let overlay_contents =
        std::fs::read_to_string(&overlay_path).expect("should read config/external-mcp.yaml");
    let overlay = parse_external_mcp_config_overlay(&overlay_contents)
        .expect("external MCP overlay should parse");

    apply_external_mcp_config_overlay(&mut parsed, overlay);
    let parsed = resolve_loaded_config_with_lookup(parsed, &|_| None).expect("config should load");

    assert!(parsed.runtime.external_mcp.enabled);
    assert_eq!(parsed.runtime.external_mcp.port, 3848);
    assert_eq!(parsed.runtime.external_mcp.host, "127.0.0.1");
    assert_eq!(parsed.runtime.external_mcp.max_restart_attempts, 3);
    assert_eq!(parsed.runtime.external_mcp.restart_delay_ms, 2000);
    assert_eq!(parsed.runtime.external_mcp.human_wait_timeout_secs, 285);
}

#[test]
fn test_embedded_external_mcp_overlay_restores_expected_defaults() {
    let mut parsed = parse_raw_config(EMBEDDED_CONFIG).expect("embedded config should parse");
    let overlay = load_embedded_external_mcp_config_overlay()
        .expect("embedded external MCP overlay should parse");

    apply_external_mcp_config_overlay(&mut parsed, overlay);
    let parsed = resolve_loaded_config_with_lookup(parsed, &|_| None).expect("config should load");

    assert!(parsed.runtime.external_mcp.enabled);
    assert_eq!(parsed.runtime.external_mcp.port, 3848);
    assert_eq!(parsed.runtime.external_mcp.host, "127.0.0.1");
}

#[test]
fn test_external_mcp_overlay_file_takes_precedence_over_embedded_defaults() {
    let mut parsed = parse_raw_config(EMBEDDED_CONFIG).expect("embedded config should parse");
    let temp_dir = tempfile::TempDir::new().expect("temp dir");
    let overlay_path = temp_dir.path().join("external-mcp.yaml");
    std::fs::write(
        &overlay_path,
        r#"
external_mcp:
  enabled: false
  port: 4999
  host: "0.0.0.0"
"#,
    )
    .expect("write external MCP overlay");

    apply_external_mcp_overlay_or_embedded_from_path(&mut parsed, &overlay_path);
    let parsed = resolve_loaded_config_with_lookup(parsed, &|_| None).expect("config should load");

    assert!(!parsed.runtime.external_mcp.enabled);
    assert_eq!(parsed.runtime.external_mcp.port, 4999);
    assert_eq!(parsed.runtime.external_mcp.host, "0.0.0.0");
}

#[test]
fn test_external_mcp_overlay_falls_back_to_embedded_defaults_when_file_missing() {
    let mut parsed = parse_raw_config(EMBEDDED_CONFIG).expect("embedded config should parse");
    let temp_dir = tempfile::TempDir::new().expect("temp dir");
    let missing_overlay_path = temp_dir.path().join("missing-external-mcp.yaml");

    apply_external_mcp_overlay_or_embedded_from_path(&mut parsed, &missing_overlay_path);
    let parsed = resolve_loaded_config_with_lookup(parsed, &|_| None).expect("config should load");

    assert!(parsed.runtime.external_mcp.enabled);
    assert_eq!(parsed.runtime.external_mcp.port, 3848);
    assert_eq!(parsed.runtime.external_mcp.host, "127.0.0.1");
}

// ── Agent extends inheritance tests ─────────────────────────────

#[test]
fn test_extends_inherits_parent_tools() {
    let yaml = r#"
claude:
  mcp_server_name: ralphx
  permission_mode: default
  dangerously_skip_permissions: false
  permission_prompt_tool: permission_request
agents:
  - name: base-worker
    system_prompt_file: plugins/app/agents/worker.md
    model: sonnet
    tools: { extends: base_tools, include: [Write, Edit, Task] }
    mcp_tools: [start_step, complete_step]
    preapproved_cli_tools: [Write, Edit, Bash]
  - name: worker-team
    extends: base-worker
    system_prompt_file: plugins/app/agents/worker-team.md
    model: opus
"#;
    let parsed = parse_config(yaml).expect("config should parse");
    let team = parsed
        .agents
        .iter()
        .find(|a| a.name == "worker-team")
        .expect("worker-team should exist");

    // model overridden by child
    assert_eq!(team.model.as_deref(), Some("opus"));
    // system_prompt_file overridden by child
    assert_eq!(team.system_prompt_file, "plugins/app/agents/worker-team.md");
    // tools inherited from parent (child didn't specify)
    assert!(team.resolved_cli_tools.contains(&"Write".to_string()));
    assert!(team.resolved_cli_tools.contains(&"Edit".to_string()));
    assert!(team.resolved_cli_tools.contains(&"Task".to_string()));
    // mcp_tools inherited from parent
    assert!(team.allowed_mcp_tools.contains(&"start_step".to_string()));
    assert!(team
        .allowed_mcp_tools
        .contains(&"complete_step".to_string()));
    // preapproved_cli_tools inherited from parent
    assert!(team.preapproved_cli_tools.contains(&"Write".to_string()));
}

#[test]
fn test_extends_child_overrides_mcp_tools() {
    let yaml = r#"
claude:
  mcp_server_name: ralphx
  permission_mode: default
  dangerously_skip_permissions: false
  permission_prompt_tool: permission_request
agents:
  - name: base-worker
    system_prompt_file: plugins/app/agents/worker.md
    model: sonnet
    tools: { extends: base_tools, include: [Write] }
    mcp_tools: [start_step, complete_step]
    preapproved_cli_tools: [Write]
  - name: custom-worker
    extends: base-worker
    mcp_tools: [get_task_context]
"#;
    let parsed = parse_config(yaml).expect("config should parse");
    let custom = parsed
        .agents
        .iter()
        .find(|a| a.name == "custom-worker")
        .expect("custom-worker should exist");

    // mcp_tools overridden by child
    assert_eq!(custom.allowed_mcp_tools, vec!["get_task_context"]);
    // model inherited
    assert_eq!(custom.model.as_deref(), Some("sonnet"));
    // system_prompt_file inherited
    assert_eq!(custom.system_prompt_file, "plugins/app/agents/worker.md");
}

#[test]
fn test_extends_circular_detection() {
    let yaml = r#"
claude:
  mcp_server_name: ralphx
  permission_mode: default
  dangerously_skip_permissions: false
  permission_prompt_tool: permission_request
agents:
  - name: agent-a
    extends: agent-b
    system_prompt_file: plugins/app/agents/worker.md
  - name: agent-b
    extends: agent-a
    system_prompt_file: plugins/app/agents/worker.md
"#;
    // Raw parsing should preserve the two YAML rows; loaded config now expands the canonical
    // prompt-backed runtime roster on top of that compatibility surface.
    let raw = parse_raw_config(yaml).expect("raw config should parse despite circular extends");
    assert_eq!(raw.agents.len(), 2);

    let parsed = parse_config(yaml).expect("config should load despite circular extends");
    assert!(parsed.agents.iter().any(|agent| agent.name == "agent-a"));
    assert!(parsed.agents.iter().any(|agent| agent.name == "agent-b"));
}

#[test]
fn test_extends_unknown_parent_keeps_child_as_is() {
    let yaml = r#"
claude:
  mcp_server_name: ralphx
  permission_mode: default
  dangerously_skip_permissions: false
  permission_prompt_tool: permission_request
agents:
  - name: orphan-agent
    extends: nonexistent-parent
    system_prompt_file: plugins/app/agents/worker.md
    model: haiku
"#;
    let parsed = parse_config(yaml).expect("config should parse");
    let agent = parsed
        .agents
        .iter()
        .find(|a| a.name == "orphan-agent")
        .expect("orphan-agent should exist");
    assert_eq!(agent.model.as_deref(), Some("haiku"));
}

#[test]
fn test_extends_chained_inheritance() {
    let yaml = r#"
claude:
  mcp_server_name: ralphx
  permission_mode: default
  dangerously_skip_permissions: false
  permission_prompt_tool: permission_request
agents:
  - name: grandparent
    system_prompt_file: plugins/app/agents/worker.md
    model: haiku
    mcp_tools: [tool_a]
    preapproved_cli_tools: [Bash]
  - name: parent
    extends: grandparent
    model: sonnet
    mcp_tools: [tool_b]
  - name: child
    extends: parent
    model: opus
"#;
    let parsed = parse_config(yaml).expect("config should parse");
    let child = parsed
        .agents
        .iter()
        .find(|a| a.name == "child")
        .expect("child should exist");

    // model from child
    assert_eq!(child.model.as_deref(), Some("opus"));
    // mcp_tools from parent (overrides grandparent)
    assert_eq!(child.allowed_mcp_tools, vec!["tool_b"]);
    // system_prompt_file from grandparent (inherited through chain)
    assert_eq!(child.system_prompt_file, "plugins/app/agents/worker.md");
    // preapproved_cli_tools from grandparent
    assert!(child.preapproved_cli_tools.contains(&"Bash".to_string()));
}

#[test]
fn test_no_extends_backward_compatible() {
    // Agents without extends should work exactly as before
    let yaml = r#"
claude:
  mcp_server_name: ralphx
  permission_mode: default
  dangerously_skip_permissions: false
  permission_prompt_tool: permission_request
agents:
  - name: standalone
    system_prompt_file: plugins/app/agents/worker.md
    model: sonnet
    tools: { extends: base_tools, include: [Write] }
    mcp_tools: [get_task_context]
    preapproved_cli_tools: [Write, Bash]
"#;
    let parsed = parse_config(yaml).expect("config should parse");
    let agent = parsed
        .agents
        .iter()
        .find(|a| a.name == "standalone")
        .expect("standalone should exist");
    assert_eq!(agent.model.as_deref(), Some("sonnet"));
    assert!(agent.resolved_cli_tools.contains(&"Write".to_string()));
}

// ── Process mapping integration tests ────────

#[test]
fn test_process_mapping_parsed_from_full_config() {
    let yaml = r#"
claude:
  mcp_server_name: ralphx
  permission_mode: default
  dangerously_skip_permissions: false
  permission_prompt_tool: permission_request
process_mapping:
  execution:
    default: ralphx-execution-worker
  ideation:
    default: ralphx-ideation
agents:
  - name: ralphx-execution-worker
    system_prompt_file: plugins/app/agents/worker.md
    tools: { extends: base_tools, include: [Write] }
    mcp_tools: [get_task_context]
    preapproved_cli_tools: []
"#;
    let parsed = parse_config(yaml).expect("config should parse");
    assert_eq!(
        parsed.process_mapping.slots["execution"].default,
        "ralphx-execution-worker"
    );
    assert_eq!(
        parsed.process_mapping.slots["ideation"].default,
        "ralphx-ideation"
    );
    assert_eq!(
        parsed.process_mapping.slots["review"].default,
        "ralphx-execution-reviewer"
    );
}

#[test]
fn test_missing_process_mapping_uses_canonical_default() {
    let yaml = r#"
claude:
  mcp_server_name: ralphx
  permission_mode: default
  dangerously_skip_permissions: false
  permission_prompt_tool: permission_request
agents:
  - name: ralphx-execution-worker
system_prompt_file: plugins/app/agents/worker.md
tools: { extends: base_tools, include: [Write] }
mcp_tools: [get_task_context]
preapproved_cli_tools: []
"#;
    let parsed = parse_config(yaml).expect("config should parse");
    assert_eq!(
        parsed.process_mapping,
        canonical_process_mapping(),
        "missing process_mapping should resolve to the canonical process mapping"
    );
}

#[test]
fn test_canonical_process_mapping_overrides_divergent_runtime_yaml_slot() {
    let yaml = r#"
claude:
  mcp_server_name: ralphx
  permission_mode: default
  dangerously_skip_permissions: false
  permission_prompt_tool: permission_request
process_mapping:
  execution:
    default: wrong-worker
agents:
  - name: ralphx-execution-worker
    system_prompt_file: plugins/app/agents/worker.md
    tools: { extends: base_tools, include: [Write] }
    mcp_tools: [get_task_context]
    preapproved_cli_tools: []
"#;
    let parsed = parse_config(yaml).expect("config should parse");
    assert_eq!(
        parsed.process_mapping.slots["execution"].default,
        "ralphx-execution-worker"
    );
    assert!(parsed.process_mapping.slots["execution"]
        .variants
        .is_empty());
}

#[test]
fn test_process_config_overlay_overrides_unknown_process_entries_from_main_config() {
    let yaml = r#"
claude:
  mcp_server_name: ralphx
  permission_mode: default
  dangerously_skip_permissions: false
  permission_prompt_tool: permission_request
process_mapping:
  custom_process:
    default: yaml-agent
agents:
  - name: ralphx-execution-worker
    system_prompt_file: plugins/app/agents/worker.md
    tools: { extends: base_tools, include: [Write] }
    mcp_tools: [get_task_context]
    preapproved_cli_tools: []
"#;
    let mut parsed = parse_config(yaml).expect("config should parse");
    let overlay = parse_process_config_overlay(
        r#"
process_mapping:
  custom_process:
    default: overlay-agent
"#,
    )
    .expect("overlay should parse");

    apply_process_config_overlay(&mut parsed, overlay);

    assert_eq!(
        parsed.process_mapping.slots["custom_process"].default,
        "overlay-agent"
    );
}

#[test]
fn test_process_config_overlay_partial_sections_do_not_clobber_other_main_config_sections() {
    let yaml = r#"
claude:
  mcp_server_name: ralphx
  permission_mode: default
  dangerously_skip_permissions: false
  permission_prompt_tool: permission_request
agents:
  - name: ralphx-execution-worker
    system_prompt_file: plugins/app/agents/worker.md
    tools: { extends: base_tools, include: [Write] }
    mcp_tools: [get_task_context]
    preapproved_cli_tools: []
"#;
    let mut parsed = parse_config(yaml).expect("config should parse");
    let overlay = parse_process_config_overlay(
        r#"
process_mapping:
  custom_process:
    default: overlay-agent
"#,
    )
    .expect("overlay should parse");

    apply_process_config_overlay(&mut parsed, overlay);

    assert_eq!(
        parsed.process_mapping.slots["custom_process"].default,
        "overlay-agent"
    );
}

// ==================== Effort Field Tests ====================

#[test]
fn test_effort_field_parsed_from_yaml() {
    let yaml = r#"
claude:
  mcp_server_name: ralphx
  permission_mode: default
  dangerously_skip_permissions: false
  permission_prompt_tool: permission_request
agents:
  - name: rally-agent
    effort: high
    tools:
      extends: base_tools
    mcp_tools: []
    preapproved_cli_tools: []
    system_prompt_file: plugins/app/agents/worker.md
"#;
    let parsed = parse_config(yaml).expect("config should parse");
    let agent = parsed
        .agents
        .iter()
        .find(|a| a.name == "rally-agent")
        .expect("rally-agent should exist");
    assert_eq!(agent.effort, Some("high".to_string()));
}

#[test]
fn test_effort_inheritance_via_extends() {
    let yaml = r#"
claude:
  mcp_server_name: ralphx
  permission_mode: default
  dangerously_skip_permissions: false
  permission_prompt_tool: permission_request
agents:
  - name: parent-agent
    effort: max
    tools:
      extends: base_tools
    mcp_tools: []
    preapproved_cli_tools: []
    system_prompt_file: plugins/app/agents/worker.md
  - name: child-agent
    extends: parent-agent
    system_prompt_file: plugins/app/agents/worker.md
"#;
    let parsed = parse_config(yaml).expect("config should parse");
    let child = parsed
        .agents
        .iter()
        .find(|a| a.name == "child-agent")
        .expect("child-agent should exist");
    assert_eq!(
        child.effort,
        Some("max".to_string()),
        "child should inherit parent's effort: max"
    );
}

#[test]
fn test_effort_child_overrides_parent() {
    let yaml = r#"
claude:
  mcp_server_name: ralphx
  permission_mode: default
  dangerously_skip_permissions: false
  permission_prompt_tool: permission_request
agents:
  - name: parent-agent
    effort: max
    tools:
      extends: base_tools
    mcp_tools: []
    preapproved_cli_tools: []
    system_prompt_file: plugins/app/agents/worker.md
  - name: child-agent
    extends: parent-agent
    effort: high
    system_prompt_file: plugins/app/agents/worker.md
"#;
    let parsed = parse_config(yaml).expect("config should parse");
    let child = parsed
        .agents
        .iter()
        .find(|a| a.name == "child-agent")
        .expect("child-agent should exist");
    assert_eq!(
        child.effort,
        Some("high".to_string()),
        "child's effort: high should override parent's effort: max"
    );
}

#[test]
fn test_resolve_effort_returns_per_agent_effort_for_known_agent() {
    use crate::infrastructure::agents::claude::resolve_effort;
    // ralphx-ideation has effort: max in config/ralphx.yaml
    let effort = resolve_effort(Some("ralphx-ideation"));
    assert_eq!(effort, "max");
}

#[test]
fn test_resolve_effort_returns_global_default_for_unknown_agent() {
    use crate::infrastructure::agents::claude::resolve_effort;
    let effort = resolve_effort(Some("unknown-agent-xyz-that-does-not-exist"));
    assert_eq!(
        effort, "medium",
        "unknown agent should fall back to global default_effort"
    );
}

#[test]
fn test_resolve_effort_returns_global_default_when_none() {
    use crate::infrastructure::agents::claude::resolve_effort;
    let effort = resolve_effort(None);
    assert_eq!(
        effort, "medium",
        "None agent type should return global default_effort"
    );
}

#[test]
fn test_invalid_effort_value_rejected_at_parse_time() {
    let yaml = r#"
claude:
  mcp_server_name: ralphx
  permission_mode: default
  dangerously_skip_permissions: false
  permission_prompt_tool: permission_request
agents:
  - name: test-agent
    effort: turbo
    tools:
      extends: base_tools
    mcp_tools: []
    preapproved_cli_tools: []
    system_prompt_file: plugins/app/agents/worker.md
"#;
    let parsed = parse_config(yaml).expect("config should parse");
    let agent = parsed
        .agents
        .iter()
        .find(|a| a.name == "test-agent")
        .expect("test-agent should exist");
    assert_eq!(
        agent.effort, None,
        "invalid effort value 'turbo' should be rejected (filtered to None)"
    );
}

#[test]
fn test_invalid_global_default_effort_falls_back_to_medium() {
    let yaml = r#"
claude:
  mcp_server_name: ralphx
  permission_mode: default
  dangerously_skip_permissions: false
  permission_prompt_tool: permission_request
  default_effort: turbo
agents:
  - name: test-agent
    tools:
      extends: base_tools
    mcp_tools: []
    preapproved_cli_tools: []
    system_prompt_file: plugins/app/agents/worker.md
"#;
    let parsed = parse_config(yaml).expect("config should parse");
    assert_eq!(
        parsed.claude.default_effort, "medium",
        "invalid global default_effort should fall back to 'medium'"
    );
}

#[test]
fn test_default_effort_carried_through_to_claude_runtime_config() {
    let yaml = r#"
claude:
  mcp_server_name: ralphx
  permission_mode: default
  dangerously_skip_permissions: false
  permission_prompt_tool: permission_request
  default_effort: high
agents:
  - name: test-agent
    tools:
      extends: base_tools
    mcp_tools: []
    preapproved_cli_tools: []
    system_prompt_file: plugins/app/agents/worker.md
"#;
    let parsed = parse_config(yaml).expect("config should parse");
    assert_eq!(
        parsed.claude.default_effort, "high",
        "default_effort should be carried through to ClaudeRuntimeConfig"
    );
}

#[test]
fn test_xhigh_effort_carried_through_to_claude_runtime_config() {
    let yaml = r#"
claude:
  mcp_server_name: ralphx
  permission_mode: default
  dangerously_skip_permissions: false
  permission_prompt_tool: permission_request
  default_effort: xhigh
agents:
  - name: test-agent
    effort: xhigh
    tools:
      extends: base_tools
    mcp_tools: []
    preapproved_cli_tools: []
    system_prompt_file: plugins/app/agents/worker.md
"#;
    let parsed = parse_config(yaml).expect("config should parse");
    let agent = parsed
        .agents
        .iter()
        .find(|agent| agent.name == "test-agent")
        .expect("test-agent should exist");

    assert_eq!(parsed.claude.default_effort, "xhigh");
    assert_eq!(agent.effort.as_deref(), Some("xhigh"));
}

#[test]
fn test_fallback_loaded_config_has_default_effort() {
    // The fallback LoadedConfig (used when embedded config fails to parse) must include
    // default_effort: "medium". We verify the production loaded config has a valid effort value.
    let effort = &claude_runtime_config().default_effort;
    assert!(
        super::VALID_EFFORT_LEVELS.contains(&effort.as_str()),
        "claude_runtime_config().default_effort must be a valid effort level, got: {}",
        effort
    );
}

#[test]
fn test_default_effort_omitted_from_yaml_defaults_to_medium() {
    let yaml = r#"
claude:
  mcp_server_name: ralphx
  permission_mode: default
  dangerously_skip_permissions: false
  permission_prompt_tool: permission_request
agents:
  - name: test-agent
    tools:
      extends: base_tools
    mcp_tools: []
    preapproved_cli_tools: []
    system_prompt_file: plugins/app/agents/worker.md
"#;
    let parsed = parse_config(yaml).expect("config should parse");
    assert_eq!(
        parsed.claude.default_effort, "medium",
        "missing default_effort in YAML should default to 'medium'"
    );
}

// ── Permission mode tests ────────────────────────────────────────

#[test]
fn test_permission_mode_worker_is_accept_edits() {
    let config =
        get_agent_config("ralphx-execution-worker").expect("ralphx-execution-worker should exist");
    assert_eq!(
        config.permission_mode.as_deref(),
        Some("acceptEdits"),
        "ralphx-execution-worker should have acceptEdits permission mode"
    );
}

#[test]
fn test_permission_mode_coder_is_accept_edits() {
    let config =
        get_agent_config("ralphx-execution-coder").expect("ralphx-execution-coder should exist");
    assert_eq!(
        config.permission_mode.as_deref(),
        Some("acceptEdits"),
        "ralphx-execution-coder should have acceptEdits permission mode"
    );
}

#[test]
fn test_permission_mode_merger_is_accept_edits() {
    let config =
        get_agent_config("ralphx-execution-merger").expect("ralphx-execution-merger should exist");
    assert_eq!(
        config.permission_mode.as_deref(),
        Some("acceptEdits"),
        "ralphx-execution-merger should have acceptEdits permission mode"
    );
}

#[test]
fn test_permission_mode_qa_executor_is_accept_edits() {
    let config = get_agent_config("ralphx-qa-executor").expect("ralphx-qa-executor should exist");
    assert_eq!(
        config.permission_mode.as_deref(),
        Some("acceptEdits"),
        "ralphx-qa-executor should have acceptEdits permission mode"
    );
}

#[test]
fn test_permission_mode_memory_maintainer_is_accept_edits() {
    let config = get_agent_config("ralphx-memory-maintainer")
        .expect("ralphx-memory-maintainer should exist");
    assert_eq!(
        config.permission_mode.as_deref(),
        Some("acceptEdits"),
        "ralphx-memory-maintainer should have acceptEdits permission mode"
    );
}

#[test]
fn test_permission_mode_memory_capture_is_accept_edits() {
    let config =
        get_agent_config("ralphx-memory-capture").expect("ralphx-memory-capture should exist");
    assert_eq!(
        config.permission_mode.as_deref(),
        Some("acceptEdits"),
        "ralphx-memory-capture should have acceptEdits permission mode"
    );
}

#[test]
fn test_permission_mode_chat_agent_is_none() {
    // Non-worker agents should NOT have a permission_mode override; they inherit the global mode.
    let config = get_agent_config("ralphx-chat-task").expect("ralphx-chat-task should exist");
    assert_eq!(
        config.permission_mode, None,
        "ralphx-chat-task should not have a per-agent permission_mode override"
    );
}

#[test]
fn test_get_agent_config_accepts_legacy_agent_aliases() {
    let cases = [
        ("orchestrator-ideation", "ralphx-ideation"),
        ("ralphx-worker", "ralphx-execution-worker"),
        ("session-namer", "ralphx-utility-session-namer"),
        ("pr-describer", "ralphx-utility-pr-describer"),
    ];

    for (legacy_name, canonical_name) in cases {
        let config = get_agent_config(legacy_name)
            .unwrap_or_else(|| panic!("legacy alias {legacy_name} should resolve"));
        assert_eq!(config.name, canonical_name);
    }
}

#[test]
fn test_preapproved_tools_always_contains_permission_request() {
    // Every known agent should have permission_request in their preapproved tools
    for agent_name in &[
        "ralphx-execution-worker",
        "ralphx-execution-coder",
        "ralphx-execution-merger",
        "ralphx-utility-session-namer",
        "ralphx-utility-pr-describer",
        "ralphx-chat-task",
    ] {
        let tools = get_preapproved_tools(agent_name).unwrap_or_default();
        assert!(
            tools.contains("mcp__ralphx__permission_request"),
            "Agent {} missing mcp__ralphx__permission_request in preapproved tools: {}",
            agent_name,
            tools
        );
    }
}

// ── UI Feature Flags Config tests ─────────────────────────────────────────────

#[test]
fn test_ui_feature_flags_defaults() {
    let flags = UiFeatureFlagsConfig::default();
    assert!(flags.activity_page, "activity_page should default to true");
    assert!(
        flags.extensibility_page,
        "extensibility_page should default to true"
    );
    assert!(
        flags.automations_page,
        "automations_page should default to true"
    );
    assert!(
        !flags.atlassian_oauth,
        "atlassian_oauth should default to false"
    );
    assert!(
        !flags.ticketing_dashboard,
        "ticketing_dashboard should default to false"
    );
}

#[test]
fn test_ui_config_default_no_feature_flags() {
    let ui = UiConfig::default();
    assert!(
        ui.feature_flags.is_none(),
        "UiConfig::default() should have no feature_flags"
    );
}

#[test]
fn test_yaml_parsing_with_ui_section() {
    let yaml = r#"
ui:
  feature_flags:
    activity_page: false
    extensibility_page: true
    ticketing_dashboard: true
    automations_page: true
"#;
    let cfg = parse_config_no_env_overrides(yaml).expect("should parse yaml with ui section");
    assert!(
        !cfg.runtime.ui_feature_flags.activity_page,
        "activity_page should be false from yaml"
    );
    assert!(
        cfg.runtime.ui_feature_flags.extensibility_page,
        "extensibility_page should be true"
    );
    assert!(
        cfg.runtime.ui_feature_flags.ticketing_dashboard,
        "ticketing_dashboard should be true from yaml"
    );
    assert!(
        cfg.runtime.ui_feature_flags.automations_page,
        "automations_page should be true from yaml"
    );
}

#[test]
fn test_yaml_parsing_ignores_removed_legacy_ui_feature_flags() {
    let yaml = r#"
ui:
  feature_flags:
    activity_page: false
    extensibility_page: false
    automations_page: true
    ticketing_dashboard: true
    ideation_page: false
    battle_mode: false
"#;
    let cfg = parse_config_no_env_overrides(yaml)
        .expect("removed legacy UI feature flags should not break parsing");

    assert!(!cfg.runtime.ui_feature_flags.activity_page);
    assert!(!cfg.runtime.ui_feature_flags.extensibility_page);
    assert!(cfg.runtime.ui_feature_flags.automations_page);
    assert!(cfg.runtime.ui_feature_flags.ticketing_dashboard);
}

#[test]
fn agent_personas_flag_defaults_false_without_config_key() {
    let yaml = r#"
ui:
  feature_flags:
    ticketing_dashboard: true
"#;
    let cfg = parse_config_no_env_overrides(yaml)
        .expect("should parse yaml without the agent_personas feature flag");

    assert!(cfg.runtime.ui_feature_flags.ticketing_dashboard);
    assert!(!cfg.runtime.ui_feature_flags.agent_personas);
    assert!(
        !cfg.runtime
            .ui_feature_flags
            .persona_switch_forces_fresh_provider_session
    );
}

#[test]
fn test_yaml_parsing_without_ui_section_backward_compat() {
    // YAML without ui section: core pages default visible.
    let yaml = r#"
claude:
  mcp_server_name: ralphx
  permission_mode: default
  permission_prompt_tool: permission_request
agents: []
"#;
    let cfg = parse_config_no_env_overrides(yaml).expect("should parse yaml without ui section");
    assert!(
        cfg.runtime.ui_feature_flags.activity_page,
        "should default to true when ui section absent"
    );
    assert!(
        cfg.runtime.ui_feature_flags.extensibility_page,
        "should default to true when ui section absent"
    );
    assert!(
        cfg.runtime.ui_feature_flags.automations_page,
        "automations_page should default to true when ui section absent"
    );
    assert!(
        !cfg.runtime.ui_feature_flags.atlassian_oauth,
        "atlassian_oauth should default to false when ui section absent"
    );
    assert!(
        !cfg.runtime.ui_feature_flags.ticketing_dashboard,
        "ticketing_dashboard should default to false when ui section absent"
    );
}

#[test]
fn test_env_override_activity_page_false() {
    let mut cfg = runtime_config::AllRuntimeConfig {
        stream: runtime_config::StreamTimeoutsConfig::default(),
        reconciliation: runtime_config::ReconciliationConfig::default(),
        git: runtime_config::GitRuntimeConfig::default(),
        scheduler: runtime_config::SchedulerConfig::default(),
        supervisor: runtime_config::SupervisorRuntimeConfig::default(),
        limits: runtime_config::LimitsConfig::default(),
        verification: runtime_config::VerificationConfig::default(),
        delegation: runtime_config::DelegationConfig::default(),
        workspace_review: runtime_config::WorkspaceReviewRuntimeConfig::default(),
        external_mcp: runtime_config::ExternalMcpConfig::default(),
        child_session_activity_threshold_secs: None,
        ui_feature_flags: Default::default(),
        database_maintenance: runtime_config::DatabaseMaintenanceConfig::default(),
    };
    // Start with activity_page enabled (default), apply "false" override
    runtime_config::apply_env_overrides_with_lookup(&mut cfg, &|name| match name {
        "RALPHX_UI_ACTIVITY_PAGE" => Some("false".to_string()),
        _ => None,
    });
    assert!(
        !cfg.ui_feature_flags.activity_page,
        "env override false should disable activity_page"
    );
    assert!(
        cfg.ui_feature_flags.extensibility_page,
        "extensibility_page untouched"
    );
}

#[test]
fn test_env_override_true_value_enables_flag() {
    let mut cfg = runtime_config::AllRuntimeConfig {
        stream: runtime_config::StreamTimeoutsConfig::default(),
        reconciliation: runtime_config::ReconciliationConfig::default(),
        git: runtime_config::GitRuntimeConfig::default(),
        scheduler: runtime_config::SchedulerConfig::default(),
        supervisor: runtime_config::SupervisorRuntimeConfig::default(),
        limits: runtime_config::LimitsConfig::default(),
        verification: runtime_config::VerificationConfig::default(),
        delegation: runtime_config::DelegationConfig::default(),
        workspace_review: runtime_config::WorkspaceReviewRuntimeConfig::default(),
        external_mcp: runtime_config::ExternalMcpConfig::default(),
        child_session_activity_threshold_secs: None,
        database_maintenance: runtime_config::DatabaseMaintenanceConfig::default(),
        ui_feature_flags: UiFeatureFlagsConfig {
            activity_page: false,
            extensibility_page: false,
            automations_page: false,
            atlassian_oauth: false,
            ticketing_dashboard: false,
            agent_personas: false,
            persona_switch_forces_fresh_provider_session: false,
            standalone_conversations: false,
        },
    };
    runtime_config::apply_env_overrides_with_lookup(&mut cfg, &|name| match name {
        "RALPHX_UI_ACTIVITY_PAGE" => Some("true".to_string()),
        "RALPHX_UI_EXTENSIBILITY_PAGE" => Some("1".to_string()),
        _ => None,
    });
    assert!(
        cfg.ui_feature_flags.activity_page,
        "env 'true' should enable activity_page"
    );
    assert!(
        cfg.ui_feature_flags.extensibility_page,
        "env '1' should enable extensibility_page"
    );
    assert!(
        !cfg.ui_feature_flags.automations_page,
        "automations_page untouched"
    );

    runtime_config::apply_env_overrides_with_lookup(&mut cfg, &|name| match name {
        "RALPHX_UI_AUTOMATIONS_PAGE" => Some("1".to_string()),
        _ => None,
    });
    assert!(
        cfg.ui_feature_flags.automations_page,
        "env '1' should enable automations_page"
    );
    assert!(
        !cfg.ui_feature_flags.atlassian_oauth,
        "atlassian_oauth untouched"
    );
    assert!(
        !cfg.ui_feature_flags.ticketing_dashboard,
        "ticketing_dashboard untouched"
    );
}

#[test]
fn test_env_override_atlassian_oauth() {
    let mut cfg = runtime_config::AllRuntimeConfig {
        stream: runtime_config::StreamTimeoutsConfig::default(),
        reconciliation: runtime_config::ReconciliationConfig::default(),
        git: runtime_config::GitRuntimeConfig::default(),
        scheduler: runtime_config::SchedulerConfig::default(),
        supervisor: runtime_config::SupervisorRuntimeConfig::default(),
        limits: runtime_config::LimitsConfig::default(),
        verification: runtime_config::VerificationConfig::default(),
        delegation: runtime_config::DelegationConfig::default(),
        workspace_review: runtime_config::WorkspaceReviewRuntimeConfig::default(),
        external_mcp: runtime_config::ExternalMcpConfig::default(),
        child_session_activity_threshold_secs: None,
        ui_feature_flags: Default::default(),
        database_maintenance: runtime_config::DatabaseMaintenanceConfig::default(),
    };

    runtime_config::apply_env_overrides_with_lookup(&mut cfg, &|name| match name {
        "RALPHX_UI_ATLASSIAN_OAUTH" => Some("true".to_string()),
        _ => None,
    });
    assert!(
        cfg.ui_feature_flags.atlassian_oauth,
        "env 'true' should enable atlassian_oauth"
    );

    runtime_config::apply_env_overrides_with_lookup(&mut cfg, &|name| match name {
        "RALPHX_UI_ATLASSIAN_OAUTH" => Some("false".to_string()),
        _ => None,
    });
    assert!(
        !cfg.ui_feature_flags.atlassian_oauth,
        "env 'false' should disable atlassian_oauth"
    );
}

#[test]
fn test_env_override_ticketing_dashboard() {
    let mut cfg = runtime_config::AllRuntimeConfig {
        stream: runtime_config::StreamTimeoutsConfig::default(),
        reconciliation: runtime_config::ReconciliationConfig::default(),
        git: runtime_config::GitRuntimeConfig::default(),
        scheduler: runtime_config::SchedulerConfig::default(),
        supervisor: runtime_config::SupervisorRuntimeConfig::default(),
        limits: runtime_config::LimitsConfig::default(),
        verification: runtime_config::VerificationConfig::default(),
        delegation: runtime_config::DelegationConfig::default(),
        workspace_review: runtime_config::WorkspaceReviewRuntimeConfig::default(),
        external_mcp: runtime_config::ExternalMcpConfig::default(),
        child_session_activity_threshold_secs: None,
        ui_feature_flags: Default::default(),
        database_maintenance: runtime_config::DatabaseMaintenanceConfig::default(),
    };

    runtime_config::apply_env_overrides_with_lookup(&mut cfg, &|name| match name {
        "RALPHX_UI_TICKETING_DASHBOARD" => Some("true".to_string()),
        _ => None,
    });
    assert!(
        cfg.ui_feature_flags.ticketing_dashboard,
        "env 'true' should enable ticketing_dashboard"
    );

    runtime_config::apply_env_overrides_with_lookup(&mut cfg, &|name| match name {
        "RALPHX_UI_TICKETING_DASHBOARD" => Some("false".to_string()),
        _ => None,
    });
    assert!(
        !cfg.ui_feature_flags.ticketing_dashboard,
        "env 'false' should disable ticketing_dashboard"
    );
}

#[test]
fn test_ui_feature_flags_config_accessor_returns_defaults() {
    // The accessor is backed by OnceLock — just verify it returns a valid struct
    let flags = ui_feature_flags_config();
    // All fields should be bool (any value — loaded from yaml)
    let _ = flags.activity_page;
    let _ = flags.extensibility_page;
    let _ = flags.automations_page;
    let _ = flags.atlassian_oauth;
    let _ = flags.ticketing_dashboard;
}
