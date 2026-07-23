use super::codex_cli_client::CodexCliClient;
use crate::domain::agents::AgentConfig;
use std::path::{Path, PathBuf};

fn create_codex_agent_fixture(root: &Path) -> PathBuf {
    let plugin_dir = root.join("plugins/app");
    let agent_dir = root.join("agents/ralphx-execution-worker");
    std::fs::create_dir_all(agent_dir.join("codex")).expect("create Codex agent fixture");
    std::fs::create_dir_all(agent_dir.join("profiles/skill_distiller/codex"))
        .expect("create Codex profile fixture");
    std::fs::create_dir_all(plugin_dir.join("ralphx-mcp-server/build"))
        .expect("create MCP build fixture");
    std::fs::write(
        agent_dir.join("agent.yaml"),
        "name: ralphx-execution-worker\nrole: execution_worker\nprofiles:\n  skill_distiller:\n    role: skill_distiller\n    capabilities:\n      mcp_tools: [upsert_project_skill, patch_project_skill, retire_project_skill]\n    harnesses:\n      codex:\n        runtime_features:\n          shell_tool: false\n",
    )
    .expect("write shared agent definition");
    std::fs::write(agent_dir.join("codex/agent.yaml"), "runtime_features: {}\n")
        .expect("write Codex agent metadata");
    std::fs::write(
        agent_dir.join("codex/prompt.md"),
        "You are the execution worker.",
    )
    .expect("write Codex prompt");
    std::fs::write(
        agent_dir.join("profiles/skill_distiller/codex/prompt.md"),
        "You are the profile-scoped skill distiller.",
    )
    .expect("write Codex profile prompt");
    std::fs::write(
        plugin_dir.join("ralphx-mcp-server/build/index.js"),
        "// fake MCP server",
    )
    .expect("write MCP fixture");
    plugin_dir
}

fn codex_mcp_args(overrides: &[String]) -> Vec<String> {
    let encoded = overrides
        .iter()
        .find_map(|entry| entry.strip_prefix("mcp_servers.ralphx.args="))
        .expect("Codex MCP args override");
    serde_json::from_str(encoded).expect("JSON encoded Codex MCP args")
}

fn has_arg_pair(args: &[String], flag: &str, value: &str) -> bool {
    args.windows(2)
        .any(|pair| pair[0] == flag && pair[1] == value)
}

#[test]
fn prepare_spawn_reuses_full_context_for_prompt_and_mcp_overrides() {
    let temp_dir = tempfile::tempdir().expect("temp dir");
    let plugin_dir = create_codex_agent_fixture(temp_dir.path());
    let config = AgentConfig::worker("Implement the requested change")
        .with_agent("ralphx:ralphx-execution-worker")
        .with_plugin_dir(plugin_dir)
        .with_working_dir("/trusted/workspace")
        .with_env("RALPHX_PROJECT_ID", "project-1")
        .with_env("RALPHX_CONTEXT_TYPE", "task_execution")
        .with_env("RALPHX_CONTEXT_ID", "context-1")
        .with_env("RALPHX_CONVERSATION_ID", "conversation-1")
        .with_env("RALPHX_AGENT_RUN_ID", "run-1")
        .with_env("RALPHX_TASK_ID", "task-1")
        .with_env("RALPHX_TASK_STATE", "executing")
        .with_env("RALPHX_PARENT_CONVERSATION_ID", "conversation-parent");

    let preparation = CodexCliClient::new()
        .prepare_spawn(&config)
        .expect("Codex spawn preparation");
    let args = codex_mcp_args(&preparation.config_overrides);

    assert!(preparation
        .prompt
        .contains("Implement the requested change"));
    assert!(has_arg_pair(&args, "--context-type", "task_execution"));
    assert!(has_arg_pair(&args, "--context-id", "context-1"));
    assert!(has_arg_pair(&args, "--conversation-id", "conversation-1"));
    assert!(has_arg_pair(&args, "--agent-run-id", "run-1"));
    assert!(has_arg_pair(&args, "--task-id", "task-1"));
    assert!(has_arg_pair(&args, "--task-state", "executing"));
    assert!(has_arg_pair(&args, "--project-id", "project-1"));
    assert!(has_arg_pair(
        &args,
        "--working-directory",
        "/trusted/workspace"
    ));
    assert!(has_arg_pair(
        &args,
        "--parent-conversation-id",
        "conversation-parent"
    ));
    assert!(!has_arg_pair(&args, "--context-id", "task-1"));
}

#[test]
fn prepare_spawn_omits_partial_runtime_identity_without_project_scope() {
    let temp_dir = tempfile::tempdir().expect("temp dir");
    let plugin_dir = create_codex_agent_fixture(temp_dir.path());
    let config = AgentConfig::worker("Implement the requested change")
        .with_agent("ralphx:ralphx-execution-worker")
        .with_plugin_dir(plugin_dir)
        .with_working_dir("/trusted/workspace")
        .with_env("RALPHX_CONTEXT_ID", "context-without-project")
        .with_env("RALPHX_TASK_ID", "task-without-project")
        .with_env("RALPHX_PARENT_CONVERSATION_ID", "conversation-parent");

    let preparation = CodexCliClient::new()
        .prepare_spawn(&config)
        .expect("Codex spawn preparation");
    let args = codex_mcp_args(&preparation.config_overrides);

    for forbidden in [
        "--context-id",
        "--task-id",
        "--project-id",
        "--working-directory",
        "--parent-conversation-id",
    ] {
        assert!(
            !args.iter().any(|arg| arg == forbidden),
            "partial runtime identity leaked through {forbidden}: {args:?}"
        );
    }
}

#[test]
fn prepare_spawn_uses_backend_selected_agent_profile_for_prompt_and_mcp_surface() {
    let temp_dir = tempfile::tempdir().expect("temp dir");
    let plugin_dir = create_codex_agent_fixture(temp_dir.path());
    let config = AgentConfig::worker("Author reusable guidance")
        .with_agent("ralphx:ralphx-execution-worker")
        .with_plugin_dir(plugin_dir)
        .with_working_dir("/trusted/workspace")
        .with_env("RALPHX_PROJECT_ID", "project-1")
        .with_env("RALPHX_AGENT_PROFILE", "skill_distiller");

    let preparation = CodexCliClient::new()
        .prepare_spawn(&config)
        .expect("profile-aware Codex spawn preparation");
    let args = codex_mcp_args(&preparation.config_overrides);

    assert!(preparation
        .prompt
        .contains("You are the profile-scoped skill distiller."));
    assert!(preparation.prompt.contains("Author reusable guidance"));
    assert!(has_arg_pair(&args, "--agent-profile", "skill_distiller"));
}
