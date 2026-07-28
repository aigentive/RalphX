use std::collections::{BTreeMap, BTreeSet};

use ralphx_remote_protocol::{class_permits, Capability, RiskClass};

use super::authority_audit::{
    agent_consumed_content_writers, closure_is_arming, load_production_sources,
    parse_registered_commands, repo_root, spawn_triggering_writers, tokens_reach_any, CallGraph,
    StateSurfaceEntry, AGENT_CONSUMED_CONTENT_WRITE_SURFACE, CONTENT_WRITE_EXEMPTIONS,
    PROCESS_LAUNCH_SINKS, SPAWN_TRIGGERING_STATE_SURFACE, TRANSITION_SINKS,
};
use super::capability_ledger::{
    policy_for, AUTHORITY_REDUCING_EXEMPTIONS, COMMAND_OVERRIDES, CONDITIONAL_CAPABILITIES,
    DECLARED_MEMBERSHIPS, MODULE_DEFAULTS,
};
use super::registry::{find_spec, REMOTE_COMMANDS};

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
            .filter(|tool| classify_granted_tool(tool) == GrantedToolClass::ContentRead)
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

/// Worker-granted MCP tools that read content a remote writer could poison.
const CONTENT_READ_TOOLS: &[&str] = &[
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
    "get_memories_for_paths",
    // Surfaced by the fail-closed classifier below: worker-granted live reads the old
    // `matches!` allowlist silently dropped while `coverage.agentConsumedContent` still read
    // "complete".
    "get_step_progress",
    "get_issue_progress",
    "get_merge_target",
    "list_ticket_attachments",
    "fetch_ticket_attachment",
];

/// Worker-granted MCP tools deliberately OUTSIDE the content-read surface: each one WRITES or
/// steers rather than reading worker-consumed content. Explicit, because silence is what made
/// the old allowlist fail open.
const NON_CONTENT_TOOLS: &[&str] = &[
    "add_step",
    "claim_agent_task",
    "complete_agent_task",
    "complete_merge",
    "complete_review",
    "complete_step",
    "create_agent_task",
    "create_followup_agent_conversation",
    "delegate_cancel",
    "delegate_start",
    "delegate_wait",
    "execution_complete",
    "fail_step",
    "mark_issue_addressed",
    "mark_issue_in_progress",
    "register_agent_issue",
    "report_conflict",
    "report_incomplete",
    "run_task_validation",
    "skip_step",
    "start_step",
    "update_agent_task",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum GrantedToolClass {
    ContentRead,
    NonContent,
}

/// Fail-CLOSED classification (R5-H2).
///
/// The previous `matches!` allowlist returned `false` for an unknown tool, so a newly granted
/// content read was silently dropped from the surface while `coverage.agentConsumedContent`
/// kept claiming "complete". An unclassifiable worker grant is now a hard failure: the surface
/// cannot claim completeness over a tool nobody has classified.
fn classify_granted_tool(tool: &str) -> GrantedToolClass {
    if CONTENT_READ_TOOLS.contains(&tool) {
        return GrantedToolClass::ContentRead;
    }
    if NON_CONTENT_TOOLS.contains(&tool) {
        return GrantedToolClass::NonContent;
    }
    panic!(
        "worker-granted MCP tool `{tool}` is unclassified; add it to CONTENT_READ_TOOLS or \
         NON_CONTENT_TOOLS before the agent-consumed-content surface can claim completeness"
    );
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
    } else if tool.contains("ticket") {
        "ticket attachments"
    } else if tool.contains("merge") {
        "merge target/branch state"
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
        ("start_step", "tauri-command", "task_steps", None),
        ("complete_step", "tauri-command", "task_steps", None),
        ("skip_step", "tauri-command", "task_steps", None),
        ("fail_step", "tauri-command", "task_steps", None),
        (
            "verify_issue",
            "tauri-command",
            "review_notes/task_issues",
            None,
        ),
        (
            "reopen_issue",
            "tauri-command",
            "review_notes/task_issues",
            None,
        ),
        (
            "mark_issue_in_progress",
            "tauri-command",
            "review_notes/task_issues",
            None,
        ),
        (
            "mark_issue_addressed",
            "tauri-command",
            "review_notes/task_issues",
            None,
        ),
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

/// The worker-safe task projection, DERIVED from `WorkerTaskView` rather than restated as a
/// literal: a field added to the struct appears here (and stales the manifest) instead of
/// silently widening the projection behind a hand-written list.
fn worker_task_view_allowlist() -> Vec<String> {
    use crate::domain::entities::{IdeationSessionId, ProjectId, Task, WorkerTaskView};

    let mut task = Task::new(ProjectId::new(), "allowlist probe".to_string());
    // Every optional field must be populated or `skip_serializing_if` hides it from the
    // derived key set.
    task.description = Some("populated".to_string());
    task.ideation_session_id = Some(IdeationSessionId::new());
    let view: WorkerTaskView = task.into();
    serde_json::to_value(view)
        .expect("worker task view serializes")
        .as_object()
        .expect("worker task view is a JSON object")
        .keys()
        .cloned()
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
    // R5-H1's row shape: entry point, sinks reached, read-surface classification. The closure
    // is computed here anyway, so discarding everything but a boolean is what left a reviewer
    // unable to see WHICH sink or persisted state a newly flagged loop touches.
    let background_loop_inventory = graph
        .loop_roots
        .iter()
        .map(|root| {
            let closure = graph.loop_closure(root);
            let sinks_reached = closure
                .sink_hits
                .iter()
                .map(|hit| hit.sink.clone())
                .collect::<BTreeSet<_>>();
            let read_surface = SPAWN_TRIGGERING_STATE_SURFACE
                .iter()
                .filter(|entry| entry.read_by_loops.contains(&root.id.as_str()))
                .map(|entry| entry.id)
                .collect::<Vec<_>>();
            serde_json::json!({
                "id": root.id,
                "file": root.file,
                "enclosingFunction": root.enclosing_fn,
                "kind": root.kind,
                "authorityBearing": closure_is_arming(&closure),
                "sinksReached": sinks_reached,
                "readSurface": read_surface,
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
    // Only AUTHORITY-BEARING loop roots may anchor a surface. The inventory also contains ~98
    // inert roots; accepting those let a surface row inflate `agent_control_floor` on evidence
    // that nothing arms an agent.
    let loop_ids = graph
        .loop_roots
        .iter()
        .filter(|root| closure_is_arming(&graph.loop_closure(root)))
        .map(|root| root.id.as_str())
        .collect::<BTreeSet<_>>();
    let spawn_triggering_state_surface = SPAWN_TRIGGERING_STATE_SURFACE.iter().map(|entry| {
        assert!(!entry.read_by_loops.is_empty(), "surface {} names no read site", entry.id);
        assert!(entry.read_by_loops.iter().all(|id| loop_ids.contains(id)), "surface {} references a loop that is not authority-bearing", entry.id);
        let writers = spawn_triggering_writers(&graph, rows.iter().map(|(command, _)| command.clone()), std::slice::from_ref(entry));
        serde_json::json!({"id": entry.id, "surface": entry.surface, "armedValue": entry.armed_value, "readByLoops": entry.read_by_loops, "writers": writers})
    }).collect::<Vec<_>>();

    // The REGISTERED facade surface, including the two pinned permission ops. Those two are not
    // census commands (no such Tauri command exists — the live surface has only the
    // dual-decision `resolve_permission_request`), so the `ledger` table cannot carry them and
    // the manifest would otherwise publish no record of the mutating surface's shape. `pins` is
    // read straight off the specs, which is the same data the dispatch path binds — a pin cannot
    // be documented here and absent from the wire.
    let facade_ops = REMOTE_COMMANDS
        .iter()
        .map(|spec| {
            serde_json::json!({
                "command": spec.name,
                "target": spec.target,
                "class": spec.class,
                "capabilities": spec.capabilities,
                "argumentSensitive": spec.authz.is_some(),
                "pins": spec.pins.iter().map(|pin| serde_json::json!({
                    "param": pin.param,
                    "field": pin.field,
                    "value": pin.value,
                })).collect::<Vec<_>>(),
            })
        })
        .collect::<Vec<_>>();
    let conditional_capabilities = CONDITIONAL_CAPABILITIES
        .iter()
        .map(|entry| {
            serde_json::json!({
                "command": entry.command,
                "capability": entry.capability,
                "condition": entry.condition,
            })
        })
        .collect::<Vec<_>>();

    serde_json::json!({
        "schemaVersion": 2,
        "background_loop_inventory": background_loop_inventory,
        "facade_ops": facade_ops,
        "conditional_capabilities": conditional_capabilities,
        "spawn_triggering_state_surface": spawn_triggering_state_surface,
        "agent_consumed_content_surface": {
            "reads": agent_content_reads(),
            "writers": agent_content_writers(),
            "detected_writers": detected_content_writers(&graph, &rows),
            "exemptions": CONTENT_WRITE_EXEMPTIONS.iter().map(|exemption| serde_json::json!({
                "command": exemption.command,
                "reason": exemption.reason,
            })).collect::<Vec<_>>(),
        },
        "worker_task_view_allowlist": worker_task_view_allowlist(),
        "authority_reducing_exemptions": authority_reducing_exemptions,
        "declared_memberships": declared_memberships,
        "ledger": ledger,
        "agent_control_floor": agent_control_floor,
        "coverage": {
            "detectorA": detector_a_coverage(&graph, &rows),
            "detectorB": detector_b_coverage(&graph),
            "agentConsumedContent": agent_consumed_content_coverage(&graph, &rows)
        }
    })
}

/// Detector-(d)'s mechanically derived writers, as manifest rows.
fn detected_content_writers(
    graph: &CallGraph,
    rows: &[(String, String)],
) -> Vec<serde_json::Value> {
    agent_consumed_content_writers(
        graph,
        rows.iter().map(|(command, _)| command.clone()),
        AGENT_CONSUMED_CONTENT_WRITE_SURFACE,
    )
    .into_iter()
    .map(|(command, surfaces)| {
        serde_json::json!({"writer": command, "surfaces": surfaces.into_iter().collect::<Vec<_>>()})
    })
    .collect()
}

/// Registered commands whose definition deliberately lives outside `commands/`, so
/// `roots_named` cannot anchor a detector-(a) root for them. Reason-coded so the derivation
/// below stays honest instead of silently tolerating an unresolved root.
const DETECTOR_A_ROOT_EXCEPTIONS: &[(&str, &str)] = &[(
    "greet",
    "Tauri scaffold demo command defined in lib.rs rather than commands/; it takes a &str, \
     returns a String, and reaches no sink",
)];

/// `complete` only when detector (a) actually resolved: every census command reached a graph
/// node and every declared cut sink exists as a callable name somewhere in production source.
///
/// Hardcoding `"complete"` made the downstream fail-closed mirror gate validate a constant —
/// it could never fire. These derivations can.
fn detector_a_coverage(graph: &CallGraph, rows: &[(String, String)]) -> String {
    let unresolved = rows
        .iter()
        .filter(|(command, _)| {
            graph.roots_named(command).is_empty()
                && !DETECTOR_A_ROOT_EXCEPTIONS
                    .iter()
                    .any(|(name, _)| name == command)
        })
        .count();
    if unresolved > 0 {
        return format!("incomplete:{unresolved}-unresolved-command-roots");
    }
    let missing_sinks = TRANSITION_SINKS
        .iter()
        .filter(|sink| graph.roots_named(sink).is_empty())
        .count();
    if missing_sinks > 0 {
        return format!("incomplete:{missing_sinks}-unresolved-transition-sinks");
    }
    "complete".to_string()
}

/// `complete` only when every declared surface row still anchors to a real authority-bearing
/// loop and names at least one write site.
fn detector_b_coverage(graph: &CallGraph) -> String {
    let authority_bearing = graph
        .loop_roots
        .iter()
        .filter(|root| closure_is_arming(&graph.loop_closure(root)))
        .map(|root| root.id.as_str())
        .collect::<BTreeSet<_>>();
    let dangling = SPAWN_TRIGGERING_STATE_SURFACE
        .iter()
        .filter(|entry| {
            entry.read_by_loops.is_empty()
                || !entry
                    .read_by_loops
                    .iter()
                    .all(|id| authority_bearing.contains(id))
        })
        .count();
    if dangling > 0 {
        return format!("incomplete:{dangling}-surface-rows-without-a-live-loop");
    }
    let unmarked = SPAWN_TRIGGERING_STATE_SURFACE
        .iter()
        .filter(|entry| entry.write_markers.is_empty() && entry.declared_writers.is_empty())
        .count();
    if unmarked > 0 {
        return format!("incomplete:{unmarked}-surface-rows-without-a-write-site");
    }
    "complete".to_string()
}

/// `complete` only when the read half classified every worker grant (`classify_granted_tool`
/// panics otherwise, so reaching here IS the proof) and the write half's detector produced a
/// non-empty derivation in which every candidate is discharged.
fn agent_consumed_content_coverage(graph: &CallGraph, rows: &[(String, String)]) -> String {
    let reads = agent_content_reads();
    if reads.is_empty() {
        return "incomplete:no-classified-content-reads".to_string();
    }
    let detected = agent_consumed_content_writers(
        graph,
        rows.iter().map(|(command, _)| command.clone()),
        AGENT_CONSUMED_CONTENT_WRITE_SURFACE,
    );
    if detected.is_empty() {
        return "incomplete:content-write-detector-derived-nothing".to_string();
    }
    let modules = rows.iter().cloned().collect::<BTreeMap<_, _>>();
    let undischarged = detected
        .keys()
        .filter(|command| !content_writer_is_discharged(command, &modules))
        .count();
    if undischarged > 0 {
        return format!("incomplete:{undischarged}-undischarged-content-writers");
    }
    "complete".to_string()
}

/// A detected writer is discharged by carrying the capability, by a conditional annotation
/// backed by an `authz:` predicate, or by a reason-coded exemption.
fn content_writer_is_discharged(command: &str, modules: &BTreeMap<String, String>) -> bool {
    if CONTENT_WRITE_EXEMPTIONS
        .iter()
        .any(|exemption| exemption.command == command)
    {
        return true;
    }
    if CONDITIONAL_CAPABILITIES.iter().any(|entry| {
        entry.command == command && entry.capability == Capability::MutatesAgentConsumedContent
    }) {
        return true;
    }
    modules
        .get(command)
        .and_then(|module| policy_for(command, module))
        .is_some_and(|row| {
            row.capabilities
                .contains(&Capability::MutatesAgentConsumedContent)
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

/// The annotation ⇔ predicate tie (§3.3 conditional capability).
///
/// A conditional capability is a promise that SOME arguments need a higher scope than the
/// command's class. The only thing that can keep that promise is an argument-sensitive `authz:`
/// predicate on the registered spec. This asserts the tie in BOTH directions, so neither half can
/// be removed alone:
///
/// * annotation → predicate: every `CONDITIONAL_CAPABILITIES` row is registered, sits at a class
///   that does NOT already permit the capability (otherwise the annotation is noise), and carries
///   a predicate;
/// * content-writer → annotation-or-capability: every registered command the 1.3 content-surface
///   enumeration names as a writer either declares `MutatesAgentConsumedContent` outright or is
///   annotated conditional. A registered `Operate` content writer with neither fails here.
#[test]
fn conditional_capabilities_are_discharged_by_a_live_predicate() {
    for entry in CONDITIONAL_CAPABILITIES {
        let spec = find_spec(entry.command).unwrap_or_else(|| {
            panic!(
                "`{}` carries a conditional capability but is not registered; \
                 an annotation on an unreachable command proves nothing",
                entry.command
            )
        });
        assert!(
            !class_permits(spec.class, &[entry.capability]),
            "`{}` is registered at {:?}, which already permits {:?} — declare it in `caps:` \
             instead of annotating it as conditional",
            entry.command,
            spec.class,
            entry.capability
        );
        assert!(
            spec.authz.is_some(),
            "`{}` is annotated with a conditional {:?} but has no `authz:` predicate to \
             discharge it — the annotation would be the only thing standing between a \
             ui:operate device and the content surface",
            entry.command,
            entry.capability
        );
    }

    let annotated = CONDITIONAL_CAPABILITIES
        .iter()
        .map(|entry| entry.command)
        .collect::<BTreeSet<_>>();
    for row in agent_content_writers() {
        let writer = row["writer"].as_str().expect("writer is a string");
        let Some(spec) = find_spec(writer) else {
            // Unregistered writers are unreachable remotely; nothing to discharge.
            continue;
        };
        let declares = spec
            .capabilities
            .contains(&Capability::MutatesAgentConsumedContent);
        assert!(
            declares || annotated.contains(writer),
            "`{writer}` writes the agent-consumed content surface and is registered at {:?}, \
             but declares neither MutatesAgentConsumedContent nor a conditional annotation",
            spec.class
        );
        // A conditional annotation is only honest when the manifest row is also marked
        // conditional; otherwise the audit output and the ledger disagree about the same write.
        if !declares {
            assert!(
                row.get("conditional").is_some(),
                "`{writer}` is annotated conditional in the ledger but the content-surface \
                 row publishes it as an unconditional write"
            );
        }
    }
}

/// P-1 feeder: a dual-decision sink is only safe when the decision is SERVER-controlled.
#[test]
fn no_facade_op_accepts_a_client_supplied_decision() {
    assert!(
        find_spec("resolve_permission_request").is_none(),
        "the raw dual-decision command must never be registered; the facade exposes only the \
         two single-purpose pinned ops"
    );

    let pinned = REMOTE_COMMANDS
        .iter()
        .filter(|spec| spec.pins.iter().any(|pin| pin.field == "decision"))
        .map(|spec| (spec.name, spec.pins[0].value))
        .collect::<BTreeMap<_, _>>();
    assert_eq!(
        pinned,
        [
            ("approve_permission_request", "allow"),
            ("deny_permission_request", "deny"),
        ]
        .into_iter()
        .collect::<BTreeMap<_, _>>(),
        "the pinned permission ops drifted from their server-controlled decisions"
    );

    // Both ops target the SAME existing fn (A-7: no forked command fns) and are separated only
    // by the pin and the class.
    let approve = find_spec("approve_permission_request").expect("approve op is registered");
    let deny = find_spec("deny_permission_request").expect("deny op is registered");
    assert_eq!(approve.target, deny.target);
    assert_eq!(approve.class, RiskClass::AgentControl);
    assert_eq!(deny.class, RiskClass::Operate);
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
        if row.class == RiskClass::Denied {
            assert!(
                find_spec(&command).is_none(),
                "Denied ledger row `{command}` must not be registered"
            );
        } else {
            assert!(
                class_permits(row.class, row.capabilities),
                "ledger class/capability mismatch for `{command}`"
            );
        }
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
                matches!(
                    row.class,
                    RiskClass::AgentControl | RiskClass::Elevated | RiskClass::Denied
                ),
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
        541,
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
        // PR 3.1-b batch 1 — enumerating open gates seeds no scheduler-consumed state. The
        // detector-(a)/(b) floors above would already fail these `Read` rows if it did;
        // naming them keeps the audit note and the mechanism in the same place.
        "list_pending_permission_gates",
        "list_pending_question_gates",
        // PR 3.1-b batch 2 — the census `B1` read cluster. Each was reclassified out of
        // `conservative-module-default` on the strength of detectors (a)/(b)/(c) being
        // silent; naming them here turns that observation into a standing assertion, so a
        // refactor that makes one of them seed scheduler-consumed state fails CI instead of
        // shipping a `Read` row with spawn-triggering authority.
        "get_archived_count",
        "get_tasks_awaiting_review",
        "get_session_task_history_availability",
        "get_task_state_transitions",
        "get_task_dependency_graph",
        "get_task_timeline_events",
        "get_task_agent_workspace",
        "get_task_steps",
        "get_step_progress",
        "get_execution_settings",
        "get_global_execution_settings",
        "get_active_project",
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
            matches!(
                row.class,
                RiskClass::AgentControl | RiskClass::Elevated | RiskClass::Denied
            ),
            "detector (b) writer {command} fell below AgentControl"
        );
    }
}

#[test]
fn detector_b_proof_classes_are_flagged_and_in_the_floor() {
    let graph = CallGraph::build(&load_production_sources());
    let rows = census();
    let commands = rows
        .iter()
        .map(|(command, _)| command.clone())
        .collect::<Vec<_>>();
    let detector_b = spawn_triggering_writers(&graph, commands, SPAWN_TRIGGERING_STATE_SURFACE);
    let manifest = generated_manifest();
    let floor = manifest["agent_control_floor"]
        .as_array()
        .expect("generated floor is an array")
        .iter()
        .filter_map(serde_json::Value::as_str)
        .collect::<BTreeSet<_>>();
    let modules = rows.into_iter().collect::<BTreeMap<_, _>>();

    // Detector-(a)-alone spawn-free status varies per command as the production call graph
    // evolves. The invariant here is detector-(b) classification, floor membership, and
    // AgentControl-or-stronger ledgering regardless of detector-(a). inject_task and
    // finalize_automation also prove detector-(b) catches genuinely spawn-free commands that
    // detector-(a) misses; resume_automation and the auto-publish bridge writer independently
    // carry detector-(a) authority, which is a stronger outcome, not a gap.
    for command in [
        "inject_task",
        "finalize_automation",
        "resume_automation",
        "set_agent_conversation_workspace_auto_publish",
    ] {
        assert!(
            detector_b.contains(command),
            "detector (b) missed proof-class writer {command}"
        );
        assert!(
            floor.contains(command),
            "generated AgentControl floor missed {command}"
        );
        let row = policy_for(command, &modules[command]).expect("proof-class writer is ledgered");
        assert!(
            matches!(
                row.class,
                RiskClass::AgentControl | RiskClass::Elevated | RiskClass::Denied
            ),
            "proof-class writer {command} fell below AgentControl"
        );
    }

    for command in ["inject_task", "finalize_automation"] {
        assert!(
            !closure_is_arming(&graph.closure([command.to_string()])),
            "{command} must demonstrate detector (b), not detector (a)"
        );
    }

    // automation_commands.rs:313 reaches startup_background.rs:366 through reopen redrive, so
    // resume_automation has real detector-(a) send_message authority as well as detector-(b).
    assert!(
        closure_is_arming(&graph.closure(["resume_automation".to_string()])),
        "resume_automation must retain its stronger detector-(a) classification"
    );
    // unified_chat_commands/mod.rs:5277 returns through agent_workspace_response_for_state;
    // its :1038 recovery scheduling reaches pr_merge_poller.rs:2616 send_message authority.
    assert!(
        closure_is_arming(
            &graph.closure(["set_agent_conversation_workspace_auto_publish".to_string(),])
        ),
        "auto-publish must retain its stronger detector-(a) classification"
    );

    for command in [
        "pause_task",
        "block_task",
        "stop_task",
        "pause_tasks_in_group",
        "deny_permission_request",
        "list_tasks",
        "get_task",
        "search_tasks",
        "health_check",
    ] {
        assert!(
            !closure_is_arming(&graph.closure([command.to_string()])),
            "brake/read {command} was falsely flagged by detector (a)"
        );
        assert!(
            !detector_b.contains(command),
            "brake/read {command} was falsely flagged by detector (b)"
        );
    }
}

#[test]
fn synthetic_unregistered_authority_loop_requires_a_surface_tie_and_stales_manifest() {
    let manifest: serde_json::Value =
        serde_json::from_str(include_str!("../../../docs/generated/remote-commands.json"))
            .expect("checked-in manifest parses");
    let manifest_loop_ids = manifest["background_loop_inventory"]
        .as_array()
        .expect("background loop inventory is an array")
        .iter()
        .filter_map(|row| row["id"].as_str())
        .collect::<BTreeSet<_>>();

    let mut sources = load_production_sources();
    sources.push((
        "synthetic/unregistered_interval.rs".to_string(),
        r#"
            fn synthetic_unregistered_interval() {
                tokio::spawn(async move {
                    let mut interval = tokio::time::interval(duration());
                    loop {
                        interval.tick().await;
                        read_synthetic_armed_state();
                        send_message();
                    }
                });
            }
            fn synthetic_writer() {
                write_synthetic_armed_state();
            }
        "#
        .to_string(),
    ));
    let graph = CallGraph::build(&sources);
    let synthetic_root = graph
        .loop_roots
        .iter()
        .find(|root| root.file == "synthetic/unregistered_interval.rs")
        .expect("synthetic interval loop is discovered");
    assert!(
        closure_is_arming(&graph.loop_closure(synthetic_root)),
        "synthetic send_message loop is authority-bearing"
    );
    assert!(
        !manifest_loop_ids.contains(synthetic_root.id.as_str()),
        "an unregistered production loop must make the checked-in inventory stale"
    );

    let leaked_id: &'static str = Box::leak(synthetic_root.id.clone().into_boxed_str());
    let read_by_loops: &'static [&'static str] = Box::leak(Box::new([leaked_id]));
    let synthetic_surface = StateSurfaceEntry {
        id: "synthetic-armed-state",
        surface: "synthetic.armed_state",
        armed_value: "true",
        read_by_loops,
        write_markers: &["write_synthetic_armed_state"],
        armed_markers: &[],
        declared_writers: &[],
    };
    let without_surface = spawn_triggering_writers(
        &graph,
        ["synthetic_writer".to_string()],
        SPAWN_TRIGGERING_STATE_SURFACE,
    );
    assert!(
        !without_surface.contains("synthetic_writer"),
        "an omitted read-site surface leaves its writer orphaned"
    );
    let with_surface = spawn_triggering_writers(
        &graph,
        ["synthetic_writer".to_string()],
        std::slice::from_ref(&synthetic_surface),
    );
    assert!(
        synthetic_surface
            .read_by_loops
            .contains(&synthetic_root.id.as_str()),
        "synthetic surface must tie its read site to the discovered loop"
    );
    assert!(
        with_surface.contains("synthetic_writer"),
        "adding the required surface tie must classify its arming writer"
    );
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
    let published: serde_json::Value =
        serde_json::from_str(include_str!("../../../docs/generated/remote-commands.json"))
            .expect("checked-in manifest parses");
    let published_surfaces = published["spawn_triggering_state_surface"]
        .as_array()
        .expect("published state surface is an array");
    let mut uniquely_load_bearing = 0usize;

    for index in 0..SPAWN_TRIGGERING_STATE_SURFACE.len() {
        let entry = &SPAWN_TRIGGERING_STATE_SURFACE[index];
        let own = spawn_triggering_writers(&graph, commands.clone(), std::slice::from_ref(entry));
        // A surface whose markers stopped matching anything has silently evaporated even though
        // the row is still present.
        assert!(
            !own.is_empty(),
            "state surface {} attributes no writer at all",
            entry.id
        );
        // Its attribution is published and CI-compared, so deleting the row cannot be invisible.
        let row = published_surfaces
            .iter()
            .find(|row| row["id"] == entry.id)
            .unwrap_or_else(|| panic!("state surface {} is not in the manifest", entry.id));
        let published_writers = row["writers"]
            .as_array()
            .expect("published writers are an array")
            .iter()
            .filter_map(serde_json::Value::as_str)
            .map(str::to_string)
            .collect::<BTreeSet<_>>();
        assert_eq!(
            published_writers, own,
            "state surface {} drifted from its published writer attribution",
            entry.id
        );

        let mut stripped = SPAWN_TRIGGERING_STATE_SURFACE.to_vec();
        stripped.remove(index);
        let reduced = spawn_triggering_writers(&graph, commands.clone(), &stripped);
        // Write-site markers legitimately overlap between surfaces (one command can write two
        // of them), so a shrinking global floor is required only where this surface is the sole
        // attributor. Where it is, removal must be visible in the floor as well as the manifest.
        if own.iter().any(|writer| !reduced.contains(writer)) {
            uniquely_load_bearing += 1;
            assert!(
                reduced.len() < complete.len(),
                "removing state surface {} lost a sole-attributed writer without shrinking the floor",
                entry.id
            );
        }
    }
    assert!(
        uniquely_load_bearing > 0,
        "no state surface is the sole attributor of any writer; the floor rests entirely on overlap"
    );
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
    // Mirrors `detector_b_surface_rows_cannot_evaporate`: each row must be load-bearing on the
    // CI-gated artifact. The previous form asserted `Vec::remove` arithmetic, which holds for
    // any input — deleting a content-writer row and regenerating passed every gate.
    let published: serde_json::Value =
        serde_json::from_str(include_str!("../../../docs/generated/remote-commands.json"))
            .expect("checked-in manifest parses");
    let published_writers = published["agent_consumed_content_surface"]["writers"]
        .as_array()
        .expect("published content writers are an array");
    let writers = agent_content_writers();
    assert_eq!(
        published_writers, &writers,
        "content-writer surface drifted from the checked-in manifest"
    );
    for index in 0..writers.len() {
        let mut stripped = writers.clone();
        let removed = stripped.remove(index);
        assert_ne!(
            &stripped, published_writers,
            "removing content writer {removed} left the gated manifest surface unchanged"
        );
        // Every remaining row must still be a row the manifest publishes; a strip must remove
        // exactly one observable row, never silently re-derive it.
        assert!(
            !stripped.contains(&removed),
            "content writer {removed} appears more than once"
        );
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
fn extended_deny_surface_is_denied_and_not_remotely_registrable() {
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
        "cleanup_task",
        "cleanup_tasks_in_group",
        "publish_agent_conversation_workspace",
        "close_agent_workspace_pr",
        "update_agent_conversation_workspace_from_base",
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
            assert_eq!(
                row.class,
                RiskClass::Denied,
                "P-17c deny surface `{module}::{command}` must be Denied"
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

/// P-17 binding gate: the class the runtime ENFORCES must be the class the ledger FLOORS.
///
/// `enforce_scope` reads `RemoteCommandSpec.class` — the value declared in `remote_commands!`.
/// The ledger/detector floor was a disconnected value nothing compared it against, so a
/// registration declaring `Read` for a command the ledger classifies `Elevated` shipped with
/// green CI. That is a scope escalation: the request is admitted on `ui:read`.
#[test]
fn every_registered_spec_matches_its_ledger_row() {
    let modules = census().into_iter().collect::<BTreeMap<_, _>>();
    assert!(
        !REMOTE_COMMANDS.is_empty(),
        "the registered surface must not be empty or this gate is vacuous"
    );
    for spec in REMOTE_COMMANDS {
        // A pinned facade op is a SYNTHESISED name: no such Tauri command exists, so it has no
        // census row of its own. It still may not float free of the ledger — it inherits the
        // scrutiny of the fn it targets, plus two extra obligations, because splitting one
        // command into two ops is precisely how an authority boundary gets quietly widened.
        let module = match modules.get(spec.name) {
            Some(module) => module,
            None => {
                assert!(
                    !spec.pins.is_empty(),
                    "registered command `{}` is not in the live census and is not a pinned \
                     facade op; the facade may only expose existing commands",
                    spec.name
                );
                let target_command = spec
                    .target
                    .rsplit("::")
                    .next()
                    .expect("target path has a final segment");
                let target_module = modules.get(target_command).unwrap_or_else(|| {
                    panic!(
                        "pinned op `{}` targets `{target_command}`, which is not a live command",
                        spec.name
                    )
                });
                let target_row = policy_for(target_command, target_module)
                    .unwrap_or_else(|| panic!("`{target_command}` is not ledgered"));
                let own_row = policy_for(spec.name, target_module).unwrap_or_else(|| {
                    panic!(
                        "pinned op `{}` needs its own COMMAND_OVERRIDES row; inheriting the \
                         module default would silently reclassify it",
                        spec.name
                    )
                });
                assert_eq!(
                    spec.class, own_row.class,
                    "pinned op `{}` is registered {:?} but ledgered {:?}",
                    spec.name, spec.class, own_row.class
                );
                assert_eq!(spec.capabilities, own_row.capabilities);
                // Weakening below the target's class is only legitimate when the pin makes the
                // op authority-REDUCING, and that claim must be recorded, not asserted in a
                // comment.
                if own_row.class != target_row.class {
                    assert!(
                        AUTHORITY_REDUCING_EXEMPTIONS.iter().any(|exemption| {
                            exemption.subject == spec.name && exemption.kind == "command"
                        }),
                        "pinned op `{}` is ledgered {:?} while its target `{target_command}` is \
                         {:?}, with no authority-reducing exemption to justify the gap",
                        spec.name,
                        own_row.class,
                        target_row.class
                    );
                }
                continue;
            }
        };
        let row = policy_for(spec.name, module)
            .unwrap_or_else(|| panic!("registered command `{}` is not ledgered", spec.name));
        assert_eq!(
            spec.class, row.class,
            "`{}` is registered as {:?} but the ledger floors it at {:?}; runtime authorization \
             would admit it on the weaker scope",
            spec.name, spec.class, row.class
        );
        assert_eq!(
            spec.capabilities, row.capabilities,
            "`{}` declares capabilities {:?} but the ledger records {:?}",
            spec.name, spec.capabilities, row.capabilities
        );
    }
}

/// R4-C1: a marker that is the writer's own command name matches that command against itself.
///
/// Every function node carries its own bare name as a token, so such a marker flags its
/// command regardless of what the body does, and a NEW writer of the same surface is a silent
/// false negative. Markers must come from the write site, not the command list.
#[test]
fn state_surface_markers_are_never_census_command_names() {
    let commands = census()
        .into_iter()
        .map(|(command, _)| command)
        .collect::<BTreeSet<_>>();
    let mut marker_count = 0usize;
    for entry in SPAWN_TRIGGERING_STATE_SURFACE {
        assert!(
            !entry.write_markers.is_empty(),
            "surface {} has no write site",
            entry.id
        );
        for marker in entry.write_markers.iter().chain(entry.armed_markers.iter()) {
            marker_count += 1;
            assert!(
                !commands.contains(*marker),
                "surface {} marker `{marker}` is a census command name; the match is tautological",
                entry.id
            );
        }
        for declared in entry.declared_writers {
            assert!(
                !declared.reason.is_empty(),
                "surface {} declares writer {} without a reason code",
                entry.id,
                declared.command
            );
        }
    }
    assert!(
        marker_count >= SPAWN_TRIGGERING_STATE_SURFACE.len(),
        "marker set collapsed"
    );
}

/// P-17g follow-through: each proof-class writer must be flagged by a marker that is NOT its
/// own name — otherwise the proof-class assertions are self-fulfilling.
#[test]
fn proof_class_writers_are_flagged_by_write_site_markers() {
    let graph = CallGraph::build(&load_production_sources());
    for command in [
        "inject_task",
        "finalize_automation",
        "resume_automation",
        "set_agent_conversation_workspace_auto_publish",
    ] {
        let tokens = graph.closure([command.to_string()]).tokens;
        let flagging = SPAWN_TRIGGERING_STATE_SURFACE
            .iter()
            .filter(|entry| entry.flags(command, &tokens))
            .flat_map(|entry| entry.matched_markers(&tokens))
            .collect::<BTreeSet<_>>();
        assert!(
            !flagging.is_empty(),
            "proof-class writer {command} is flagged by no marker"
        );
        assert!(
            !flagging.contains(command),
            "proof-class writer {command} is flagged by its own name"
        );
    }
}

/// Detector (c): a process launch reached from a command is authority the `Read`/`Operate`
/// classes cannot express — `class_permits` allows `SpawnsProcess` only under `Elevated`.
///
/// Until this gate existed every audit sink was an AGENT-authority sink, so a `CommandOverride`
/// lowering a process-spawning getter to `Read` passed CI by construction. That is exactly the
/// shape the `list_projects`/`get_project` mislabel had, and running this gate for the first
/// time found a live one: `list_remote_advertised_endpoints` was ledgered `Read` while
/// resolving the Tailscale CLI.
#[test]
fn detector_c_floors_process_spawn_authority() {
    let graph = CallGraph::build(&load_production_sources());
    let mut spawners = BTreeSet::new();
    let mut registered_spawners = Vec::new();

    for (command, module) in census() {
        let tokens = graph.closure([command.clone()]).tokens;
        if !tokens_reach_any(&tokens, PROCESS_LAUNCH_SINKS) {
            continue;
        }
        spawners.insert(command.clone());
        let row = policy_for(&command, &module).expect("census is ledgered");
        assert!(
            !matches!(row.class, RiskClass::Read | RiskClass::Operate),
            "detector (c): `{command}` resolves a CLI binary but is ledgered {:?}; \
             SpawnsProcess is only expressible under Elevated",
            row.class
        );
        if find_spec(&command).is_some() {
            registered_spawners.push(command.clone());
        }
    }
    // Reported as a set rather than on first hit: a registration sweep that trips this gate
    // typically trips it many times, and failing one-at-a-time turns a single audit finding into
    // a sequence of misleading ones.
    assert!(
        registered_spawners.is_empty(),
        "detector (c): {registered_spawners:?} carry process authority and must not be \
         registered on the remote facade in this PR"
    );

    // Calibration — the detector must actually fire, or the floor above is vacuous.
    for command in [
        "list_projects",
        "get_git_branches",
        "get_task_file_changes",
        "get_codex_cli_diagnostics",
    ] {
        assert!(
            spawners.contains(command),
            "detector (c) missed known process-spawning command {command}"
        );
    }
    for command in [
        "health_check",
        "get_valid_transitions",
        "list_tasks",
        "get_task",
        "search_tasks",
        // PR 3.1-b batch 1 — the hand audit that authorized the `Read` rows for the 2.7
        // gate reads claimed both closures are launch-sink-free. This is that claim made
        // mechanical: if a future refactor routes gate rehydration through anything that
        // resolves a CLI path, the registration fails here rather than shipping.
        "list_pending_permission_gates",
        "list_pending_question_gates",
        // PR 3.1-b batch 2 — census `B1` read cluster. `get_execution_status` and
        // `get_running_processes` are deliberately ABSENT: detector (c) fires on both, which
        // is exactly why they stayed above `Read`.
        "get_archived_count",
        "get_tasks_awaiting_review",
        "get_session_task_history_availability",
        "get_task_state_transitions",
        "get_task_dependency_graph",
        "get_task_timeline_events",
        "get_task_agent_workspace",
        "get_task_steps",
        "get_step_progress",
        "get_execution_settings",
        "get_global_execution_settings",
        "get_active_project",
    ] {
        assert!(
            !spawners.contains(command),
            "detector (c) false-positive on registered read {command}"
        );
    }
}

#[test]
#[ignore = "calibration probe"]
fn probe_detector_calibration() {
    let live = live_mcp_tool_names();
    for agent in WORKER_AGENTS {
        let path = repo_root().join("agents").join(agent).join("agent.yaml");
        let tools = yaml_mcp_tools(&std::fs::read_to_string(path).unwrap());
        for tool in tools.intersection(&live) {
            if !CONTENT_READ_TOOLS.contains(&tool.as_str())
                && !NON_CONTENT_TOOLS.contains(&tool.as_str())
            {
                eprintln!("PROBE unclassified-tool {agent} {tool}");
            }
        }
    }

    let graph = CallGraph::build(&load_production_sources());
    for command in [
        "pause_task",
        "block_task",
        "stop_task",
        "pause_tasks_in_group",
        "deny_permission_request",
        "list_tasks",
        "get_task",
        "search_tasks",
        "health_check",
        "get_valid_transitions",
        "list_remote_advertised_endpoints",
        "list_remote_audit_entries",
    ] {
        let tokens = graph.closure([command.to_string()]).tokens;
        for entry in SPAWN_TRIGGERING_STATE_SURFACE {
            if entry.flags(command, &tokens) {
                eprintln!(
                    "PROBE detb {command} <- {} via {:?}",
                    entry.id,
                    entry.matched_markers(&tokens)
                );
            }
        }
        let proc = PROCESS_LAUNCH_SINKS
            .iter()
            .copied()
            .filter(|sink| tokens_reach_any(&tokens, &[sink]))
            .collect::<Vec<_>>();
        eprintln!("PROBE detc {command} proc={proc:?}");
    }

    let commands = census()
        .into_iter()
        .map(|(command, _)| command)
        .collect::<Vec<_>>();
    let token_sets = commands
        .iter()
        .map(|command| (command.clone(), graph.closure([command.clone()]).tokens))
        .collect::<Vec<_>>();
    for candidate in [
        "create",
        "restart_terminal_task_to_ready_with_history_for_action",
        "InternalStatus::Ready",
        "ensure_re_review_from_escalated_status",
        "add_note",
        "InternalStatus::PendingReview",
        "InternalStatus::RevisionNeeded",
        "transition_automation_status",
        "transition_automation_status_or_conflict",
        "reopen_run_corrective",
        "compare_and_swap_status",
        "AutomationStatus::Active",
        "create_or_update",
        "update_links",
        "restore_after_restart",
        "update_status",
        "update_pr_supervision_preferences",
        "linked_ideation_session_id",
        "AgentConversationWorkspaceMode::Ideation",
        "AgentConversationWorkspaceMode::Plan",
        "AgentConversationWorkspaceMode::Edit",
        "AgentConversationWorkspaceMode::Tasks",
        "insert_event",
        "insert_event_once_for_attempt",
        "update_auto_publish_preferences",
        "update_auto_publish_initial_pr_preference",
        "auto_publish",
        "update_settings",
        "require_workspace_review",
    ] {
        let hits = token_sets
            .iter()
            .filter(|(_, tokens)| tokens.contains(candidate))
            .map(|(command, _)| command.as_str())
            .collect::<Vec<_>>();
        eprintln!(
            "PROBE token {candidate} hits={} sample={:?}",
            hits.len(),
            hits.iter().take(6).collect::<Vec<_>>()
        );
    }
    for entry in SPAWN_TRIGGERING_STATE_SURFACE {
        let writers =
            spawn_triggering_writers(&graph, commands.clone(), std::slice::from_ref(entry));
        eprintln!(
            "PROBE surface {} writers={} {:?}",
            entry.id,
            writers.len(),
            writers.iter().take(14).collect::<Vec<_>>()
        );
    }
}

#[test]
fn wry_monomorphic_remote_reads_are_ledgered_but_unregistered() {
    let row = policy_for("list_remote_audit_entries", "remote_device_commands").unwrap();
    assert_eq!(row.class, RiskClass::Read);
    assert!(
        find_spec("list_remote_audit_entries").is_none(),
        "AppHandle-only command must await PR 3.1"
    );

    // Not a read: detector (c) proved this one resolves the Tailscale CLI.
    let advertised =
        policy_for("list_remote_advertised_endpoints", "remote_host_commands").unwrap();
    assert_eq!(advertised.class, RiskClass::Elevated);
    assert_eq!(advertised.capabilities, &[Capability::SpawnsProcess]);
    assert!(
        find_spec("list_remote_advertised_endpoints").is_none(),
        "process-spawning endpoint listing must not be registered"
    );
}

/// PR 3.1-b batch 2 — the census `B1` reclassifications are reviewed rows, not defaults.
///
/// The failure this pins is subtle and was the whole reason batch 1 could not register
/// anything: a command can LOOK classified while carrying only
/// `conservative-module-default`. Asserting the class alone would pass for a row that
/// merely inherited its module policy, so each row is checked against its module default
/// too — the reason string must have changed, and the class must sit strictly below the
/// module's.
#[test]
fn b1_read_reclassifications_are_reviewed_rather_than_module_defaults() {
    const RECLASSIFIED_READS: &[(&str, &str)] = &[
        ("get_archived_count", "task_commands"),
        ("get_tasks_awaiting_review", "task_commands"),
        ("get_session_task_history_availability", "task_commands"),
        ("get_task_state_transitions", "task_commands"),
        ("get_task_dependency_graph", "task_commands"),
        ("get_task_timeline_events", "task_commands"),
        ("get_task_agent_workspace", "task_commands"),
        ("get_task_steps", "task_step_commands"),
        ("get_step_progress", "task_step_commands"),
        ("get_execution_settings", "execution_commands"),
        ("get_global_execution_settings", "execution_commands"),
        ("get_active_project", "execution_commands"),
    ];

    for (command, module) in RECLASSIFIED_READS {
        let row = policy_for(command, module).expect("reclassified command is ledgered");
        assert_eq!(
            row.class,
            RiskClass::Read,
            "`{command}` must be ledgered Read"
        );
        assert!(
            row.capabilities.is_empty(),
            "`{command}` is a Read row and must carry no capability"
        );
        assert!(
            !row.reason.contains("conservative-module-default"),
            "`{command}` still carries the module-default reason; a reclassification must \
             record the audit that justified it"
        );

        let module_row = MODULE_DEFAULTS
            .iter()
            .find(|entry| entry.module == *module)
            .expect("module has a default");
        assert_eq!(
            module_row.policy.class,
            RiskClass::AgentControl,
            "`{module}` default must still be conservative; these rows are exceptions to it, \
             not evidence the whole module is safe"
        );

        assert!(
            find_spec(command).is_some(),
            "`{command}` audited clean and must be registered"
        );
    }

    // The counterweight: the two fail-OPEN gate siblings audit as reads too, and are
    // deliberately NOT reclassified. `get_pending_permissions`/`get_pending_questions`
    // return `Ok(vec![])` when the repository read fails, so registering them would let a
    // remote client be told "no gates are open" because a read errored. Batch 1 recorded
    // this; here it is enforced.
    for command in ["get_pending_permissions", "get_pending_questions"] {
        let module = if command.contains("permission") {
            "permission_commands"
        } else {
            "question_commands"
        };
        let row = policy_for(command, module).expect("fail-open sibling is ledgered");
        assert_eq!(
            row.class,
            RiskClass::AgentControl,
            "`{command}` is fail-open and must not be reclassified downward by module analogy"
        );
        assert!(
            find_spec(command).is_none(),
            "`{command}` must not be registered"
        );
    }
}

/// The `B1` commands that audited DIRTY, and the distinct reason each stays above `Read`.
///
/// A reclassification sweep is only as trustworthy as the rows it declines to move. These
/// three sit in modules whose other getters were just reclassified, so "it is a getter in a
/// reclassified module" must not be sufficient grounds — each is pinned to its own finding.
#[test]
fn b1_sibling_getters_that_audit_dirty_stay_above_read() {
    let graph = CallGraph::build(&load_production_sources());

    // Detector (c) is the mechanism, not a note: both resolve a process-inspection CLI.
    for command in ["get_execution_status", "get_running_processes"] {
        let tokens = graph.closure([command.to_string()]).tokens;
        assert!(
            tokens_reach_any(&tokens, PROCESS_LAUNCH_SINKS),
            "`{command}` is excluded from the Read cluster because detector (c) fires; if it \
             no longer does, the exclusion needs a new reason or the row can be reclassified"
        );
        let row = policy_for(command, "execution_commands").expect("ledgered");
        assert!(
            !matches!(row.class, RiskClass::Read | RiskClass::Operate),
            "`{command}` resolves a CLI and cannot be Read/Operate"
        );
        assert!(
            find_spec(command).is_none(),
            "`{command}` must not be registered"
        );
    }

    // No detector models this one: `set_active_project` writes the runtime scheduler quota
    // through `sync_quota_from_project`. It is a hand-audited exclusion, recorded here so the
    // reason survives the next sweep.
    let row = policy_for("set_active_project", "execution_commands").expect("ledgered");
    assert_eq!(
        row.class,
        RiskClass::AgentControl,
        "set_active_project syncs the execution concurrency quota, which is how waiting \
         Ready tasks get picked up; it is not a getter"
    );
    assert!(
        find_spec("set_active_project").is_none(),
        "set_active_project must not be registered"
    );
}

/// Calibration probe for the spawn-free remote send.
///
/// Prints the detector verdict for `send_remote_chat_message` alongside the two commands
/// it deliberately is NOT — `send_agent_message` and `start_agent_conversation`, which
/// fire all three. Run before registering the command on the facade; a `c=true` here is a
/// redesign signal, never an exemption request.
#[test]
#[ignore = "calibration probe"]
fn probe_remote_chat_send_sink_paths() {
    const SUBJECTS: &[&str] = &[
        "send_remote_chat_message",
        "send_agent_message",
        "start_agent_conversation",
    ];
    let graph = CallGraph::build(&load_production_sources());
    let rows = census();
    let commands = rows.iter().map(|(c, _)| c.clone()).collect::<Vec<_>>();
    let detector_b = spawn_triggering_writers(&graph, commands, SPAWN_TRIGGERING_STATE_SURFACE);
    let modules = rows.into_iter().collect::<BTreeMap<_, _>>();

    for command in SUBJECTS {
        let Some(module) = modules.get(*command) else {
            eprintln!("PROBE-CHATSEND {command} NOT-IN-CENSUS");
            continue;
        };
        let closure = graph.closure([command.to_string()]);
        let row = policy_for(command, module).expect("ledgered");
        eprintln!(
            "PROBE-CHATSEND {command} module={module} a={} b={} c={} class={:?} caps={:?}",
            closure_is_arming(&closure),
            detector_b.contains(*command),
            tokens_reach_any(&closure.tokens, PROCESS_LAUNCH_SINKS),
            row.class,
            row.capabilities,
        );
        for sinks in [
            ("STEER", &super::authority_audit::STEER_SINKS[..]),
            ("SCHEDULER", &super::authority_audit::SCHEDULER_SINKS[..]),
            ("TRANSITION", &super::authority_audit::TRANSITION_SINKS[..]),
            ("LAUNCH", PROCESS_LAUNCH_SINKS),
        ] {
            let hits = closure
                .tokens
                .iter()
                .filter(|t| tokens_reach_any(&std::iter::once((*t).clone()).collect(), sinks.1))
                .cloned()
                .collect::<Vec<_>>();
            if !hits.is_empty() {
                eprintln!("PROBE-CHATSEND   {command} {} -> {hits:?}", sinks.0);
            }
        }
    }
}

#[test]
#[ignore = "calibration probe"]
fn probe_chat_send_trio_sink_paths() {
    const SUBJECTS: &[&str] = &[
        "send_agent_message",
        "start_agent_conversation",
        "send_chat_message",
        "create_agent_conversation",
        "start_automation",
        "resume_automation",
        "finalize_automation",
        "stop_automation",
        "pause_automation",
    ];
    let graph = CallGraph::build(&load_production_sources());
    let rows = census();
    let commands = rows.iter().map(|(c, _)| c.clone()).collect::<Vec<_>>();
    let detector_b = spawn_triggering_writers(&graph, commands, SPAWN_TRIGGERING_STATE_SURFACE);
    let modules = rows.into_iter().collect::<BTreeMap<_, _>>();

    for command in SUBJECTS {
        let Some(module) = modules.get(*command) else {
            eprintln!("PROBE-TRIO {command} NOT-IN-CENSUS");
            continue;
        };
        let closure = graph.closure([command.to_string()]);
        let row = policy_for(command, module).expect("ledgered");
        eprintln!(
            "PROBE-TRIO {command} module={module} a={} b={} c={} class={:?} caps={:?}",
            closure_is_arming(&closure),
            detector_b.contains(*command),
            tokens_reach_any(&closure.tokens, PROCESS_LAUNCH_SINKS),
            row.class,
            row.capabilities,
        );
        for sinks in [
            ("STEER", &super::authority_audit::STEER_SINKS[..]),
            ("SCHEDULER", &super::authority_audit::SCHEDULER_SINKS[..]),
            ("TRANSITION", &super::authority_audit::TRANSITION_SINKS[..]),
            ("LAUNCH", PROCESS_LAUNCH_SINKS),
        ] {
            let hits = closure
                .tokens
                .iter()
                .filter(|t| tokens_reach_any(&std::iter::once((*t).clone()).collect(), sinks.1))
                .cloned()
                .collect::<Vec<_>>();
            if !hits.is_empty() {
                eprintln!("PROBE-TRIO   {command} {} -> {hits:?}", sinks.0);
            }
        }
    }
}

#[test]
#[ignore = "calibration probe"]
fn probe_b1_module_batch_audit() {
    const B1_MODULES: &[&str] = &[
        "execution_commands",
        "permission_commands",
        "question_commands",
        "task_commands",
        "task_step_commands",
    ];
    let graph = CallGraph::build(&load_production_sources());
    let rows = census();
    let commands = rows.iter().map(|(c, _)| c.clone()).collect::<Vec<_>>();
    let detector_b = spawn_triggering_writers(&graph, commands, SPAWN_TRIGGERING_STATE_SURFACE);

    for (command, module) in &rows {
        if !B1_MODULES.contains(&module.as_str()) {
            continue;
        }
        let closure = graph.closure([command.clone()]);
        let a = closure_is_arming(&closure);
        let b = detector_b.contains(command);
        let c = tokens_reach_any(&closure.tokens, PROCESS_LAUNCH_SINKS);
        let row = policy_for(command, module).expect("ledgered");
        eprintln!(
            "PROBE-B1 {module} {command} a={a} b={b} c={c} class={:?} caps={:?} registered={}",
            row.class,
            row.capabilities,
            find_spec(command).is_some(),
        );
    }
}

// ---------------------------------------------------------------------------------------
// §3.3 backstop #2 — detector (d): a command reaching a content-write sink must declare
// `MutatesAgentConsumedContent`, be conditionally annotated, or carry a reason-coded
// exemption. Before this gate the writer surface was a hand list with nothing behind it: a
// new command writing task_steps/artifacts/review feedback classified by module default and
// its omission from the list was silent.
// ---------------------------------------------------------------------------------------

#[test]
fn detected_content_writers_are_discharged_and_published() {
    let graph = CallGraph::build(&load_production_sources());
    let rows = census();
    let modules = rows.iter().cloned().collect::<BTreeMap<_, _>>();
    let detected = agent_consumed_content_writers(
        &graph,
        rows.iter().map(|(command, _)| command.clone()),
        AGENT_CONSUMED_CONTENT_WRITE_SURFACE,
    );

    // Calibration: the detector must find the known writers and must NOT flag pure readers or
    // the authority-reducing brakes. Without this the gate could pass by deriving nothing.
    for command in [
        "create_task_step",
        "update_task_step",
        "create_artifact",
        "update_artifact",
        "add_artifact_relation",
        "approve_review",
        "reject_review",
        "request_changes",
    ] {
        assert!(
            detected.contains_key(command),
            "detector (d) missed known content writer {command}"
        );
    }
    for command in [
        "pause_task",
        "block_task",
        "stop_task",
        "list_tasks",
        "health_check",
        "get_task_steps",
        "get_step_progress",
    ] {
        assert!(
            !detected.contains_key(command),
            "detector (d) false-positive on a non-writer: {command}"
        );
    }

    let published = agent_content_writers()
        .into_iter()
        .filter_map(|row| row["writer"].as_str().map(str::to_string))
        .collect::<BTreeSet<_>>();
    for (command, surfaces) in &detected {
        assert!(
            content_writer_is_discharged(command, &modules),
            "`{command}` writes the agent-consumed content surface {surfaces:?} but declares \
             neither MutatesAgentConsumedContent, nor a conditional annotation, nor a \
             reason-coded CONTENT_WRITE_EXEMPTIONS entry"
        );
        let exempt = CONTENT_WRITE_EXEMPTIONS
            .iter()
            .any(|exemption| exemption.command == command);
        assert!(
            exempt || published.contains(command),
            "`{command}` is a detected content writer but the published writer surface omits \
             it — the manifest and the detector disagree about the same write"
        );
    }

    // An exemption for a command the detector does not flag is dead weight that would outlive
    // the write it was written for.
    for exemption in CONTENT_WRITE_EXEMPTIONS {
        assert!(
            detected.contains_key(exemption.command),
            "content-write exemption `{}` names a command detector (d) does not flag",
            exemption.command
        );
        assert!(
            exemption.reason.len() > 40,
            "content-write exemption `{}` carries no substantive reason",
            exemption.command
        );
    }
}

/// R5-H1 names `set_agent_conversation_workspace_auto_publish` and the
/// `require_workspace_review` setter as detector-(b) writers carrying
/// `SeedsSpawnTriggeringState`. Asserting only their class let the evidence tag be dropped
/// from either row without CI noticing, and let the tag be attached to a command with no
/// detector-(b) evidence at all. Both directions are pinned here.
#[test]
fn seeds_spawn_triggering_state_tags_track_detector_b_evidence() {
    let graph = CallGraph::build(&load_production_sources());
    let rows = census();
    let modules = rows.iter().cloned().collect::<BTreeMap<_, _>>();
    let detector_b = spawn_triggering_writers(
        &graph,
        rows.iter().map(|(command, _)| command.clone()),
        SPAWN_TRIGGERING_STATE_SURFACE,
    );

    for command in [
        "inject_task",
        "resume_automation",
        "finalize_automation",
        "set_agent_conversation_workspace_auto_publish",
        "update_review_settings",
    ] {
        let row = policy_for(command, &modules[command]).expect("pinned writer is ledgered");
        assert!(
            row.capabilities
                .contains(&Capability::SeedsSpawnTriggeringState),
            "`{command}` is an R5-H1-named detector-(b) writer but its ledger row records no \
             SeedsSpawnTriggeringState evidence"
        );
    }

    // Annotation → evidence: nothing may claim the tag without the detector flagging it.
    for (command, module) in &rows {
        let Some(row) = policy_for(command, module) else {
            continue;
        };
        if row
            .capabilities
            .contains(&Capability::SeedsSpawnTriggeringState)
        {
            assert!(
                detector_b.contains(command),
                "`{command}` carries SeedsSpawnTriggeringState but detector (b) finds no \
                 spawn-triggering write behind it"
            );
        }
    }
}

/// P-3's discharging proof: no registered facade target may reach a corrective transition or a
/// raw `internal_status` mutator. Reaching a transition sink makes a command AgentControl; it
/// does not make it registrable, and until now only a doc comment said so.
#[test]
fn no_registered_facade_target_reaches_a_corrective_transition() {
    const CORRECTIVE_SINKS: &[&str] = &[
        "transition_task_corrective",
        "transition_task_corrective_with_exit",
        "apply_corrective_transition",
    ];
    // The one shipped deviation, named rather than hidden: `move_task`'s terminal-restart
    // branch reaches a corrective jump through `restart_terminal_task_to_ready`, a sanctioned
    // mediator that fixes its own target (Ready) instead of taking one from the caller. The
    // exemption is per-command so a NEW registration that reaches a corrective sink still
    // fails, which is what P-3 exists to prevent.
    const MEDIATED_CORRECTIVE_REACH: &[(&str, &str)] = &[(
        "move_task",
        "restart_terminal_task_to_ready pins its own Ready target; the caller cannot choose a \
         corrective destination",
    )];
    let graph = CallGraph::build(&load_production_sources());

    for spec in REMOTE_COMMANDS {
        let target = spec
            .target
            .rsplit("::")
            .next()
            .expect("a facade target names a function");
        let closure = graph.closure([target.to_string()]);
        let reached = closure
            .sink_hits
            .iter()
            .filter(|hit| CORRECTIVE_SINKS.contains(&hit.sink.as_str()))
            .map(|hit| hit.sink.clone())
            .collect::<BTreeSet<_>>();
        let mediated = MEDIATED_CORRECTIVE_REACH
            .iter()
            .any(|(command, _)| *command == spec.name);
        assert!(
            reached.is_empty() || mediated,
            "registered command `{}` resolves into corrective-transition sinks {reached:?}; \
             corrective jumps are repair-path-only and must never be remotely reachable",
            spec.name
        );
    }

    // A stale exemption is as dangerous as a missing gate: it would silently cover a future
    // unmediated reach from the same command.
    for (command, reason) in MEDIATED_CORRECTIVE_REACH {
        let spec = find_spec(command).expect("a mediated-reach exemption names a registered op");
        let target = spec.target.rsplit("::").next().expect("target names a fn");
        let reaches = graph
            .closure([target.to_string()])
            .sink_hits
            .iter()
            .any(|hit| CORRECTIVE_SINKS.contains(&hit.sink.as_str()));
        assert!(
            reaches,
            "mediated-corrective exemption for `{command}` is stale; it no longer reaches a \
             corrective sink"
        );
        assert!(
            reason.len() > 40,
            "`{command}` carries no substantive reason"
        );
    }

    // The denylist must name sinks the audit actually models, or this test guards nothing.
    for sink in CORRECTIVE_SINKS {
        assert!(
            TRANSITION_SINKS.contains(sink),
            "corrective denylist entry `{sink}` is not a modelled transition sink"
        );
    }
}

/// A detector-(a) root exception that no longer names an unresolved command would silently
/// tolerate the next one.
#[test]
fn detector_a_root_exceptions_are_not_stale() {
    let graph = CallGraph::build(&load_production_sources());
    let census = census();
    for (command, reason) in DETECTOR_A_ROOT_EXCEPTIONS {
        assert!(
            census.iter().any(|(name, _)| name == command),
            "detector-(a) root exception `{command}` is not a registered command"
        );
        assert!(
            graph.roots_named(command).is_empty(),
            "detector-(a) root exception `{command}` now resolves; drop the exception"
        );
        assert!(
            reason.len() > 40,
            "`{command}` carries no substantive reason"
        );
    }
}
