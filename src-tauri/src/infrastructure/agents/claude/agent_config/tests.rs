use super::*;
use crate::domain::agents::{AgentHarnessKind, AgentLane, LogicalEffort};
use crate::infrastructure::agents::claude::agent_names::{
    SHORT_CHAT_PROJECT, SHORT_CHAT_TASK, SHORT_CODER, SHORT_DEEP_RESEARCHER,
    SHORT_AGENT_WORKSPACE_REPAIR, SHORT_GENERAL_EXPLORER, SHORT_GENERAL_WORKER,
    SHORT_IDEATION_ADVOCATE, SHORT_IDEATION_CRITIC, SHORT_IDEATION_SPECIALIST_BACKEND,
    SHORT_IDEATION_SPECIALIST_CODE_QUALITY, SHORT_IDEATION_SPECIALIST_FRONTEND,
    SHORT_IDEATION_SPECIALIST_INFRA, SHORT_IDEATION_SPECIALIST_UX, SHORT_IDEATION_TEAM_LEAD,
    SHORT_IDEATION_TEAM_MEMBER, SHORT_MEMORY_CAPTURE, SHORT_MEMORY_MAINTAINER, SHORT_MERGER,
    SHORT_ORCHESTRATOR, SHORT_ORCHESTRATOR_IDEATION, SHORT_ORCHESTRATOR_IDEATION_READONLY,
    SHORT_PLAN_CRITIC_COMPLETENESS, SHORT_PLAN_CRITIC_IMPLEMENTATION_FEASIBILITY,
    SHORT_PLAN_VERIFIER, SHORT_PR_DESCRIBER, SHORT_PROJECT_ANALYZER, SHORT_QA_EXECUTOR,
    SHORT_QA_PREP, SHORT_REVIEWER, SHORT_REVIEW_CHAT, SHORT_REVIEW_HISTORY,
    SHORT_SESSION_NAMER, SHORT_WORKER, SHORT_WORKER_TEAM,
};
use crate::infrastructure::agents::harness_agent_catalog::{
    has_canonical_agent_definition, list_canonical_prompt_backed_agents,
    load_harness_agent_prompt, AgentPromptHarness,
};
use std::collections::HashSet;
use std::path::PathBuf;

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

    let live_agents = list_canonical_prompt_backed_agents(&project_root, AgentPromptHarness::Claude);
    assert!(
        live_agents.contains(&SHORT_ORCHESTRATOR_IDEATION.to_string()),
        "canonical project root should expose live Claude agents for runtime config synthesis"
    );
    assert!(
        live_agents.contains(&SHORT_PLAN_VERIFIER.to_string()),
        "canonical project root should expose verifier agents for runtime config synthesis"
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

    let missing_config_path = temp_dir.path().join("missing").join("config").join("ralphx.yaml");

    assert_eq!(
        canonical_agent_project_root_from_config_path(&missing_config_path, Some(&plugin_dir)),
        runtime_root.canonicalize().expect("canonical runtime root")
    );
}

#[test]
fn test_canonical_agent_project_root_ignores_runtime_plugin_dir_without_agents() {
    let temp_dir = tempfile::TempDir::new().expect("temp dir");
    let plugin_dir = temp_dir.path().join("runtime-root").join("plugins").join("app");
    std::fs::create_dir_all(&plugin_dir).expect("plugin dir");

    let missing_config_path = temp_dir.path().join("missing").join("config").join("ralphx.yaml");

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
        SHORT_AGENT_WORKSPACE_REPAIR,
        SHORT_WORKER,
        SHORT_CODER,
        SHORT_REVIEWER,
        SHORT_QA_PREP,
        SHORT_QA_EXECUTOR,
        SHORT_ORCHESTRATOR,
        SHORT_DEEP_RESEARCHER,
        SHORT_PROJECT_ANALYZER,
        SHORT_MERGER,
        SHORT_MEMORY_MAINTAINER,
        SHORT_MEMORY_CAPTURE,
        // Plan verification critic agents
        SHORT_PLAN_CRITIC_COMPLETENESS,
        SHORT_PLAN_CRITIC_IMPLEMENTATION_FEASIBILITY,
        // Plan verifier agent (owns the verification round loop)
        SHORT_PLAN_VERIFIER,
        // Team lead variants
        SHORT_IDEATION_TEAM_LEAD,
        SHORT_WORKER_TEAM,
        SHORT_IDEATION_TEAM_MEMBER,
        // Ideation specialist agents (spawned by ralphx-ideation-team-lead)
        SHORT_IDEATION_SPECIALIST_BACKEND,
        SHORT_IDEATION_SPECIALIST_FRONTEND,
        SHORT_IDEATION_SPECIALIST_INFRA,
        SHORT_IDEATION_SPECIALIST_UX,
        SHORT_IDEATION_SPECIALIST_CODE_QUALITY,
        SHORT_IDEATION_ADVOCATE,
        SHORT_IDEATION_CRITIC,
        // Prompt quality specialist added in recent commits
        "ralphx-ideation-specialist-prompt-quality",
        // Intent alignment specialist added in recent commits
        "ralphx-ideation-specialist-intent",
        // Pipeline safety specialist added in synthetic-data hardening session
        "ralphx-ideation-specialist-pipeline-safety",
        // State machine safety specialist added in synthetic-data hardening session
        "ralphx-ideation-specialist-state-machine",
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
        if agent.name == SHORT_IDEATION_TEAM_MEMBER {
            continue;
        }
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
    let expected_config =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../config/ralphx.yaml");
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
fn test_live_runtime_agents_no_longer_reference_deprecated_plugin_prompt_paths() {
    for agent in agent_configs() {
        if agent.name == SHORT_IDEATION_TEAM_MEMBER {
            continue;
        }

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
fn test_plan_verifier_prompt_keeps_runtime_failure_and_optional_specialist_rules() {
    let project_root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..");
    let prompt = load_harness_agent_prompt(
        &project_root,
        "ralphx-plan-verifier",
        AgentPromptHarness::Claude,
    )
    .expect("failed to load canonical ralphx-plan-verifier prompt");

    assert!(
        prompt.contains("run_verification_round"),
        "ralphx-plan-verifier prompt must route round execution through the backend-owned round helper"
    );
    assert!(
        prompt.contains("Optional specialists are advisory"),
        "ralphx-plan-verifier prompt must keep optional specialists non-authoritative"
    );
    assert!(
        prompt.contains("Infra/runtime failure is not a plan verdict"),
        "ralphx-plan-verifier prompt must make runtime failure non-authoritative"
    );
    assert!(
        !prompt.contains("Task("),
        "ralphx-plan-verifier prompt should not mention removed provider-specific subagent syntax"
    );
}

#[test]
fn test_plan_verifier_prompt_uses_backend_owned_verification_helpers() {
    let project_root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..");
    let prompt = load_harness_agent_prompt(
        &project_root,
        "ralphx-plan-verifier",
        AgentPromptHarness::Claude,
    )
    .expect("failed to load canonical ralphx-plan-verifier prompt");

    assert!(
        prompt.contains("mcp__ralphx__run_verification_enrichment"),
        "ralphx-plan-verifier prompt must use the backend-owned enrichment helper"
    );
    assert!(
        prompt.contains("mcp__ralphx__run_verification_round"),
        "ralphx-plan-verifier prompt must route full rounds through the backend-owned round helper"
    );
    assert!(
        prompt.contains("round_report"),
        "ralphx-plan-verifier prompt must treat the backend round report as the authoritative next-step signal"
    );
    assert!(
        prompt.contains("treat it as actionable plan feedback: revise the plan first"),
        "ralphx-plan-verifier prompt must revise first on actionable needs_revision feedback"
    );
    assert!(
        prompt.contains("do not call terminal cleanup immediately after an actionable `needs_revision` round report"),
        "ralphx-plan-verifier prompt must not finalize immediately after actionable needs_revision feedback"
    );
    assert!(
        prompt.contains("actionable `needs_revision` is non-terminal until you have a terminal `convergence_reason`"),
        "ralphx-plan-verifier prompt must treat actionable needs_revision as non-terminal until a terminal convergence reason exists"
    );
    assert!(
        prompt.contains("if terminal cleanup rejects actionable `needs_revision` because `convergence_reason` is missing"),
        "ralphx-plan-verifier prompt must tell the model to continue the loop when terminal cleanup rejects actionable needs_revision"
    );
    assert!(
        !prompt.contains("if `round_report.status === \"needs_revision\"` and `round_report.in_progress === false`, finish as `needs_revision`"),
        "ralphx-plan-verifier prompt should not finalize immediately on every terminal needs_revision round report"
    );
    assert!(
        !prompt.contains("created_after"),
        "ralphx-plan-verifier prompt should not supply round-settlement timestamps from prompt memory"
    );
    assert!(
        prompt.contains("omit `session_id`"),
        "ralphx-plan-verifier prompt must rely on backend parent-session injection for verifier-owned helpers"
    );
    assert!(
        !prompt.contains("mcp__ralphx__delegate_start"),
        "ralphx-plan-verifier prompt should not drift back to manual delegate orchestration"
    );
    assert!(
        !prompt.contains("mcp__ralphx__await_verification_round_settlement"),
        "ralphx-plan-verifier prompt should not drift back to low-level settlement choreography"
    );
    assert!(
        !prompt.contains("required_delegates"),
        "ralphx-plan-verifier prompt should not pass required delegate state during terminal cleanup"
    );
    assert!(
        prompt.contains("do not hand-assemble final gaps"),
        "ralphx-plan-verifier prompt must leave terminal gap derivation to the backend helper"
    );
    assert!(
        !prompt.contains("rescue_budget_exhausted"),
        "ralphx-plan-verifier prompt should not pass settlement bookkeeping flags during terminal cleanup"
    );
    assert!(
        !prompt.contains("\"gaps\": <final_merged_gaps>"),
        "ralphx-plan-verifier prompt should not require hand-assembled terminal gaps once the backend helper derives canonical round gaps"
    );
    assert!(
        !prompt.contains("get_session_messages(session_id: <the SESSION_ID value above>)"),
        "ralphx-plan-verifier intent specialist prompt must anchor on the plan goal instead of raw parent chat history"
    );
    assert!(
        !prompt.contains("mcp__ralphx__get_verification_round_artifacts"),
        "ralphx-plan-verifier prompt should not drift back to manual artifact polling"
    );
    assert!(
        !prompt.contains("round_result.gap_counts.critical === 0"),
        "ralphx-plan-verifier prompt should not re-implement zero-blocking convergence in prompt logic"
    );
    assert!(
        !prompt.contains("current_round >= max_rounds"),
        "ralphx-plan-verifier prompt should not re-implement max-round terminal logic in prompt prose"
    );
    assert!(
        !prompt.contains("Task(subagent_type:"),
        "ralphx-plan-verifier prompt should use RalphX-native delegation rather than Claude Task syntax"
    );
    assert!(
        !prompt.contains("max_wait_ms: 8000"),
        "ralphx-plan-verifier prompt should not drift back to tiny required-critic wait budgets"
    );
    assert!(
        !prompt.contains("max_wait_ms: 4000"),
        "ralphx-plan-verifier prompt should not drift back to tiny enrichment wait budgets"
    );
}

#[test]
fn test_ideation_claude_prompt_prioritizes_explicit_reverify_requests() {
    let project_root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..");
    let prompt = load_harness_agent_prompt(
        &project_root,
        "ralphx-ideation",
        AgentPromptHarness::Claude,
    )
    .expect("failed to load canonical ralphx-ideation prompt");

    assert!(
        prompt.contains(
            "If the user explicitly asks to re-run or start a fresh verification round"
        ),
        "ralphx-ideation Claude prompt must prioritize explicit rerun requests over stale terminal verification results"
    );
    assert!(
        prompt.contains(
            "start a fresh verification child immediately instead of summarizing blockers and reopening choices"
        ),
        "ralphx-ideation Claude prompt must not reopen planning choices when the user already requested a rerun"
    );
}

#[test]
fn test_ideation_claude_prompt_keeps_provider_resume_silent_by_default() {
    let project_root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..");
    let prompt = load_harness_agent_prompt(
        &project_root,
        "ralphx-ideation",
        AgentPromptHarness::Claude,
    )
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
  global:
    global_max_concurrent: 28
    global_ideation_max: 5
    allow_ideation_borrow_idle_execution: true
"#;
    let parsed = parse_config_no_env_overrides(yaml).expect("config should parse");

    assert_eq!(parsed.execution_defaults.project.max_concurrent_tasks, 14);
    assert_eq!(parsed.execution_defaults.project.project_ideation_max, 3);
    assert!(!parsed.execution_defaults.project.auto_commit);
    assert!(!parsed.execution_defaults.project.pause_on_failure);
    assert_eq!(parsed.execution_defaults.global.global_max_concurrent, 28);
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
        if agent.name == SHORT_IDEATION_TEAM_MEMBER {
            continue;
        }
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
    assert!(ideation.allowed_mcp_tools.contains(&"create_task_proposal".to_string()));
    assert!(
        ideation
            .preapproved_cli_tools
            .contains(&"Task(Plan)".to_string())
    );
    assert!(
        !ideation
            .preapproved_cli_tools
            .contains(&"Task(Explore)".to_string())
    );

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
    assert!(qa_executor.resolved_cli_tools.contains(&"Write".to_string()));
    assert_eq!(
        qa_executor.allowed_mcp_tools,
        vec!["delegate_start", "delegate_wait", "delegate_cancel"]
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
"#,
    )
    .expect("overlay should parse");

    apply_external_mcp_config_overlay(&mut parsed, overlay);

    assert!(parsed.external_mcp.enabled);
    assert_eq!(parsed.external_mcp.port, 4949);
    assert_eq!(parsed.external_mcp.host, "0.0.0.0");
    assert_eq!(parsed.external_mcp.max_restart_attempts, 3);
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

    let yaml_path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../config/harnesses/codex.yaml");
    let contents = std::fs::read_to_string(&yaml_path).expect("should read config/harnesses/codex.yaml");
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
        assert_eq!(actual, expected, "codex harness config should stay aligned for {lane:?}");
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
fn test_plan_verifier_mcp_tools_match_current_prompt_contract() {
    let config =
        get_agent_config("ralphx-plan-verifier").expect("ralphx-plan-verifier should exist");
    let manifest_dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let codex_prompt = std::fs::read_to_string(
        manifest_dir.join("../agents/ralphx-plan-verifier/codex/prompt.md"),
    )
    .expect("codex verifier prompt should load");
    let claude_prompt = std::fs::read_to_string(
        manifest_dir.join("../agents/ralphx-plan-verifier/claude/prompt.md"),
    )
    .expect("claude verifier prompt should load");

    for tool in [
        "get_session_plan",
        "get_parent_session_context",
        "run_verification_enrichment",
        "run_verification_round",
        "complete_plan_verification",
        "get_plan_verification",
        "update_plan_artifact",
        "edit_plan_artifact",
    ] {
        assert!(
            config.allowed_mcp_tools.contains(&tool.to_string()),
            "ralphx-plan-verifier missing expected MCP tool {tool}"
        );
    }

    assert!(
        !config
            .allowed_mcp_tools
            .contains(&"send_ideation_session_message".to_string()),
        "ralphx-plan-verifier should not be able to send chat nudges to itself or other sessions"
    );

    assert!(
        !config
            .allowed_mcp_tools
            .contains(&"get_session_messages".to_string()),
        "ralphx-plan-verifier should not read raw parent chat history for intent anchoring"
    );

    assert!(
        !config
            .allowed_mcp_tools
            .contains(&"get_verification_round_artifacts".to_string()),
        "ralphx-plan-verifier should not include low-level artifact polling helpers"
    );

    assert!(
        !config
            .allowed_mcp_tools
            .contains(&"run_required_verification_critic_round".to_string()),
        "ralphx-plan-verifier should not include low-level required-critic orchestration helpers"
    );

    assert!(
        !config
            .allowed_mcp_tools
            .contains(&"await_verification_round_settlement".to_string()),
        "ralphx-plan-verifier should not include low-level settlement choreography helpers"
    );

    assert!(
        !config
            .allowed_mcp_tools
            .contains(&"get_team_artifacts".to_string()),
        "ralphx-plan-verifier should not include stale generic team artifact listing tool"
    );

    assert!(
        !config
            .allowed_mcp_tools
            .contains(&"get_artifact".to_string()),
        "ralphx-plan-verifier should not include stale generic artifact fetch tool"
    );

    assert!(
        !config
            .allowed_mcp_tools
            .contains(&"update_plan_verification".to_string()),
        "ralphx-plan-verifier should not include stale generic verification update tool"
    );

    assert!(
        !config
            .allowed_mcp_tools
            .contains(&"get_child_session_status".to_string()),
        "ralphx-plan-verifier should not include stale MCP tool get_child_session_status"
    );

    for prompt in [&codex_prompt, &claude_prompt] {
        assert!(
            prompt.contains(
                "do not wait for child shutdown or terminal cleanup before editing"
            ),
            "verifier prompts should describe in-place revision rather than waiting for child shutdown"
        );
        assert!(
            prompt.contains(
                "do not mention or invent `caller_session_id` or any manual freeze-bypass parameter"
            ),
            "verifier prompts should forbid manual caller_session_id freeze-bypass narration"
        );
    }
}

#[test]
fn test_enrichment_specialist_mcp_tools_match_prompt_contract() {
    let audited_agents = [
        (
            "ralphx-ideation-specialist-code-quality",
            vec![
                "publish_verification_finding",
                "get_session_plan",
                "get_artifact",
            ],
            vec![
                "list_session_proposals",
                "get_proposal",
                "get_parent_session_context",
                "search_memories",
                "get_memory",
                "get_memories_for_paths",
            ],
        ),
        (
            "ralphx-ideation-specialist-prompt-quality",
            vec![
                "publish_verification_finding",
                "get_session_plan",
                "get_artifact",
            ],
            vec![
                "list_session_proposals",
                "get_proposal",
                "get_parent_session_context",
                "search_memories",
                "get_memory",
                "get_memories_for_paths",
            ],
        ),
        (
            "ralphx-ideation-specialist-ux",
            vec![
                "publish_verification_finding",
                "get_session_plan",
                "get_artifact",
            ],
            vec![
                "list_session_proposals",
                "get_proposal",
                "get_parent_session_context",
                "search_memories",
                "get_memory",
                "get_memories_for_paths",
            ],
        ),
        (
            "ralphx-ideation-specialist-pipeline-safety",
            vec![
                "publish_verification_finding",
                "get_session_plan",
                "get_artifact",
            ],
            vec![
                "list_session_proposals",
                "get_proposal",
                "get_parent_session_context",
                "search_memories",
                "get_memory",
                "get_memories_for_paths",
            ],
        ),
        (
            "ralphx-ideation-specialist-state-machine",
            vec![
                "publish_verification_finding",
                "get_session_plan",
                "get_artifact",
            ],
            vec![
                "list_session_proposals",
                "get_proposal",
                "get_parent_session_context",
                "search_memories",
                "get_memory",
                "get_memories_for_paths",
            ],
        ),
        (
            "ralphx-ideation-specialist-intent",
            vec![
                "publish_verification_finding",
                "get_session_plan",
                "get_artifact",
                "get_session_messages",
                "search_memories",
                "get_memory",
                "get_memories_for_paths",
            ],
            vec![
                "list_session_proposals",
                "get_proposal",
                "get_parent_session_context",
            ],
        ),
    ];

    for (agent_name, expected, absent) in audited_agents {
        let config = get_agent_config(agent_name).unwrap_or_else(|| panic!("{agent_name} missing"));
        for tool in expected {
            assert!(
                config.allowed_mcp_tools.contains(&tool.to_string()),
                "{agent_name} missing expected MCP tool {tool}"
            );
        }
        for tool in absent {
            assert!(
                !config.allowed_mcp_tools.contains(&tool.to_string()),
                "{agent_name} should not include stale MCP tool {tool}"
            );
        }
        assert!(
            !config
                .allowed_mcp_tools
                .contains(&"create_team_artifact".to_string()),
            "{agent_name} should use publish_verification_finding instead of create_team_artifact"
        );
        assert!(
            !config
                .allowed_mcp_tools
                .contains(&"get_team_artifacts".to_string()),
            "{agent_name} should stay on typed verification findings instead of get_team_artifacts"
        );
    }
}

#[test]
fn test_plan_critic_mcp_tools_match_prompt_contract() {
    for agent_name in [
        "ralphx-plan-critic-completeness",
        "ralphx-plan-critic-implementation-feasibility",
    ] {
        let config = get_agent_config(agent_name).expect("plan critic should exist");

        for tool in ["get_session_plan", "publish_verification_finding"] {
            assert!(
                config.allowed_mcp_tools.contains(&tool.to_string()),
                "{agent_name} missing expected MCP tool {tool}"
            );
        }

        assert!(
            !config
                .allowed_mcp_tools
                .contains(&"get_team_artifacts".to_string()),
            "{agent_name} should stay bounded and not depend on get_team_artifacts"
        );
        assert!(
            !config
                .allowed_mcp_tools
                .contains(&"create_team_artifact".to_string()),
            "{agent_name} should use publish_verification_finding instead of generic create_team_artifact"
        );
    }
}

#[test]
fn test_plan_critic_prompts_forbid_restatements_of_planned_future_state() {
    let project_root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..");

    for harness in [AgentPromptHarness::Claude, AgentPromptHarness::Codex] {
        for agent_name in [
            "ralphx-plan-critic-completeness",
            "ralphx-plan-critic-implementation-feasibility",
        ] {
            let prompt = load_harness_agent_prompt(&project_root, agent_name, harness)
                .unwrap_or_else(|| panic!("failed to load prompt for {agent_name} ({harness:?})"));

            assert!(
                prompt.contains("Do not restate the before-state as if that absence alone were a plan gap."),
                "{agent_name} ({harness:?}) prompt must explicitly forbid before-state restatement gaps"
            );
            assert!(
                prompt.contains("Bad gap: \"current code does not pass executionPlanId\" when the plan explicitly says to add that wiring."),
                "{agent_name} ({harness:?}) prompt must include a concrete bad-gap example"
            );
            assert!(
                prompt.contains("Good gap: \"the plan never specifies where TaskGraphView gets executionPlanId from\"."),
                "{agent_name} ({harness:?}) prompt must include a concrete good-gap example"
            );
        }
    }
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

    assert_eq!(
        qa_prep.preapproved_cli_tools,
        vec!["Task(Plan)"]
    );
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

    let yaml_path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../config/harnesses/claude.yaml");
    let contents = std::fs::read_to_string(&yaml_path).expect("should read config/harnesses/claude.yaml");
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

    let yaml_path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../config/harnesses/claude.yaml");
    let contents = std::fs::read_to_string(&yaml_path).expect("should read config/harnesses/claude.yaml");
    let parsed: ClaudeHarnessConfigMirror =
        serde_yaml::from_str(&contents).expect("should parse config/harnesses/claude.yaml");

    assert_eq!(parsed.claude.mcp_server_name, "ralphx");
    assert_eq!(parsed.claude.setting_sources, Some(vec![
        "user".to_string(),
        "project".to_string(),
        "local".to_string()
    ]));
    assert_eq!(parsed.claude.permission_mode, "default");
    assert!(!parsed.claude.dangerously_skip_permissions);
    assert_eq!(parsed.claude.permission_prompt_tool, "permission_request");
    assert!(parsed.claude.append_system_prompt_file);
    assert_eq!(parsed.claude.settings_profile.as_deref(), Some("default"));
    assert_eq!(parsed.claude.default_effort.as_deref(), Some("medium"));
    assert!(parsed.claude.settings_profiles.contains_key("default"));
}

#[test]
fn test_embedded_config_omits_claude_globals_and_overlay_restores_expected_defaults() {
    let mut parsed = parse_raw_config(EMBEDDED_CONFIG).expect("embedded config should parse");
    let overlay_path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../config/harnesses/claude.yaml");
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
    assert_eq!(parsed.claude.permission_mode, "default");
    assert_eq!(parsed.claude.permission_prompt_tool, "mcp__ralphx__permission_request");
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
    let overlay_path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../config/external-mcp.yaml");
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

// ── Process mapping + team constraints integration tests ────────

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
    team: ralphx-execution-team-lead
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
        parsed.process_mapping.slots["execution"]
            .variants
            .get("team")
            .unwrap(),
        "ralphx-execution-team-lead"
    );
    assert_eq!(
        parsed.process_mapping.slots["review"].default,
        "ralphx-execution-reviewer"
    );
}

#[test]
fn test_team_constraints_parsed_from_full_config() {
    let yaml = r#"
claude:
  mcp_server_name: ralphx
  permission_mode: default
  dangerously_skip_permissions: false
  permission_prompt_tool: permission_request
team_constraints:
  _defaults:
    max_teammates: 5
    model_cap: sonnet
  execution:
    max_teammates: 3
    mode: dynamic
    timeout_minutes: 30
  custom_process:
    max_teammates: 2
    model_cap: haiku
agents:
  - name: ralphx-execution-worker
    system_prompt_file: plugins/app/agents/worker.md
    tools: { extends: base_tools, include: [Write] }
    mcp_tools: [get_task_context]
    preapproved_cli_tools: []
"#;
    let parsed = parse_config(yaml).expect("config should parse");
    let defaults = parsed.team_constraints.defaults.as_ref().unwrap();
    assert_eq!(defaults.max_teammates, 5);
    let exec = &parsed.team_constraints.processes["execution"];
    assert_eq!(exec.max_teammates, 5);
    assert_eq!(exec.timeout_minutes, 30);
    assert_eq!(parsed.team_constraints.processes["custom_process"].model_cap, "haiku");
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
    assert_eq!(
        parsed.team_constraints,
        canonical_team_constraints_config(),
        "missing team_constraints should resolve to the canonical team constraints"
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
    assert_eq!(
        parsed.process_mapping.slots["execution"]
            .variants
            .get("team")
            .map(String::as_str),
        Some("ralphx-execution-team-lead")
    );
}

#[test]
fn test_canonical_team_constraints_override_divergent_runtime_yaml_process() {
    let yaml = r#"
claude:
  mcp_server_name: ralphx
  permission_mode: default
  dangerously_skip_permissions: false
  permission_prompt_tool: permission_request
team_constraints:
  execution:
    max_teammates: 1
    model_cap: haiku
agents:
  - name: ralphx-execution-worker
    system_prompt_file: plugins/app/agents/worker.md
    tools: { extends: base_tools, include: [Write] }
    mcp_tools: [get_task_context]
    preapproved_cli_tools: []
"#;
    let parsed = parse_config(yaml).expect("config should parse");
    let execution = &parsed.team_constraints.processes["execution"];
    assert_eq!(execution.max_teammates, 5);
    assert_eq!(execution.model_cap, "sonnet");
    assert_eq!(execution.timeout_minutes, 30);
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
team_constraints:
  custom_process:
    max_teammates: 2
    model_cap: haiku
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
team_constraints:
  custom_process:
    max_teammates: 4
    model_cap: opus
"#,
    )
    .expect("overlay should parse");

    apply_process_config_overlay(&mut parsed, overlay);

    assert_eq!(
        parsed.process_mapping.slots["custom_process"].default,
        "overlay-agent"
    );
    assert_eq!(parsed.team_constraints.processes["custom_process"].max_teammates, 4);
    assert_eq!(parsed.team_constraints.processes["custom_process"].model_cap, "opus");
}

#[test]
fn test_process_config_overlay_partial_sections_do_not_clobber_other_main_config_sections() {
    let yaml = r#"
claude:
  mcp_server_name: ralphx
  permission_mode: default
  dangerously_skip_permissions: false
  permission_prompt_tool: permission_request
team_constraints:
  custom_process:
    max_teammates: 2
    model_cap: haiku
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
    assert_eq!(parsed.team_constraints.processes["custom_process"].model_cap, "haiku");
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
fn test_permission_mode_worker_team_inherits_accept_edits() {
    let config = get_agent_config("ralphx-execution-team-lead")
        .expect("ralphx-execution-team-lead should exist");
    assert_eq!(
        config.permission_mode.as_deref(),
        Some("acceptEdits"),
        "ralphx-execution-team-lead should have acceptEdits (inherited or explicit)"
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
    // Non-worker agents should NOT have a permission_mode override (inherits global "default")
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
        ("plan-verifier", "ralphx-plan-verifier"),
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
fn test_ui_feature_flags_default_all_enabled() {
    let flags = UiFeatureFlagsConfig::default();
    assert!(flags.activity_page, "activity_page should default to true");
    assert!(
        flags.extensibility_page,
        "extensibility_page should default to true"
    );
    assert!(flags.battle_mode, "battle_mode should default to true");
    assert!(!flags.team_mode, "team_mode should default to false");
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
}

#[test]
fn test_yaml_parsing_without_ui_section_backward_compat() {
    // YAML without ui section: defaults to all flags enabled
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
        cfg.runtime.ui_feature_flags.battle_mode,
        "should default to true when ui section absent"
    );
    assert!(
        !cfg.runtime.ui_feature_flags.team_mode,
        "team_mode should default to false when ui section absent"
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
        external_mcp: runtime_config::ExternalMcpConfig::default(),
        child_session_activity_threshold_secs: None,
        ui_feature_flags: Default::default(),
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
        external_mcp: runtime_config::ExternalMcpConfig::default(),
        child_session_activity_threshold_secs: None,
        ui_feature_flags: UiFeatureFlagsConfig {
            activity_page: false,
            extensibility_page: false,
            battle_mode: false,
            team_mode: false,
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
    assert!(!cfg.ui_feature_flags.team_mode, "team_mode untouched");
}

#[test]
fn test_env_override_battle_mode() {
    let mut cfg = runtime_config::AllRuntimeConfig {
        stream: runtime_config::StreamTimeoutsConfig::default(),
        reconciliation: runtime_config::ReconciliationConfig::default(),
        git: runtime_config::GitRuntimeConfig::default(),
        scheduler: runtime_config::SchedulerConfig::default(),
        supervisor: runtime_config::SupervisorRuntimeConfig::default(),
        limits: runtime_config::LimitsConfig::default(),
        verification: runtime_config::VerificationConfig::default(),
        external_mcp: runtime_config::ExternalMcpConfig::default(),
        child_session_activity_threshold_secs: None,
        ui_feature_flags: Default::default(), // battle_mode defaults to true
    };
    // Override battle_mode to false
    runtime_config::apply_env_overrides_with_lookup(&mut cfg, &|name| match name {
        "RALPHX_UI_BATTLE_MODE" => Some("false".to_string()),
        _ => None,
    });
    assert!(
        !cfg.ui_feature_flags.battle_mode,
        "env 'false' should disable battle_mode"
    );
    assert!(
        cfg.ui_feature_flags.activity_page,
        "activity_page untouched"
    );
    assert!(
        cfg.ui_feature_flags.extensibility_page,
        "extensibility_page untouched"
    );
    assert!(!cfg.ui_feature_flags.team_mode, "team_mode untouched");

    // Override battle_mode to true via "1"
    cfg.ui_feature_flags.battle_mode = false;
    runtime_config::apply_env_overrides_with_lookup(&mut cfg, &|name| match name {
        "RALPHX_UI_BATTLE_MODE" => Some("1".to_string()),
        _ => None,
    });
    assert!(
        cfg.ui_feature_flags.battle_mode,
        "env '1' should enable battle_mode"
    );
}

#[test]
fn test_env_override_team_mode() {
    let mut cfg = runtime_config::AllRuntimeConfig {
        stream: runtime_config::StreamTimeoutsConfig::default(),
        reconciliation: runtime_config::ReconciliationConfig::default(),
        git: runtime_config::GitRuntimeConfig::default(),
        scheduler: runtime_config::SchedulerConfig::default(),
        supervisor: runtime_config::SupervisorRuntimeConfig::default(),
        limits: runtime_config::LimitsConfig::default(),
        verification: runtime_config::VerificationConfig::default(),
        external_mcp: runtime_config::ExternalMcpConfig::default(),
        child_session_activity_threshold_secs: None,
        ui_feature_flags: Default::default(),
    };

    runtime_config::apply_env_overrides_with_lookup(&mut cfg, &|name| match name {
        "RALPHX_UI_TEAM_MODE" => Some("true".to_string()),
        _ => None,
    });
    assert!(cfg.ui_feature_flags.team_mode, "env 'true' should enable team_mode");

    runtime_config::apply_env_overrides_with_lookup(&mut cfg, &|name| match name {
        "RALPHX_UI_TEAM_MODE" => Some("false".to_string()),
        _ => None,
    });
    assert!(!cfg.ui_feature_flags.team_mode, "env 'false' should disable team_mode");
}

#[test]
fn test_ui_feature_flags_config_accessor_returns_defaults() {
    // The accessor is backed by OnceLock — just verify it returns a valid struct
    let flags = ui_feature_flags_config();
    // All fields should be bool (any value — loaded from yaml)
    let _ = flags.activity_page;
    let _ = flags.extensibility_page;
    let _ = flags.battle_mode;
    let _ = flags.team_mode;
}
