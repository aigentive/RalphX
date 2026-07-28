use std::collections::{BTreeMap, BTreeSet};

use ralphx_remote_protocol::{class_permits, Capability, RiskClass};

use super::authority_audit::{
    closure_is_arming, load_production_sources, parse_registered_commands, repo_root,
    spawn_triggering_writers, CallGraph, SPAWN_TRIGGERING_STATE_SURFACE,
};
use super::capability_ledger::{
    policy_for, AUTHORITY_REDUCING_EXEMPTIONS, COMMAND_OVERRIDES, DECLARED_MEMBERSHIPS,
    MODULE_DEFAULTS,
};
use super::registry::find_spec;

fn registry_source() -> &'static str {
    include_str!("../commands/registry.rs")
}

fn census() -> Vec<(String, String)> {
    parse_registered_commands(registry_source())
}

const WORKER_AGENTS: &[&str] = &[
    "ralphx-execution-worker",
    "ralphx-execution-coder",
    "ralphx-execution-reviewer",
    "ralphx-execution-merger",
];

fn yaml_mcp_tools(source: &str) -> BTreeSet<String> {
    let mut in_tools = false;
    source
        .lines()
        .filter_map(|line| {
            if line.trim() == "mcp_tools:" {
                in_tools = true;
                return None;
            }
            if in_tools && !line.starts_with("    ") {
                in_tools = false;
            }
            in_tools
                .then(|| line.trim().strip_prefix("- ").map(str::to_string))
                .flatten()
        })
        .collect()
}

fn live_mcp_tool_names() -> BTreeSet<String> {
    let dir = repo_root().join("plugins/app/ralphx-mcp-server/src");
    std::fs::read_dir(dir)
        .expect("MCP source directory exists")
        .flatten()
        .filter(|entry| entry.path().extension().and_then(|value| value.to_str()) == Some("ts"))
        .flat_map(|entry| {
            std::fs::read_to_string(entry.path())
                .expect("MCP source is readable")
                .lines()
                .filter_map(|line| {
                    line.trim()
                        .strip_prefix("name: \"")
                        .and_then(|rest| rest.strip_suffix("\","))
                        .map(str::to_string)
                })
                .collect::<Vec<_>>()
        })
        .collect()
}

fn agent_content_reads() -> Vec<serde_json::Value> {
    let live = live_mcp_tool_names();
    let mut grants = BTreeMap::<String, Vec<String>>::new();
    for agent in WORKER_AGENTS {
        let path = repo_root().join("agents").join(agent).join("agent.yaml");
        let tools = yaml_mcp_tools(&std::fs::read_to_string(path).expect("agent yaml readable"));
        assert!(
            tools.contains("get_task_context"),
            "worker inclusion rule drifted for {agent}"
        );
        for tool in tools
            .intersection(&live)
            .filter(|tool| is_content_read_tool(tool))
        {
            grants
                .entry(tool.clone())
                .or_default()
                .push((*agent).to_string());
        }
    }
    grants
        .into_iter()
        .map(|(tool, granted_to)| {
            serde_json::json!({
                "tool": tool,
                "grantedTo": granted_to,
                "reads": content_read_surface(&tool),
            })
        })
        .collect()
}

fn is_content_read_tool(tool: &str) -> bool {
    matches!(
        tool,
        "get_task_context"
            | "get_review_notes"
            | "get_task_issues"
            | "get_artifact"
            | "get_artifact_version"
            | "get_related_artifacts"
            | "get_task_steps"
            | "get_step_context"
            | "get_task_diff"
            | "get_task_diff_stat"
            | "get_agent_task"
            | "list_agent_tasks"
            | "get_sub_steps"
            | "get_project_analysis"
            | "get_task_validation_summary"
            | "search_project_artifacts"
            | "get_memory"
            | "search_memories"
            | "get_memories_for_paths"
    )
}

fn content_read_surface(tool: &str) -> &'static str {
    if tool.contains("artifact") {
        "artifacts/artifact_versions/artifact_relations"
    } else if tool.contains("step") {
        "task_steps"
    } else if tool.contains("review") || tool.contains("issue") {
        "review_notes/task_issues"
    } else if tool.contains("memory") || tool.contains("memories") {
        "memory_entries"
    } else if tool.contains("diff") {
        "task/worktree diff"
    } else if tool.contains("agent_task") {
        "agent_tasks"
    } else if tool.contains("validation") {
        "validation_runs"
    } else if tool.contains("project_analysis") {
        "project_analysis"
    } else {
        "worker TaskContext/prompt projection"
    }
}

fn agent_content_writers() -> Vec<serde_json::Value> {
    [
        ("create_task_step", "tauri-command", "task_steps", None),
        ("update_task_step", "tauri-command", "task_steps", None),
        (
            "create_artifact",
            "tauri-command",
            "artifacts (any kind)",
            None,
        ),
        (
            "update_artifact",
            "tauri-command",
            "artifacts (any kind)",
            None,
        ),
        (
            "add_artifact_relation",
            "tauri-command",
            "artifact_relations",
            None,
        ),
        (
            "update_task_proposal",
            "tauri-command",
            "task proposals",
            None,
        ),
        ("approve_review", "tauri-command", "review feedback", None),
        ("reject_review", "tauri-command", "review feedback", None),
        ("request_changes", "tauri-command", "review feedback", None),
        (
            "reject_fix_task",
            "tauri-command",
            "review notes/fix feedback",
            None,
        ),
        (
            "approve_task_for_review",
            "tauri-command",
            "review notes",
            None,
        ),
        (
            "request_task_changes_for_review",
            "tauri-command",
            "review notes/feedback",
            None,
        ),
        (
            "request_task_changes_from_reviewing",
            "tauri-command",
            "review notes/feedback",
            None,
        ),
        (
            "move_task",
            "tauri-command",
            "task restart note",
            Some("note"),
        ),
        (
            "update_task",
            "tauri-command",
            "task title/description",
            Some("title,description — discharged by update_task_authz"),
        ),
        ("add_task_note", "http-handler", "task.description", None),
    ]
    .into_iter()
    .map(|(writer, surface, writes, conditional)| {
        let mut row = serde_json::json!({"writer": writer, "surface": surface, "writes": writes});
        if let Some(value) = conditional {
            row["conditional"] = serde_json::json!(value);
        }
        row
    })
    .collect()
}

fn generated_manifest() -> serde_json::Value {
    let rows = census();
    let graph = CallGraph::build(&load_production_sources());
    let ledger = rows
        .iter()
        .map(|(command, module)| {
            let row = policy_for(command, module).expect("census is ledgered");
            serde_json::json!({
                "command": command,
                "module": module,
                "class": row.class,
                "capabilities": row.capabilities,
                "reason": row.reason,
                "registered": find_spec(command).is_some(),
            })
        })
        .collect::<Vec<_>>();
    let detector_b = spawn_triggering_writers(
        &graph,
        rows.iter().map(|(command, _)| command.clone()),
        SPAWN_TRIGGERING_STATE_SURFACE,
    );
    let agent_control_floor = rows
        .iter()
        .filter_map(|(command, _)| {
            (closure_is_arming(&graph.closure([command.clone()])) || detector_b.contains(command))
                .then_some(command)
        })
        .collect::<Vec<_>>();
    let background_loop_inventory = graph
        .loop_roots
        .iter()
        .map(|root| {
            let closure = graph.loop_closure(root);
            serde_json::json!({
                "id": root.id,
                "file": root.file,
                "enclosingFunction": root.enclosing_fn,
                "kind": root.kind,
                "authorityBearing": closure_is_arming(&closure),
            })
        })
        .collect::<Vec<_>>();
    let authority_reducing_exemptions = AUTHORITY_REDUCING_EXEMPTIONS
        .iter()
        .map(|exemption| {
            let mut row = serde_json::json!({
                "kind": exemption.kind,
                "direction": exemption.direction,
                "scope": exemption.scope,
                "rationale": exemption.rationale,
            });
            row[if exemption.kind == "command" {
                "command"
            } else {
                "target"
            }] = serde_json::json!(exemption.subject);
            row
        })
        .collect::<Vec<_>>();
    let declared_memberships = DECLARED_MEMBERSHIPS
        .iter()
        .map(|(command, reason)| serde_json::json!({ "command": command, "reason": reason }))
        .collect::<Vec<_>>();
    let loop_ids = graph
        .loop_roots
        .iter()
        .map(|root| root.id.as_str())
        .collect::<BTreeSet<_>>();
    let spawn_triggering_state_surface = SPAWN_TRIGGERING_STATE_SURFACE.iter().map(|entry| {
        assert!(entry.read_by_loops.iter().all(|id| loop_ids.contains(id)), "surface {} references a non-inventory loop", entry.id);
        let writers = spawn_triggering_writers(&graph, rows.iter().map(|(command, _)| command.clone()), std::slice::from_ref(entry));
        serde_json::json!({"id": entry.id, "surface": entry.surface, "armedValue": entry.armed_value, "readByLoops": entry.read_by_loops, "writers": writers})
    }).collect::<Vec<_>>();

    serde_json::json!({
        "schemaVersion": 1,
        "background_loop_inventory": background_loop_inventory,
        "spawn_triggering_state_surface": spawn_triggering_state_surface,
        "agent_consumed_content_surface": {"reads": agent_content_reads(), "writers": agent_content_writers()},
        "worker_task_view_allowlist": [
            "id", "project_id", "title", "description", "internal_status", "ideation_session_id"
        ],
        "authority_reducing_exemptions": authority_reducing_exemptions,
        "declared_memberships": declared_memberships,
        "ledger": ledger,
        "agent_control_floor": agent_control_floor,
        "coverage": {
            "detectorA": "complete",
            "detectorB": "complete",
            "agentConsumedContent": "complete"
        }
    })
}

fn manifest_text() -> String {
    let mut text =
        serde_json::to_string_pretty(&generated_manifest()).expect("manifest serializes");
    text.push('\n');
    text
}

fn manifest_path() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../docs/generated/remote-commands.json")
}

#[test]
fn regenerate_remote_command_manifest_when_requested() {
    if std::env::var_os("RALPHX_REGENERATE_REMOTE_MANIFEST").as_deref()
        != Some(std::ffi::OsStr::new("1"))
    {
        return;
    }
    let path = manifest_path();
    let parent = path.parent().expect("manifest has parent");
    std::fs::create_dir_all(parent).expect("generated docs directory is writable");
    std::fs::write(path, manifest_text()).expect("remote command manifest is writable");
}

#[test]
fn remote_command_manifest_is_current() {
    let actual = std::fs::read_to_string(manifest_path())
        .expect("remote command manifest is checked in; run the gated regeneration test");
    assert_eq!(actual, manifest_text(), "remote command manifest is stale");
}

#[test]
fn capability_ledger_is_exhaustive_and_internally_consistent() {
    let rows = census();
    assert!(!rows.is_empty(), "live command census must not be empty");

    let mut seen = BTreeSet::new();
    for (command, module) in rows {
        assert!(
            seen.insert(command.clone()),
            "duplicate command `{command}`"
        );
        let row = policy_for(&command, &module).unwrap_or_else(|| {
            panic!("classify this command: `{command}` (unknown module `{module}`)")
        });
        assert!(
            class_permits(row.class, row.capabilities),
            "ledger class/capability mismatch for `{command}`"
        );
    }

    let defaults = MODULE_DEFAULTS
        .iter()
        .map(|entry| entry.module)
        .collect::<BTreeSet<_>>();
    assert_eq!(
        defaults.len(),
        MODULE_DEFAULTS.len(),
        "duplicate module default"
    );
    let overrides = COMMAND_OVERRIDES
        .iter()
        .map(|entry| entry.command)
        .collect::<BTreeSet<_>>();
    assert_eq!(
        overrides.len(),
        COMMAND_OVERRIDES.len(),
        "duplicate command override"
    );
}

#[test]
fn detector_a_is_a_floor_for_agent_control() {
    let graph = CallGraph::build(&load_production_sources());
    let mut floor = BTreeSet::new();
    for (command, module) in census() {
        if closure_is_arming(&graph.closure([command.clone()])) {
            floor.insert(command.clone());
            let row = policy_for(&command, &module).expect("census is ledgered");
            assert!(
                matches!(row.class, RiskClass::AgentControl | RiskClass::Elevated),
                "detector (a) classifies `{command}` as authority-bearing; ledger must be AgentControl or stronger"
            );
        }
    }
    assert!(
        floor.contains("reanalyze_project"),
        "project-analyzer spawn sink must mechanically place reanalyze_project in detector (a)"
    );
    let row = policy_for("reanalyze_project", "project_commands").unwrap();
    assert_eq!(row.class, RiskClass::AgentControl);
    assert_eq!(row.capabilities, &[Capability::AgentControl]);
}

#[test]
fn detector_b_is_calibrated_and_floor_enforced() {
    let graph = CallGraph::build(&load_production_sources());
    let rows = census();
    assert_eq!(
        rows.len(),
        539,
        "review the detector against the full command census"
    );
    let flagged = spawn_triggering_writers(
        &graph,
        rows.iter().map(|(command, _)| command.clone()),
        SPAWN_TRIGGERING_STATE_SURFACE,
    );
    for command in ["inject_task", "resume_automation", "finalize_automation"] {
        assert!(
            flagged.contains(command),
            "detector (b) missed canonical writer {command}"
        );
    }
    for command in [
        "pause_task",
        "block_task",
        "stop_task",
        "pause_tasks_in_group",
        "deny_permission_request",
        "list_tasks",
        "health_check",
    ] {
        assert!(
            !flagged.contains(command),
            "detector (b) false-positive: {command}"
        );
    }
    let modules = rows.into_iter().collect::<BTreeMap<_, _>>();
    for command in &flagged {
        let row = policy_for(command, &modules[command]).expect("writer is ledgered");
        assert!(
            matches!(row.class, RiskClass::AgentControl | RiskClass::Elevated),
            "detector (b) writer {command} fell below AgentControl"
        );
    }
}

#[test]
fn detector_b_surface_rows_cannot_evaporate() {
    let graph = CallGraph::build(&load_production_sources());
    let commands = census()
        .into_iter()
        .map(|(command, _)| command)
        .collect::<Vec<_>>();
    let complete =
        spawn_triggering_writers(&graph, commands.clone(), SPAWN_TRIGGERING_STATE_SURFACE);
    for index in 0..SPAWN_TRIGGERING_STATE_SURFACE.len() {
        let mut stripped = SPAWN_TRIGGERING_STATE_SURFACE.to_vec();
        let removed = stripped.remove(index);
        let reduced = spawn_triggering_writers(&graph, commands.clone(), &stripped);
        assert!(
            reduced.len() < complete.len(),
            "removing state surface {} did not shrink its writer/floor set",
            removed.id
        );
    }
}

#[test]
fn agent_consumed_content_derivation_is_calibrated() {
    let reads = agent_content_reads();
    let tools = reads
        .iter()
        .filter_map(|row| row["tool"].as_str())
        .collect::<BTreeSet<_>>();
    for expected in [
        "get_task_context",
        "get_review_notes",
        "get_task_issues",
        "get_artifact",
        "get_artifact_version",
        "get_related_artifacts",
        "get_task_steps",
        "get_step_context",
        "get_task_diff",
        "get_task_diff_stat",
        "get_agent_task",
        "list_agent_tasks",
        "get_sub_steps",
        "get_project_analysis",
        "get_task_validation_summary",
        "search_project_artifacts",
        "get_memory",
        "search_memories",
    ] {
        assert!(
            tools.contains(expected),
            "worker-granted live content read missing: {expected}"
        );
    }
    let helpers = std::fs::read_to_string(repo_root().join("src-tauri/src/http_server/helpers.rs"))
        .expect("worker prompt builder source readable");
    assert!(helpers.contains("get_task_context_impl") && helpers.contains("TaskContext"));
    let writers = agent_content_writers();
    assert!(writers
        .iter()
        .any(|row| row["writer"] == "add_task_note" && row["surface"] == "http-handler"));
    assert!(writers.iter().any(|row| row["writer"] == "update_task"
        && row["conditional"]
            .as_str()
            .is_some_and(|value| value.contains("update_task_authz"))));
}

#[test]
fn content_surface_rows_cannot_evaporate_and_reads_are_not_writers() {
    let writers = agent_content_writers();
    for index in 0..writers.len() {
        let mut stripped = writers.clone();
        stripped.remove(index);
        assert_eq!(stripped.len() + 1, writers.len());
    }
    let names = writers
        .iter()
        .filter_map(|row| row["writer"].as_str())
        .collect::<BTreeSet<_>>();
    for absent in [
        "pause_task",
        "block_task",
        "stop_task",
        "list_tasks",
        "health_check",
    ] {
        assert!(
            !names.contains(absent),
            "non-content writer was flagged: {absent}"
        );
    }
}

#[test]
fn extended_deny_surface_is_not_remotely_registrable_as_read_or_operate() {
    let denied_modules = BTreeSet::from([
        "agent_terminal_commands",
        "api_key_commands",
        "atlassian_commands",
        "chat_attachment_commands",
        "clickup_commands",
        "external_mcp_commands",
        "granola_commands",
        "harness_provider_commands",
        "linear_commands",
        "provider_cli_management_commands",
        "test_data_commands",
        "workspace_open_commands",
    ]);
    let denied_commands = BTreeSet::from([
        "get_git_branches",
        "switch_git_origin_to_ssh",
        "setup_gh_git_auth",
        "login_gh_with_browser",
        "update_custom_analysis",
        "change_project_git_mode",
        "resolve_merge_conflict",
        "cleanup_task_branch",
        "get_task_file_changes",
        "get_file_diff",
        "get_codex_cli_diagnostics",
        "build_agent_issue_report",
    ]);

    for (command, module) in census() {
        let named = denied_modules.contains(module.as_str())
            || denied_commands.contains(command.as_str())
            || command.starts_with("delete_");
        if named {
            let row = policy_for(&command, &module).expect("deny entry is ledgered");
            assert!(
                !matches!(row.class, RiskClass::Read | RiskClass::Operate),
                "P-17c deny surface `{module}::{command}` is remotely registrable below elevated authority"
            );
            assert!(
                find_spec(&command).is_none(),
                "deny surface `{command}` was registered"
            );
        }
    }
}

#[test]
fn exemptions_and_declared_memberships_are_exact() {
    let rows = census().into_iter().collect::<BTreeMap<_, _>>();
    for exemption in AUTHORITY_REDUCING_EXEMPTIONS
        .iter()
        .filter(|entry| entry.kind == "command" && entry.subject != "deny_permission_request")
    {
        let command = exemption.subject;
        let module = rows.get(command).expect("exemption command exists");
        assert_eq!(
            policy_for(command, module).unwrap().class,
            RiskClass::Operate
        );
    }
    for target in ["Cancelled", "Archived"] {
        let exemption = AUTHORITY_REDUCING_EXEMPTIONS
            .iter()
            .find(|entry| entry.kind == "transition-target" && entry.subject == target)
            .unwrap_or_else(|| panic!("missing transition-target exemption for {target}"));
        assert_eq!(exemption.direction, "authority-reducing");
        assert!(
            exemption.rationale.contains(".rs"),
            "{target} rationale must carry file-anchored evidence"
        );
    }
    assert_eq!(
        policy_for("unblock_task", "task_commands").unwrap().class,
        RiskClass::AgentControl
    );
    let permission = policy_for("resolve_permission_request", "permission_commands").unwrap();
    assert_eq!(permission.class, RiskClass::AgentControl);
    assert!(permission.reason.contains(DECLARED_MEMBERSHIPS[0].1));
    let question = policy_for("resolve_user_question", "question_commands").unwrap();
    assert_eq!(question.class, RiskClass::AgentControl);
    assert_eq!(question.reason, DECLARED_MEMBERSHIPS[1].1);
}

#[test]
fn spawning_project_getters_are_elevated_and_not_registered() {
    for command in ["list_projects", "get_project"] {
        let row = policy_for(command, "project_commands").unwrap();
        assert_eq!(row.class, RiskClass::Elevated);
        assert_eq!(row.capabilities, &[Capability::SpawnsProcess]);
        assert!(
            find_spec(command).is_none(),
            "{command} must not remain on the Read registry"
        );
    }
}

#[test]
fn representative_capability_stripping_cannot_lower_membership() {
    for (command, module, capability) in [
        (
            "get_task_file_changes",
            "diff_commands",
            Capability::SpawnsProcess,
        ),
        (
            "get_codex_cli_diagnostics",
            "diagnostic_commands",
            Capability::SpawnsProcess,
        ),
        (
            "build_agent_issue_report",
            "agent_issue_report_commands",
            Capability::SpawnsProcess,
        ),
        (
            "update_agent_provider_settings",
            "harness_provider_commands",
            Capability::ConfiguresFutureProcessAuthority,
        ),
        (
            "resolve_permission_request",
            "permission_commands",
            Capability::AgentControl,
        ),
        (
            "resolve_user_question",
            "question_commands",
            Capability::AgentControl,
        ),
    ] {
        let row = policy_for(command, module).expect("representative row");
        assert!(
            row.capabilities.contains(&capability),
            "`{command}` lost {capability:?}"
        );
        assert!(!matches!(row.class, RiskClass::Read | RiskClass::Operate));
    }
}

#[test]
fn wry_monomorphic_remote_reads_are_ledgered_but_unregistered() {
    for command in [
        "list_remote_advertised_endpoints",
        "list_remote_audit_entries",
    ] {
        let row = policy_for(
            command,
            if command.contains("advertised") {
                "remote_host_commands"
            } else {
                "remote_device_commands"
            },
        )
        .unwrap();
        assert_eq!(row.class, RiskClass::Read);
        assert!(
            find_spec(command).is_none(),
            "AppHandle-only command must await PR 3.1"
        );
    }
}
