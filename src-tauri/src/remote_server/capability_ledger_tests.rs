use std::collections::{BTreeMap, BTreeSet};

use ralphx_remote_protocol::{class_permits, AuditRefusalReason, Capability, RiskClass};

use super::authority_audit::{
    agent_consumed_content_writers, closure_is_arming, load_production_sources,
    parse_registered_commands, repo_root, spawn_triggering_writers, tokens_reach_any, CallGraph,
    StateSurfaceEntry, AGENT_CONSUMED_CONTENT_WRITE_SURFACE, CONTENT_WRITE_EXEMPTIONS,
    PROCESS_LAUNCH_SINKS, SPAWN_TRIGGERING_STATE_SURFACE, TRANSITION_SINKS,
};
use super::capability_ledger::{
    audit_refusal_for, policy_for, AUDIT_REFUSALS, AUTHORITY_REDUCING_EXEMPTIONS,
    COMMAND_OVERRIDES, CONDITIONAL_CAPABILITIES, DECLARED_MEMBERSHIPS, MODULE_DEFAULTS,
    SCOPE_CONFINEMENTS,
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
            let mut row_json = serde_json::json!({
                "command": command,
                "module": module,
                "class": row.class,
                "capabilities": row.capabilities,
                "reason": row.reason,
                "registered": find_spec(command).is_some(),
                // P-11 batch B0: the third disposition. Rendered from the row rather than
                // re-derived downstream, so the drift scan and the census read one authority.
                //
                // PR 3.1-b batch 9: the audit-refusal table is the second input. Mechanical
                // refusals still win inside `v1_resolution_with_audit`, so a hand-written row
                // can never downgrade a proven denial.
                "v1Resolution": ralphx_remote_protocol::v1_resolution_with_audit(
                    row.class,
                    row.capabilities,
                    audit_refusal_for(command).is_some(),
                ),
            });
            // Rendered alongside the resolution, never instead of it: `v1-audit-refused` is
            // only as trustworthy as the finding behind it, so the finding ships too.
            //
            // OMITTED rather than rendered `null` for the 535 rows that have none. An always-on
            // field would put 535 lines of noise into every future manifest diff, which is a
            // real cost on an artifact whose whole job is to be reviewed by hand.
            if let Some(refusal) = audit_refusal_for(command) {
                row_json["auditRefusal"] = serde_json::json!({
                    "reason": refusal.reason,
                    "finding": refusal.finding,
                    "batch": refusal.batch,
                });
            }
            row_json
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
                // A registered command can also refuse a well-scoped device for an argument
                // reason. Published for the same reason `pins` is: a refusal reachable on the
                // wire must be discoverable from the manifest, not only from the source.
                "scopeConfined": spec.validate.is_some(),
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

    let scope_confinements = SCOPE_CONFINEMENTS
        .iter()
        .map(|entry| {
            serde_json::json!({
                "command": entry.command,
                "argument": entry.argument,
                "reason": entry.reason,
            })
        })
        .collect::<Vec<_>>();

    serde_json::json!({
        "schemaVersion": 2,
        "background_loop_inventory": background_loop_inventory,
        "facade_ops": facade_ops,
        "conditional_capabilities": conditional_capabilities,
        "scope_confinements": scope_confinements,
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

/// P-11 batch B0 — the manifest is the third classification source.
///
/// The drift scan resolves a frontend invoke name it cannot find in `remote_commands!` or in
/// `local-only-commands.ts` by reading this row's `v1Resolution`. Two invariants make that
/// safe, and both are asserted here rather than in the scan:
///
/// * **totality** — every ledger row renders a resolution, so "absent field" can never read as
///   "classified";
/// * **non-contradiction** — a row that renders a refusal must not also be registered. A
///   registered-and-denied row would let the scan classify a name the facade actually serves.
#[test]
fn manifest_renders_a_non_contradictory_v1_resolution_for_every_row() {
    let manifest = generated_manifest();
    let ledger = manifest["ledger"]
        .as_array()
        .expect("manifest renders a ledger array");
    assert!(!ledger.is_empty(), "ledger must not be empty");

    let mut counts: BTreeMap<String, usize> = BTreeMap::new();
    for row in ledger {
        let command = row["command"].as_str().expect("row names a command");
        let resolution = row["v1Resolution"]
            .as_str()
            .unwrap_or_else(|| panic!("row `{command}` renders no v1Resolution"));
        assert!(
            matches!(
                resolution,
                "registerable"
                    | "host-denied"
                    | "host-denied-spawns-process"
                    | "v1-deferred"
                    | "v1-audit-refused"
            ),
            "row `{command}` renders an unknown v1Resolution `{resolution}`"
        );
        if resolution != "registerable" {
            assert_eq!(
                row["registered"],
                serde_json::Value::Bool(false),
                "`{command}` is registered but renders `{resolution}` — the ledger and the \
                 registry contradict each other, and the drift scan would classify a name the \
                 facade actually serves"
            );
        }
        *counts.entry(resolution.to_string()).or_default() += 1;
    }

    // Each refusal class is non-empty: a scan that only ever sees `registerable` would pass
    // this test while classifying nothing.
    for resolution in [
        "host-denied",
        "host-denied-spawns-process",
        "v1-deferred",
        "v1-audit-refused",
    ] {
        assert!(
            counts.get(resolution).copied().unwrap_or_default() > 0,
            "no ledger row renders `{resolution}`"
        );
    }

    // PR 3.1-b batch 9 — `v1-audit-refused` is the one resolution a human can grant by writing
    // a table row, so the manifest must show the row and the finding together or not at all.
    for row in ledger {
        let command = row["command"].as_str().expect("row names a command");
        let audit_refused = row["v1Resolution"] == "v1-audit-refused";
        let has_finding = row["auditRefusal"].is_object();
        assert_eq!(
            has_finding,
            audit_refusal_for(command).is_some(),
            "`{command}` renders auditRefusal={:?} but the ledger table disagrees; the manifest \
             must mirror the table exactly",
            row["auditRefusal"],
        );
        assert!(
            !audit_refused || has_finding,
            "`{command}` renders `v1-audit-refused` with no finding; that would be an \
             unfalsifiable classification"
        );
        // The converse is deliberately NOT asserted: a command may carry BOTH an audit finding
        // and a mechanical denial, and `v1_resolution_with_audit` renders the mechanical one.
        // Requiring the audit resolution there would let a table row mask a proven denial.
    }
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
        // 541 -> 544: batch 4's three spawn-free transcript reads. Bumped because commands were
        // ADDED to the census, not because the detector changed; all three are detector-silent
        // by construction and `remote_transcript_reads_never_reach_the_wake` proves it.
        // 544 -> 546: batch 5's two spawn-free conversation-list reads, same reasoning, with
        // `remote_conversation_list_reads_carry_no_spawn_authority` as the proof.
        // 546 -> 548: the two merged frontend commands dispositioned here. This is a census
        // count change; their mechanisms are pinned independently below.
        // 548 -> 551: three commands were added to `generate_handler!` on this branch after the
        // 548 comment without bumping this literal; reconciled here to the measured census.
        // 551 -> 552: the spawn-free Agents-sidebar inbox read `list_remote_agent_sidebar_conversations`.
        // Detector-silent by construction (recovery-free hydrator seam);
        // `remote_agent_sidebar_read_carries_no_spawn_authority` is the proof.
        // 552 -> 553: the spawn-free remote provider projection `list_remote_agent_providers`.
        // A pure `agent_provider_settings_repo.list()` read + struct mapping; carries no
        // AppHandle/ExecutionState/ChatService, so it is detector-silent by construction, the
        // same clearance class as the two `remote_workspace_commands` reads it sits beside.
        // 553 -> 555: the spawn-free conversation-start pair. `request_remote_agent_conversation_start`
        // persists a start intent (detector (b) flags it via the `remote-conversation-start`
        // surface row; silent on (a)/(c)); `get_remote_conversation_start_request` is a pure read.
        555,
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

    for (index, entry) in SPAWN_TRIGGERING_STATE_SURFACE.iter().enumerate() {
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
    // PR 3.1-b batch 10 relaxed this from a VERBATIM equality to the `contains` form its
    // `resolve_permission_request` sibling four lines above already used, and the relaxation is
    // the point rather than a convenience. The verbatim pin meant the row could carry the
    // membership OR the finding, never both — which is exactly what left batch 9 unable to
    // correct a class it had measured as wrong. `contains` keeps the membership mandatory while
    // letting the row also state why it is classified as it is; the class is now asserted
    // independently, so nothing is weakened by the change.
    let question = policy_for("resolve_user_question", "question_commands").unwrap();
    assert_eq!(
        question.class,
        RiskClass::Elevated,
        "batch 10 corrected the understated class; see \
         batch10_closes_the_declared_membership_process_launch_gap"
    );
    assert!(question.reason.contains(DECLARED_MEMBERSHIPS[1].1));

    // Every declared membership must be carried by the ledger row it describes, or the
    // declaration and the policy could drift apart silently. Asserted over the whole table
    // rather than per-row, so a future membership cannot be added without this holding.
    let rows_by_command = census().into_iter().collect::<BTreeMap<_, _>>();
    for (command, membership) in DECLARED_MEMBERSHIPS {
        // Facade-only names (`approve_permission_request` is a pinned alias for
        // `resolve_permission_request`, not a Tauri command) have no census row of their own,
        // and their membership is asserted against the target row above.
        let Some(module) = rows_by_command.get(*command) else {
            assert!(
                find_spec(command).is_some(),
                "declared membership `{command}` is neither a census command nor a facade op"
            );
            continue;
        };
        let row = policy_for(command, module)
            .unwrap_or_else(|| panic!("declared membership `{command}` is not ledgered"));
        assert!(
            row.reason.contains(membership),
            "`{command}` declares membership `{membership}` but its ledger reason does not carry \
             it; the declaration and the policy would drift apart with nothing failing"
        );
    }
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
        // Batch 10 corrected this row from `AgentControl`/`AGENT` to `Elevated`/`SpawnsProcess`
        // after measuring that answering a live gate resumes the agent turn through three CLI
        // resolvers. The representative capability moves with it: `SpawnsProcess` is now the
        // capability whose loss would lower the membership, and it is a STRONGER guard than the
        // one it replaces — stripping it drops the row out of `host-denied-spawns-process`
        // entirely rather than merely down a scope.
        (
            "resolve_user_question",
            "question_commands",
            Capability::SpawnsProcess,
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
        // PR 3.1-b batch 7 — census `B3` read cluster. `get_task_validation_summary` is
        // deliberately ABSENT and asserted as a spawner just below: detector (c) fires on it
        // once same-name delegation edges are kept, which is exactly why it is not here.
        "get_pending_reviews",
        "get_review_by_id",
        "get_reviews_by_task_id",
        "get_task_state_history",
        "get_fix_task_attempts",
        "get_task_issues",
        "get_issue_progress",
        "get_review_settings",
        "get_qa_settings",
        "get_task_qa",
        "get_qa_results",
        "get_merge_pipeline",
        "get_merge_progress",
        "get_merge_phase_list",
    ] {
        assert!(
            !spawners.contains(command),
            "detector (c) false-positive on registered read {command}"
        );
    }

    // PR 3.1-b batch 7 — the member the corrected call graph rejected. Calibration in the
    // positive direction: if this ever stops firing, either the census §5.1 caching remedy
    // landed (and the row can be reclassified) or the delegation edge was lost again.
    assert!(
        spawners.contains("get_task_validation_summary"),
        "detector (c) must fire on `get_task_validation_summary`: it delegates to \
         `TaskValidationService::get_task_validation_summary`, which shells out to \
         `git rev-parse HEAD` for the head-sha stamp"
    );
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
    // PR 3.1-b batch 10 REGISTERED it — at `ui:agent`, never at the `Read` class this test
    // guards. The exclusion recorded above was always "not a getter", and that claim is
    // unchanged and still asserted by the class check; what batch 10 added is the arming audit
    // the row needed to be served at the arming scope. The surviving invariant is the one this
    // test exists for: a scheduler-quota write must never be reachable from the default
    // `ui:read`+`ui:operate` pairing.
    let spec = find_spec("set_active_project")
        .expect("batch 10 registered set_active_project at ui:agent");
    assert_eq!(
        spec.class,
        RiskClass::AgentControl,
        "set_active_project syncs the execution concurrency quota; registering it anywhere a \
         default pairing can reach would expose scheduler arming to a viewer"
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
            ("STEER", super::authority_audit::STEER_SINKS),
            ("SCHEDULER", super::authority_audit::SCHEDULER_SINKS),
            ("TRANSITION", super::authority_audit::TRANSITION_SINKS),
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
            ("STEER", super::authority_audit::STEER_SINKS),
            ("SCHEDULER", super::authority_audit::SCHEDULER_SINKS),
            ("TRANSITION", super::authority_audit::TRANSITION_SINKS),
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

/// Calibration probe for census `B4` — ideation, plans, methodology, workflow.
///
/// Same shape as [`probe_b3_module_batch_audit`]. Worth re-running rather than reusing any
/// pre-batch-7 output: before the same-name delegation fix, `ideation_commands` and
/// `workflow_commands` were among the heaviest users of the command → identically-named
/// service shape, so their old detector verdicts were vacuous.
#[test]
#[ignore = "calibration probe"]
fn probe_b4_module_batch_audit() {
    const B4_MODULES: &[&str] = &[
        "agent_plan_commands",
        "ideation_commands",
        "methodology_commands",
        "plan_commands",
        "workflow_commands",
    ];
    let graph = CallGraph::build(&load_production_sources());
    let rows = census();
    let commands = rows.iter().map(|(c, _)| c.clone()).collect::<Vec<_>>();
    let detector_b = spawn_triggering_writers(&graph, commands, SPAWN_TRIGGERING_STATE_SURFACE);

    for (command, module) in &rows {
        if !B4_MODULES.contains(&module.as_str()) {
            continue;
        }
        let closure = graph.closure([command.clone()]);
        let a = closure_is_arming(&closure);
        let b = detector_b.contains(command);
        let c = tokens_reach_any(&closure.tokens, PROCESS_LAUNCH_SINKS);
        let row = policy_for(command, module).expect("ledgered");
        eprintln!(
            "PROBE-B4 {module} {command} a={a} b={b} c={c} class={:?} caps={:?} registered={}",
            row.class,
            row.capabilities,
            find_spec(command).is_some(),
        );
    }
}

/// Sink evidence behind every census-`B4` detector-(c) hit.
///
/// [`probe_b4_module_batch_audit`] prints a boolean; batch 11 classifies twelve B4 members as
/// `host-denied-spawns-process` on the strength of that boolean, and a manifest reason that
/// asserts "reaches a process launch" has to be argued against a concrete resolver token rather
/// than a `true`. This prints the launcher tokens inside each member's own closure so the
/// classification can be checked by reading it, and so a member that STOPS reaching a launcher
/// shows up as an empty list instead of silently keeping a stale refusal.
#[test]
#[ignore = "calibration probe"]
fn probe_b4_detector_c_sink_evidence() {
    const B4_MODULES: &[&str] = &[
        "agent_plan_commands",
        "ideation_commands",
        "methodology_commands",
        "plan_commands",
        "workflow_commands",
    ];
    let graph = CallGraph::build(&load_production_sources());
    for (command, module) in &census() {
        if !B4_MODULES.contains(&module.as_str()) {
            continue;
        }
        let closure = graph.closure([command.clone()]);
        if !tokens_reach_any(&closure.tokens, PROCESS_LAUNCH_SINKS) {
            continue;
        }
        let launchers = closure
            .tokens
            .iter()
            .filter(|t| {
                tokens_reach_any(
                    &std::iter::once((*t).clone()).collect(),
                    PROCESS_LAUNCH_SINKS,
                )
            })
            .cloned()
            .collect::<Vec<_>>();
        eprintln!(
            "PROBE-B4C {command} visited={} launchers={launchers:?}",
            closure.tokens.len()
        );
    }
}

/// Calibration probe for census `B3` — review, QA, merge pipeline, validation.
///
/// Same shape as [`probe_b1_module_batch_audit`]: prints the live detector (a)/(b)/(c) verdict
/// per member alongside its ledger row, plus the concrete sink tokens behind every hit so the
/// hand audit can be argued against a path rather than a boolean. Run before the batch decides
/// which members register and which are refused.
#[test]
#[ignore = "calibration probe"]
fn probe_b3_module_batch_audit() {
    const B3_MODULES: &[&str] = &[
        "merge_pipeline_commands",
        "qa_commands",
        "review_commands",
        "validation_commands",
    ];
    let graph = CallGraph::build(&load_production_sources());
    let rows = census();
    let commands = rows.iter().map(|(c, _)| c.clone()).collect::<Vec<_>>();
    let detector_b = spawn_triggering_writers(&graph, commands, SPAWN_TRIGGERING_STATE_SURFACE);

    for (command, module) in &rows {
        if !B3_MODULES.contains(&module.as_str()) {
            continue;
        }
        let closure = graph.closure([command.clone()]);
        let a = closure_is_arming(&closure);
        let b = detector_b.contains(command);
        let c = tokens_reach_any(&closure.tokens, PROCESS_LAUNCH_SINKS);
        let row = policy_for(command, module).expect("ledgered");
        eprintln!(
            "PROBE-B3 {module} {command} a={a} b={b} c={c} class={:?} caps={:?} registered={}",
            row.class,
            row.capabilities,
            find_spec(command).is_some(),
        );
        for sinks in [
            ("STEER", super::authority_audit::STEER_SINKS),
            ("SCHEDULER", super::authority_audit::SCHEDULER_SINKS),
            ("TRANSITION", super::authority_audit::TRANSITION_SINKS),
            ("LAUNCH", PROCESS_LAUNCH_SINKS),
        ] {
            let hits = closure
                .tokens
                .iter()
                .filter(|t| tokens_reach_any(&std::iter::once((*t).clone()).collect(), sinks.1))
                .cloned()
                .collect::<Vec<_>>();
            if !hits.is_empty() {
                eprintln!("PROBE-B3   {command} {} -> {hits:?}", sinks.0);
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

// ---------------------------------------------------------------------------------------
// PR 3.1-b batch 3 — the Operate-brakes probe.
//
// Batch 2 §5 pre-scoped four halting candidates out of the `B1` remainder. This probe is the
// mechanical first half of their audit: it prints detectors (a)/(b)/(c)/(d) plus the live
// ledger row for each, so the reclassification below starts from measured output rather than
// from batch 2's recollection of it. Detector silence is necessary, never sufficient — the
// `set_active_project` exclusion in batch 2 was a detector-silent command disqualified only by
// hand-tracing, and this batch repeats that discipline against `sync_quota_from_project`.
// ---------------------------------------------------------------------------------------
#[test]
#[ignore = "calibration probe"]
fn probe_operate_brakes_audit() {
    const CANDIDATES: &[&str] = &[
        "pause_execution",
        "stop_execution",
        "cancel_tasks_in_group",
        "archive_tasks_in_group",
        // Registered siblings, as calibration: the probe must reproduce the known-good rows.
        "pause_task",
        "stop_task",
        "pause_tasks_in_group",
        // Batch 2's recorded exclusion, as negative calibration.
        "set_active_project",
    ];
    let graph = CallGraph::build(&load_production_sources());
    let rows = census();
    let commands = rows.iter().map(|(c, _)| c.clone()).collect::<Vec<_>>();
    let detector_b =
        spawn_triggering_writers(&graph, commands.clone(), SPAWN_TRIGGERING_STATE_SURFACE);
    let detector_d =
        agent_consumed_content_writers(&graph, commands, AGENT_CONSUMED_CONTENT_WRITE_SURFACE);

    for (command, module) in &rows {
        if !CANDIDATES.contains(&command.as_str()) {
            continue;
        }
        let closure = graph.closure([command.clone()]);
        let a = closure_is_arming(&closure);
        let b = detector_b.contains(command);
        let c = tokens_reach_any(&closure.tokens, PROCESS_LAUNCH_SINKS);
        let d = detector_d.contains_key(command);
        let t = tokens_reach_any(&closure.tokens, TRANSITION_SINKS);
        let row = policy_for(command, module).expect("ledgered");
        eprintln!(
            "PROBE-BRAKES {module} {command} a={a} b={b} c={c} d={d} transition={t} \
             class={:?} caps={:?} registered={} reason={:?}",
            row.class,
            row.capabilities,
            find_spec(command).is_some(),
            row.reason,
        );
    }
}

/// The batch-3 brakes are REVIEWED rows, not inherited module defaults.
///
/// Batch 1 could not register anything because every candidate sat at the conservative
/// `task_commands` / `execution_commands` default. A row that merely "looks classified" while
/// still carrying that default reason is exactly the condition that blocked it, so the reason
/// string is asserted to have moved — and the module defaults are asserted NOT to have, since
/// these are exceptions and not a verdict on modules that still hold `move_task`,
/// `unblock_task`, `inject_task`, `restart_task` and the execution-plan controls.
#[test]
fn batch3_brake_reclassifications_are_reviewed_rather_than_module_defaults() {
    let rows = census().into_iter().collect::<BTreeMap<_, _>>();

    for command in ["pause_execution", "stop_execution"] {
        let module = rows.get(command).expect("brake is a live command");
        let row = policy_for(command, module).expect("brake is ledgered");

        assert_eq!(
            row.class,
            RiskClass::Operate,
            "`{command}` must be ledgered Operate"
        );
        assert!(
            row.capabilities.is_empty(),
            "`{command}` must carry no capability; `Operate` permits none, so a non-empty set \
             here is the under-labelling signature"
        );
        assert!(
            !row.reason.contains("conservative-module-default"),
            "`{command}` still carries the module-default reason; it was not reviewed"
        );
        assert!(
            row.reason.contains("authority-reducing"),
            "`{command}` must record WHY it sits below its module default"
        );
        assert!(
            find_spec(command).is_some(),
            "`{command}` is reclassified but not registered"
        );

        // The exemption is what licenses the drop below the module default, and it must carry
        // file-anchored evidence rather than a bare claim.
        let exemption = AUTHORITY_REDUCING_EXEMPTIONS
            .iter()
            .find(|entry| entry.kind == "command" && entry.subject == command)
            .unwrap_or_else(|| {
                panic!(
                    "`{command}` sits below its module default with no \
                                       authority-reducing exemption"
                )
            });
        assert_eq!(exemption.scope, "ui:operate");
        assert!(
            exemption.rationale.contains(".rs"),
            "`{command}` exemption must carry file-anchored evidence"
        );
    }

    // The modules stay conservative. If either default ever drops, these two stop being
    // exceptions and the audit that justified them no longer applies.
    for module in ["execution_commands", "task_commands"] {
        let default = MODULE_DEFAULTS
            .iter()
            .find(|entry| entry.module == module)
            .expect("module has a default");
        assert_eq!(
            default.policy.class,
            RiskClass::AgentControl,
            "`{module}` default weakened; the batch-3 brakes were registered as EXCEPTIONS to it"
        );
    }
}

/// The refusal, pinned.
///
/// `archive_tasks_in_group` is the batch-3 counterpart of batch 2's `set_active_project`: it
/// audits detector-silent, sits in the same module as the bulk brakes, takes the same
/// `(groupKind, groupId, projectId)` argument shape, and is NOT a brake.
///
/// Archiving writes `archived_at` and emits an event. That is all. There is no
/// `InternalStatus::Archived`, so the ledger's `Archived` transition-target exemption never
/// reaches this command, and the probe shows it as the only candidate in the cluster with NO
/// transition sink at all. An Executing task that is archived keeps its agent process, keeps
/// its execution slot, disappears from every reconciliation query (which filter
/// `archived_at IS NULL`), and can no longer be recovered — `transition_task` refuses archived
/// tasks outright. Authority-OBSCURING, not authority-reducing.
///
/// Without this test, "it is a bulk group op in a module whose bulk group ops are registered"
/// could later be treated as sufficient grounds.
#[test]
fn bulk_archive_is_not_a_brake_and_stays_unregistered() {
    let rows = census().into_iter().collect::<BTreeMap<_, _>>();
    let module = rows
        .get("archive_tasks_in_group")
        .expect("archive_tasks_in_group is a live command");
    let row = policy_for("archive_tasks_in_group", module).expect("ledgered");

    assert_eq!(
        row.class,
        RiskClass::AgentControl,
        "archive_tasks_in_group must stay at its conservative module default"
    );
    assert!(
        find_spec("archive_tasks_in_group").is_none(),
        "archive_tasks_in_group was registered; it hides running agents instead of stopping them"
    );
    assert!(
        !AUTHORITY_REDUCING_EXEMPTIONS
            .iter()
            .any(|entry| entry.kind == "command" && entry.subject == "archive_tasks_in_group"),
        "archive_tasks_in_group must never acquire an authority-reducing exemption"
    );

    // The structural reason, asserted against the source rather than remembered in a comment:
    // the bulk archive never reaches the state machine, while its registered siblings do.
    let mutation = include_str!("../commands/task_commands/mutation.rs");
    let archive_body = mutation
        .split("pub async fn archive_tasks_in_group")
        .nth(1)
        .expect("archive_tasks_in_group is defined in mutation.rs")
        .split("\npub async fn ")
        .next()
        .expect("the body is delimited by the next command");
    assert!(
        !archive_body.contains("transition_task"),
        "archive_tasks_in_group now transitions; re-run the audit — the refusal rested on it \
         writing archived_at without quiescing the task"
    );
    assert!(
        !archive_body.contains("running_agent_registry"),
        "archive_tasks_in_group now touches the agent registry; re-run the audit"
    );

    let cancel_body = mutation
        .split("pub async fn cancel_tasks_in_group")
        .nth(1)
        .expect("cancel_tasks_in_group is defined in mutation.rs")
        .split("\npub async fn ")
        .next()
        .expect("the body is delimited by the next command");
    assert!(
        cancel_body.contains("InternalStatus::Cancelled"),
        "calibration: the registered bulk brake must still transition to Cancelled, or the \
         contrast this refusal rests on has evaporated"
    );
    assert!(
        cancel_body.contains("is_terminal"),
        "calibration: the registered bulk brake must still skip already-terminal tasks"
    );

    // And there is no `Archived` status for the transition-target exemption to have covered.
    assert!(
        "cancelled"
            .parse::<crate::domain::entities::InternalStatus>()
            .is_ok(),
        "calibration: `cancelled` must parse, or the negative below proves nothing about \
         `archived` specifically"
    );
    assert!(
        "archived"
            .parse::<crate::domain::entities::InternalStatus>()
            .is_err(),
        "an InternalStatus::Archived appeared; the `Archived` transition-target exemption may \
         now genuinely apply and this refusal must be re-audited"
    );
}

// ---------------------------------------------------------------------------------------
// PR 3.1-b batch 3 — census `B2` reconnaissance probe.
//
// `B2` is the census's highest-risk batch: 51 register-candidates across six modules, and it
// contains both the detector-(a) steer sink (`send_agent_message`) and the workspace-publish
// `git push` surface. This probe is the mechanical first half only. It registers nothing and
// decides nothing; it exists so the batch that takes `B2` starts from measured detector output
// instead of from the census's prose.
// ---------------------------------------------------------------------------------------
#[test]
#[ignore = "calibration probe"]
fn probe_b2_module_batch_audit() {
    const B2_MODULES: &[&str] = &[
        "agent_composer_commands",
        "agent_model_commands",
        "agent_sidebar_commands",
        "conversation_folder_reference_commands",
        "conversation_stats_commands",
        "unified_chat_commands",
    ];
    let graph = CallGraph::build(&load_production_sources());
    let rows = census();
    let commands = rows.iter().map(|(c, _)| c.clone()).collect::<Vec<_>>();
    let detector_b =
        spawn_triggering_writers(&graph, commands.clone(), SPAWN_TRIGGERING_STATE_SURFACE);
    let detector_d =
        agent_consumed_content_writers(&graph, commands, AGENT_CONSUMED_CONTENT_WRITE_SURFACE);

    for (command, module) in &rows {
        if !B2_MODULES.contains(&module.as_str()) {
            continue;
        }
        let closure = graph.closure([command.clone()]);
        let a = closure_is_arming(&closure);
        let b = detector_b.contains(command);
        let c = tokens_reach_any(&closure.tokens, PROCESS_LAUNCH_SINKS);
        let d = detector_d.contains_key(command);
        let t = tokens_reach_any(&closure.tokens, TRANSITION_SINKS);
        let row = policy_for(command, module).expect("ledgered");
        eprintln!(
            "PROBE-B2 {module} {command} a={a} b={b} c={c} d={d} transition={t} class={:?} registered={}",
            row.class,
            find_spec(command).is_some(),
        );
    }
}

/// The `B2` stats reclassifications are reviewed rows, and the module stays conservative.
#[test]
fn b2_stats_reclassifications_are_reviewed_rather_than_module_defaults() {
    let rows = census().into_iter().collect::<BTreeMap<_, _>>();

    for command in [
        "get_agent_conversation_stats",
        "get_project_chat_usage_stats",
        "get_task_chat_usage_stats",
        "get_insights_chat_usage_stats",
    ] {
        let module = rows.get(command).expect("stats command is live");
        assert_eq!(
            module, "conversation_stats_commands",
            "`{command}` moved module; re-run the audit"
        );
        let row = policy_for(command, module).expect("ledgered");
        assert_eq!(row.class, RiskClass::Read);
        assert!(row.capabilities.is_empty());
        assert!(
            !row.reason.contains("conservative-module-default"),
            "`{command}` still carries the module-default reason; it was not reviewed"
        );
        // The fail-open shape is what kept two batch-1 candidates unregistered. Each reason
        // records that this cluster does not have it.
        assert!(
            row.reason.contains("propagates read errors"),
            "`{command}` must record that its reads fail closed"
        );
        assert!(find_spec(command).is_some());
    }

    let default = MODULE_DEFAULTS
        .iter()
        .find(|entry| entry.module == "conversation_stats_commands")
        .expect("module has a default");
    assert_eq!(
        default.policy.class,
        RiskClass::AgentControl,
        "the module default weakened; these four were registered as EXCEPTIONS to it"
    );

    // `unified_chat_commands` is emphatically untouched by this batch.
    let chat_default = MODULE_DEFAULTS
        .iter()
        .find(|entry| entry.module == "unified_chat_commands")
        .expect("module has a default");
    assert_eq!(chat_default.policy.class, RiskClass::AgentControl);
}

/// Diagnostic: shortest call path from a command root to each ARMING sink hit.
///
/// Detector (a) reports only a boolean. For the PR 3.2 transcript reads the question is not
/// *whether* they arm but *where* the arming edge enters, so this walks the same graph the
/// detector walks and prints the path.
#[test]
#[ignore = "diagnostic probe"]
fn probe_transcript_read_arming_paths() {
    use super::authority_audit::{verdict_for, HitVerdict};
    use std::collections::VecDeque;

    let graph = CallGraph::build(&load_production_sources());
    let sinks: BTreeSet<String> = TRANSITION_SINKS
        .iter()
        .map(|s| (*s).to_string())
        .collect::<BTreeSet<_>>();

    for command in [
        "get_agent_conversation",
        "get_agent_conversation_messages_page",
        "get_agent_conversation_timeline_page",
        "get_agent_conversation_summary", // negative calibration: detector-silent sibling
        // The pure-read seams the split registers.
        "get_agent_conversation_messages_page_for_app_state",
        "get_agent_conversation_timeline_page_for_app_state",
        "get_conversation_with_messages",
        "wake_agent_workspace_for_bridge_events",
    ] {
        eprintln!("\n===== {command} =====");
        let roots = graph.roots_named(command);
        eprintln!("roots: {roots:?}");

        // BFS with parent tracking, mirroring `CallGraph::closure`'s cut behaviour.
        let mut parent: BTreeMap<String, String> = BTreeMap::new();
        let mut visited: BTreeSet<String> = BTreeSet::new();
        let mut queue: VecDeque<String> = roots.iter().cloned().collect();
        for r in &roots {
            visited.insert(r.clone());
        }
        let mut arming_nodes: Vec<(String, String)> = Vec::new();
        while let Some(name) = queue.pop_front() {
            let Some(node) = graph.nodes.get(&name) else {
                continue;
            };
            for hit in &node.sink_hits {
                if verdict_for(hit) == HitVerdict::Arming {
                    arming_nodes.push((name.clone(), format!("{}{:?}", hit.sink, hit.targets)));
                }
            }
            for callee in &node.callees {
                if sinks.contains(callee.as_str()) || visited.contains(callee) {
                    continue;
                }
                visited.insert(callee.clone());
                parent.insert(callee.clone(), name.clone());
                queue.push_back(callee.clone());
            }
        }

        if arming_nodes.is_empty() {
            eprintln!("  NO ARMING HITS");
        }
        for (node, hit) in &arming_nodes {
            let mut path = vec![node.clone()];
            let mut cur = node.clone();
            while let Some(p) = parent.get(&cur) {
                path.push(p.clone());
                cur = p.clone();
            }
            path.reverse();
            eprintln!("  ARMING sink={hit}\n    path: {}", path.join(" -> "));
        }
    }
}

/// Diagnostic: the same walk as `probe_transcript_read_arming_paths`, for the conversation
/// LIST reads batch 4 deferred (batch 5 item 1).
///
/// A transcript read without a list to pick from is useless, so these two complete PR 3.2's
/// read surface. Batch 4 deferred them on the `tauri::AppHandle` carrier; this walk is what
/// establishes where — and whether — the arming edge actually enters.
#[test]
#[ignore = "diagnostic probe"]
fn probe_conversation_list_arming_paths() {
    use super::authority_audit::{verdict_for, HitVerdict};
    use std::collections::VecDeque;

    let graph = CallGraph::build(&load_production_sources());
    let sinks: BTreeSet<String> = TRANSITION_SINKS
        .iter()
        .map(|s| (*s).to_string())
        .collect::<BTreeSet<_>>();

    for command in [
        "list_agent_conversations",
        "list_agent_conversations_page",
        // The pure seams this batch extracts.
        "list_agent_conversations_for_app_state",
        "list_agent_conversations_page_for_app_state",
        // Shared helpers both list reads funnel through.
        "filter_agent_list_visible_conversations",
        "agent_conversation_responses_for_state",
        "create_chat_service",
    ] {
        eprintln!("\n===== {command} =====");
        let roots = graph.roots_named(command);
        eprintln!("roots: {roots:?}");

        let mut parent: BTreeMap<String, String> = BTreeMap::new();
        let mut visited: BTreeSet<String> = BTreeSet::new();
        let mut queue: VecDeque<String> = roots.iter().cloned().collect();
        for r in &roots {
            visited.insert(r.clone());
        }
        let mut arming_nodes: Vec<(String, String)> = Vec::new();
        while let Some(name) = queue.pop_front() {
            let Some(node) = graph.nodes.get(&name) else {
                continue;
            };
            for hit in &node.sink_hits {
                if verdict_for(hit) == HitVerdict::Arming {
                    arming_nodes.push((name.clone(), format!("{}{:?}", hit.sink, hit.targets)));
                }
            }
            for callee in &node.callees {
                if sinks.contains(callee.as_str()) || visited.contains(callee) {
                    continue;
                }
                visited.insert(callee.clone());
                parent.insert(callee.clone(), name.clone());
                queue.push_back(callee.clone());
            }
        }

        if arming_nodes.is_empty() {
            eprintln!("  NO ARMING HITS");
        }
        for (node, hit) in &arming_nodes {
            let mut path = vec![node.clone()];
            let mut cur = node.clone();
            while let Some(p) = parent.get(&cur) {
                path.push(p.clone());
                cur = p.clone();
            }
            path.reverse();
            eprintln!("  ARMING sink={hit}\n    path: {}", path.join(" -> "));
        }
    }
}

/// Diagnostic: detector verdicts for the batch-5 B2 read-shaped candidates.
#[test]
#[ignore = "diagnostic probe"]
fn probe_b2_remaining_read_candidates() {
    let graph = CallGraph::build(&load_production_sources());

    for command in [
        "get_agent_conversation_workspace",
        "get_agent_conversation_workspace_freshness",
        "get_agent_conversation_workspace_freshness_for_app_state",
        "is_chat_service_available",
        "list_agent_conversation_workspaces_by_project",
        "list_agent_sidebar_conversations",
        "list_agent_sidebar_conversations_for_app_state",
        "search_agent_composer_entries",
        "agent_workspace_response_for_state",
        "schedule_external_pr_reconciliation_for_workspace",
    ] {
        let closure = graph.closure([command.to_string()]);
        eprintln!(
            "{command}: visited={} arming={} launch={} transition={}",
            closure.visited.len(),
            closure_is_arming(&closure),
            tokens_reach_any(&closure.tokens, PROCESS_LAUNCH_SINKS),
            tokens_reach_any(&closure.tokens, TRANSITION_SINKS),
        );
    }
}

/// The scope-confinement annotation and its predicate are inseparable (batch 4).
///
/// Mirrors `conditional_capabilities_are_discharged_by_a_live_predicate`. Asserted in BOTH
/// directions so neither half can be dropped alone:
///
/// * annotation → predicate: every `SCOPE_CONFINEMENTS` row is registered and carries a
///   `validate:` predicate. An annotation over a command with no predicate is a comment.
/// * predicate → annotation: every registered spec carrying a `validate:` predicate has a
///   `SCOPE_CONFINEMENTS` row. A predicate with no annotation is an undocumented refusal that
///   a client author cannot discover.
#[test]
fn scope_confinements_are_enforced_by_a_live_predicate() {
    for entry in SCOPE_CONFINEMENTS {
        let spec = find_spec(entry.command).unwrap_or_else(|| {
            panic!(
                "`{}` is annotated scope-confined but is not registered; \
                 an annotation on an unreachable command proves nothing",
                entry.command
            )
        });
        assert!(
            spec.validate.is_some(),
            "`{}` is annotated scope-confined but has no `validate:` predicate — the annotation \
             would be the only thing standing between a default-paired device and the \
             all-projects sweep",
            entry.command
        );
        // The reason must record the limit, not just the win. A confinement described as
        // total when it is partial is the false-success shape this ledger exists to prevent.
        assert!(
            entry.reason.contains("NOT"),
            "`{}`'s reason must state what the confinement does NOT cover",
            entry.command
        );
    }

    let annotated = SCOPE_CONFINEMENTS
        .iter()
        .map(|entry| entry.command)
        .collect::<BTreeSet<_>>();
    for spec in REMOTE_COMMANDS
        .iter()
        .filter(|spec| spec.validate.is_some())
    {
        assert!(
            annotated.contains(spec.name),
            "`{}` carries a `validate:` predicate with no SCOPE_CONFINEMENTS row; a silent \
             refusal is undiscoverable to a client author",
            spec.name
        );
    }
}

// ---------------------------------------------------------------------------------------
// Batch 4 — the transcript-read seam split (the PR 3.2 dependency)
// ---------------------------------------------------------------------------------------

const REMOTE_TRANSCRIPT_READS: &[&str] = &[
    "get_remote_agent_conversation",
    "get_remote_agent_conversation_messages_page",
    "get_remote_agent_conversation_timeline_page",
];

const LOCAL_TRANSCRIPT_READS: &[&str] = &[
    "get_agent_conversation",
    "get_agent_conversation_messages_page",
    "get_agent_conversation_timeline_page",
];

/// The split is real: the registered reads reach no arming sink, and the local ones still do.
///
/// Both halves matter. Without the second, this test would keep passing if the wake were
/// removed from the local commands entirely — a much larger behavioural change — and the
/// "split" would be proving nothing about a split. The local commands are the CALIBRATION:
/// they establish that the detector still fires on this exact code shape, so the registered
/// variants' silence is a property of the seam and not of a detector that stopped working.
#[test]
fn remote_transcript_reads_never_reach_the_wake() {
    let graph = CallGraph::build(&load_production_sources());

    for command in REMOTE_TRANSCRIPT_READS {
        let closure = graph.closure([(*command).to_string()]);
        assert!(
            !closure.visited.is_empty(),
            "`{command}` resolved to an empty closure; the graph did not find its body and \
             this assertion would be vacuous"
        );
        assert!(
            !closure_is_arming(&closure),
            "`{command}` reaches an arming sink; the spawn-free seam has been broken"
        );
        assert!(
            !tokens_reach_any(&closure.tokens, PROCESS_LAUNCH_SINKS),
            "`{command}` reaches a process-launch sink"
        );
        assert!(
            !tokens_reach_any(&closure.tokens, TRANSITION_SINKS),
            "`{command}` reaches a transition sink"
        );
        // The wake is the specific edge this split removes; name it, so a future re-introduction
        // fails with the reason rather than with a generic detector message.
        assert!(
            !closure
                .visited
                .iter()
                .any(|node| node.contains("wake_agent_workspace_for_bridge_events")),
            "`{command}` reaches `wake_agent_workspace_for_bridge_events`; the whole point of \
             this module is that it cannot"
        );
    }

    for command in LOCAL_TRANSCRIPT_READS {
        let closure = graph.closure([(*command).to_string()]);
        assert!(
            closure_is_arming(&closure),
            "`{command}` no longer arms. If the wake was deliberately removed from the local \
             read, delete the seam split instead of keeping a module that justifies itself by \
             a hazard that no longer exists."
        );
    }
}

/// The local transcript reads stay unregistered, asserted rather than merely omitted.
#[test]
fn the_local_transcript_reads_stay_unregistered() {
    for command in LOCAL_TRANSCRIPT_READS {
        assert!(
            find_spec(command).is_none(),
            "`{command}` reaches the agent-wake steer sink and must not be remotely reachable; \
             the registered answer is its `get_remote_*` twin"
        );
    }

    // The un-truncated tool-payload escape hatches were deliberately NOT part of batch 4, which
    // asked a later batch to argue for them explicitly. WP3 is that argument: the fail-open they
    // were refused over is fixed at its source and both are registered at `ui:read`. See
    // `wp3_tool_call_detail_reads_are_registered_reviewed_read_rows` for the positive pin.
}

/// WP3 — the tool-call-detail pair, registered on a fixed fail-open plus a per-command audit.
///
/// The batch-4 refusal was `FailOpenUntilFixed`: `load_delegated_tool_runtime_snapshot` applied
/// `.ok().flatten()` to five repository reads, so an outage served the STALE persisted tool
/// result as current. The manifest's standing rule is that a repaired error path is not by
/// itself a registration decision, so this pin asserts BOTH halves: a reviewed `Read` row (not
/// module-default inheritance, which is `AgentControl` for `unified_chat_commands`) and an
/// actual registration.
#[test]
fn wp3_tool_call_detail_reads_are_registered_reviewed_read_rows() {
    for command in [
        "get_agent_message_tool_call_detail",
        "get_agent_timeline_item_tool_call_detail",
    ] {
        let row = policy_for(command, "unified_chat_commands").expect("ledgered");
        assert_eq!(
            row.class,
            RiskClass::Read,
            "`{command}` is a pure repository read once the delegated-run reads propagate"
        );
        assert!(
            row.capabilities.is_empty(),
            "`{command}` carries no capability; it reads and returns"
        );
        assert!(
            COMMAND_OVERRIDES
                .iter()
                .any(|entry| entry.command == command),
            "`{command}` must carry its own audited row, not inherit the module default"
        );
        assert!(
            !AUDIT_REFUSALS
                .iter()
                .any(|refusal| refusal.command == command),
            "`{command}` cannot be both registered and audit-refused"
        );
        let spec = find_spec(command)
            .unwrap_or_else(|| panic!("`{command}` must be registered on the facade"));
        assert_eq!(
            spec.class,
            RiskClass::Read,
            "`{command}` must be served at the read tier, matching its transcript-twin sibling"
        );
    }
}

/// The registered reads are reviewed `Read` rows, not module-default inheritance.
#[test]
fn the_remote_transcript_reads_are_reviewed_read_rows() {
    let rows = census().into_iter().collect::<BTreeMap<_, _>>();

    for command in REMOTE_TRANSCRIPT_READS {
        let module = rows
            .get(*command)
            .unwrap_or_else(|| panic!("`{command}` must be a live Tauri command"));
        assert_eq!(module, "remote_transcript_commands");
        let row = policy_for(command, module).expect("ledgered");
        assert_eq!(row.class, RiskClass::Read);
        assert!(
            row.capabilities.is_empty(),
            "`{command}` is a pure read; a Read row with capabilities is a mislabel"
        );
        assert!(
            !row.reason.contains("conservative-module-default"),
            "`{command}` still carries the module-default reason; it was not reviewed"
        );
        assert!(
            row.reason.contains("propagates read errors"),
            "`{command}` must record that its reads fail closed"
        );
        assert!(
            row.reason.contains("no wake"),
            "`{command}`'s reason must record the property the seam exists to guarantee"
        );
        assert!(find_spec(command).is_some());
    }

    // The module default stays conservative: these three are EXCEPTIONS to it.
    let default = MODULE_DEFAULTS
        .iter()
        .find(|entry| entry.module == "remote_transcript_commands")
        .expect("module has a default");
    assert_eq!(default.policy.class, RiskClass::AgentControl);

    // `unified_chat_commands`, which owns the local twins, is untouched.
    let chat_default = MODULE_DEFAULTS
        .iter()
        .find(|entry| entry.module == "unified_chat_commands")
        .expect("module has a default");
    assert_eq!(chat_default.policy.class, RiskClass::AgentControl);
}

// ---------------------------------------------------------------------------------------
// Batch 5 — the conversation-LIST seam split (completes PR 3.2's read surface)
// ---------------------------------------------------------------------------------------

const REMOTE_CONVERSATION_LIST_READS: &[&str] = &[
    "list_remote_agent_conversations",
    "list_remote_agent_conversations_page",
];

const LOCAL_CONVERSATION_LIST_READS: &[&str] =
    &["list_agent_conversations", "list_agent_conversations_page"];

/// The registered list reads reach no spawn/steer authority.
///
/// NOTE ON THE SHAPE, because it differs from batch 4 and the difference is the finding:
/// `probe_conversation_list_arming_paths` reports NO ARMING HITS for the LOCAL list commands
/// too. Unlike the transcript reads, these never had a wake on their path. So the
/// arming-calibration batch 4 used ("the local ones still arm") is unavailable here, and
/// asserting it would be a false statement about this code. The disqualifier was carrier-
/// shaped, not wake-shaped, and the calibration that matches it is
/// `the_spawn_free_remote_read_module_carries_no_authority_carriers` below.
#[test]
fn remote_conversation_list_reads_carry_no_spawn_authority() {
    let graph = CallGraph::build(&load_production_sources());

    for command in REMOTE_CONVERSATION_LIST_READS {
        let closure = graph.closure([(*command).to_string()]);
        assert!(
            !closure.visited.is_empty(),
            "`{command}` resolved to an empty closure; the graph did not find its body and \
             this assertion would be vacuous"
        );
        assert!(
            !closure_is_arming(&closure),
            "`{command}` reaches an arming sink"
        );
        assert!(
            !tokens_reach_any(&closure.tokens, PROCESS_LAUNCH_SINKS),
            "`{command}` reaches a process-launch sink"
        );
        assert!(
            !tokens_reach_any(&closure.tokens, TRANSITION_SINKS),
            "`{command}` reaches a transition sink"
        );
    }
}

// ---------------------------------------------------------------------------------------
// The Agents-sidebar inbox seam split (registers the paired-device inbox read)
// ---------------------------------------------------------------------------------------
//
// Unlike the batch-5 list reads, the sidebar list WAS a genuine detector-(c) refusal: it
// hydrates each workspace through `agent_workspace_response_for_state`, whose FIRST action is
// to schedule PR-supervision recovery, and the recovery scheduler reaches the git CLI
// resolver. The registered twin routes through a recovery-free hydrator instead. The split is
// a REAL by-name call graph split, not a runtime flag: the two hydrators are separate named
// functions so the detector can prove which one each entry point reaches.

/// The registered sidebar twin reaches NO arming sink and NO process-launch sink.
///
/// This is the detector-(c) clearance for the registration: it is the whole reason the split
/// exists, so it is asserted directly over the call graph rather than trusted from the ledger
/// reason. If a future edit reconnects the recovery-scheduling hydrator to the remote path,
/// this fails instead of silently re-arming the paired-device inbox read.
#[test]
fn remote_agent_sidebar_read_carries_no_spawn_authority() {
    let graph = CallGraph::build(&load_production_sources());

    let closure = graph.closure(["list_remote_agent_sidebar_conversations".to_string()]);
    assert!(
        !closure.visited.is_empty(),
        "`list_remote_agent_sidebar_conversations` resolved to an empty closure; the graph did \
         not find its body and this assertion would be vacuous"
    );
    assert!(
        !closure_is_arming(&closure),
        "`list_remote_agent_sidebar_conversations` reaches an arming sink; the recovery-free \
         hydrator seam has been broken"
    );
    assert!(
        !tokens_reach_any(&closure.tokens, PROCESS_LAUNCH_SINKS),
        "`list_remote_agent_sidebar_conversations` reaches a process-launch sink; the remote \
         inbox read must not resolve a CLI"
    );
    assert!(
        !tokens_reach_any(&closure.tokens, TRANSITION_SINKS),
        "`list_remote_agent_sidebar_conversations` reaches a transition sink"
    );
}

/// CALIBRATION: the LOCAL sidebar command still arms and still reaches the CLI resolver.
///
/// The split is only meaningful if the detector still fires on the local shape — otherwise the
/// remote twin's silence would be a property of a detector that stopped working, not of the
/// seam. The local `list_agent_sidebar_conversations` remains host-denied because it schedules
/// PR-supervision recovery on every inbox load; that behaviour, and its process-launch reach,
/// is pinned here so a refactor that quietly drops it returns the command to review.
#[test]
fn local_agent_sidebar_read_still_schedules_recovery_and_reaches_the_cli() {
    let graph = CallGraph::build(&load_production_sources());

    let closure = graph.closure(["list_agent_sidebar_conversations".to_string()]);
    assert!(
        !closure.visited.is_empty(),
        "`list_agent_sidebar_conversations` resolved to an empty closure"
    );
    assert!(
        closure_is_arming(&closure),
        "`list_agent_sidebar_conversations` stopped arming; if the local inbox load no longer \
         schedules recovery, re-audit its host-denied ledger row"
    );
    assert!(
        tokens_reach_any(&closure.tokens, PROCESS_LAUNCH_SINKS),
        "`list_agent_sidebar_conversations` no longer reaches a process-launch sink; re-audit \
         its `host-denied-spawns-process` classification"
    );
    // It must stay OFF the facade: only the recovery-free twin is registered.
    assert!(
        find_spec("list_agent_sidebar_conversations").is_none(),
        "the recovery-scheduling local sidebar command must never be registered on the facade"
    );
}

/// CALIBRATION for the batch-5 split: the spawn-free read module holds none of the three
/// authority carriers, asserted over SOURCE rather than left as prose.
///
/// This is what makes the list registration safe. The list commands were never disqualified
/// by a detector — `list_agent_conversations` was disqualified because it accepted
/// `tauri::AppHandle` and `ExecutionState` purely to build a `ChatService` whose invoked
/// method (`list_conversations`) is a straight repository delegation. A seam that drops those
/// carriers is only meaningful if the carriers genuinely cannot re-enter, so the module's
/// stated contract is checked mechanically here. Batch 4 asserted this contract in prose only.
#[test]
fn the_spawn_free_remote_read_module_carries_no_authority_carriers() {
    let sources = load_production_sources();
    let (_, module) = sources
        .iter()
        .find(|(file, _)| file == "commands/remote_transcript_commands.rs")
        .expect("the spawn-free remote read module must exist");

    // Comments are stripped: the module doc NAMES the carriers in order to explain why they are
    // absent, and the contract is about code, not prose.
    let code = module
        .lines()
        .filter(|line| !line.trim_start().starts_with("//"))
        .collect::<Vec<_>>()
        .join("\n");
    assert!(
        code.contains("pub async fn list_remote_agent_conversations"),
        "comment stripping ate the module body; this assertion would be vacuous"
    );

    for carrier in [
        "AppHandle",
        "ExecutionState",
        "create_chat_service",
        "build_chat_service",
    ] {
        assert!(
            !code.contains(carrier),
            "`remote_transcript_commands` mentions `{carrier}`. The whole contract of this \
             module is that the spawn/steer authority carriers are absent by construction, \
             which is what lets its commands sit at `ui:read` with no capability."
        );
    }
}

/// The local list commands stay unregistered; the registered answer is the `*_remote_*` twin.
#[test]
fn the_local_conversation_lists_stay_unregistered() {
    for command in LOCAL_CONVERSATION_LIST_READS {
        assert!(
            find_spec(command).is_none(),
            "`{command}` is a Tauri-extractor-shaped local command and must not be remotely \
             reachable; the registered answer is its `list_remote_*` twin"
        );
    }
}

/// The registered list reads are reviewed `Read` rows, not module-default inheritance.
#[test]
fn the_remote_conversation_list_reads_are_reviewed_read_rows() {
    let rows = census().into_iter().collect::<BTreeMap<_, _>>();

    for command in REMOTE_CONVERSATION_LIST_READS {
        let module = rows
            .get(*command)
            .unwrap_or_else(|| panic!("`{command}` must be a live Tauri command"));
        assert_eq!(module, "remote_transcript_commands");
        let row = policy_for(command, module).expect("ledgered");
        assert_eq!(row.class, RiskClass::Read);
        assert!(
            row.capabilities.is_empty(),
            "`{command}` is a pure read; a Read row with capabilities is a mislabel"
        );
        assert!(
            !row.reason.contains("conservative-module-default"),
            "`{command}` still carries the module-default reason; it was not reviewed"
        );
        assert!(
            row.reason.contains("propagates read errors"),
            "`{command}` must record that its reads fail closed"
        );
        assert!(find_spec(command).is_some());
    }
}

/// The six remaining read-SHAPED B2 candidates, all refused, each with its mechanism pinned.
///
/// Batch 4 closed by saying B2's remainder "is the workspace/publish surface, which fires
/// (a)+(b)+(c) together". That was a hand-wave, and this test is the evidence for it: the six
/// commands that still LOOK like reads are audited here and every one is disqualified. A
/// refusal cluster is the whole result of this audit, which is a legitimate outcome — the
/// alternative was registering a `Read` row over a surface that shells out to git.
///
/// The mechanisms are asserted, not just the absence of a spec, so that a refactor which
/// removes a disqualifier fails here and returns the command to review instead of leaving the
/// refusal to survive as folklore.
#[test]
fn the_b2_workspace_read_refusals_are_pinned() {
    let graph = CallGraph::build(&load_production_sources());
    let sources = load_production_sources();

    for command in [
        // Shared hydrator arms; see below.
        "get_agent_conversation_workspace",
        "list_agent_conversation_workspaces_by_project",
        "list_agent_sidebar_conversations",
        // Shells out to git/gh for base-vs-remote comparison.
        "get_agent_conversation_workspace_freshness",
        // Harness capability probe + the AppHandle/ExecutionState carriers.
        "is_chat_service_available",
        // Process launch + fail-open fallback + host path disclosure.
        "search_agent_composer_entries",
    ] {
        assert!(
            find_spec(command).is_none(),
            "`{command}` was audited and refused in batch 5; registering it needs a new \
             argument, not a quiet re-add"
        );
    }

    // MECHANISM 1 — the shared workspace hydrator arms, which is what disqualifies the whole
    // workspace read cluster. Every workspace-returning command funnels through it, so this is
    // the single fact that makes three of the six refusals non-negotiable.
    let hydrator = graph.closure(["agent_workspace_response_for_state".to_string()]);
    assert!(
        !hydrator.visited.is_empty(),
        "the hydrator did not resolve; this assertion would be vacuous"
    );
    assert!(
        closure_is_arming(&hydrator),
        "`agent_workspace_response_for_state` no longer arms. If the workspace read surface \
         became genuinely passive, re-audit the three refusals it disqualifies rather than \
         deleting this assertion."
    );

    // MECHANISM 2 — the sidebar list reaches the same arming surface through its own seam, so
    // its `_for_app_state` shape is NOT sufficient to make the LOCAL command registrable: it
    // hydrates through `agent_workspace_response_for_state`, which schedules PR-supervision
    // recovery. That is why `list_agent_sidebar_conversations` stays host-denied and off the
    // facade. The paired-device inbox read is served instead by a REGISTERED remote twin,
    // `list_remote_agent_sidebar_conversations`, which routes through a recovery-free hydrator
    // and blanks `worktree_path` — see `remote_agent_sidebar_read_carries_no_spawn_authority`
    // and `local_agent_sidebar_read_still_schedules_recovery_and_reaches_the_cli`. The refusal
    // recorded here is the refusal of the LOCAL/arming command, which is unchanged.
    let sidebar = graph.closure(["list_agent_sidebar_conversations_for_app_state".to_string()]);
    assert!(!sidebar.visited.is_empty());
    assert!(
        closure_is_arming(&sidebar),
        "the sidebar list stopped arming; re-audit it as a registration candidate"
    );

    // MECHANISM 3 — freshness and the composer search are process launchers.
    for command in [
        "get_agent_conversation_workspace_freshness_for_app_state",
        "search_agent_composer_entries",
    ] {
        let closure = graph.closure([command.to_string()]);
        assert!(!closure.visited.is_empty(), "`{command}` did not resolve");
        assert!(
            tokens_reach_any(&closure.tokens, PROCESS_LAUNCH_SINKS),
            "`{command}` no longer reaches a process-launch sink; re-audit it"
        );
    }

    // MECHANISM 4 — `get_agent_conversation_workspace` also SCHEDULES external PR
    // reconciliation from a nominally read-only command. Asserted over source because the
    // scheduling call is what a reviewer would miss; the command body otherwise reads as a
    // plain repository lookup.
    let (_, chat_commands) = sources
        .iter()
        .find(|(file, _)| file == "commands/unified_chat_commands/mod.rs")
        .expect("the chat command module must exist");
    assert!(
        chat_commands.contains("schedule_external_pr_reconciliation_for_workspace"),
        "the reconciliation scheduler vanished; re-audit `get_agent_conversation_workspace`"
    );

    // MECHANISM 5 — the composer search WAS the fail-open shape batch 4 refused four times.
    //
    // MECHANISM UPDATED (B0 lane): the fail-open is FIXED. `collect_git_entries` is now
    // tri-state — `Ok(None)` only when git RAN and said "not a checkout", `Err` when git could
    // not be consulted — so a broken git no longer renders as a complete file list. The old
    // assertion pinned the broken shape by source text and would now fail, which is exactly the
    // re-audit it asked for; this is that re-audit's result.
    //
    // The refusal STANDS on the two reasons that survive: the command launches a process
    // (`SpawnsProcess`, unexposable at any v1 scope) and it discloses host absolute paths. The
    // repaired error path removes one argument for refusal, not the disqualifying ones.
    let (_, project_entries) = sources
        .iter()
        .find(|(file, _)| file == "commands/agent_composer_commands/project_entries.rs")
        .expect("the composer entry module must exist");
    assert!(
        !project_entries
            .contains("collect_git_entries(root).unwrap_or_else(|| collect_fs_entries(root))"),
        "the composer search's fail-open fallback came back; it was repaired in the B0 lane"
    );
    assert!(
        project_entries.contains(
            "fn collect_git_entries(root: &Path) -> Result<Option<Vec<IndexedEntry>>, String>"
        ),
        "the composer search's git probe is no longer tri-state; a two-state probe cannot \
         distinguish `not a checkout` from `git failed`, which is the fail-open returning"
    );
    assert!(
        project_entries.contains("Command::new(resolve_git_cli_path())"),
        "the composer search no longer launches git; if the process launch is gone, re-audit \
         whether SpawnsProcess still disqualifies it"
    );
}

// ---------------------------------------------------------------------------------------
// Batch 4 — the B2 detector-silent getters
// ---------------------------------------------------------------------------------------

const B2_REGISTERED_GETTERS: &[&str] = &[
    "get_agent_conversation_summary",
    "get_agent_conversation_runtime_index",
    "list_agent_conversation_workspace_publication_events",
    "get_bulk_workspace_publication_states",
    "list_agent_models",
];

/// The five registered getters are reviewed `Read` rows, not module-default inheritance.
#[test]
fn the_b2_getters_are_reviewed_read_rows() {
    let rows = census().into_iter().collect::<BTreeMap<_, _>>();

    for command in B2_REGISTERED_GETTERS {
        let module = rows
            .get(*command)
            .unwrap_or_else(|| panic!("`{command}` must be a live Tauri command"));
        let row = policy_for(command, module).expect("ledgered");
        assert_eq!(row.class, RiskClass::Read, "`{command}`");
        assert!(
            row.capabilities.is_empty(),
            "`{command}` is a pure read; a Read row with capabilities is a mislabel"
        );
        assert!(
            !row.reason.contains("conservative-module-default"),
            "`{command}` still carries the module-default reason; it was not reviewed"
        );
        // The shared structural claim of the whole cluster.
        assert!(
            row.reason.contains("propagates read errors"),
            "`{command}` must record that its reads fail closed"
        );
        assert!(
            find_spec(command).is_some(),
            "`{command}` must be registered"
        );
    }

    // Every module the five come from keeps its conservative default; these are EXCEPTIONS.
    for module in [
        "unified_chat_commands",
        "agent_sidebar_commands",
        "agent_model_commands",
    ] {
        let default = MODULE_DEFAULTS
            .iter()
            .find(|entry| entry.module == module)
            .unwrap_or_else(|| panic!("`{module}` has a default"));
        assert_eq!(
            default.policy.class,
            RiskClass::AgentControl,
            "`{module}`'s default weakened; its members were registered as exceptions to it"
        );
    }
}

/// The twelve candidates that were NOT registered, with their reasons pinned by class.
///
/// Absence alone is not evidence of review — an unregistered command looks identical whether
/// it was audited and refused or simply never examined. Each group below asserts the specific
/// property that disqualified it, so a future change that removes the hazard fails this test
/// and forces the command back through review rather than letting the refusal go stale.
#[test]
fn the_b2_getter_refusals_are_pinned() {
    for command in [
        // Fail-open: swallows a repository/filesystem error into an empty-but-successful
        // result — the false-success shape this ledger exists to prevent.
        //
        // MECHANISM UPDATED (B0 lane): the fail-open itself is FIXED. The trait method now
        // returns `Result`, so a failed registry list propagates instead of reporting "nothing
        // is running"; `ipc_contract_bulk_running_states_propagates_a_registry_read_failure`
        // pins that. These stay refused because the fail-open was never the only reason — the
        // per-command audit that would clear them for the facade has not been done, and a
        // repaired error path is not a registration decision.
        //
        // BATCH 8: `search_agent_composer_plan_references` LEFT this list. The per-command
        // audit this comment asks for was done — pure read, every repository error propagated,
        // no `AppHandle`/`ExecutionState`/chat service — and it is now registered at `ui:read`
        // and pinned by `b2_read_reclassifications_are_reviewed_rather_than_module_defaults`.
        // The rest stay: their audits have still not been done.
        "get_agent_running_states",
        "get_agent_conversation_runtime_statuses",
        "list_agent_composer_skills",
        // In-memory registry WRITE from a nominally read-only command.
        "is_agent_running",
        // Raw un-truncated content: pending prompt text.
        //
        // WP3: `get_agent_message_tool_call_detail` and `get_agent_timeline_item_tool_call_detail`
        // LEFT this list. Their refusal was the fail-open, and the fail-open is fixed at its
        // source (`load_delegated_tool_runtime_snapshot` now returns `AppResult`, with `Ok(None)`
        // reserved for genuine absence) plus the per-command audit this comment demands. The
        // content argument does not hold them back on its own: the registered
        // `get_remote_agent_conversation` twin already serves the same tool blocks at `ui:read`,
        // truncated. Pinned by `wp3_tool_call_detail_reads_are_registered_reviewed_read_rows`.
        "get_queued_agent_messages",
        // Host path disclosure: returns absolute directories from the operator's machine.
        "list_conversation_folder_references",
        // Fail-open on enrichment: model fields go silently null when the lookup errors, so
        // the cluster's "propagates read errors" invariant does not hold.
        "get_agent_run_status_unified",
        // Batch 4 deferred `list_agent_conversations` / `_page` here and batch 5 resolved them
        // via the seam split, so they now live in `the_local_conversation_lists_stay_
        // unregistered` with their `list_remote_*` twins registered. They stay listed here
        // because the local commands themselves are still refused, and the reason is unchanged.
        //
        // Batch 4's note said "both take `tauri::AppHandle`". That was true only of
        // `list_agent_conversations`; `list_agent_conversations_page` never took one — its
        // only barrier was the `State<'_, AppState>` extractor. Corrected in batch 5.
        "list_agent_conversations",
        "list_agent_conversations_page",
    ] {
        assert!(
            find_spec(command).is_none(),
            "`{command}` was audited and refused/deferred in batch 4; registering it needs a \
             new argument, not a quiet re-add"
        );
    }

    // The registry-writing pair must keep reaching the cleanup path. If a future change makes
    // them genuinely read-only, this fails and the refusal returns to review instead of
    // persisting as folklore.
    //
    // Asserted over SOURCE rather than the call graph, and that is itself the finding: the
    // command bodies call `service.is_agent_running(..)` through the `ChatService` trait, and
    // the graph does not resolve trait dispatch. The write is two levels below a body that
    // reads as a pure delegation, which is precisely why every detector was silent on these
    // and why the hand-trace — not the probe — is what disqualified them.
    let chat_service = load_production_sources()
        .into_iter()
        .find(|(file, _)| file == "application/chat_service/mod.rs")
        .map(|(_, source)| source)
        .expect("chat_service source is loaded");

    for caller in ["\"is_agent_running\"", "\"get_agent_running_states\""] {
        assert!(
            chat_service.contains(caller),
            "`AppChatService` no longer passes {caller} as a registry-cleanup source; the \
             stated reason these are unregistered has changed — re-audit rather than leaving \
             a stale refusal."
        );
    }
    assert!(
        chat_service.contains("cleanup_stale_registry_block")
            && chat_service.contains("cleanup_inactive_registry_block")
            && chat_service.contains("RegistryCleanupCaller::ReadOnly"),
        "the read-only registry-cleanup path is gone; re-audit `is_agent_running` and \
         `get_agent_running_states`"
    );

    // Honesty about the strength of this refusal, recorded where it cannot be lost: the
    // cleanup is liveness-GUARDED. `registry_entry_blocks_send_but_is_stale` requires
    // `!is_process_alive(pid)`, and the `ReadOnly` arm of
    // `registry_entry_blocks_send_because_run_inactive` bails out when the process is alive.
    // So the write reaps a dead entry; it cannot evict a live agent's concurrency gate. These
    // two are refused on the judgement that a `Read` row must not write at all, NOT on a
    // proven hazard — and the guard is pinned so a future change that REMOVES it makes this
    // a genuine hazard loudly rather than quietly.
    assert!(
        chat_service.contains("!is_process_alive(info.pid)"),
        "the staleness guard on read-only registry cleanup is gone; the refusal of \
         `is_agent_running` was based on a GUARDED reap, and an unguarded one is a much \
         stronger hazard that needs its own review"
    );

    // Calibration: a registered sibling from the SAME module is not implicated, so the
    // criterion above discriminates rather than matching all of `unified_chat_commands`.
    let graph = CallGraph::build(&load_production_sources());
    let clean = graph.closure(["get_agent_conversation_summary".to_string()]);
    assert!(
        !clean.visited.is_empty(),
        "calibration closure is empty and proves nothing"
    );
    assert!(
        !closure_is_arming(&clean),
        "`get_agent_conversation_summary` arms; it should not have been registered"
    );
}

// ---------------------------------------------------------------------------------------
// PR 3.1-b batch 7 — census `B3` (review, QA, merge pipeline, validation).
// ---------------------------------------------------------------------------------------

/// The `B3` rows reclassified to `Read` carry a reviewed reason, not a module default, and
/// audit clean on all three detectors.
///
/// The mechanical half of the claim: a future refactor that routes any of these through a
/// transition, a spawn-arming write or a CLI resolution fails here rather than shipping a
/// `ui:read` registration over authority it acquired later.
#[test]
fn b3_read_reclassifications_are_reviewed_rather_than_module_defaults() {
    const RECLASSIFIED: &[(&str, &str)] = &[
        ("get_pending_reviews", "review_commands"),
        ("get_review_by_id", "review_commands"),
        ("get_reviews_by_task_id", "review_commands"),
        ("get_task_state_history", "review_commands"),
        ("get_fix_task_attempts", "review_commands"),
        ("get_task_issues", "review_commands"),
        ("get_issue_progress", "review_commands"),
        ("get_review_settings", "review_commands"),
        ("get_qa_settings", "qa_commands"),
        ("get_task_qa", "qa_commands"),
        ("get_qa_results", "qa_commands"),
        ("get_merge_pipeline", "merge_pipeline_commands"),
        ("get_merge_progress", "merge_pipeline_commands"),
        ("get_merge_phase_list", "merge_pipeline_commands"),
    ];

    let graph = CallGraph::build(&load_production_sources());
    let rows = census();
    let commands = rows
        .iter()
        .map(|(command, _)| command.clone())
        .collect::<Vec<_>>();
    let detector_b = spawn_triggering_writers(&graph, commands, SPAWN_TRIGGERING_STATE_SURFACE);

    for (command, module) in RECLASSIFIED {
        let row = policy_for(command, module).expect("reclassified row is ledgered");
        assert_eq!(
            row.class,
            RiskClass::Read,
            "`{command}` is registered at `ui:read` and must be ledgered `Read`"
        );
        assert!(
            row.capabilities.is_empty(),
            "`{command}` is a read and must declare no capability; got {:?}",
            row.capabilities
        );
        assert!(
            !row.reason.contains("conservative-module-default"),
            "`{command}` still carries the module-default placeholder as its reason; a `Read` \
             row below its module default needs a reviewed structural reason"
        );

        let closure = graph.closure([(*command).to_string()]);
        assert!(
            !closure_is_arming(&closure),
            "detector (a): `{command}` reaches a transition/steer sink and cannot be `Read`"
        );
        assert!(
            !detector_b.contains(*command),
            "detector (b): `{command}` writes spawn-triggering state and cannot be `Read`"
        );
        assert!(
            !tokens_reach_any(&closure.tokens, PROCESS_LAUNCH_SINKS),
            "detector (c): `{command}` resolves a CLI and cannot be `Read`"
        );

        let spec = find_spec(command).expect("reclassified row is registered");
        assert_eq!(spec.class, RiskClass::Read);
        assert!(spec.capabilities.is_empty());
    }
}

/// The `B3` members this batch declined to register, each pinned to its OWN finding.
///
/// A registration sweep is only as trustworthy as the rows it declines to move, and every
/// member below sits in a module whose getters were just reclassified — so "it is in a
/// reclassified module" must never be sufficient grounds in either direction.
#[test]
fn b3_members_that_audit_dirty_stay_unregistered() {
    let graph = CallGraph::build(&load_production_sources());
    let rows = census();
    let commands = rows
        .iter()
        .map(|(command, _)| command.clone())
        .collect::<Vec<_>>();
    let detector_b = spawn_triggering_writers(&graph, commands, SPAWN_TRIGGERING_STATE_SURFACE);

    // --- (c): a getter that shells out. Not registerable at any v1 scope, and resolved
    // through the manifest rather than a client-local reason: it is a host command the
    // facade denies, not one the client handles.
    let validation =
        policy_for("get_task_validation_summary", "validation_commands").expect("ledgered");
    assert_eq!(validation.class, RiskClass::Elevated);
    assert_eq!(validation.capabilities, &[Capability::SpawnsProcess]);
    assert_eq!(
        ralphx_remote_protocol::v1_resolution(validation.class, validation.capabilities),
        ralphx_remote_protocol::V1Resolution::HostDeniedSpawnsProcess,
        "`get_task_validation_summary` must resolve through the manifest, not a local-only row"
    );

    // --- (a): the human review transitions. `approve_task_for_review` is registered with the
    // same detector-(a) profile, so the transition sink alone is not what excludes these —
    // each is held back on its own additional finding, recorded per group below.
    //
    // (a) + (b): these three seed spawn-triggering state as well as transitioning, which is
    // the combination `update_review_settings` is flagged for and no registered row carries.
    // PR 3.1-b batch 10 resolved all three, and deliberately NOT the same way — which is the
    // point of keeping them in one loop. The detector profile they share (a AND b) was never
    // the disqualifier; batch 9 proved the facade serves that profile. What separated them was
    // a body finding, so two are registered at `ui:agent` and one is classified fail-open.
    //
    // The detector assertions are RETAINED for all three. They are what makes the split
    // falsifiable: if (a) or (b) stopped firing, these are no longer the commands that were
    // audited and every disposition below needs re-deriving.
    for command in [
        "request_task_changes_for_review",
        "request_task_changes_from_reviewing",
        "re_review_task_from_escalated",
    ] {
        let closure = graph.closure([command.to_string()]);
        assert!(
            closure_is_arming(&closure),
            "`{command}` was audited with detector (a) firing; if it no longer does, the \
             disposition needs a new reason"
        );
        assert!(
            detector_b.contains(command),
            "`{command}` was audited with detector (b) firing alongside (a)"
        );
    }
    // The one that audits dirty: it reaches the same `RevisionNeeded` transition as its
    // registered sibling, but first rewrites `task.metadata` through two `unwrap_or_else`
    // fallbacks that replace an unparseable blob with a stub, dropping every other field while
    // still returning `Ok`.
    assert!(
        find_spec("request_task_changes_from_reviewing").is_none(),
        "`request_task_changes_from_reviewing` is classified fail-open and must not be registered"
    );
    assert!(
        audit_refusal_for("request_task_changes_from_reviewing").is_some(),
        "its refusal must be recorded, or it silently returns to the ratchet"
    );
    for command in [
        "request_task_changes_for_review",
        "re_review_task_from_escalated",
    ] {
        let spec = find_spec(command)
            .unwrap_or_else(|| panic!("batch 10 registered `{command}` at ui:agent"));
        assert_eq!(
            spec.class,
            RiskClass::AgentControl,
            "`{command}` transitions a task and must never be reachable below the arming scope"
        );
    }

    // (a) alone, plus corrective-transition authority. PR 3.1-b batch 14 SPLIT this pair, which
    // batch 3 recorded together and batch 10 held together. The shared property that justified
    // holding both — "registering one half of a repair pair while the other is held is worse
    // than holding both" — was re-examined against the current registry and did not survive:
    // `block_task` and `stop_task` are registered, so the remote brake the pair argument was
    // protecting already exists. `reject_fix_task` is now classified `v1-audit-refused` on the
    // batch-14 `reaches-corrective-transition` reason and `approve_fix_task` is registered.
    //
    // Detector (a) must still fire on BOTH — that is what put them in this block — and the pair's
    // mechanical split is pinned separately by
    // `batch14_held_pair_splits_on_the_corrective_reach`.
    for command in ["approve_fix_task", "reject_fix_task"] {
        let closure = graph.closure([command.to_string()]);
        assert!(
            closure_is_arming(&closure),
            "`{command}` is excluded because detector (a) fires"
        );
    }
    assert!(
        find_spec("reject_fix_task").is_none(),
        "`reject_fix_task` reaches a corrective transition and must never be registered"
    );
    assert!(
        find_spec("approve_fix_task").is_some(),
        "`approve_fix_task` was released from its batch-10 pair hold in batch 14; if it is          withheld again, restore the pair reasoning here rather than silently unregistering it"
    );

    // (b): the write half of the settings pair whose READ half batch 7 registered. PR 3.1-b
    // batch 10 registered this half too, at `ui:agent` — the registered `inject_task`,
    // `resume_automation` and `finalize_automation` all carry the same
    // `SeedsSpawnTriggeringState` capability, so seeding spawn-triggering state was never the
    // disqualifier here; the absence of an audit was.
    //
    // Both surviving invariants are still asserted: the capability must stay declared, and the
    // row must stay at the arming class rather than drifting down toward its registered read
    // half by module analogy.
    let review_settings =
        policy_for("update_review_settings", "review_commands").expect("ledgered");
    assert!(
        review_settings
            .capabilities
            .contains(&Capability::SeedsSpawnTriggeringState),
        "`update_review_settings` seeds spawn-triggering state; its read half is registered \
         and this half must not be reclassified downward by module analogy"
    );
    assert!(detector_b.contains("update_review_settings"));
    let review_settings_spec = find_spec("update_review_settings")
        .expect("batch 10 registered update_review_settings at ui:agent");
    assert_eq!(review_settings_spec.class, RiskClass::AgentControl);
    assert!(
        review_settings_spec
            .capabilities
            .contains(&Capability::SeedsSpawnTriggeringState),
        "the registered spec must declare the seeding capability its ledger row records, or a \
         reader of the facade would not see why the op is arming"
    );

    // No detector models this one, so it is a hand-audited exclusion recorded here. Detector
    // (b)'s surface covers persisted spawn-triggering state; `update_qa_settings` writes
    // `AppState::qa_settings` IN MEMORY, and `qa_enabled` / `auto_qa_for_ui_tasks` /
    // `auto_qa_for_api_tasks` are precisely what decide whether QA is armed automatically.
    // That is the same authority detector (b) flags `update_review_settings` for, reached
    // through a surface the detector does not model — so the read half is registered and the
    // write half is not.
    let qa_settings = policy_for("update_qa_settings", "qa_commands").expect("ledgered");
    assert_eq!(
        qa_settings.class,
        RiskClass::AgentControl,
        "update_qa_settings arms automatic QA through an in-memory settings surface detector \
         (b) does not model; it is not a getter"
    );
    // Batch 10 REGISTERED it at `ui:agent`, with an explicit DECLARED_MEMBERSHIPS row
    // (`arms-auto-qa`) precisely because no detector models the in-memory surface described
    // above — without that row the generated P-17b suite could never prove a registered
    // arming write is unreachable from a default pairing. The invariant this test keeps is
    // the class: an auto-QA arming write must never be reachable below `ui:agent`.
    let qa_spec = find_spec("update_qa_settings").expect("batch 10 registered update_qa_settings");
    assert_eq!(qa_spec.class, RiskClass::AgentControl);
    assert!(
        DECLARED_MEMBERSHIPS
            .iter()
            .any(|(command, _)| *command == "update_qa_settings"),
        "a registered arming write that no detector models MUST carry a declared membership, \
         or the generated negative suite will never cover it"
    );

    // --- The batch-7 detector-(c) FALSE POSITIVE, now CLEARED (PR 3.1-b batch 8, ITEM 0).
    //
    // Batch 7 recorded that `issue.reopen(reason)`'s real target, `ReviewIssueEntity::reopen`,
    // lived in the `ralphx-domain` crate outside the scanned tree, so the resolver fell back to
    // every method named `reopen` and picked up `SessionReopenService::reopen`, which reaches
    // git. Batch 8 widened the walk to the linked workspace crates, the real definition is now
    // visible, and the spurious process-launch hit is gone.
    //
    // The refusal SURVIVES the cleared detector, on a different and stronger ground: the body
    // is a repository `update` behind a status guard, i.e. a WRITE. Detector silence never
    // licenses registering a writer, and this row is the reason the two facts are asserted
    // separately rather than as one "audits clean → register" step.
    let closure = graph.closure(["reopen_issue".to_string()]);
    assert!(
        !tokens_reach_any(&closure.tokens, PROCESS_LAUNCH_SINKS),
        "`reopen_issue`'s detector-(c) hit was an artefact of the missing domain crate; if it \
         fires again the workspace-crate walk regressed"
    );
    let reopen = policy_for("reopen_issue", "review_commands").expect("ledgered");
    assert!(
        !reopen.capabilities.contains(&Capability::SpawnsProcess),
        "`reopen_issue` does not spawn a process; declaring `SpawnsProcess` to buy a manifest \
         classification would put a false statement in the ledger"
    );
    assert_eq!(
        reopen.class,
        RiskClass::AgentControl,
        "`reopen_issue` persists a status change through `review_issue_repo.update`; a cleared \
         detector does not turn a writer into a getter"
    );
    // Batch 10 registered it at `ui:agent` after the writer audit batch 7 said it needed:
    // a `review_issue_repo.get_by_id` -> `status == Addressed` guard -> `review_issue_repo
    // .update`, taking only `&AppState`, with every error propagated. The two assertions
    // above are the ones that must survive registration — the cleared detector must stay
    // cleared, and the row must never claim `SpawnsProcess` to buy a classification.
    let reopen_spec = find_spec("reopen_issue").expect("batch 10 registered reopen_issue");
    assert_eq!(reopen_spec.class, RiskClass::AgentControl);

    // --- `start_research` lost ALL THREE detectors to the same walk fix, and stays refused.
    //
    // Its `process.start()` resolved, pre-fix, to an unrelated `start` and dragged an entire
    // agent-prompt/git closure behind it (6478 tokens → 1631 once `ResearchProcess::start`,
    // a two-line `self.status = Running` setter, became visible). Nothing about that correction
    // makes the command registerable: it still ends in `process_repo.create(process)`. This is
    // the one row in the batch whose every mechanical hold disappeared at once, so it is pinned
    // to the write finding explicitly — a later batch must not read the silence as a licence.
    let research = graph.closure(["start_research".to_string()]);
    assert!(
        !closure_is_arming(&research) && !tokens_reach_any(&research.tokens, PROCESS_LAUNCH_SINKS),
        "start_research is expected to audit clean after the workspace-crate walk"
    );
    let research_row = policy_for("start_research", "research_commands").expect("ledgered");
    assert_eq!(
        research_row.class,
        RiskClass::AgentControl,
        "start_research creates a research process row; it audits clean only because the \
         pre-fix closure was a same-name artefact, not because it reads"
    );
    // Batch 10 registered it at `ui:agent`, and the ledger reason records the honest limit
    // of what that buys: the body ends at `process_repo.create`, no spawn is reached
    // transitively, and NO production consumer scans for Running `ResearchProcess` rows. It
    // is registered as a guarded durable write, never as a research launcher.
    let research_spec = find_spec("start_research").expect("batch 10 registered start_research");
    assert_eq!(
        research_spec.class,
        RiskClass::AgentControl,
        "a durable write whose detectors are clear only because a same-name artefact was \
         removed must still be registered at the arming class, not below it"
    );
}

/// The `B4` rows reclassified to `Read` carry a reviewed reason and audit clean, exactly as
/// the `B3` cluster does.
#[test]
fn b4_read_reclassifications_are_reviewed_rather_than_module_defaults() {
    const RECLASSIFIED: &[(&str, &str)] = &[
        ("get_active_plan", "plan_commands"),
        ("get_active_execution_plan", "plan_commands"),
        ("list_plan_selector_candidates", "plan_commands"),
        ("get_methodologies", "methodology_commands"),
        ("get_active_methodology", "methodology_commands"),
        ("get_workflows", "workflow_commands"),
        ("get_workflow", "workflow_commands"),
        ("get_builtin_workflows", "workflow_commands"),
        ("get_active_workflow_columns", "workflow_commands"),
    ];

    let graph = CallGraph::build(&load_production_sources());
    let rows = census();
    let commands = rows
        .iter()
        .map(|(command, _)| command.clone())
        .collect::<Vec<_>>();
    let detector_b = spawn_triggering_writers(&graph, commands, SPAWN_TRIGGERING_STATE_SURFACE);

    for (command, module) in RECLASSIFIED {
        let row = policy_for(command, module).expect("reclassified row is ledgered");
        assert_eq!(
            row.class,
            RiskClass::Read,
            "`{command}` must be ledgered Read"
        );
        assert!(
            row.capabilities.is_empty(),
            "`{command}` is a read and must declare no capability; got {:?}",
            row.capabilities
        );
        assert!(
            !row.reason.contains("conservative-module-default"),
            "`{command}` still carries the module-default placeholder as its reason"
        );

        let closure = graph.closure([(*command).to_string()]);
        assert!(
            !closure_is_arming(&closure),
            "detector (a): `{command}` reaches a transition/steer sink and cannot be `Read`"
        );
        assert!(
            !detector_b.contains(*command),
            "detector (b): `{command}` writes spawn-triggering state and cannot be `Read`"
        );
        assert!(
            !tokens_reach_any(&closure.tokens, PROCESS_LAUNCH_SINKS),
            "detector (c): `{command}` resolves a CLI and cannot be `Read`"
        );

        let spec = find_spec(command).expect("reclassified row is registered");
        assert_eq!(spec.class, RiskClass::Read);
        assert!(spec.capabilities.is_empty());
    }
}

/// `set_active_plan` audits clean on all three detectors and is still refused.
///
/// No detector models this: the body swallows TWO errors. The execution-plan lookup is
/// `if let Ok(Some(ep))`, so a repository failure is indistinguishable from "this session has
/// no execution plan", and the follow-up `set_execution_plan_id` write is discarded with
/// `let _ =`. Both failures leave `Ok(())` on the wire, so a remote client is told the active
/// plan was set while the derived execution-plan id silently did not move — and the execution
/// plan id is what the Kanban/Graph filters and the scheduler read.
///
/// This is the same fail-open disqualification batch 2 applied to
/// `get_pending_permissions`/`get_pending_questions`, in the write direction. Registered
/// siblings in the same module do not license it; the fix is to propagate both errors, and
/// until then the honest answer is that it stays out of the facade.
#[test]
fn b4_members_that_audit_dirty_stay_unregistered() {
    let graph = CallGraph::build(&load_production_sources());
    let rows = census();
    let commands = rows
        .iter()
        .map(|(command, _)| command.clone())
        .collect::<Vec<_>>();
    let detector_b = spawn_triggering_writers(&graph, commands, SPAWN_TRIGGERING_STATE_SURFACE);

    let closure = graph.closure(["set_active_plan".to_string()]);
    assert!(
        !closure_is_arming(&closure)
            && !detector_b.contains("set_active_plan")
            && !tokens_reach_any(&closure.tokens, PROCESS_LAUNCH_SINKS),
        "`set_active_plan` is a HAND-audited exclusion; if a detector now fires on it, the \
         reason recorded here is no longer the operative one and must be re-derived"
    );
    let row = policy_for("set_active_plan", "plan_commands").expect("ledgered");
    assert_eq!(
        row.class,
        RiskClass::AgentControl,
        "set_active_plan swallows the execution-plan lookup error and the \
         set_execution_plan_id write error, so it can report success on a partial write; its \
         read siblings being registered does not license reclassifying it"
    );
    assert!(
        find_spec("set_active_plan").is_none(),
        "set_active_plan must not be registered"
    );

    // The other two writers in the reclassified modules were held here only on the ordinary
    // write/read split — never on a finding — which is exactly the "no audit was done" state
    // batch 9 named. PR 3.1-b batch 10 did the audit and registered both at `ui:agent`.
    //
    // `clear_active_plan` is the sharp one, and the contrast with `set_active_plan` above is
    // the whole reason both live in this test: they are sibling writers of the same pointer,
    // and only one of them swallows its errors. Holding them together would have implied the
    // refusal was about the surface; the split proves it was about the body.
    //
    // The invariant this test still enforces is unchanged: neither may be reclassified DOWNWARD
    // by module analogy into a scope a default pairing can reach.
    for (command, module) in [
        ("clear_active_plan", "plan_commands"),
        ("seed_builtin_workflows", "workflow_commands"),
    ] {
        let row = policy_for(command, module).expect("ledgered");
        assert_eq!(
            row.class,
            RiskClass::AgentControl,
            "`{command}` writes and must not be reclassified downward by module analogy"
        );
        let spec = find_spec(command)
            .unwrap_or_else(|| panic!("batch 10 registered `{command}` at ui:agent"));
        assert_eq!(spec.class, RiskClass::AgentControl);
        assert!(
            !row.reason.contains("conservative-module-default"),
            "`{command}` is registered on the module-default placeholder rather than a \
             reviewed reason"
        );
    }
}

/// Calibration probe: every detector verdict that moved when the linked workspace crates
/// entered the authority graph (PR 3.1-b batch 8, ITEM 0).
///
/// Not an assertion — it prints the shift table that batch 8 hand-verified before regenerating
/// the manifest. The fault being closed is direction-agnostic, so this reports BOTH directions.
#[test]
#[ignore = "calibration probe"]
fn probe_workspace_crate_walk_verdict_shifts() {
    use super::authority_audit::{collect_rs_files, crate_src_root};

    let app_only = {
        let root = crate_src_root();
        let mut files = Vec::new();
        collect_rs_files(&root, &root, &mut files);
        files.sort_by(|a, b| a.0.cmp(&b.0));
        files
    };
    let full = load_production_sources();
    println!("app-only sources: {}, full: {}", app_only.len(), full.len());

    let before = CallGraph::build(&app_only);
    let after = CallGraph::build(&full);
    let rows = census();
    let commands = rows
        .iter()
        .map(|(command, _)| command.clone())
        .collect::<Vec<_>>();

    let before_b =
        spawn_triggering_writers(&before, commands.clone(), SPAWN_TRIGGERING_STATE_SURFACE);
    let after_b =
        spawn_triggering_writers(&after, commands.clone(), SPAWN_TRIGGERING_STATE_SURFACE);

    for (command, module) in &rows {
        let before_closure = before.closure([command.clone()]);
        let after_closure = after.closure([command.clone()]);
        let a0 = closure_is_arming(&before_closure);
        let a1 = closure_is_arming(&after_closure);
        let b0 = before_b.contains(command);
        let b1 = after_b.contains(command);
        let c0 = tokens_reach_any(&before_closure.tokens, PROCESS_LAUNCH_SINKS);
        let c1 = tokens_reach_any(&after_closure.tokens, PROCESS_LAUNCH_SINKS);
        if (a0, b0, c0) != (a1, b1, c1) {
            println!("SHIFT {command} ({module}): a {a0}->{a1}  b {b0}->{b1}  c {c0}->{c1}");
        }
    }
}

/// The `B2` rows reclassified to `Read` carry a reviewed reason and audit clean.
///
/// Same shape as the `B3`/`B4` clusters. `B2` is the census's highest-risk batch — it also
/// holds `send_agent_message`, the workspace publish surface and the conversation lifecycle
/// writes — so the module default stays `AgentControl` and each row below is asserted
/// individually.
#[test]
fn b2_read_reclassifications_are_reviewed_rather_than_module_defaults() {
    const RECLASSIFIED: &[(&str, &str)] = &[(
        "search_agent_composer_plan_references",
        "agent_composer_commands",
    )];

    let graph = CallGraph::build(&load_production_sources());
    let rows = census();
    let commands = rows
        .iter()
        .map(|(command, _)| command.clone())
        .collect::<Vec<_>>();
    let detector_b = spawn_triggering_writers(&graph, commands, SPAWN_TRIGGERING_STATE_SURFACE);

    for (command, module) in RECLASSIFIED {
        let row = policy_for(command, module).expect("reclassified row is ledgered");
        assert_eq!(
            row.class,
            RiskClass::Read,
            "`{command}` must be ledgered Read"
        );
        assert!(
            row.capabilities.is_empty(),
            "`{command}` is a read and must carry no capability"
        );
        assert!(
            find_spec(command).is_some(),
            "`{command}` must be registered on the facade"
        );

        let closure = graph.closure([(*command).to_string()]);
        assert!(
            !closure_is_arming(&closure),
            "`{command}` reaches an arming sink and cannot be Read"
        );
        assert!(
            !detector_b.contains(*command),
            "`{command}` writes spawn-triggering state and cannot be Read"
        );
        assert!(
            !tokens_reach_any(&closure.tokens, PROCESS_LAUNCH_SINKS),
            "`{command}` reaches a process-launch sink and cannot be Read"
        );
    }

    // The module defaults are NOT relaxed by the reclassifications above: the neighbours that
    // make them conservative are still in the same modules and still unregistered.
    for (command, module) in [
        ("send_agent_message", "unified_chat_commands"),
        (
            "publish_agent_conversation_workspace",
            "unified_chat_commands",
        ),
        ("start_agent_conversation", "unified_chat_commands"),
    ] {
        let row = policy_for(command, module).expect("ledgered");
        assert_ne!(
            row.class,
            RiskClass::Read,
            "`{command}` must not be pulled down to Read by its module's read cluster"
        );
        assert!(
            find_spec(command).is_none(),
            "`{command}` stays unregistered"
        );
    }
}

/// `B2` members that audit clean but are still refused, each pinned to its OWN finding.
///
/// Detector silence is not a licence. These six are the ones a "detectors clean → register"
/// rule would have taken, and each is held back by something only a body read shows.
#[test]
fn b2_detector_clean_members_refused_on_their_own_findings() {
    // --- Fail-open reads. A repository or filesystem failure is rendered as absence, so a
    // remote client cannot distinguish "nothing here" from "the host could not tell".
    //
    // `list_agent_composer_skills` swallows the plugin/settings reads wholesale — e.g.
    // `agent_composer_commands/skills.rs:299` `let Ok(raw) = std::fs::read_to_string(..) else`
    // and `:766`, where losing the Codex disabled-skill list reports DISABLED skills as
    // enabled. That is a fail-open that changes the answer, not just its completeness.
    //
    // WP3: `get_agent_message_tool_call_detail` and `get_agent_timeline_item_tool_call_detail`
    // used to be here for sharing `load_delegated_tool_runtime_snapshot`, whose `.ok().flatten()`
    // on every repository read made an outage return the STALE persisted tool result as though
    // it were current. The helper now returns `AppResult` and every read propagates, so the
    // finding that held them no longer describes the code; they are registered and pinned by
    // `wp3_tool_call_detail_reads_are_registered_reviewed_read_rows`.
    for (command, module) in [("list_agent_composer_skills", "agent_composer_commands")] {
        let row = policy_for(command, module).expect("ledgered");
        assert_ne!(
            row.class,
            RiskClass::Read,
            "`{command}` is fail-open; a fail-open shape is fixed or refused, never registered"
        );
        assert!(
            find_spec(command).is_none(),
            "`{command}` must not register"
        );
    }

    // --- Reads that construct spawn-capable machinery to serve a read.
    //
    // Both take `execution_state` and `tauri::AppHandle` and call `create_chat_service`
    // (`unified_chat_commands/mod.rs:9657`, `:4263`) purely to reach one getter. The service
    // is not USED to steer, but exposing the command hands a read-scoped caller a constructed
    // steer surface, and the read-only seam that would fix this does not exist yet. Contrast
    // `list_agent_conversations`, whose `_for_app_state` seam was extracted for exactly this
    // reason and which IS registered above.
    for (command, module) in [
        ("get_agent_run_status_unified", "unified_chat_commands"),
        ("get_queued_agent_messages", "unified_chat_commands"),
    ] {
        let row = policy_for(command, module).expect("ledgered");
        assert_ne!(
            row.class,
            RiskClass::Read,
            "`{command}` builds a spawn-capable chat service to serve a read"
        );
        assert!(
            find_spec(command).is_none(),
            "`{command}` must not register"
        );
    }

    // --- Already answered by a different seam, so registering the local name would DUPLICATE
    // a deliberate architectural decision rather than extend the facade.
    //
    // Batch 8's audit found `list_agent_conversations` and `list_agent_conversations_page`
    // detector-clean, fail-open-free, and reached through a documented `_for_app_state` seam
    // carrying no spawn authority — i.e. it would have registered them on the evidence. Batch 5
    // had already resolved them by SPLITTING the seam: `list_remote_agent_conversations` and
    // `list_remote_agent_conversations_page` in `remote_transcript_commands` are the registered
    // answer, and the local twins stay off the facade. Registering the local names would put two
    // facade paths on one query for no new capability. The refusal is pinned in
    // `the_local_conversation_lists_stay_unregistered`; this assertion records WHY batch 8 did
    // not re-add them, so the next batch does not rediscover the same clean audit and act on it.
    for command in ["list_agent_conversations", "list_agent_conversations_page"] {
        assert!(
            find_spec(command).is_none(),
            "`{command}` is answered by its registered `list_remote_*` twin, not by the local \
             command; a clean audit is not a reason to add a second path to one query"
        );
        assert!(
            find_spec(&format!(
                "list_remote_{}",
                command.trim_start_matches("list_")
            ))
            .is_some(),
            "the `list_remote_*` twin that justifies refusing `{command}` must exist"
        );
    }

    // --- A transport-shape refusal, not an authority one.
    //
    // `list_conversation_folder_references` is a pure `SELECT ... WHERE removed_at IS NULL`
    // with no fail-open and no spawn authority — it audits cleaner than the rows registered
    // above. It returns `Result<_, AppError>`, and `AppError` is not `Serialize`, so the
    // `fallible` dispatch arm cannot render its error. Changing the command's error contract
    // is an implementation change, not a census decision, so it is deferred rather than
    // registered or ledgered as though it had authority it does not have.
    let folder = policy_for(
        "list_conversation_folder_references",
        "conversation_folder_reference_commands",
    )
    .expect("ledgered");
    assert!(
        find_spec("list_conversation_folder_references").is_none(),
        "list_conversation_folder_references is deferred on its error type, not registered"
    );
    assert_ne!(
        folder.class,
        RiskClass::Denied,
        "the deferral is a transport-shape gap; recording it as Denied would misstate the finding"
    );
}

/// Calibration probe for PR 3.1-b batch 9 ITEM 0 — the retroactive-closure candidate set.
///
/// Every command batches 1–8 audited and refused, measured against the CURRENT graph. The
/// batch-8 workspace-crate walk invalidated pre-batch-8 detector verdicts, and `reopen_issue`
/// is the standing proof that a tracker's recorded detector verdict can be an artefact. So the
/// manifest classification each refusal receives is derived from THIS output, never from the
/// reason phrase a tracker recorded.
#[test]
#[ignore = "calibration probe"]
fn probe_batch9_retroactive_closure_candidates() {
    const CANDIDATES: &[&str] = &[
        "approve_fix_task",
        "approve_review",
        "archive_tasks_in_group",
        "clear_active_plan",
        "get_agent_conversation",
        "get_agent_conversation_messages_page",
        "get_agent_conversation_runtime_statuses",
        "get_agent_conversation_timeline_page",
        "get_agent_conversation_workspace",
        "get_agent_conversation_workspace_freshness",
        "get_agent_message_tool_call_detail",
        "get_agent_run_status_unified",
        "get_agent_running_states",
        "get_agent_timeline_item_tool_call_detail",
        "get_execution_status",
        "get_pending_permissions",
        "get_pending_questions",
        "get_queued_agent_messages",
        "get_running_processes",
        "is_agent_running",
        "is_chat_service_available",
        "list_agent_composer_skills",
        "list_agent_conversation_workspaces_by_project",
        "list_agent_conversations",
        "list_agent_conversations_page",
        "list_agent_sidebar_conversations",
        "list_conversation_folder_references",
        "mark_issue_addressed",
        "mark_issue_in_progress",
        "re_review_task_from_escalated",
        "reject_fix_task",
        "reject_review",
        "reopen_issue",
        "request_changes",
        "request_task_changes_for_review",
        "request_task_changes_from_reviewing",
        "resolve_permission_request",
        "resolve_user_question",
        "retry_qa",
        "search_agent_composer_entries",
        "seed_builtin_workflows",
        "send_agent_message",
        "set_active_plan",
        "set_active_project",
        "skip_qa",
        "start_agent_conversation",
        "start_research",
        "update_qa_settings",
        "update_review_settings",
        "verify_issue",
    ];
    let graph = CallGraph::build(&load_production_sources());
    let rows = census();
    let commands = rows.iter().map(|(c, _)| c.clone()).collect::<Vec<_>>();
    let detector_b = spawn_triggering_writers(&graph, commands, SPAWN_TRIGGERING_STATE_SURFACE);

    for (command, module) in &rows {
        if !CANDIDATES.contains(&command.as_str()) {
            continue;
        }
        let closure = graph.closure([command.clone()]);
        let a = closure_is_arming(&closure);
        let b = detector_b.contains(command);
        let c = tokens_reach_any(&closure.tokens, PROCESS_LAUNCH_SINKS);
        let row = policy_for(command, module).expect("ledgered");
        eprintln!(
            "PROBE-B9 {module} {command} a={a} b={b} c={c} class={:?} caps={:?}",
            row.class, row.capabilities,
        );
    }
}

/// Sink-level evidence for the batch-9 detector-(c) hits, so no classification is bought with
/// a same-name artefact (the `reopen_issue` lesson, made a standing procedure).
#[test]
#[ignore = "calibration probe"]
fn probe_batch9_detector_c_sink_evidence() {
    const SPAWNERS: &[&str] = &[
        "archive_tasks_in_group",
        "get_agent_conversation_runtime_statuses",
        "get_agent_conversation_workspace",
        "get_agent_conversation_workspace_freshness",
        "get_agent_running_states",
        "get_execution_status",
        "get_running_processes",
        "is_agent_running",
        "is_chat_service_available",
        "list_agent_conversation_workspaces_by_project",
        "list_agent_sidebar_conversations",
        "resolve_user_question",
        "search_agent_composer_entries",
        "send_agent_message",
        "start_agent_conversation",
    ];
    let graph = CallGraph::build(&load_production_sources());
    let census_rows = census();
    for command in SPAWNERS {
        let present = census_rows.iter().any(|(c, _)| c == command);
        let closure = graph.closure([(*command).to_string()]);
        let sinks = closure
            .sink_hits
            .iter()
            .map(|hit| hit.sink.clone())
            .collect::<BTreeSet<_>>();
        let launchers = PROCESS_LAUNCH_SINKS
            .iter()
            .filter(|sink| closure.tokens.contains(**sink))
            .collect::<Vec<_>>();
        eprintln!(
            "PROBE-B9C {command} in_census={present} visited={} launchers={:?} arming={:?}",
            closure.visited.len(),
            launchers,
            sinks,
        );
    }
}

/// PR 3.1-b batch 9 ITEM 0 — the detector-(c) refusals declare the capability they REACH.
///
/// Fourteen commands were audited and refused across batches 1–8 with "it shells out" as the
/// mechanism, and every one kept the `AgentControl` module default. A row whose closure resolves
/// a CLI binary while rendering `registerable` is precisely the P-11 ratchet's definition of an
/// unresolved name, so the audits were invisible to the gate built to count them.
///
/// This gate is the honesty condition on that correction, in BOTH directions: the capability is
/// only declarable while the launch path is measurably there. If a command stops reaching a
/// process-launch sink, this fails and the row returns to review instead of keeping a
/// classification it no longer earns.
#[test]
fn batch9_detector_c_refusals_declare_the_capability_they_reach() {
    let graph = CallGraph::build(&load_production_sources());
    let modules = census().into_iter().collect::<BTreeMap<_, _>>();

    let mut checked = 0;
    for entry in COMMAND_OVERRIDES {
        if entry.policy.capabilities != [Capability::SpawnsProcess]
            || !entry.policy.reason.starts_with("detector-c:")
        {
            continue;
        }
        checked += 1;
        let command = entry.command;
        let module = modules
            .get(command)
            .unwrap_or_else(|| panic!("`{command}` carries a ledger override but left the census"));

        let closure = graph.closure([command.to_string()]);
        assert!(
            !closure.visited.is_empty(),
            "`{command}` did not resolve; this assertion would be vacuous"
        );
        let launchers = PROCESS_LAUNCH_SINKS
            .iter()
            .filter(|sink| closure.tokens.contains(**sink))
            .collect::<Vec<_>>();
        assert!(
            !launchers.is_empty(),
            "`{command}` declares SpawnsProcess but reaches no PROCESS_LAUNCH_SINKS resolver; \
             either the launch path was removed — in which case re-audit the row rather than \
             keeping the capability — or the call graph regressed"
        );

        let row = policy_for(command, module).expect("ledgered");
        assert_eq!(
            row.class,
            RiskClass::Elevated,
            "`{command}` carries SpawnsProcess, which class_permits admits only under Elevated"
        );
        assert_eq!(
            ralphx_remote_protocol::v1_resolution_with_audit(
                row.class,
                row.capabilities,
                audit_refusal_for(command).is_some(),
            ),
            ralphx_remote_protocol::V1Resolution::HostDeniedSpawnsProcess,
            "`{command}` must resolve through the manifest, not stay on the ratchet"
        );
        assert!(
            find_spec(command).is_none(),
            "`{command}` renders a host denial and must not be registered"
        );
    }

    assert_eq!(
        checked, 26,
        "batch 9 classified thirteen detector-(c) refusals, batch 10 closed the fourteenth \
         (`resolve_user_question`), batch 13 added `get_mcp_catalog` and `refresh_mcp_catalog`, \
         and batch 14 added TEN more — its twelve detector-DETECTED floor rows, of which \
         `set_agent_conversation_workspace_auto_publish` was already counted here and one other \
         was already an override. All are genuinely detector-visible, so they belong UNDER this \
         gate's re-measurement rather than beside it. Rows deliberately absent: batch 12's three \
         floor rows open `detector-c, hand-traced:` rather than `detector-c:`, batch 13's third \
         refusal (`retry_legacy_mcp_registration_repair`) is excluded on purpose, and batch 14's \
         THIRTEEN hand-traced floor rows open `detector-c-MISS, hand-traced:` — detector (c) \
         cannot see their launches, so a gate that re-measures the sink would fail on rows that \
         are nonetheless \
         correct. A change to this count is a census decision and must be made deliberately"
    );
}

/// PR 3.1-b batch 10 — the batch-9 declared-membership shadowing gap, CLOSED.
///
/// Batch 9 measured `resolve_user_question` as the fourteenth detector-(c) refusal and could not
/// act on it. Two things blocked a quiet fix, and both were gates doing their job:
///
/// 1. the command already had a `CommandOverride`, so an APPENDED `process_refusal` row was
///    silently shadowed by `policy_for`'s first-match lookup — the duplicate-override assert
///    caught it, and the detector-(c) gate then failed on the shadowed row's class;
/// 2. `exemptions_and_declared_memberships_are_exact` pinned the row's reason as VERBATIM equal
///    to `DECLARED_MEMBERSHIPS[1].1`, so any reason carrying the finding broke the pin.
///
/// Batch 10 fixes it the only way that leaves one authoritative statement: the EXISTING row is
/// corrected in place, and the pin is relaxed to the `contains` form its `resolve_permission_
/// request` sibling already used. The declared membership itself is preserved and re-asserted
/// below — it was never the false part, and it is what keeps the command in the P-17b suite and
/// its `ANCHORS` list.
///
/// The correction is authority-INCREASING: the command moves from "unreachable from a default
/// pairing" to "unreachable at every scope".
#[test]
fn batch10_closes_the_declared_membership_process_launch_gap() {
    let graph = CallGraph::build(&load_production_sources());
    let closure = graph.closure(["resolve_user_question".to_string()]);
    assert!(
        !closure.visited.is_empty(),
        "`resolve_user_question` did not resolve; this assertion would be vacuous"
    );
    let launchers = PROCESS_LAUNCH_SINKS
        .iter()
        .filter(|sink| closure.tokens.contains(**sink))
        .collect::<Vec<_>>();
    assert!(
        !launchers.is_empty(),
        "`resolve_user_question` no longer reaches a process-launch sink; the SpawnsProcess \
         capability is no longer earned and the row must be re-audited rather than kept"
    );

    let row = policy_for("resolve_user_question", "question_commands").expect("ledgered");
    assert_eq!(
        row.class,
        RiskClass::Elevated,
        "the correction moves the row to the only class `class_permits` admits SpawnsProcess under"
    );
    assert_eq!(row.capabilities, &[Capability::SpawnsProcess]);
    assert_eq!(
        ralphx_remote_protocol::v1_resolution_with_audit(
            row.class,
            row.capabilities,
            audit_refusal_for("resolve_user_question").is_some(),
        ),
        ralphx_remote_protocol::V1Resolution::HostDeniedSpawnsProcess,
        "the gap is closed by the row resolving through the manifest, not by a comment"
    );
    assert!(
        find_spec("resolve_user_question").is_none(),
        "a host denial must not be registered"
    );

    // The half of the fix that is easiest to lose: correcting the class must NOT silently drop
    // the declared membership, or the command would fall out of the generated `ui:agent` suite
    // and out of its anchor list while looking more strictly classified than before.
    let membership = DECLARED_MEMBERSHIPS
        .iter()
        .find(|(command, _)| *command == "resolve_user_question")
        .expect("the declared membership is preserved by the correction, not replaced by it");
    assert!(
        row.reason.contains(membership.1),
        "the corrected row must still carry the declared membership `{}`; it states what the \
         command DOES, while the class states what authority it reaches",
        membership.1
    );
    assert!(
        row.reason.starts_with("detector-c:"),
        "the row must state the finding that justifies the class"
    );
}

/// PR 3.1-b batch 11 — the census `B4` remainder, one disposition per member.
///
/// Same three-valued shape batch 10 established, and the same non-negotiable inside it: the
/// detector-(c) floor is RE-MEASURED per member here rather than inherited from the batch's
/// probe output. A registered member that grows a process-launch path fails loudly instead of
/// keeping its registration, and a refused member that LOSES its launch path fails too — a
/// stale refusal is as dishonest as a stale registration, and `host-denied-spawns-process`
/// asserts a reachable sink, not a historical one.
///
/// This test is also the live pin for the seven `v1-audit-refused` rows batch 11 added: each is
/// named below, so deleting the table takes the classifications down with it.
#[test]
fn batch11_closes_the_b4_remainder() {
    // `Read` and `Agent` are registered; `FailOpen` and `Floor` are manifest classifications.
    #[derive(PartialEq, Eq, Clone, Copy, Debug)]
    enum Disposition {
        Read,
        Agent,
        FailOpen,
        Floor,
    }
    use Disposition::*;

    const TABLE: &[(&str, &str, Disposition)] = &[
        // The reads: hand-audited write-free, dropped from the conservative module default.
        ("get_ideation_session", "ideation_commands", Read),
        ("get_ideation_session_with_data", "ideation_commands", Read),
        ("get_ideation_agent_workspace", "ideation_commands", Read),
        ("list_ideation_sessions", "ideation_commands", Read),
        ("get_session_group_counts", "ideation_commands", Read),
        ("list_sessions_by_group", "ideation_commands", Read),
        ("get_child_sessions", "ideation_commands", Read),
        ("get_latest_child_session_id", "ideation_commands", Read),
        ("get_task_proposal", "ideation_commands", Read),
        ("list_session_proposals", "ideation_commands", Read),
        ("get_proposal_dependencies", "ideation_commands", Read),
        ("get_proposal_dependents", "ideation_commands", Read),
        ("get_task_blockers", "ideation_commands", Read),
        ("get_blocked_tasks", "ideation_commands", Read),
        ("get_tasks_disable_impact", "ideation_commands", Read),
        ("get_ideation_settings", "ideation_commands", Read),
        ("get_ideation_effort_settings", "ideation_commands", Read),
        ("get_ideation_model_settings", "ideation_commands", Read),
        ("get_agent_lane_settings", "ideation_commands", Read),
        // The writers: registered at `ui:agent`, never dropped to Read on detector silence.
        ("update_ideation_session_title", "ideation_commands", Agent),
        ("reorder_proposals", "ideation_commands", Agent),
        ("assess_proposal_priority", "ideation_commands", Agent),
        ("assess_all_priorities", "ideation_commands", Agent),
        ("remove_proposal_dependency", "ideation_commands", Agent),
        ("update_ideation_settings", "ideation_commands", Agent),
        (
            "update_ideation_effort_settings",
            "ideation_commands",
            Agent,
        ),
        ("update_ideation_model_settings", "ideation_commands", Agent),
        ("update_agent_lane_settings", "ideation_commands", Agent),
        ("create_workflow", "workflow_commands", Agent),
        ("update_workflow", "workflow_commands", Agent),
        ("set_default_workflow", "workflow_commands", Agent),
        ("activate_methodology", "methodology_commands", Agent),
        ("deactivate_methodology", "methodology_commands", Agent),
        // The fail-opens: an error discarded on a path that still returns Ok, where losing it
        // changes the ANSWER rather than its completeness.
        ("analyze_dependencies", "ideation_commands", FailOpen),
        ("export_ideation_session", "ideation_commands", FailOpen),
        ("create_task_proposal", "ideation_commands", FailOpen),
        (
            "get_agent_harness_availability",
            "ideation_commands",
            FailOpen,
        ),
        (
            "get_ideation_harness_availability",
            "ideation_commands",
            FailOpen,
        ),
        (
            "create_cross_project_session",
            "ideation_commands",
            FailOpen,
        ),
        ("import_ideation_session", "ideation_commands", FailOpen),
        // The floor: each hand-traced to a concrete Command::new, not accepted on the boolean.
        ("copy_agent_conversation_plan", "agent_plan_commands", Floor),
        (
            "import_agent_conversation_plan",
            "agent_plan_commands",
            Floor,
        ),
        ("activate_agent_task_pipeline", "agent_plan_commands", Floor),
        (
            "activate_agent_plan_direct_implementation",
            "agent_plan_commands",
            Floor,
        ),
        ("start_agent_task_pipeline", "agent_plan_commands", Floor),
        ("create_ideation_session", "ideation_commands", Floor),
        ("archive_ideation_session", "ideation_commands", Floor),
        ("reopen_ideation_session", "ideation_commands", Floor),
        ("spawn_session_namer", "ideation_commands", Floor),
        ("apply_proposals_to_kanban", "ideation_commands", Floor),
        (
            "restart_ideation_implementation",
            "ideation_commands",
            Floor,
        ),
        ("set_tasks_feature_enabled", "ideation_commands", Floor),
    ];

    let graph = CallGraph::build(&load_production_sources());

    for (command, module, disposition) in TABLE {
        let row = policy_for(command, module)
            .unwrap_or_else(|| panic!("`{command}` must be ledgered by batch 11"));

        // (1) The floor, re-measured per member. This is the gate, not a formality.
        let closure = graph.closure([(*command).to_string()]);
        let reaches_launch = tokens_reach_any(&closure.tokens, PROCESS_LAUNCH_SINKS);
        if *disposition == Floor {
            assert!(
                reaches_launch,
                "`{command}` is classified host-denied-spawns-process but no longer reaches a \
                 process-launch sink; the claim is now false and the row must be re-audited \
                 rather than kept"
            );
        } else {
            assert!(
                !reaches_launch,
                "`{command}` now reaches a process-launch sink; the absolute floor forecloses \
                 every v1 scope, so its batch-11 disposition is void"
            );
        }

        // (2) A member whose CLASS moved may not keep the placeholder that means "no audit was
        // done". `FailOpen` is deliberately exempt: an audit refusal does not change the class,
        // so its row legitimately keeps the module default and the finding lives in
        // `AuditRefusal::finding` instead — asserted per-member in the match below. Requiring a
        // reviewed reason there would force a false statement into the class reason to buy a
        // classification, which is the exact trade the ledger's two-table split exists to prevent.
        if *disposition != FailOpen {
            assert!(
                !row.reason.contains("conservative-module-default"),
                "`{command}` changed class but carries the module-default reason; every batch-11 \
                 reclassification must record the audit that bought it"
            );
        }

        match disposition {
            Read => {
                assert_eq!(row.class, RiskClass::Read, "`{command}` must be Read");
                assert!(
                    row.capabilities.is_empty(),
                    "`{command}` is a Read row and must carry no capability"
                );
                assert!(
                    find_spec(command).is_some(),
                    "`{command}` audited clean and must be registered"
                );
            }
            Agent => {
                assert_eq!(
                    row.class,
                    RiskClass::AgentControl,
                    "`{command}` writes; a silent detector never licenses dropping it to Read"
                );
                let spec = find_spec(command)
                    .unwrap_or_else(|| panic!("`{command}` audited clean and must be registered"));
                assert_eq!(spec.class, RiskClass::AgentControl);
            }
            FailOpen => {
                let refusal = audit_refusal_for(command).unwrap_or_else(|| {
                    panic!("`{command}` audited dirty and must carry an audit refusal")
                });
                assert_eq!(
                    refusal.reason,
                    ralphx_remote_protocol::AuditRefusalReason::FailOpenUntilFixed,
                    "`{command}` was refused for a fail-open and must say so"
                );
                // The finding is what the exempted class reason would otherwise have carried, so
                // it has to be specific enough to falsify rather than a restatement of the code.
                assert!(
                    refusal.finding.len() > 80
                        && !refusal.finding.contains("conservative-module-default"),
                    "`{command}` must record a falsifiable finding, not a placeholder"
                );
                assert!(
                    find_spec(command).is_none(),
                    "`{command}` is refused and must not be registered"
                );
                assert_eq!(
                    ralphx_remote_protocol::v1_resolution_with_audit(
                        row.class,
                        row.capabilities,
                        true,
                    ),
                    ralphx_remote_protocol::V1Resolution::V1AuditRefused,
                    "`{command}` must resolve through the manifest, not through a comment"
                );
            }
            Floor => {
                assert_eq!(
                    row.class,
                    RiskClass::Elevated,
                    "`{command}` reaches a launch; Elevated is the only class permitting \
                     SpawnsProcess"
                );
                assert_eq!(row.capabilities, &[Capability::SpawnsProcess]);
                assert!(
                    row.reason.starts_with("detector-c"),
                    "`{command}` must state the finding that justifies the class"
                );
                assert!(
                    find_spec(command).is_none(),
                    "`{command}` is host-denied and must not be registered"
                );
                assert_eq!(
                    ralphx_remote_protocol::v1_resolution_with_audit(
                        row.class,
                        row.capabilities,
                        audit_refusal_for(command).is_some(),
                    ),
                    ralphx_remote_protocol::V1Resolution::HostDeniedSpawnsProcess,
                );
            }
        }
    }

    // The two arming writes no detector models. Registering either without a declaration would
    // put an arming write at `ui:agent` that the generated P-17b negative suite never proves
    // unreachable from a default pairing — the exact hole the chat-send declaration closed.
    for command in ["update_ideation_settings", "update_agent_lane_settings"] {
        let declared = DECLARED_MEMBERSHIPS
            .iter()
            .any(|(name, _)| *name == command);
        assert!(
            declared,
            "`{command}` arms a spawner through a surface no detector watches and must declare \
             its membership"
        );
    }
}

/// PR 3.1-b batch 9 ITEM 0 — `v1-audit-refused` cannot be granted without a live pin.
///
/// This is the fail-closed proof for the one resolution a human can hand out by writing a table
/// row. Three conditions, each of which alone would be forgeable:
///
/// * the command is real, still in the census, and NOT served by the facade;
/// * the mechanical resolution is `Registerable`, so the row is doing the classifying and is not
///   quietly riding on a denial it did not earn;
/// * the command is NAMED inside a pinned-refusal test, so the mechanism behind the finding is
///   asserted somewhere CI actually runs. A row whose pin is deleted fails here.
#[test]
fn batch9_audit_refusals_are_tied_to_a_live_pin() {
    const PINNED_REFUSAL_TESTS: &[&str] = &[
        "the_b2_workspace_read_refusals_are_pinned",
        "the_b2_getter_refusals_are_pinned",
        "b2_detector_clean_members_refused_on_their_own_findings",
        "b3_members_that_audit_dirty_stay_unregistered",
        "b4_members_that_audit_dirty_stay_unregistered",
        "b1_sibling_getters_that_audit_dirty_stay_above_read",
        "b1_read_reclassifications_are_reviewed_rather_than_module_defaults",
        // Batch 10's disposition table is the pin for the seven members it classified: it names
        // each one and re-measures the detector-(c) floor on it, so deleting the table takes the
        // classifications down with it.
        "batch10_closes_the_batch9_arming_and_write_block",
        // Batch 11's disposition table is the pin for the seven B4 members it classified
        // `v1-audit-refused`: it names each one and re-measures the detector-(c) floor on it.
        "batch11_closes_the_b4_remainder",
        // Batch 14's disposition table is the pin for the NINE refusals that closed the ratchet:
        // the seven transport-shape rows, `get_manual_role_defaults`, and `reject_fix_task`. It
        // names each one and re-measures the detector-(c) floor on it, so deleting the table
        // takes the classifications down with it.
        "batch14_closes_the_ratchet_at_zero",
    ];
    let own_source = include_str!("capability_ledger_tests.rs");
    let pin_bodies = PINNED_REFUSAL_TESTS
        .iter()
        .map(|name| {
            let start = own_source
                .find(&format!("fn {name}("))
                .unwrap_or_else(|| panic!("pinned-refusal test `{name}` no longer exists"));
            let rest = &own_source[start..];
            let end = rest.find("\n}\n").expect("pin body is brace-terminated");
            &rest[..end]
        })
        .collect::<Vec<_>>();

    let modules = census().into_iter().collect::<BTreeMap<_, _>>();
    let mut seen_reasons = BTreeSet::new();

    for refusal in AUDIT_REFUSALS {
        let command = refusal.command;
        let module = modules.get(command).unwrap_or_else(|| {
            panic!("`{command}` carries an audit refusal but is not in the census")
        });
        assert!(
            find_spec(command).is_none(),
            "`{command}` renders `v1-audit-refused` but is registered; the ledger and the \
             registry contradict each other"
        );

        let row = policy_for(command, module).expect("ledgered");
        assert_eq!(
            ralphx_remote_protocol::v1_resolution(row.class, row.capabilities),
            ralphx_remote_protocol::V1Resolution::Registerable,
            "`{command}`'s mechanical resolution already refuses it, so the audit row adds \
             nothing and would only obscure which fact is load-bearing"
        );
        assert_eq!(
            ralphx_remote_protocol::v1_resolution_with_audit(row.class, row.capabilities, true),
            ralphx_remote_protocol::V1Resolution::V1AuditRefused,
        );

        assert!(
            pin_bodies
                .iter()
                .any(|body| body.contains(&format!("\"{command}\""))),
            "`{command}` is classified `v1-audit-refused` but no pinned-refusal test names it. \
             The classification records the disposition; the pin records the mechanism, and the \
             ratchet may only count a refusal that still has both."
        );
        assert!(
            refusal.finding.len() > 40,
            "`{command}`'s finding is too thin to falsify"
        );
        seen_reasons.insert(refusal.reason);
    }

    // Representative-per-class: an unused reason variant is an invitation to reach for it
    // without evidence, and an unexercised one proves nothing about the derivation.
    for reason in ralphx_remote_protocol::AUDIT_REFUSAL_REASONS {
        assert!(
            seen_reasons.contains(reason),
            "no audit refusal uses `{reason:?}`; drop the variant or evidence it"
        );
    }
    assert_eq!(
        AUDIT_REFUSALS.len(),
        32,
        "batch 9 recorded eleven audit refusals, batch 10 added seven, batch 11 added seven more \
         (the B4 fail-opens), and batch 14 added the final NINE that closed the ratchet: seven \
         `TransportShapeDeferred` rows sharing ONE non-Serialize `AppError` contract, \
         `get_manual_role_defaults` on a fail-open, and `reject_fix_task` on the newly minted \
         `ReachesCorrectiveTransition`. WP3 then REMOVED two — the tool-call-detail pair, whose \
         `FailOpenUntilFixed` finding was fixed at its source and cleared by a per-command \
         audit. Changing that count is a census decision"
    );
}

/// PR 3.1-b batch 10 — the batch-9 arming/write block, CLOSED.
///
/// Batch 9 measured the facade and refused to map arming onto a host denial: 83 registered ops
/// included 16 at `agentControl`, four carrying `Capability::AgentControl` and three carrying
/// `SeedsSpawnTriggeringState`. So "detector (a) fires" or "it writes" recorded which batch ran
/// out of scope, not a property of the command, and classifying the 25 anyway would have moved
/// the ratchet by 25 while putting 25 false statements in the ledger.
///
/// What that argument implied — and what batch 9 explicitly deferred — is that the block was
/// blocked on an AUDIT, not on a mechanism. Batch 10 did the audit and this test records its
/// result per member, so the block is closed by evidence rather than by the mapping batch 9
/// refused. Seven members audited dirty and are classified on their own findings; none of the
/// 25 was resolved by analogy to another command's verdict.
#[test]
fn batch10_closes_the_batch9_arming_and_write_block() {
    // The 25 members of batch 9's block, each with the disposition batch 10's audit produced.
    // `true` = registered at `ui:agent`; `false` = classified `v1-audit-refused`. There is no
    // third column on purpose: the closed-loop rule is that an audited member leaves with a
    // ratchet-moving disposition, so a member that is neither would fail both arms below.
    const REGISTERED: &str = "registered";
    const CLASSIFIED: &str = "classified";
    const UNRESOLVED: &str = "unresolved";
    const DISPOSITIONS: &[(&str, &str)] = &[
        // --- Registered at `ui:agent`. Human gate decisions and guarded writes of exactly the
        //     shape the facade already serves.
        ("request_task_changes_for_review", REGISTERED),
        ("re_review_task_from_escalated", REGISTERED),
        ("update_review_settings", REGISTERED),
        ("reopen_issue", REGISTERED),
        ("verify_issue", REGISTERED),
        ("mark_issue_in_progress", REGISTERED),
        ("mark_issue_addressed", REGISTERED),
        ("approve_review", REGISTERED),
        ("reject_review", REGISTERED),
        ("request_changes", REGISTERED),
        ("retry_qa", REGISTERED),
        ("update_qa_settings", REGISTERED),
        ("clear_active_plan", REGISTERED),
        ("seed_builtin_workflows", REGISTERED),
        ("start_research", REGISTERED),
        ("set_active_project", REGISTERED),
        // --- Classified `v1-audit-refused`. Four surfaces the facade already answers under
        //     another name, and three writes whose audit came back fail-open.
        ("get_agent_conversation", CLASSIFIED),
        ("get_agent_conversation_messages_page", CLASSIFIED),
        ("get_agent_conversation_timeline_page", CLASSIFIED),
        ("resolve_permission_request", CLASSIFIED),
        ("archive_tasks_in_group", CLASSIFIED),
        ("request_task_changes_from_reviewing", CLASSIFIED),
        ("skip_qa", CLASSIFIED),
        // --- The fix-task repair pair, the one part of batch 9's block batch 10 could not
        //     close. It stayed UNRESOLVED here for four batches and PR 3.1-b batch 14 closed it,
        //     because driving the ratchet to zero left no batch to defer to. Both dispositions
        //     are recorded in place rather than deleted, so this table stays the honest history
        //     of what each batch could and could not do.
        //
        //     `reject_fix_task` is now CLASSIFIED. The finding never changed — it reaches
        //     `transition_task_corrective` and
        //     `no_registered_facade_target_reaches_a_corrective_transition` forbids that at
        //     every scope — but batch 10 had no honest code for it, and batch 9's standing rule
        //     forbids buying a classification with an ill-fitting one. Batch 14 minted
        //     `AuditRefusalReason::ReachesCorrectiveTransition`: one specific, separately
        //     CI-enforced mechanism, not the generic arming code the vocabulary still withholds.
        //
        //     `approve_fix_task` is now REGISTERED. Its body audited CLEAN here and it was
        //     withheld purely on the pair argument — no remote way to reject. That argument was
        //     re-checked against the current registry and did not survive: `block_task` and
        //     `stop_task` are both registered, so the brake it was protecting exists.
        ("approve_fix_task", REGISTERED),
        ("reject_fix_task", CLASSIFIED),
    ];

    let modules = census().into_iter().collect::<BTreeMap<_, _>>();
    let graph = CallGraph::build(&load_production_sources());

    let mut registered = 0usize;
    let mut classified = 0usize;
    let mut unresolved = 0usize;
    for (command, disposition) in DISPOSITIONS {
        let module = modules
            .get(*command)
            .unwrap_or_else(|| panic!("`{command}` left the census"));
        let row = policy_for(command, module).expect("ledgered");

        // The absolute floor, re-measured per member rather than inherited from batch 9's
        // probe. Detector (c) outranks every scope decision: a command whose closure resolves a
        // CLI binary is not registerable at ANY v1 scope, so if one of these ever grows a launch
        // path the registration below must be withdrawn, not re-justified.
        let closure = graph.closure([(*command).to_string()]);
        assert!(
            !closure.visited.is_empty(),
            "`{command}` did not resolve; the floor check would be vacuous"
        );
        assert!(
            !tokens_reach_any(&closure.tokens, PROCESS_LAUNCH_SINKS),
            "`{command}` now reaches a process-launch sink. The detector-(c) floor is absolute \
             and is not a scope decision: withdraw the batch-10 disposition and reclassify the \
             row `host-denied-spawns-process` rather than keeping it on the facade."
        );

        if *disposition == UNRESOLVED {
            // Held members must stay COUNTABLE. The failure this forecloses is the quiet one:
            // a member dropped from the block without being registered or classified would
            // vanish from every batch report while still sitting on the ratchet.
            unresolved += 1;
            assert!(
                find_spec(command).is_none(),
                "`{command}` is recorded unresolved but is registered"
            );
            assert!(
                audit_refusal_for(command).is_none(),
                "`{command}` is recorded unresolved but carries an audit refusal"
            );
            assert_eq!(
                ralphx_remote_protocol::v1_resolution_with_audit(
                    row.class,
                    row.capabilities,
                    false,
                ),
                ralphx_remote_protocol::V1Resolution::Registerable,
                "`{command}` must stay countable as unresolved"
            );
        } else if *disposition == REGISTERED {
            registered += 1;
            let spec = find_spec(command)
                .unwrap_or_else(|| panic!("`{command}` is recorded registered but has no spec"));
            assert_eq!(
                spec.class,
                RiskClass::AgentControl,
                "`{command}` was registered below the arming class it was audited at"
            );
            assert!(
                audit_refusal_for(command).is_none(),
                "`{command}` is registered AND carries an audit refusal; the ledger and the \
                 registry contradict each other"
            );
            assert!(
                !row.reason.contains("conservative-module-default"),
                "`{command}` is registered on the module-default placeholder, which records that \
                 no judgement was made. A registered command needs a reviewed reason."
            );
        } else {
            classified += 1;
            assert!(
                audit_refusal_for(command).is_some(),
                "`{command}` is recorded classified but carries no audit refusal, so it would \
                 silently stay on the ratchet"
            );
            assert!(
                find_spec(command).is_none(),
                "`{command}` renders `v1-audit-refused` but is registered"
            );
            assert_eq!(
                ralphx_remote_protocol::v1_resolution_with_audit(row.class, row.capabilities, true,),
                ralphx_remote_protocol::V1Resolution::V1AuditRefused,
            );
        }
    }

    // Counts updated by PR 3.1-b batch 14, which closed the two members batch 10 left open.
    // The table still records batch 10's audit; these totals record the CURRENT disposition of
    // batch 9's block, which is what the assertions below can actually verify.
    assert_eq!(
        registered, 17,
        "batch 10 registered sixteen of the block and batch 14 released `approve_fix_task` from \
         its pair hold"
    );
    assert_eq!(
        classified, 8,
        "batch 10 classified seven of the block and batch 14 classified `reject_fix_task` on the \
         newly minted `reaches-corrective-transition` reason"
    );
    assert_eq!(
        unresolved, 0,
        "batch 9's block is fully closed. The fix-task repair pair was the last open part and \
         batch 14 resolved both halves; a member reappearing here means a disposition was \
         withdrawn without being replaced"
    );
    assert_eq!(
        DISPOSITIONS.len(),
        25,
        "batch 9 left exactly 25 members; changing that count is a census decision"
    );

    // Batch 9's counter-evidence, preserved rather than discarded. It is the reason this block
    // was registerable at all: "detector (a) fires" or "it writes" cannot mean "the host denies
    // it" while the facade serves arming authority. Batch 10 acted on that argument, so if the
    // facade ever stops serving it, this test's premise is gone and it must fail loudly.
    let arming_ops = REMOTE_COMMANDS
        .iter()
        .filter(|spec| {
            matches!(
                policy_for(spec.name, modules.get(spec.name).map_or("", |m| m.as_str()))
                    .map(|row| row.class),
                Some(RiskClass::AgentControl)
            )
        })
        .count();
    assert!(
        arming_ops >= 16,
        "the facade serves {arming_ops} agentControl ops; batch 9 refused to call arming \
         authority `host-denied` BECAUSE the facade grants it, and batch 10 registered 18 more \
         on that argument. If that changed, re-derive the decision rather than editing this \
         number."
    );
}

/// Retained from batch 9 as the record of WHY the block was registerable, now that batch 10 has
/// acted on it. The pairing table is the sharpest form of the counter-evidence and outlives the
/// block it justified: each left-column command is served by the facade under the right-column
/// name, so calling any of them `host-denied` would have been false.
#[test]
fn the_registered_twin_pairings_that_justified_the_batch10_registrations_hold() {
    for (refused, registered_twin) in [
        // Twin-classified by batch 10: the facade answers the query under the right-hand name.
        ("resolve_permission_request", "approve_permission_request"),
        ("get_agent_conversation", "get_remote_agent_conversation"),
        (
            "get_agent_conversation_messages_page",
            "get_remote_agent_conversation_messages_page",
        ),
        (
            "get_agent_conversation_timeline_page",
            "get_remote_agent_conversation_timeline_page",
        ),
        (
            "list_agent_conversations",
            "list_remote_agent_conversations",
        ),
        // Host-denied, and still the standing proof that a denial needs a MECHANISM: this pair
        // sits at the same declared membership, and only the detector-(c) launch path separates
        // them.
        ("resolve_user_question", "answer_user_question"),
    ] {
        assert!(
            find_spec(refused).is_none(),
            "`{refused}` is refused; if it was registered, update this pairing"
        );
        assert!(
            find_spec(registered_twin).is_some(),
            "`{registered_twin}` is the registered answer that makes refusing `{refused}` a \
             seam decision rather than a capability gap; if it was unregistered, the twin \
             classification must be re-derived"
        );
    }
}

// =======================================================================================
// PR 3.1-b batch 12 — probes
// =======================================================================================

/// Shortest call path from `root` to a body that itself contains a `PROCESS_LAUNCH_SINKS`
/// token, using the same bare-name expansion `CallGraph::closure` uses.
///
/// `closure()` unions the tokens of every visited node, so detector (c) reports only a
/// boolean and the *reason* recorded on a refusal row is written by hand. This reconstructs
/// the edge chain that actually carried the launcher token, so a refusal reason can be argued
/// against a path instead of a `true` — and so an attribution that rides on a bare-name
/// collision is visible as such.
#[cfg(test)]
fn launcher_paths(graph: &CallGraph, root: &str) -> Vec<Vec<String>> {
    sink_paths(graph, root, PROCESS_LAUNCH_SINKS)
}

/// Shortest call paths from `root` to a body whose own tokens name one of `sinks`.
#[cfg(test)]
fn sink_paths(graph: &CallGraph, root: &str, sinks: &[&str]) -> Vec<Vec<String>> {
    use std::collections::VecDeque;
    let mut queue = VecDeque::new();
    let mut seen = BTreeSet::new();
    let mut found: Vec<Vec<String>> = Vec::new();
    let starts = if graph.definitions_snapshot().contains_key(root) {
        graph.roots_named(root)
    } else {
        std::iter::once(root.to_string()).collect()
    };
    for s in starts {
        queue.push_back(vec![s]);
    }
    while let Some(path) = queue.pop_front() {
        let name = path.last().expect("nonempty").clone();
        if !seen.insert(name.clone()) {
            continue;
        }
        let own: BTreeSet<String> = graph.own_tokens_of(&name);
        if tokens_reach_any(&own, sinks) {
            found.push(path.clone());
            continue;
        }
        for callee in graph.callees_of(&name) {
            let mut next = path.clone();
            next.push(callee);
            queue.push_back(next);
        }
    }
    found
}

/// Attribution evidence for the two scanner errors batch 11 recorded by hand.
///
/// Batch 11 asserted that `resolve_manual_role_spawn_settings` is treated as launch-reaching
/// while terminating in pure DB/YAML, and that `resolve_node_cli_path` is confused with the
/// `find_node_cli_path` that `git_cmd` reaches through `ensure_resolved_node_bin_in_path`.
/// Both claims were made from a hand trace, never from the engine. This prints the engine's
/// own answer so the claims can be confirmed or refuted before anything is "fixed".
#[test]
#[ignore = "calibration probe"]
fn probe_scanner_attribution_bugs() {
    let graph = CallGraph::build(&load_production_sources());
    for subject in [
        "resolve_manual_role_spawn_settings",
        "ensure_resolved_node_bin_in_path",
        "resolved_node_bin_dir",
        "find_node_cli_path",
        "git_cmd",
    ] {
        let defs = graph.definitions_snapshot().get(subject).cloned();
        let closure = graph.closure([subject.to_string()]);
        let c = tokens_reach_any(&closure.tokens, PROCESS_LAUNCH_SINKS);
        let launchers = closure
            .tokens
            .iter()
            .filter(|t| {
                tokens_reach_any(
                    &std::iter::once((*t).clone()).collect(),
                    PROCESS_LAUNCH_SINKS,
                )
            })
            .cloned()
            .collect::<Vec<_>>();
        eprintln!(
            "PROBE-ATTR {subject} defs={:?} visited={} c={c} launchers={launchers:?}",
            defs,
            closure.visited.len()
        );
        for path in launcher_paths(&graph, subject).iter().take(6) {
            eprintln!("PROBE-ATTR   path: {}", path.join(" -> "));
        }
    }
}

/// Positive control for [`launcher_paths`] plus the census `B5`/`B7` module sweep.
///
/// The control matters more than the sweep: [`probe_scanner_attribution_bugs`] reports that
/// NEITHER helper batch 11 named as a scanner error reaches a launcher when taken as a root,
/// which would make the engine correct and batch 11's *diagnosis* wrong. That conclusion is
/// only worth anything if `launcher_paths` can find a path when one exists, so this prints the
/// real edge chain for B4 members batch 11 refused on a hand trace.
#[test]
#[ignore = "calibration probe"]
fn probe_b5_b7_module_batch_audit() {
    const CONTROL: &[&str] = &[
        "create_ideation_session",
        "activate_agent_task_pipeline",
        "apply_proposals_to_kanban",
        "copy_agent_conversation_plan",
    ];
    const MODULES: &[&str] = &[
        "activity_commands",
        "automation_commands",
        "metrics_commands",
        "research_commands",
        "artifact_commands",
        "notification_commands",
        "release_notes_commands",
        "task_context_commands",
        "ui_commands",
        "update_channel_commands",
    ];
    let graph = CallGraph::build(&load_production_sources());
    let rows = census();
    let commands = rows.iter().map(|(c, _)| c.clone()).collect::<Vec<_>>();
    let detector_b = spawn_triggering_writers(&graph, commands, SPAWN_TRIGGERING_STATE_SURFACE);

    for command in CONTROL {
        let closure = graph.closure([(*command).to_string()]);
        let c = tokens_reach_any(&closure.tokens, PROCESS_LAUNCH_SINKS);
        eprintln!("PROBE-CTRL {command} c={c}");
        for path in launcher_paths(&graph, command).iter().take(3) {
            eprintln!("PROBE-CTRL   path: {}", path.join(" -> "));
        }
    }

    for (command, module) in &rows {
        if !MODULES.contains(&module.as_str()) {
            continue;
        }
        let closure = graph.closure([command.clone()]);
        let a = closure_is_arming(&closure);
        let b = detector_b.contains(command);
        let c = tokens_reach_any(&closure.tokens, PROCESS_LAUNCH_SINKS);
        let row = policy_for(command, module).expect("ledgered");
        eprintln!(
            "PROBE-B57 {module} {command} a={a} b={b} c={c} class={:?} caps={:?} registered={}",
            row.class,
            row.capabilities,
            find_spec(command).is_some(),
        );
        if c {
            let launchers = closure
                .tokens
                .iter()
                .filter(|t| {
                    tokens_reach_any(
                        &std::iter::once((*t).clone()).collect(),
                        PROCESS_LAUNCH_SINKS,
                    )
                })
                .cloned()
                .collect::<Vec<_>>();
            eprintln!("PROBE-B57   launchers={launchers:?}");
            for path in launcher_paths(&graph, command).iter().take(3) {
                eprintln!("PROBE-B57   path: {}", path.join(" -> "));
            }
        }
    }
}

/// Detector-(a) evidence for `save_metrics_config`.
///
/// The sweep reports detector (a) firing on a command whose body is a single
/// `project_metrics_config` upsert. A pure settings write that reads as "arming" is either a
/// real transitive edge into a transition sink or an attribution artifact, and registering
/// over it either way would be registering over a boolean. This prints the sink hits and their
/// named targets so the answer is a path.
#[test]
#[ignore = "calibration probe"]
fn probe_save_metrics_config_arming_evidence() {
    let graph = CallGraph::build(&load_production_sources());
    for command in [
        "save_metrics_config",
        "get_metrics_config",
        "get_task_metrics",
    ] {
        let closure = graph.closure([command.to_string()]);
        eprintln!(
            "PROBE-MET {command} arming={} visited={}",
            closure_is_arming(&closure),
            closure.visited.len()
        );
        for hit in &closure.sink_hits {
            eprintln!("PROBE-MET   sink={} targets={:?}", hit.sink, hit.targets);
        }
        for path in sink_paths(&graph, command, &["send_message"])
            .iter()
            .take(2)
        {
            eprintln!("PROBE-MET   path: {}", path.join(" -> "));
        }
    }
}

/// PR 3.1-b batch 12 — the census `B5` block (activity, automation, metrics, research).
///
/// Same contract as [`batch11_closes_the_b4_remainder`]: every one of the 33 ratchet members is
/// named with its disposition, and the detector-(c) floor is RE-MEASURED per member so a
/// registered member that grows a launch path and a refused member that loses one both fail
/// loudly rather than drifting.
///
/// The block's own character is different from B4's, and the table records it. B4 was carried by
/// a large family of pure reads; `automation_commands` is the densest arming surface in the
/// census, and four of its members flip `automations.status` to `Active` — the armed value the
/// `automation-active` state surface names — while carrying none of that surface's write
/// markers. Those four are pinned as [`Arming`] so a future edit that drops the capability or
/// the declaration fails here.
#[test]
fn batch12_closes_the_b5_block() {
    #[derive(PartialEq, Eq, Clone, Copy, Debug)]
    enum Disposition {
        /// Registered `Read`/no-capability on a body audit that found no write.
        Read,
        /// Registered `AgentControl` — a writer a silent detector never licenses dropping.
        Agent,
        /// Registered `AgentControl` + `SeedsSpawnTriggeringState`: writes the armed value of
        /// a scanned surface AND is flagged by detector (b), which is what that capability
        /// asserts (see `seeds_spawn_triggering_state_tags_track_detector_b_evidence`).
        ArmingDetected,
        /// Registered `AgentControl` + a `DECLARED_MEMBERSHIPS` row: arms just as really, but
        /// invisibly to detector (b), so it may NOT claim the evidence capability.
        ArmingDeclared,
        /// `host-denied-spawns-process`, hand-traced to a concrete launch.
        Floor,
    }
    use Disposition::*;

    const TABLE: &[(&str, &str, Disposition)] = &[
        // --- activity_commands: five cursor-paginated reads, every error `map_err(..)?` ------
        ("list_task_activity_events", "activity_commands", Read),
        ("list_session_activity_events", "activity_commands", Read),
        ("list_all_activity_events", "activity_commands", Read),
        ("count_task_activity_events", "activity_commands", Read),
        ("count_session_activity_events", "activity_commands", Read),
        // --- metrics_commands: eight aggregate reads plus one settings upsert ---------------
        ("get_insights_stats", "metrics_commands", Read),
        ("get_project_stats", "metrics_commands", Read),
        ("get_insights_pr_insights", "metrics_commands", Read),
        ("get_project_pr_insights", "metrics_commands", Read),
        ("get_insights_trends", "metrics_commands", Read),
        ("get_project_trends", "metrics_commands", Read),
        ("get_metrics_config", "metrics_commands", Read),
        ("get_task_metrics", "metrics_commands", Read),
        ("save_metrics_config", "metrics_commands", Agent),
        // --- research_commands: three reads and three status writes that arm nothing --------
        ("get_research_presets", "research_commands", Read),
        ("get_research_process", "research_commands", Read),
        ("get_research_processes", "research_commands", Read),
        ("pause_research", "research_commands", Agent),
        ("resume_research", "research_commands", Agent),
        ("stop_research", "research_commands", Agent),
        // --- automation_commands: two reads --------------------------------------------------
        ("list_automations", "automation_commands", Read),
        ("get_automation", "automation_commands", Read),
        // --- automation_commands: brakes and settings, no `Active` write ---------------------
        ("pause_automation", "automation_commands", Agent),
        ("stop_automation", "automation_commands", Agent),
        ("cancel_automation_run", "automation_commands", Agent),
        ("update_automation_settings", "automation_commands", Agent),
        // --- automation_commands: the four that write `AutomationStatus::Active` ------------
        (
            "resume_automation_run",
            "automation_commands",
            ArmingDetected,
        ),
        ("restart_automation", "automation_commands", ArmingDeclared),
        (
            "retry_automation_plan_judge",
            "automation_commands",
            ArmingDeclared,
        ),
        (
            "skip_automation_judge",
            "automation_commands",
            ArmingDeclared,
        ),
        // --- the floor: each hand-traced to a concrete launch, not taken on the boolean -----
        ("create_automation_draft", "automation_commands", Floor),
        ("trigger_automation_run_now", "automation_commands", Floor),
        ("retry_automation_judge", "automation_commands", Floor),
    ];

    let graph = CallGraph::build(&load_production_sources());

    for (command, module, disposition) in TABLE {
        let row = policy_for(command, module)
            .unwrap_or_else(|| panic!("`{command}` must be ledgered by batch 12"));

        // (1) The floor, re-measured per member.
        let closure = graph.closure([(*command).to_string()]);
        let reaches_launch = tokens_reach_any(&closure.tokens, PROCESS_LAUNCH_SINKS);
        if *disposition == Floor {
            assert!(
                reaches_launch,
                "`{command}` is classified host-denied-spawns-process but no longer reaches a \
                 process-launch sink; the claim is now false and the row must be re-audited"
            );
        } else {
            assert!(
                !reaches_launch,
                "`{command}` now reaches a process-launch sink; the absolute floor forecloses \
                 every v1 scope, so its batch-12 disposition is void"
            );
        }

        // (2) A member whose class moved may not keep the placeholder meaning "no audit done".
        assert!(
            !row.reason.contains("conservative-module-default"),
            "`{command}` changed class but carries the module-default reason; every batch-12 \
             reclassification must record the audit that bought it"
        );

        match disposition {
            Read => {
                assert_eq!(row.class, RiskClass::Read, "`{command}` must be Read");
                assert!(
                    row.capabilities.is_empty(),
                    "`{command}` is a Read row and must carry no capability"
                );
                assert!(
                    find_spec(command).is_some(),
                    "`{command}` audited clean and must be registered"
                );
            }
            Agent => {
                assert_eq!(
                    row.class,
                    RiskClass::AgentControl,
                    "`{command}` writes; a silent detector never licenses dropping it to Read"
                );
                assert!(
                    !row.capabilities
                        .contains(&Capability::SeedsSpawnTriggeringState),
                    "`{command}` is pinned as a non-arming write; if it now seeds a scanned \
                     surface it must move to the Arming rows and declare its membership"
                );
                let spec = find_spec(command)
                    .unwrap_or_else(|| panic!("`{command}` audited clean and must be registered"));
                assert_eq!(spec.class, RiskClass::AgentControl);
            }
            ArmingDetected => {
                assert_eq!(
                    row.class,
                    RiskClass::AgentControl,
                    "`{command}` arms a background loop and stays at AgentControl"
                );
                assert!(
                    row.capabilities
                        .contains(&Capability::SeedsSpawnTriggeringState),
                    "`{command}` is detector-(b) visible and must record that evidence as \
                     SeedsSpawnTriggeringState"
                );
                assert!(
                    find_spec(command).is_some(),
                    "`{command}` audited clean and must be registered"
                );
            }
            ArmingDeclared => {
                assert_eq!(
                    row.class,
                    RiskClass::AgentControl,
                    "`{command}` arms a background loop and stays at AgentControl"
                );
                // The capability is an EVIDENCE tag, not a severity dial. Claiming it without
                // detector (b) behind it is the exact forgery
                // `seeds_spawn_triggering_state_tags_track_detector_b_evidence` forbids, so an
                // invisibly-arming write buys its honesty with a declaration instead.
                assert!(
                    !row.capabilities
                        .contains(&Capability::SeedsSpawnTriggeringState),
                    "`{command}` arms invisibly to detector (b) and must NOT claim the evidence \
                     capability; it carries a DECLARED_MEMBERSHIPS row instead"
                );
                assert!(
                    find_spec(command).is_some(),
                    "`{command}` audited clean and must be registered"
                );
            }
            Floor => {
                assert_eq!(
                    row.class,
                    RiskClass::Elevated,
                    "`{command}` reaches a launch; Elevated is the only class permitting \
                     SpawnsProcess"
                );
                assert_eq!(row.capabilities, &[Capability::SpawnsProcess]);
                assert!(
                    row.reason.starts_with("detector-c"),
                    "`{command}` must state the finding that justifies the class"
                );
                assert!(
                    find_spec(command).is_none(),
                    "`{command}` is host-denied and must not be registered"
                );
                assert_eq!(
                    ralphx_remote_protocol::v1_resolution_with_audit(
                        row.class,
                        row.capabilities,
                        audit_refusal_for(command).is_some(),
                    ),
                    ralphx_remote_protocol::V1Resolution::HostDeniedSpawnsProcess,
                );
            }
        }
    }

    // The three arming writes NO detector models. `resume_automation_run` is deliberately absent:
    // detector (b) already fires on it, so it needs the capability but not a declaration — the
    // same split `resume_automation` already carries.
    for command in [
        "restart_automation",
        "retry_automation_plan_judge",
        "skip_automation_judge",
    ] {
        let declared = DECLARED_MEMBERSHIPS
            .iter()
            .any(|(name, _)| *name == command);
        assert!(
            declared,
            "`{command}` flips automations.status to Active — the armed value \
             spawn_automation_scheduler scans — through a surface detector (b) does not watch, \
             and must declare its membership"
        );
    }
}

/// PR 3.1-b batch 12 — what the detector call graph actually attributes, pinned.
///
/// Batch 11 recorded two scanner errors as fact, in a commit message, a tracker and twelve
/// ledger reasons: that `resolve_manual_role_spawn_settings` is treated as launch-reaching when
/// it terminates in pure DB/YAML, and that `resolve_node_cli_path` is confused with the
/// `find_node_cli_path` that `git_cmd` reaches via `ensure_resolved_node_bin_in_path`. Both were
/// hand-traced conclusions about the engine that were never put TO the engine.
///
/// Neither reproduces. Both helpers are launch-free by the engine's own measurement, so the
/// engine agreed with batch 11's trace all along and the diagnosis attributed a real symptom to
/// the wrong cause. The `codex`/`node` tokens batch 11 called artifacts are real, and arrive
/// through `CodexCliClient::spawn_agent`.
///
/// The genuine over-attribution is a THIRD mechanism neither batch named: callees resolve by
/// bare name, so `conn.execute(..)` in a one-statement SQL upsert binds to
/// `AgentWorkflowRunner::execute` and pulls the entire workflow runner — and a `send_message`
/// steer sink — into the closure.
///
/// This is pinned rather than fixed, deliberately. Narrowing bare-name resolution REMOVES edges,
/// and edges are what the detector-(c) floor is measured from; a graph that resolves fewer
/// callees would un-refuse commands batches 7-11 already dispositioned on the wider graph, which
/// is a safety regression dressed as a bug fix. The collision is recorded here so the next batch
/// reads a measured limit instead of re-deriving a folk explanation.
#[test]
fn batch12_detector_attribution_limits_are_measured_not_assumed() {
    let graph = CallGraph::build(&load_production_sources());

    // (1) Batch 11's two named "scanner errors" do not reproduce.
    for helper in [
        "resolve_manual_role_spawn_settings",
        "ensure_resolved_node_bin_in_path",
        "resolved_node_bin_dir",
        "find_node_cli_path",
    ] {
        let closure = graph.closure([helper.to_string()]);
        assert!(
            !tokens_reach_any(&closure.tokens, PROCESS_LAUNCH_SINKS),
            "`{helper}` reaches a process-launch sink as a root. Batch 11 recorded the OPPOSITE \
             as a scanner bug; if this now fires, the twelve batch-11 detector-(c) reasons that \
             call codex/node tokens `artifacts` have to be re-derived rather than inherited"
        );
    }

    // (2) The real over-attribution: a bare-name collision on an ultra-common method name.
    let upsert = graph.closure(["save_metrics_config".to_string()]);
    assert!(
        upsert.visited.len() > 500,
        "`save_metrics_config` is one `conn.execute` upsert but its closure was measured at 1200 \
         nodes through the bare name `execute`. A sharp drop means callee resolution changed, \
         which moves the detector-(c) floor for every previously dispositioned batch"
    );
    assert!(
        upsert
            .visited
            .iter()
            .any(|node| node.ends_with("AgentWorkflowRunner::execute")),
        "the `execute` collision that makes detector (a) fire on a settings upsert is the \
         measured reason that row is annotated; if it is gone, re-audit the row instead of \
         keeping the annotation"
    );
    // ...and it is an (a)-only artifact: no launch sink rides in on it.
    assert!(
        !tokens_reach_any(&upsert.tokens, PROCESS_LAUNCH_SINKS),
        "`save_metrics_config` now reaches a launch sink; it is registered and that forecloses it"
    );

    // (3) The collision does NOT reach the sibling read, which is why one is Read and one is not.
    let read = graph.closure(["get_metrics_config".to_string()]);
    assert!(
        read.visited.len() < 100,
        "`get_metrics_config` was measured at 23 nodes; if it has grown into the workflow runner \
         its Read classification is no longer supported by its own body"
    );
}

/// PR 3.1-b batch 13 — calibration sweep for the census blocks this batch audits.
///
/// Batch 12 swept `B7` and left its verdicts on record; this reruns that sweep against the
/// current graph (a verdict inherited across a merge is a verdict nobody measured) and extends
/// it to the two `B6` modules batch 13 takes. Same contract as
/// [`probe_b5_b7_module_batch_audit`]: print all three detectors per member, and for every
/// detector-(c) hit print the launcher tokens and the reconstructed edge chain, so the floor is
/// dispositioned on a path rather than on a boolean.
#[test]
#[ignore = "calibration probe"]
fn probe_b13_module_batch_audit() {
    const MODULES: &[&str] = &[
        "artifact_commands",
        "notification_commands",
        "release_notes_commands",
        "task_context_commands",
        "ui_commands",
        "update_channel_commands",
        "persona_commands",
        "mcp_policy_commands",
    ];
    let graph = CallGraph::build(&load_production_sources());
    let rows = census();
    let commands = rows.iter().map(|(c, _)| c.clone()).collect::<Vec<_>>();
    let detector_b = spawn_triggering_writers(&graph, commands, SPAWN_TRIGGERING_STATE_SURFACE);

    for (command, module) in &rows {
        if !MODULES.contains(&module.as_str()) {
            continue;
        }
        let closure = graph.closure([command.clone()]);
        let a = closure_is_arming(&closure);
        let b = detector_b.contains(command);
        let c = tokens_reach_any(&closure.tokens, PROCESS_LAUNCH_SINKS);
        let row = policy_for(command, module).expect("ledgered");
        eprintln!(
            "PROBE-B13 {module} {command} a={a} b={b} c={c} visited={} class={:?} caps={:?} registered={}",
            closure.visited.len(),
            row.class,
            row.capabilities,
            find_spec(command).is_some(),
        );
        if c {
            let launchers = closure
                .tokens
                .iter()
                .filter(|t| {
                    tokens_reach_any(
                        &std::iter::once((*t).clone()).collect(),
                        PROCESS_LAUNCH_SINKS,
                    )
                })
                .cloned()
                .collect::<Vec<_>>();
            eprintln!("PROBE-B13   launchers={launchers:?}");
            for path in launcher_paths(&graph, command).iter().take(3) {
                eprintln!("PROBE-B13   path: {}", path.join(" -> "));
            }
        }
    }
}

/// PR 3.1-b batch 13 — the detector-(c) FALSE NEGATIVE, measured.
///
/// `retry_legacy_mcp_registration_repair` runs `claude mcp remove ralphx -s user` through
/// `tokio::process::Command::new` (`mcp_registration_repair.rs`), and detector (c) reports
/// `c=false` for it. This prints the mechanism: `resolve_claude_cleanup_cli` reaches
/// `find_claude_cli` only by passing it to `spawn_blocking` as a bare function VALUE, and the
/// scanner records callees from call expressions, so no edge is created. The sibling
/// `resolve_codex_catalog_cli_path` wraps the same kind of resolver in a CLOSURE that calls it,
/// which is exactly why `get_mcp_catalog` is `c=true` and this one is not.
#[test]
#[ignore = "calibration probe"]
fn probe_b13_spawn_blocking_bare_fn_detector_gap() {
    let graph = CallGraph::build(&load_production_sources());
    for subject in [
        "retry_legacy_mcp_registration_repair",
        "retry_reserved_claude_registration_repair",
        "reconcile_reserved_claude_registration",
        "resolve_claude_cleanup_cli",
        "find_claude_cli",
        "remove_reserved_user_registration",
        "get_mcp_catalog",
        "resolve_codex_catalog_cli_path",
        "resolve_user_managed_codex_cli",
        "resolve_codex_cli",
    ] {
        let closure = graph.closure([subject.to_string()]);
        let c = tokens_reach_any(&closure.tokens, PROCESS_LAUNCH_SINKS);
        let callees = graph.callees_of(subject);
        eprintln!(
            "PROBE-GAP {subject} c={c} visited={} callees={:?}",
            closure.visited.len(),
            callees.iter().take(12).collect::<Vec<_>>()
        );
    }
}

/// PR 3.1-b batch 13 — does the `spawn_blocking(bare_fn)` gap expose an already-REGISTERED command?
///
/// The gap only matters if a command the facade already exposes reaches a launch through it.
/// This walks every registered spec and reports any whose closure visits one of the three
/// hand-identified launch owners detector (c) cannot see.
#[test]
#[ignore = "calibration probe"]
fn probe_b13_gap_exposure_across_registered_specs() {
    // Launch owners detector (c) misses: two `spawn_blocking(bare_fn)` sites and one direct
    // `tokio::process::Command::new(<param>)` that never names a resolver.
    const GAP_OWNERS: &[&str] = &[
        "resolve_claude_cleanup_cli",
        "resolve_user_managed_codex_cli",
        "remove_reserved_user_registration",
        "remove_reserved_user_registration_with_timeout",
        "reconcile_reserved_claude_registration",
        "reconcile_reserved_claude_registration_best_effort",
    ];
    let graph = CallGraph::build(&load_production_sources());
    let rows = census();
    let mut exposed = Vec::new();
    for (command, module) in &rows {
        let registered = find_spec(command).is_some();
        let closure = graph.closure([command.clone()]);
        let hits = GAP_OWNERS
            .iter()
            .filter(|owner| {
                closure
                    .visited
                    .iter()
                    .any(|node| node.ends_with(&format!("::{owner}")) || node.ends_with(*owner))
            })
            .collect::<Vec<_>>();
        if !hits.is_empty() {
            let c = tokens_reach_any(&closure.tokens, PROCESS_LAUNCH_SINKS);
            eprintln!(
                "PROBE-EXPO {module} {command} registered={registered} detector_c={c} hits={hits:?}"
            );
            if registered {
                exposed.push(command.clone());
            }
        }
    }
    eprintln!("PROBE-EXPO REGISTERED-AND-EXPOSED = {exposed:?}");
}

/// PR 3.1-b batch 13 — why detectors fire on two members whose bodies are pure settings/CRUD.
#[test]
#[ignore = "calibration probe"]
fn probe_b13_attribution_questions() {
    let graph = CallGraph::build(&load_production_sources());
    for command in [
        "update_notification_settings",
        "get_notification_settings",
        "create_persona_draft",
        "archive_persona",
        "unarchive_persona",
        "list_personas",
    ] {
        let closure = graph.closure([command.to_string()]);
        let collision = closure
            .visited
            .iter()
            .filter(|n| {
                n.ends_with("AgentWorkflowRunner::execute") || n.contains("workflow_runner")
            })
            .take(3)
            .collect::<Vec<_>>();
        eprintln!(
            "PROBE-ATTR13 {command} arming={} visited={} workflow_runner_nodes={:?}",
            closure_is_arming(&closure),
            closure.visited.len(),
            collision
        );
        for hit in closure.sink_hits.iter().take(6) {
            eprintln!("PROBE-ATTR13   sink={} targets={:?}", hit.sink, hit.targets);
        }
    }
}

/// PR 3.1-b batch 13 — which surface entry flags `update_notification_settings`, and on what.
#[test]
#[ignore = "calibration probe"]
fn probe_b13_notification_detector_b_entry() {
    let graph = CallGraph::build(&load_production_sources());
    for command in ["update_notification_settings", "get_notification_settings"] {
        let tokens = graph.closure([command.to_string()]).tokens;
        for entry in SPAWN_TRIGGERING_STATE_SURFACE {
            if entry.flags(command, &tokens) {
                let writes = entry
                    .write_markers
                    .iter()
                    .filter(|m| tokens.contains(**m))
                    .collect::<Vec<_>>();
                let armed = entry
                    .armed_markers
                    .iter()
                    .filter(|m| tokens.contains(**m))
                    .collect::<Vec<_>>();
                eprintln!(
                    "PROBE-NB {command} entry={} write_markers_matched={writes:?} armed_matched={armed:?}",
                    entry.id
                );
            }
        }
    }
}

/// PR 3.1-b batch 13 — the census `B7` block plus the persona and MCP-policy modules of `B6`.
///
/// Same contract as [`batch12_closes_the_b5_block`]: every one of the 52 ratchet members is named
/// with its disposition and the detector-(c) floor is RE-MEASURED per member.
///
/// One rung is NEW. Batch 12's table had a single `Floor` variant that asserted
/// `reaches_launch`, because until now every refused member was one detector (c) could see.
/// `retry_legacy_mcp_registration_repair` is the first that it CANNOT, so the table splits the
/// floor into `FloorDetected` and `FloorHandTraced` — and the hand-traced arm asserts detector (c)
/// is *silent*, so if the engine is ever widened to close the gap this test fails loudly and the
/// row is re-classified deliberately rather than drifting into agreement.
#[test]
fn batch13_closes_the_b7_block_and_two_b6_modules() {
    #[derive(PartialEq, Eq, Clone, Copy, Debug)]
    enum Disposition {
        /// Registered `Read` on a body audit that found no repository write.
        Read,
        /// Registered `AgentControl` — a writer a silent detector never licenses dropping.
        Agent,
        /// Registered `AgentControl` + `MutatesAgentConsumedContent`: writes content that agents
        /// subsequently read (persona overlay bodies, artifact visibility).
        AgentContent,
        /// Registered `AgentControl` + a `DECLARED_MEMBERSHIPS` row: changes what a LATER agent
        /// process may do. The census `B6` plan asks for `ConfiguresFutureProcessAuthority` on
        /// this shape, but `class_permits` admits that capability only under `Elevated`, so the
        /// literal reading would defer an audited-clean write by notation. The declaration is
        /// this ledger's registerable idiom for the same finding.
        AgentFutureAuthority,
        /// `host-denied-spawns-process`, and detector (c) sees the launch.
        FloorDetected,
        /// `host-denied-spawns-process` on a SOURCE trace while detector (c) is silent.
        FloorHandTraced,
        /// Ledgered `Elevated`/`HostManagement`: deferred by authority, not denied.
        DeferredHost,
    }
    use Disposition::*;

    const TABLE: &[(&str, &str, Disposition)] = &[
        // --- artifact_commands: nine reads plus two writes ---------------------------------
        ("get_artifacts", "artifact_commands", Read),
        ("get_artifact", "artifact_commands", Read),
        ("get_artifact_at_version", "artifact_commands", Read),
        ("get_artifacts_by_bucket", "artifact_commands", Read),
        ("get_artifacts_by_task", "artifact_commands", Read),
        ("get_artifact_version_history", "artifact_commands", Read),
        ("get_buckets", "artifact_commands", Read),
        ("get_system_buckets", "artifact_commands", Read),
        ("get_artifact_relations", "artifact_commands", Read),
        ("archive_artifact", "artifact_commands", AgentContent),
        ("create_bucket", "artifact_commands", Agent),
        // --- task_context_commands: five reads ---------------------------------------------
        ("get_task_context", "task_context_commands", Read),
        ("get_artifact_full", "task_context_commands", Read),
        ("get_artifact_version", "task_context_commands", Read),
        ("get_related_artifacts", "task_context_commands", Read),
        ("search_artifacts", "task_context_commands", Read),
        // --- notification_commands: four reads plus four writes ----------------------------
        ("get_notification_settings", "notification_commands", Read),
        (
            "get_unread_notification_count",
            "notification_commands",
            Read,
        ),
        ("list_attention_items", "notification_commands", Read),
        ("list_notifications", "notification_commands", Read),
        ("mark_notification_read", "notification_commands", Agent),
        (
            "mark_all_notifications_read",
            "notification_commands",
            Agent,
        ),
        ("set_dock_badge_count", "notification_commands", Agent),
        (
            "update_notification_settings",
            "notification_commands",
            Agent,
        ),
        // --- release_notes_commands: four reads plus one write -----------------------------
        ("get_current_release_notes", "release_notes_commands", Read),
        (
            "get_release_notes_for_version",
            "release_notes_commands",
            Read,
        ),
        (
            "get_last_seen_release_notes_version",
            "release_notes_commands",
            Read,
        ),
        (
            "list_release_notes_versions",
            "release_notes_commands",
            Read,
        ),
        ("mark_release_notes_seen", "release_notes_commands", Agent),
        // --- persona_commands: four reads plus eight content writes ------------------------
        ("list_personas", "persona_commands", Read),
        ("get_persona", "persona_commands", Read),
        ("list_persona_usage", "persona_commands", Read),
        ("preview_persona_overlay", "persona_commands", Read),
        ("create_persona_draft", "persona_commands", AgentContent),
        ("update_persona_draft", "persona_commands", AgentContent),
        ("update_persona", "persona_commands", AgentContent),
        ("approve_persona", "persona_commands", AgentContent),
        ("approve_persona_as_new", "persona_commands", AgentContent),
        ("reseed_persona_draft", "persona_commands", AgentContent),
        ("archive_persona", "persona_commands", AgentContent),
        ("unarchive_persona", "persona_commands", AgentContent),
        // --- ui_commands / update_channel_commands ------------------------------------------
        ("get_ui_feature_flags", "ui_commands", Read),
        (
            "update_ui_feature_flags",
            "ui_commands",
            AgentFutureAuthority,
        ),
        ("get_update_channel", "update_channel_commands", Read),
        (
            "set_update_channel",
            "update_channel_commands",
            DeferredHost,
        ),
        // --- mcp_policy_commands: four writes and three refusals ---------------------------
        (
            "update_mcp_server_override",
            "mcp_policy_commands",
            AgentFutureAuthority,
        ),
        (
            "clear_mcp_server_override",
            "mcp_policy_commands",
            AgentFutureAuthority,
        ),
        (
            "update_mcp_tool_override",
            "mcp_policy_commands",
            AgentFutureAuthority,
        ),
        (
            "clear_mcp_tool_override",
            "mcp_policy_commands",
            AgentFutureAuthority,
        ),
        ("get_mcp_catalog", "mcp_policy_commands", FloorDetected),
        ("refresh_mcp_catalog", "mcp_policy_commands", FloorDetected),
        (
            "retry_legacy_mcp_registration_repair",
            "mcp_policy_commands",
            FloorHandTraced,
        ),
    ];

    assert_eq!(TABLE.len(), 52, "batch 13 audited 52 ratchet members");

    let graph = CallGraph::build(&load_production_sources());

    for (command, module, disposition) in TABLE {
        let row = policy_for(command, module)
            .unwrap_or_else(|| panic!("`{command}` must be ledgered by batch 13"));

        // (1) The floor, re-measured per member.
        let closure = graph.closure([(*command).to_string()]);
        assert!(
            !closure.visited.is_empty(),
            "`{command}` did not resolve; every assertion below would be vacuous"
        );
        let reaches_launch = tokens_reach_any(&closure.tokens, PROCESS_LAUNCH_SINKS);
        match disposition {
            FloorDetected => assert!(
                reaches_launch,
                "`{command}` is classified host-denied-spawns-process on detector (c) but no \
                 longer reaches a process-launch sink; the claim is now false and the row must \
                 be re-audited"
            ),
            FloorHandTraced => assert!(
                !reaches_launch,
                "`{command}` is now VISIBLE to detector (c). The refusal itself stays correct — \
                 it really does run `claude mcp remove` — but the reason string says the detector \
                 cannot see it, and that has stopped being true. Move the row to FloorDetected \
                 and re-derive whether the gap it documents still exists"
            ),
            _ => assert!(
                !reaches_launch,
                "`{command}` now reaches a process-launch sink; the absolute floor forecloses \
                 every v1 scope, so its batch-13 disposition is void"
            ),
        }

        // (2) A member whose class moved may not keep the placeholder meaning "no audit done".
        assert!(
            !row.reason.contains("conservative-module-default"),
            "`{command}` changed class but carries the module-default reason; every batch-13 \
             reclassification must record the audit that bought it"
        );

        match disposition {
            Read => {
                assert_eq!(row.class, RiskClass::Read, "`{command}` must be Read");
                assert!(
                    row.capabilities.is_empty(),
                    "`{command}` is a Read row and must carry no capability"
                );
                assert!(
                    find_spec(command).is_some(),
                    "`{command}` audited clean and must be registered"
                );
            }
            Agent | AgentContent | AgentFutureAuthority => {
                assert_eq!(
                    row.class,
                    RiskClass::AgentControl,
                    "`{command}` writes; a silent detector never licenses dropping it to Read"
                );
                assert!(
                    !row.capabilities
                        .contains(&Capability::SeedsSpawnTriggeringState),
                    "`{command}` is pinned as a non-arming write; if it now seeds a scanned \
                     surface it must declare its membership rather than inherit this row"
                );
                if *disposition == AgentContent {
                    assert!(
                        row.capabilities
                            .contains(&Capability::MutatesAgentConsumedContent),
                        "`{command}` writes content agents subsequently read and must say so"
                    );
                }
                if *disposition == AgentFutureAuthority {
                    // The capability is deliberately NOT claimed: `class_permits` admits it only
                    // under `Elevated`, so claiming it would defer the command rather than
                    // describe it.
                    assert!(
                        !row.capabilities
                            .contains(&Capability::ConfiguresFutureProcessAuthority),
                        "`{command}` claims ConfiguresFutureProcessAuthority at AgentControl, \
                         which class_permits rejects; the finding is carried by a declaration"
                    );
                    assert!(
                        DECLARED_MEMBERSHIPS.iter().any(|(name, _)| name == command),
                        "`{command}` changes what a LATER agent process may do; the census B6 \
                         plan requires that finding be recorded, and the registerable way to \
                         record it is a DECLARED_MEMBERSHIPS row"
                    );
                }
                let spec = find_spec(command)
                    .unwrap_or_else(|| panic!("`{command}` audited clean and must be registered"));
                assert_eq!(spec.class, RiskClass::AgentControl);
            }
            FloorDetected | FloorHandTraced => {
                assert_eq!(
                    row.class,
                    RiskClass::Elevated,
                    "`{command}` reaches a launch; Elevated is the only class permitting \
                     SpawnsProcess"
                );
                assert_eq!(row.capabilities, &[Capability::SpawnsProcess]);
                assert!(
                    row.reason.starts_with("detector-c"),
                    "`{command}` must state the finding that justifies the class"
                );
                assert!(
                    find_spec(command).is_none(),
                    "`{command}` is host-denied and must not be registered"
                );
                assert_eq!(
                    ralphx_remote_protocol::v1_resolution_with_audit(
                        row.class,
                        row.capabilities,
                        audit_refusal_for(command).is_some(),
                    ),
                    ralphx_remote_protocol::V1Resolution::HostDeniedSpawnsProcess,
                );
            }
            DeferredHost => {
                assert_eq!(
                    row.class,
                    RiskClass::Elevated,
                    "`{command}` is refused on host authority, and Elevated is the class every \
                     other HostManagement row in this ledger sits at"
                );
                assert_eq!(row.capabilities, &[Capability::HostManagement]);
                assert!(
                    find_spec(command).is_none(),
                    "`{command}` is not registered; only its read half is"
                );
                // Deferred, NOT denied: no process is spawned, so a later scope may grant it.
                assert_eq!(
                    ralphx_remote_protocol::v1_resolution_with_audit(
                        row.class,
                        row.capabilities,
                        audit_refusal_for(command).is_some(),
                    ),
                    ralphx_remote_protocol::V1Resolution::V1Deferred,
                );
            }
        }
    }
}

/// PR 3.1-b batch 13 — three detector attribution errors, measured rather than inherited.
///
/// Batch 12 pinned ONE limit: a bare-name CALLEE collision inflating `save_metrics_config`. This
/// batch measured three more, and unlike batch 12's they are not all cosmetic — the first is a
/// detector-(c) FALSE NEGATIVE, the only kind of detector error that can put a process-spawning
/// command inside the facade.
#[test]
fn batch13_detector_gap_is_measured_not_inherited() {
    let graph = CallGraph::build(&load_production_sources());

    // ---------------------------------------------------------------------------------------
    // (1) The detector-(c) false negative, and BOTH mechanisms that cause it.
    // ---------------------------------------------------------------------------------------
    let repair = graph.closure(["retry_legacy_mcp_registration_repair".to_string()]);
    assert!(
        !tokens_reach_any(&repair.tokens, PROCESS_LAUNCH_SINKS),
        "detector (c) now SEES `retry_legacy_mcp_registration_repair`. Its ledger row and the \
         batch-13 table both document a gap that would no longer exist; re-derive both rather \
         than deleting this assertion"
    );

    // Mechanism A: `spawn_blocking(bare_fn_path)` creates no call edge. The resolver is named in
    // `resolve_claude_cleanup_cli`'s body but never appears among its callees.
    let cleanup_callees = graph.callees_of("resolve_claude_cleanup_cli");
    assert!(
        !cleanup_callees
            .iter()
            .any(|callee| callee.ends_with("find_claude_cli")),
        "`resolve_claude_cleanup_cli` now has an edge to `find_claude_cli`; mechanism A is closed \
         and the floor must be re-measured for every member, not just this one"
    );
    // ...and the resolver really does reach a launch when taken as a ROOT, which is what makes
    // the missing edge a false negative rather than a correct silence.
    assert!(
        tokens_reach_any(
            &graph.closure(["find_claude_cli".to_string()]).tokens,
            PROCESS_LAUNCH_SINKS
        ),
        "`find_claude_cli` no longer reaches a launch sink as a root; without that this whole \
         finding is vacuous"
    );

    // The control that proves mechanism A is about the CALL SHAPE and not about these two
    // functions: the sibling wraps its resolver in a closure that CALLS it, and gets the edge.
    let codex_callees = graph.callees_of("resolve_codex_catalog_cli_path");
    assert!(
        codex_callees
            .iter()
            .any(|callee| callee.ends_with("resolve_codex_cli")),
        "`resolve_codex_catalog_cli_path` lost its edge to `resolve_codex_cli`; that edge is the \
         positive control for mechanism A and the reason `get_mcp_catalog` is correctly refused"
    );
    assert!(
        tokens_reach_any(
            &graph.closure(["get_mcp_catalog".to_string()]).tokens,
            PROCESS_LAUNCH_SINKS
        ),
        "`get_mcp_catalog` must stay detector-visible; it is the contrast that makes the gap in \
         its module sibling legible"
    );

    // Mechanism B: the spawn owner takes an ALREADY-RESOLVED path, so it names no resolver for a
    // token-based sink model to match, and is silent even as a root.
    assert!(
        !tokens_reach_any(
            &graph
                .closure(["remove_reserved_user_registration".to_string()])
                .tokens,
            PROCESS_LAUNCH_SINKS
        ),
        "`remove_reserved_user_registration` is now detector-visible; mechanism B is closed"
    );

    // ---------------------------------------------------------------------------------------
    // (2) The gap's blast radius: no REGISTERED command may reach a gap owner.
    //
    // This is the assertion that matters operationally. The gap is tolerable only while every
    // command that reaches it is refused on some other ground; the moment a registered command
    // reaches one, the facade is exposing a spawn.
    // ---------------------------------------------------------------------------------------
    const GAP_OWNERS: &[&str] = &[
        "resolve_claude_cleanup_cli",
        "resolve_user_managed_codex_cli",
        "remove_reserved_user_registration",
        "remove_reserved_user_registration_with_timeout",
        "reconcile_reserved_claude_registration",
        "reconcile_reserved_claude_registration_best_effort",
    ];
    let mut exposed = Vec::new();
    for (command, _module) in census() {
        if find_spec(&command).is_none() {
            continue;
        }
        let closure = graph.closure([command.clone()]);
        if GAP_OWNERS.iter().any(|owner| {
            closure
                .visited
                .iter()
                .any(|node| node.ends_with(&format!("::{owner}")) || node.ends_with(owner))
        }) {
            exposed.push(command);
        }
    }
    assert!(
        exposed.is_empty(),
        "these REGISTERED commands reach a launch detector (c) cannot see: {exposed:?}. The \
         facade is exposing a process spawn; refuse them at the floor on the hand trace before \
         anything else"
    );

    // ---------------------------------------------------------------------------------------
    // (3) Detector (a)'s persona verdicts are batch 12's callee collision, not arming.
    // ---------------------------------------------------------------------------------------
    let draft = graph.closure(["create_persona_draft".to_string()]);
    assert!(
        draft.visited.len() > 500,
        "`create_persona_draft` is a bounded persona write measured at 1252 closure nodes through \
         a bare-name collision. A sharp drop means callee resolution changed, which moves the \
         detector-(c) floor for every previously dispositioned batch"
    );
    assert!(
        draft
            .visited
            .iter()
            .any(|node| node.contains("agent_workflow_runner")),
        "the workflow-runner nodes that make detector (a) fire on a persona write are gone; \
         re-audit the row instead of keeping the annotation"
    );
    assert!(
        !tokens_reach_any(&draft.tokens, PROCESS_LAUNCH_SINKS),
        "`create_persona_draft` now reaches a launch sink; it is registered and that forecloses it"
    );
    // The local contrast: the same-shaped sibling never touches the colliding name.
    let unarchive = graph.closure(["unarchive_persona".to_string()]);
    assert!(
        unarchive.visited.len() < 400,
        "`unarchive_persona` was measured at 120 nodes and is the control proving the sibling \
         inflation is a collision rather than a real persona-write dependency"
    );

    // ---------------------------------------------------------------------------------------
    // (4) Detector (b)'s notification verdict is a MARKER collision — a third mechanism.
    // ---------------------------------------------------------------------------------------
    let settings_tokens = graph
        .closure(["update_notification_settings".to_string()])
        .tokens;
    let matched = SPAWN_TRIGGERING_STATE_SURFACE
        .iter()
        .filter(|entry| entry.flags("update_notification_settings", &settings_tokens))
        .map(|entry| entry.id)
        .collect::<Vec<_>>();
    assert_eq!(
        matched,
        vec!["workspace-auto-review"],
        "`update_notification_settings` is flagged by a different surface than the one measured. \
         The recorded cause is a bare-name collision on the repository method `update_settings`; \
         if the matching entry changed, the ledger reason is stale"
    );
    assert!(
        settings_tokens.contains("update_settings"),
        "the colliding token `update_settings` is gone from the closure, so the recorded reason \
         for detector (b) firing no longer holds"
    );
    // The honesty condition: it is flagged, and it still must not claim the evidence capability,
    // because the flag is a collision rather than a spawn-triggering write.
    let row =
        policy_for("update_notification_settings", "notification_commands").expect("ledgered");
    assert!(
        !row.capabilities
            .contains(&Capability::SeedsSpawnTriggeringState),
        "`update_notification_settings` claims SeedsSpawnTriggeringState. Detector (b) does flag \
         it, so `seeds_spawn_triggering_state_tags_track_detector_b_evidence` would PASS — but \
         the flag is a marker collision and the tag would be false"
    );
}

/// PR 3.1-b batch 14 — the FINAL ratchet population, measured before any disposition.
#[test]
#[ignore = "calibration probe"]
fn probe_b14_final_population_audit() {
    const MEMBERS: &[&str] = &[
        "abort_seeded_agent_conversation",
        "add_conversation_folder_reference",
        "approve_fix_task",
        "archive_agent_conversation",
        "archive_task",
        "clear_manual_role_default",
        "commit_agent_conversation_workspace_locally",
        "complete_step",
        "create_agent_conversation",
        "fail_step",
        "fork_agent_conversation",
        "get_agent_conversation_role_default",
        "get_manual_role_defaults",
        "get_start_composer_role_default",
        "get_workspace_review_runtime_settings",
        "pause_execution_plan",
        "precompute_agent_conversation_workspace_pr_description",
        "reconcile_agent_conversation_workspace_publication",
        "recover_task_execution",
        "reject_fix_task",
        "remove_conversation_folder_reference",
        "reorder_task_steps",
        "reset_agent_conversation_role_default",
        "resolve_recovery_prompt",
        "restart_task",
        "restore_agent_conversation",
        "restore_task",
        "resume_execution",
        "resume_execution_plan",
        "resume_task",
        "resume_tasks_in_group",
        "retry_branch_update",
        "send_queued_agent_message_now",
        "set_agent_conversation_workspace_auto_publish",
        "set_agent_conversation_workspace_pr_supervision",
        "skip_step",
        "start_step",
        "stop_agent",
        "stop_execution_plan",
        "switch_agent_conversation_mode",
        "switch_agent_conversation_persona",
        "update_agent_conversation_coordination_mode",
        "update_agent_conversation_title",
        "update_execution_settings",
        "update_global_execution_settings",
        "update_manual_role_default",
        "update_workspace_review_runtime_settings",
        "upsert_custom_agent_model",
    ];
    let graph = CallGraph::build(&load_production_sources());
    let rows = census();
    let commands = rows.iter().map(|(c, _)| c.clone()).collect::<Vec<_>>();
    let detector_b = spawn_triggering_writers(&graph, commands, SPAWN_TRIGGERING_STATE_SURFACE);

    for (command, module) in &rows {
        if !MEMBERS.contains(&command.as_str()) {
            continue;
        }
        let closure = graph.closure([command.clone()]);
        let a = closure_is_arming(&closure);
        let b = detector_b.contains(command);
        let c = tokens_reach_any(&closure.tokens, PROCESS_LAUNCH_SINKS);
        let row = policy_for(command, module).expect("ledgered");
        eprintln!(
            "PROBE-B14 {module} {command} a={a} b={b} c={c} visited={} class={:?} caps={:?} registered={}",
            closure.visited.len(),
            row.class,
            row.capabilities,
            find_spec(command).is_some(),
        );
        if c {
            for path in launcher_paths(&graph, command).iter().take(2) {
                eprintln!("PROBE-B14   path: {}", path.join(" -> "));
            }
        }
    }
    let named = rows
        .iter()
        .filter(|(c, _)| MEMBERS.contains(&c.as_str()))
        .count();
    eprintln!("PROBE-B14 census-covered {named} of {}", MEMBERS.len());
}

/// PR 3.1-b batch 14 — the FINAL ratchet block, and the P-11 exit criterion in test form.
///
/// Same contract as [`batch13_closes_the_b7_block_and_two_b6_modules`]: every one of the 48
/// remaining members is named with its disposition and the detector-(c) floor is RE-MEASURED
/// per member.
///
/// The table splits the floor the way batch 13 did, and the split now carries most of the
/// batch: `FloorDetected` asserts detector (c) sees the launch, and `FloorHandTraced` asserts
/// it is SILENT. Thirteen members are hand-traced, so if the engine is ever widened to close
/// the M1/M2/M3 gaps this test fails loudly on all thirteen and they are re-classified
/// deliberately rather than drifting into agreement.
#[test]
fn batch14_closes_the_ratchet_at_zero() {
    #[derive(PartialEq, Eq, Clone, Copy, Debug)]
    enum Disposition {
        /// Registered `Read` on a body audit that found no repository write.
        Read,
        /// Registered `AgentControl` — a writer a silent detector never licenses dropping.
        Agent,
        /// `host-denied-spawns-process`, and detector (c) sees the launch.
        FloorDetected,
        /// `host-denied-spawns-process` on a SOURCE trace while detector (c) is SILENT.
        FloorHandTraced,
        /// Ledgered `Elevated`/`ConfiguresFutureProcessAuthority`: deferred by authority.
        DeferredFutureAuthority,
        /// `v1-audit-refused` — a recorded `AUDIT_REFUSALS` finding.
        AuditRefused,
    }
    use Disposition::*;

    const MEMBERS: &[(&str, Disposition)] = &[
        // --- floor, detector (c) DETECTED (12)
        ("resume_task", FloorDetected),
        ("retry_branch_update", FloorDetected),
        ("restart_task", FloorDetected),
        ("recover_task_execution", FloorDetected),
        ("resolve_recovery_prompt", FloorDetected),
        ("switch_agent_conversation_mode", FloorDetected),
        (
            "set_agent_conversation_workspace_auto_publish",
            FloorDetected,
        ),
        (
            "set_agent_conversation_workspace_pr_supervision",
            FloorDetected,
        ),
        (
            "reconcile_agent_conversation_workspace_publication",
            FloorDetected,
        ),
        ("commit_agent_conversation_workspace_locally", FloorDetected),
        (
            "precompute_agent_conversation_workspace_pr_description",
            FloorDetected,
        ),
        ("archive_agent_conversation", FloorDetected),
        // --- floor, detector (c) MISSED (13)
        ("resume_execution", FloorHandTraced),
        ("update_execution_settings", FloorHandTraced),
        ("update_global_execution_settings", FloorHandTraced),
        ("reset_agent_conversation_role_default", FloorHandTraced),
        ("resume_tasks_in_group", FloorHandTraced),
        ("pause_execution_plan", FloorHandTraced),
        ("resume_execution_plan", FloorHandTraced),
        ("stop_execution_plan", FloorHandTraced),
        ("fork_agent_conversation", FloorHandTraced),
        ("switch_agent_conversation_persona", FloorHandTraced),
        ("send_queued_agent_message_now", FloorHandTraced),
        ("stop_agent", FloorHandTraced),
        (
            "update_agent_conversation_coordination_mode",
            FloorHandTraced,
        ),
        // --- deferred by authority (3)
        ("add_conversation_folder_reference", DeferredFutureAuthority),
        ("update_manual_role_default", DeferredFutureAuthority),
        ("clear_manual_role_default", DeferredFutureAuthority),
        // --- audit-refused (9)
        ("start_step", AuditRefused),
        ("complete_step", AuditRefused),
        ("skip_step", AuditRefused),
        ("fail_step", AuditRefused),
        ("reorder_task_steps", AuditRefused),
        ("remove_conversation_folder_reference", AuditRefused),
        ("abort_seeded_agent_conversation", AuditRefused),
        ("get_manual_role_defaults", AuditRefused),
        ("reject_fix_task", AuditRefused),
        // --- registered (11)
        ("get_start_composer_role_default", Read),
        ("get_agent_conversation_role_default", Read),
        ("get_workspace_review_runtime_settings", Read),
        ("archive_task", Agent),
        ("restore_task", Agent),
        ("create_agent_conversation", Agent),
        ("restore_agent_conversation", Agent),
        ("update_agent_conversation_title", Agent),
        ("update_workspace_review_runtime_settings", Agent),
        ("upsert_custom_agent_model", Agent),
        ("approve_fix_task", Agent),
    ];

    assert_eq!(
        MEMBERS.len(),
        48,
        "the final ratchet block is 48 members; the table must name every one"
    );

    let graph = CallGraph::build(&load_production_sources());
    let rows = census().into_iter().collect::<BTreeMap<_, _>>();

    for (command, disposition) in MEMBERS {
        let module = rows
            .get(*command)
            .unwrap_or_else(|| panic!("`{command}` is not in the live census"));
        let row = policy_for(command, module).expect("ledgered");
        let closure = graph.closure([(*command).to_string()]);
        let reaches_launch = tokens_reach_any(&closure.tokens, PROCESS_LAUNCH_SINKS);

        match disposition {
            Read => {
                assert_eq!(
                    row.class,
                    RiskClass::Read,
                    "`{command}` is dispositioned Read but the ledger says {:?}",
                    row.class
                );
                assert!(
                    find_spec(command).is_some(),
                    "`{command}` is dispositioned Read but is not registered"
                );
                assert!(
                    !reaches_launch,
                    "`{command}` is registered as a read but reaches a launch sink"
                );
            }
            Agent => {
                assert_eq!(
                    row.class,
                    RiskClass::AgentControl,
                    "`{command}` is dispositioned AgentControl but the ledger says {:?}",
                    row.class
                );
                assert!(
                    find_spec(command).is_some(),
                    "`{command}` is dispositioned AgentControl but is not registered"
                );
                assert!(
                    !reaches_launch,
                    "`{command}` is registered but now reaches a launch sink; a registration \
                     may never sit over a CLI launch"
                );
            }
            FloorDetected => {
                assert_eq!(row.class, RiskClass::Elevated);
                assert!(
                    row.capabilities.contains(&Capability::SpawnsProcess),
                    "`{command}` is floored but does not carry SpawnsProcess"
                );
                assert!(
                    reaches_launch,
                    "`{command}` is recorded as a DETECTED floor, but detector (c) is now \
                     silent on it — re-audit the row rather than keeping the annotation"
                );
                assert!(find_spec(command).is_none());
            }
            FloorHandTraced => {
                assert_eq!(row.class, RiskClass::Elevated);
                assert!(
                    row.capabilities.contains(&Capability::SpawnsProcess),
                    "`{command}` is floored but does not carry SpawnsProcess"
                );
                // The falsifiable half. These thirteen are refused on a SOURCE trace while the
                // detector reports nothing. If the engine is widened to see M1/M2/M3, this
                // fires and the row is re-classified deliberately.
                assert!(
                    !reaches_launch,
                    "`{command}` is recorded as a hand-traced floor BECAUSE detector (c) could \
                     not see its launch. Detector (c) now flags it, so the engine changed: move \
                     the row to the detected floor and re-check the other twelve"
                );
                assert!(find_spec(command).is_none());
            }
            DeferredFutureAuthority => {
                assert_eq!(row.class, RiskClass::Elevated);
                assert!(
                    row.capabilities
                        .contains(&Capability::ConfiguresFutureProcessAuthority),
                    "`{command}` is deferred on future-process authority but does not carry it"
                );
                assert!(
                    !row.capabilities.contains(&Capability::SpawnsProcess),
                    "`{command}` is a DEFERRAL, not a denial; carrying SpawnsProcess would \
                     make it host-denied and the distinction is the point"
                );
                assert!(find_spec(command).is_none());
            }
            AuditRefused => {
                assert!(
                    audit_refusal_for(command).is_some(),
                    "`{command}` is dispositioned v1-audit-refused but has no AUDIT_REFUSALS row"
                );
                assert!(find_spec(command).is_none());
            }
        }
    }
}

/// PR 3.1-b batch 14 — the held pair, and the two halves proved to differ mechanically.
///
/// The whole vocabulary decision rests on one asymmetry: `reject_fix_task` reaches a corrective
/// transition and `approve_fix_task` does not. Both directions are asserted, so the new reason
/// cannot outlive its cause and the released half cannot silently acquire the property that
/// refused its partner.
#[test]
fn batch14_held_pair_splits_on_the_corrective_reach() {
    const CORRECTIVE_SINKS: &[&str] = &[
        "transition_task_corrective",
        "transition_task_corrective_with_exit",
        "apply_corrective_transition",
    ];
    let graph = CallGraph::build(&load_production_sources());

    let reaches = |command: &str| {
        graph
            .closure([command.to_string()])
            .sink_hits
            .iter()
            .any(|hit| CORRECTIVE_SINKS.contains(&hit.sink.as_str()))
    };

    assert!(
        reaches("reject_fix_task"),
        "`reject_fix_task` no longer reaches a corrective transition, so the \
         `reaches-corrective-transition` refusal is STALE — re-audit it for registration \
         instead of leaving a refusal whose finding has been fixed"
    );
    let refusal = audit_refusal_for("reject_fix_task").expect("the refusal row exists");
    assert_eq!(
        refusal.reason,
        AuditRefusalReason::ReachesCorrectiveTransition,
        "`reject_fix_task` is the sole justification for minting this reason; filing it under \
         another one would put a false statement in the ledger"
    );

    assert!(
        !reaches("approve_fix_task"),
        "`approve_fix_task` now reaches a corrective transition. It is REGISTERED, and \
         `no_registered_facade_target_reaches_a_corrective_transition` forbids that at any \
         scope — unregister it rather than exempting it"
    );
    assert!(
        find_spec("approve_fix_task").is_some(),
        "`approve_fix_task` was released from its four-batch hold in batch 14; if it is \
         unregistered again the ledger reason must be rewritten, not just the registration"
    );
    // The brake that dissolved the pair argument. If either is ever unregistered, the reason
    // recorded for releasing `approve_fix_task` stops being true.
    for brake in ["block_task", "stop_task"] {
        assert!(
            find_spec(brake).is_some(),
            "`{brake}` is unregistered. Batch 14 released `approve_fix_task` from its pair hold \
             BECAUSE a remote brake exists; without it the asymmetry batch 10 refused is back"
        );
    }
}

/// PR 3.1-b batch 14 — the seven transport-shape refusals are ONE mechanism, not seven calls.
#[test]
fn batch14_transport_shape_refusals_share_one_error_contract() {
    const SHARED: &[&str] = &[
        "start_step",
        "complete_step",
        "skip_step",
        "fail_step",
        "reorder_task_steps",
        "remove_conversation_folder_reference",
        "abort_seeded_agent_conversation",
    ];
    for command in SHARED {
        let refusal =
            audit_refusal_for(command).unwrap_or_else(|| panic!("`{command}` has a refusal row"));
        assert_eq!(
            refusal.reason,
            AuditRefusalReason::TransportShapeDeferred,
            "`{command}` is refused on the shared non-Serialize AppError contract; a different \
             reason here means the finding changed and the row needs re-auditing"
        );
        assert!(
            find_spec(command).is_none(),
            "`{command}` is registered despite an error type the fallible dispatch arm cannot \
             render"
        );
    }
    // The unlocking fix is shared too: making `AppError` Serialize releases all seven at once.
    // Batch 8 recorded the same mechanism for this sibling, which is why it is not a new finding.
    assert_eq!(
        audit_refusal_for("list_conversation_folder_references")
            .expect("batch 8's row")
            .reason,
        AuditRefusalReason::TransportShapeDeferred,
        "batch 8's row is the precedent these seven cite; if it moved, re-argue them"
    );
}

#[test]
fn merged_commands_pin_host_log_amplification_and_workspace_recovery_funnel() {
    let log_row =
        policy_for("log_frontend_error", "diagnostic_commands").expect("log command is ledgered");
    assert_eq!(log_row.class, RiskClass::Elevated);
    assert_eq!(log_row.capabilities, &[Capability::HostManagement]);
    assert!(
        log_row.reason.contains("without a count bound")
            && log_row.reason.contains("host tracing/file-log pipeline"),
        "the deferred disposition must remain tied to repeated bounded payloads producing an \
         unbounded host-log write stream"
    );
    assert_eq!(
        ralphx_remote_protocol::v1_resolution(log_row.class, log_row.capabilities),
        ralphx_remote_protocol::V1Resolution::V1Deferred,
    );
    assert!(find_spec("log_frontend_error").is_none());

    let mute_row = policy_for(
        "set_agent_conversation_muted",
        "agent_conversation_mute_commands",
    )
    .expect("mute command is ledgered");
    assert_eq!(mute_row.class, RiskClass::Elevated);
    assert_eq!(mute_row.capabilities, &[Capability::SpawnsProcess]);
    assert!(
        mute_row
            .reason
            .contains("agent_workspace_response_for_state")
            && mute_row
                .reason
                .contains("recover_agent_workspace_pr_supervision")
            && mute_row.reason.contains("GitService::get_head_sha"),
        "the denial must remain tied to the response projection's arming recovery funnel"
    );
    assert_eq!(
        ralphx_remote_protocol::v1_resolution(mute_row.class, mute_row.capabilities),
        ralphx_remote_protocol::V1Resolution::HostDeniedSpawnsProcess,
    );
    assert!(find_spec("set_agent_conversation_muted").is_none());

    let graph = CallGraph::build(&load_production_sources());
    let closure = graph.closure(["set_agent_conversation_muted".to_string()]);
    assert!(
        tokens_reach_any(&closure.tokens, PROCESS_LAUNCH_SINKS),
        "detector (c) no longer sees the recovery funnel; re-trace the command before changing \
         or retaining its process-floor disposition"
    );
}
