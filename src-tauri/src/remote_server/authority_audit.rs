//! Two-detector remote authority audit (PR 1.3, P-17d/e/g).
//!
//! Build-time tooling — compiled only under `cfg(test)` and never linked into the shipped
//! binary. It parses `src-tauri/src` with `syn` and answers two questions the capability
//! ledger cannot answer by inspection:
//!
//! * **Detector (a)** — which registered Tauri commands can transitively reach an agent
//!   spawn/steer sink, *target-sensitively* for `TaskTransitionService` (halting targets are
//!   exempt; arming targets classify).
//! * **Detector (b)** — which registered commands write persisted state that a **registered
//!   background loop** consumes to spawn or steer an agent. The loop inventory itself is
//!   discovered by the same call-graph machinery rooted at loop entry points, so a new loop
//!   that reaches a sink without an inventory row fails CI instead of silently existing.
//!
//! # Soundness limits (stated, because the audit is a floor and not a proof)
//!
//! Definitions are keyed by source-qualified identity (`file::Type::method` or
//! `file::::free_fn`). A method call resolves to every production definition having that
//! method name, preserving conservative trait/dyn dispatch without unioning their bodies
//! into one generic-name node. Calls originating in clean-architecture infrastructure
//! repositories/services are leaves, and registered Tauri commands do not compose other
//! registered commands. Residual multi-target method dispatch is intentionally retained as
//! a reviewable over-approximation.
//!
//! Sinks are **cut points** — traversal stops when it reaches one. Without that, every
//! command that touches `TaskTransitionService` would inherit the entry-action spawn graph
//! and the authority-reducing brakes (`pause_task`, `block_task`, `stop_task`) would all
//! classify `AgentControl`, disabling the product's promised remote brakes (codex R4-H1).
//! Cutting at the sink is what lets the transition hit be classified by its *target*.
//!
//! Known residual, recorded rather than papered over: deferred authority that no static sink
//! models (`update_custom_analysis` persisting a shell string, permission approval over a
//! `watch::Sender`) is invisible to both detectors. Those are hand-audited ledger rows and
//! reason-coded declared memberships — see `capability_ledger` and [`DECLARED_MEMBERSHIPS`].

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::path::{Path, PathBuf};

use syn::visit::Visit;

// ---------------------------------------------------------------------------------------
// Sink definitions
// ---------------------------------------------------------------------------------------

/// Sink names traversal stops at, and reaching them classifies (subject to target rules).
pub const TRANSITION_SINKS: &[&str] = &[
    "transition_task",
    "transition_task_with_metadata",
    "transition_task_corrective",
    "transition_task_corrective_with_exit",
    "apply_corrective_transition",
    "transition_to_stopped_with_context",
];

/// Scheduler activation sinks — reaching any of these arms the Ready→Executing spawn loop.
pub const SCHEDULER_SINKS: &[&str] = &[
    "try_schedule_ready_tasks",
    "spawn_ready_task_scheduler_if_needed",
    "execute_entry_actions",
];

/// Agent chat/steer sinks — a live provider process receives caller-influenced input.
pub const STEER_SINKS: &[&str] = &["send_message", "send_stdin_message", "write_message"];

/// Synthetic sink recorded when an `AgentSpawner`-shaped call is detected structurally
/// (`.spawn("worker", task_id)` / `.spawn_background("qa-prep", task_id)` — a string-literal
/// agent type as the first argument). Detected by shape rather than by receiver name so a
/// renamed field or a trait object still trips it.
pub const AGENT_SPAWN_SINK: &str = "<agent-spawner::spawn>";

/// Fixed project-analyzer process entry point. This is intentionally an exact free-function
/// name match: reaching it means the command starts an agent process, while unrelated process
/// helpers remain outside detector (a).
pub const AGENT_PROCESS_SPAWN_SINKS: &[&str] = &["spawn_project_analyzer"];

/// `InternalStatus` targets that ARM or re-enter scheduling → classify.
pub const ARMING_TRANSITION_TARGETS: &[&str] = &[
    "Ready",
    "Executing",
    "Reviewing",
    "Merging",
    "QaTesting",
    "QaPrep",
    "PendingReview",
    "Failed",
];

/// `InternalStatus` targets that only halt/park → authority-reducing, exempt.
pub const HALTING_TRANSITION_TARGETS: &[&str] =
    &["Paused", "Blocked", "Stopped", "Cancelled", "Archived"];

/// Detector (c) — process-launch resolution sinks.
///
/// Every production subprocess must resolve its binary through
/// `infrastructure/tool_paths.rs` (`.claude/rules/production-cli-resolution.md`), so reaching
/// one of these IS spawning a process. `Capability::SpawnsProcess` is permitted only under
/// `Elevated`, which makes "reaches a launch sink but is ledgered `Read`/`Operate`" a
/// mechanically detectable under-labelling — the exact shape the `list_projects` mislabel had.
pub const PROCESS_LAUNCH_SINKS: &[&str] = &[
    "resolve_gh_cli_path",
    "resolve_git_cli_path",
    "resolve_node_cli_path",
    "resolve_shell_cli_path",
    "resolve_rm_cli_path",
    "resolve_ps_cli_path",
    "resolve_lsof_cli_path",
    "resolve_pgrep_cli_path",
    "resolve_pkill_cli_path",
    "resolve_taskkill_cli_path",
    "resolve_tasklist_cli_path",
    "find_claude_cli_path",
    "claude_native_cli_path",
    "find_claude_native_cli_path",
    "find_codex_cli_path",
    "find_codex_cli_candidates",
    "find_tailscale_cli_path",
    "find_launchable_cli_path",
    "find_launchable_cli_path_without_shell",
];

// No `WritesArbitraryPath` counterpart exists, and that is a measured result rather than an
// omission: `fs::write`/`create_dir_all`/`remove_*` are reachable from the transitive closure of
// nearly every command, including `list_tasks` and the `pause_task`/`block_task`/`stop_task`
// brakes. A gate that fires on pure reads is not a floor, so path authority stays a hand-audited
// ledger judgement (`chat_attachment_commands` is `Denied` on exactly that basis) and is recorded
// here as a stated soundness limit.

/// True when any closure token names one of `sinks`, exactly or as a path suffix
/// (`write_thing`, `std::fs::write` and `fs::write` all match the sink `fs::write`; a token
/// merely *containing* the text does not).
pub fn tokens_reach_any(tokens: &BTreeSet<String>, sinks: &[&str]) -> bool {
    tokens.iter().any(|token| {
        sinks.iter().any(|sink| {
            token == sink
                || (token.ends_with(sink) && token[..token.len() - sink.len()].ends_with("::"))
        })
    })
}

fn all_cut_sinks() -> BTreeSet<&'static str> {
    TRANSITION_SINKS
        .iter()
        .chain(SCHEDULER_SINKS.iter())
        .chain(STEER_SINKS.iter())
        .chain(AGENT_PROCESS_SPAWN_SINKS.iter())
        .copied()
        .collect()
}

// ---------------------------------------------------------------------------------------
// Call graph
// ---------------------------------------------------------------------------------------

/// One sink reached from a function body, with the transition targets named at the call site.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct SinkHit {
    pub sink: String,
    /// `InternalStatus::X` variants syntactically present in the sink call's arguments.
    /// Empty means "target not statically visible" → treated as arming (fail closed).
    pub targets: BTreeSet<String>,
}

#[derive(Debug, Default, Clone)]
pub struct FnNode {
    pub callees: BTreeSet<String>,
    /// Every ident, `A::B` path, and string literal appearing in the body. Detector (b) and
    /// the content-surface detector match writer markers against this set.
    pub tokens: BTreeSet<String>,
    pub sink_hits: BTreeSet<SinkHit>,
}

/// A background-loop entry point discovered in source: the closure/async block handed to a
/// spawn/interval/`listen_any` call.
#[derive(Debug, Clone)]
pub struct LoopRoot {
    pub id: String,
    pub file: String,
    pub enclosing_fn: String,
    pub kind: String,
    pub callees: BTreeSet<String>,
    pub sink_hits: BTreeSet<SinkHit>,
}

#[derive(Debug, Default)]
pub struct CallGraph {
    pub nodes: BTreeMap<String, FnNode>,
    pub loop_roots: Vec<LoopRoot>,
    definitions: BTreeMap<String, BTreeSet<String>>,
    definition_arities: BTreeMap<String, usize>,
    definition_owners: BTreeMap<String, String>,
    definition_methods: BTreeMap<String, bool>,
    node_files: BTreeMap<String, String>,
    registered_commands: BTreeSet<String>,
    /// Conservative call sites that still resolve to multiple same-name, same-arity definitions.
    pub unresolved_dispatch: BTreeMap<String, BTreeSet<String>>,
}

/// Result of expanding a set of roots through the graph, stopping at sinks.
#[derive(Debug, Default)]
pub struct Closure {
    pub visited: BTreeSet<String>,
    pub tokens: BTreeSet<String>,
    pub sink_hits: BTreeSet<SinkHit>,
}

/// A writer whose authority is real but which no source token can express mechanically.
///
/// The R4-C3 honest form: an explicit, reason-coded declaration rather than a marker invented
/// to make a known command match. Adding one is a review event; adding a marker is not.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DeclaredWriter {
    pub command: &'static str,
    pub reason: &'static str,
}

/// Persisted state read by an authority-bearing background loop as a spawn/steer predicate.
///
/// # Why markers are persistence-layer tokens and not command names
///
/// A marker that IS the writer's own command name matches that command against itself: every
/// function node carries its own bare name as a token, so `writer_markers: &["inject_task"]`
/// flags `inject_task` no matter what its body does, and a *new* writer of the same surface is
/// a silent false negative. Markers are therefore drawn from the write site the command
/// reaches — repository/service method names, enum variant paths, and distinguishing string
/// literals — and [`super::capability_ledger_tests`] asserts mechanically that no marker is a
/// census command name.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StateSurfaceEntry {
    pub id: &'static str,
    pub surface: &'static str,
    pub armed_value: &'static str,
    pub read_by_loops: &'static [&'static str],
    /// Persistence-layer write sites for this surface. Never a registered command name.
    pub write_markers: &'static [&'static str],
    /// Tokens identifying the ARMED value. A command must reach BOTH a write site and the
    /// armed value; a write site alone is an unrelated write and an armed value alone is a
    /// read. Empty means the surface has no armed-value discriminator (any write arms it).
    pub armed_markers: &'static [&'static str],
    /// Writers not derivable from source tokens, each carrying a reason code.
    pub declared_writers: &'static [DeclaredWriter],
}

impl StateSurfaceEntry {
    /// True when `command` writes this surface's armed value.
    pub fn flags(&self, command: &str, tokens: &BTreeSet<String>) -> bool {
        if self
            .declared_writers
            .iter()
            .any(|declared| declared.command == command)
        {
            return true;
        }
        let reaches_write_site = self
            .write_markers
            .iter()
            .any(|marker| tokens.contains(*marker));
        let reaches_armed_value = self.armed_markers.is_empty()
            || self
                .armed_markers
                .iter()
                .any(|marker| tokens.contains(*marker));
        reaches_write_site && reaches_armed_value
    }

    /// The markers that actually matched, for tests that must prove a command was not flagged
    /// by its own name.
    pub fn matched_markers(&self, tokens: &BTreeSet<String>) -> BTreeSet<&'static str> {
        self.write_markers
            .iter()
            .chain(self.armed_markers.iter())
            .filter(|marker| tokens.contains(**marker))
            .copied()
            .collect()
    }
}

/// Detector-(b)'s mechanically matched command writers.
pub fn spawn_triggering_writers(
    graph: &CallGraph,
    commands: impl IntoIterator<Item = String>,
    surface: &[StateSurfaceEntry],
) -> BTreeSet<String> {
    commands
        .into_iter()
        .filter(|command| {
            let tokens = &graph.closure([command.clone()]).tokens;
            surface.iter().any(|entry| entry.flags(command, tokens))
        })
        .collect()
}

/// Derived from the read sites reached by the settled authority-bearing loop inventory.
///
/// Every `write_markers`/`armed_markers` token below is a persistence-layer identifier taken
/// from the write site itself — repository trait methods, enum variant paths, distinguishing
/// field idents and string literals — never the name of the command that reaches it.
pub const SPAWN_TRIGGERING_STATE_SURFACE: &[StateSurfaceEntry] = &[
    StateSurfaceEntry {
        id: "ready-task",
        surface: "tasks.internal_status",
        armed_value: "Ready",
        read_by_loops: &["application/ready_task_scheduler.rs::application/ready_task_scheduler.rs:::::spawn_ready_task_scheduler_if_needed@57e1eb6d86c1770f"],
        write_markers: &["restart_terminal_task_to_ready_with_history_for_action"],
        armed_markers: &["InternalStatus::Ready"],
        declared_writers: &[DeclaredWriter {
            command: "inject_task",
            reason: "seeds-ready-row-through-generic-repository-create: the row is born in Ready \
                     via `TaskRepository::create`, whose write-site token is shared with 54 \
                     unrelated creators, so no marker distinguishes this write mechanically",
        }],
    },
    StateSurfaceEntry {
        id: "pending-review-freshness",
        surface: "tasks.internal_status + task_status_history.entered_at + agent_runs.status",
        armed_value: "PendingReview with no fresh/running reviewer",
        read_by_loops: &["application/startup_background.rs::application/startup_background.rs:::::spawn_watchdog@8c28974ee8ca859d"],
        write_markers: &["ensure_re_review_from_escalated_status", "add_note"],
        armed_markers: &["InternalStatus::PendingReview", "InternalStatus::RevisionNeeded"],
        declared_writers: &[],
    },
    StateSurfaceEntry {
        id: "automation-active",
        surface: "automations.status",
        armed_value: "Active",
        read_by_loops: &["application/startup_background.rs::application/startup_background.rs:::::spawn_automation_scheduler@c034c5fc2b8fe7b8"],
        write_markers: &["reopen_run_corrective"],
        armed_markers: &["AutomationStatus::Active"],
        declared_writers: &[DeclaredWriter {
            command: "finalize_automation",
            reason: "shared-automation-status-chokepoint: the finalize path reaches \
                     `transition_automation_status` through the automation service, a token 53 \
                     of 539 census commands reach transitively, so it cannot discriminate",
        }],
    },
    StateSurfaceEntry {
        id: "workspace-bridge",
        surface: "agent_conversation_workspaces.linked_ideation_session_id/status/mode",
        armed_value: "linked active plan/edit workspace",
        read_by_loops: &["application/startup_background.rs::application/startup_background.rs:::::spawn_agent_workspace_bridge_dispatcher@11ddd3248e57299b"],
        write_markers: &["create_or_update", "update_links"],
        armed_markers: &[
            "AgentConversationWorkspaceMode::Ideation",
            "AgentConversationWorkspaceMode::Plan",
            "AgentConversationWorkspaceMode::Edit",
            "AgentConversationWorkspaceMode::Tasks",
        ],
        declared_writers: &[],
    },
    StateSurfaceEntry {
        id: "external-event-cursor",
        surface: "external_events rows/cursor",
        armed_value: "unconsumed row",
        read_by_loops: &["application/startup_background.rs::application/startup_background.rs:::::spawn_agent_workspace_bridge_dispatcher@11ddd3248e57299b"],
        write_markers: &["insert_event"],
        armed_markers: &[],
        declared_writers: &[],
    },
    StateSurfaceEntry {
        id: "workspace-auto-publish",
        surface: "agent_conversation_workspaces.auto_publish_enabled/publication_push_status",
        armed_value: "enabled and publishable/needs_agent",
        read_by_loops: &["commands/agent_workspace_auto_publish.rs::commands/agent_workspace_auto_publish.rs:::::start_agent_workspace_auto_publish_freshness_scan@3a8d62e625ea5914"],
        write_markers: &["update_auto_publish_preferences", "update_auto_publish_initial_pr_preference"],
        armed_markers: &["auto_publish"],
        declared_writers: &[],
    },
    StateSurfaceEntry {
        id: "workspace-auto-review",
        surface: "review_settings.require_workspace_review",
        armed_value: "true",
        read_by_loops: &["commands/agent_workspace_auto_review.rs::commands/agent_workspace_auto_review.rs:::::spawn_auto_review_for_workspace@a952be79d060c28f"],
        write_markers: &["update_settings"],
        armed_markers: &["require_workspace_review"],
        declared_writers: &[],
    },
];

impl CallGraph {
    pub fn build(files: &[(String, String)]) -> Self {
        let mut graph = CallGraph::default();
        let registered = files
            .iter()
            .find(|(path, _)| path == "commands/registry.rs")
            .map(|(_, source)| {
                parse_registered_commands(source)
                    .into_iter()
                    .map(|(command, _)| command)
                    .collect::<BTreeSet<_>>()
            })
            .unwrap_or_default();
        graph.registered_commands = registered.clone();
        for (path, source) in files {
            let Ok(parsed) = syn::parse_file(source) else {
                // A file this crate cannot parse would silently shrink the graph, so it is a
                // hard failure rather than a skipped file.
                panic!("authority audit could not parse {path}");
            };
            let mut visitor = FileVisitor::new(path, &mut graph);
            visitor.visit_file(&parsed);
        }
        let mut loop_ids = BTreeMap::<String, usize>::new();
        for root in &mut graph.loop_roots {
            let duplicate = loop_ids.entry(root.id.clone()).or_default();
            if *duplicate > 0 {
                root.id = format!("{}~{}", root.id, duplicate);
            }
            *duplicate += 1;
        }
        graph.resolve_dispatch(&registered);
        graph
    }

    fn node_mut(&mut self, name: &str) -> &mut FnNode {
        self.nodes.entry(name.to_string()).or_default()
    }

    fn resolve_dispatch(&mut self, registered: &BTreeSet<String>) {
        let definitions = self.definitions.clone();
        let definition_arities = self.definition_arities.clone();
        let definition_owners = self.definition_owners.clone();
        let definition_methods = self.definition_methods.clone();
        let node_files = self.node_files.clone();
        let mut unresolved_dispatch = BTreeMap::<String, BTreeSet<String>>::new();
        for (node_name, node) in &mut self.nodes {
            let source_file = node_files.get(node_name).map(String::as_str).unwrap_or("");
            if is_architecture_leaf(source_file) {
                node.callees.clear();
                continue;
            }
            let source_is_command = source_file.starts_with("commands/");
            let unresolved = std::mem::take(&mut node.callees);
            for callee in unresolved {
                let Some((kind, bare, arity, owner)) = parse_pending_call(&callee) else {
                    continue;
                };
                if source_is_command && registered.contains(bare) {
                    continue;
                }
                if all_cut_sinks().contains(bare) {
                    continue;
                }
                if let Some(targets) = definitions.get(bare) {
                    let resolved = dispatch_candidates(
                        targets,
                        arity,
                        owner,
                        kind,
                        &definition_arities,
                        &definition_owners,
                        &definition_methods,
                    );
                    if resolved.len() > 1 {
                        unresolved_dispatch
                            .entry(format!("{node_name} -> {bare}/{arity}"))
                            .or_default()
                            .extend(resolved.iter().cloned());
                    }
                    node.callees.extend(resolved);
                }
            }
        }
        for root in &mut self.loop_roots {
            let unresolved = std::mem::take(&mut root.callees);
            for callee in unresolved {
                let Some((kind, bare, arity, owner)) = parse_pending_call(&callee) else {
                    continue;
                };
                if all_cut_sinks().contains(bare) {
                    continue;
                }
                if let Some(targets) = definitions.get(bare) {
                    let resolved = dispatch_candidates(
                        targets,
                        arity,
                        owner,
                        kind,
                        &definition_arities,
                        &definition_owners,
                        &definition_methods,
                    );
                    if resolved.len() > 1 {
                        unresolved_dispatch
                            .entry(format!("{} -> {bare}/{arity}", root.id))
                            .or_default()
                            .extend(resolved.iter().cloned());
                    }
                    root.callees.extend(resolved);
                }
            }
        }
        self.unresolved_dispatch = unresolved_dispatch;
    }

    pub fn roots_named(&self, name: &str) -> BTreeSet<String> {
        let definitions = self.definitions.get(name).cloned().unwrap_or_default();
        if !self.registered_commands.contains(name) {
            return definitions;
        }
        definitions
            .into_iter()
            .filter(|node| {
                self.node_files
                    .get(node)
                    .is_some_and(|file| file.starts_with("commands/"))
            })
            .collect()
    }

    /// Expands `roots` transitively, stopping at (but recording) sinks.
    pub fn closure(&self, roots: impl IntoIterator<Item = String>) -> Closure {
        let sinks = all_cut_sinks();
        let mut result = Closure::default();
        let mut queue = VecDeque::new();
        for root in roots {
            if self.nodes.contains_key(&root) {
                queue.push_back(root);
            } else {
                queue.extend(self.roots_named(&root));
            }
        }
        while let Some(name) = queue.pop_front() {
            if !result.visited.insert(name.clone()) {
                continue;
            }
            let Some(node) = self.nodes.get(&name) else {
                continue;
            };
            result.tokens.extend(node.tokens.iter().cloned());
            result.sink_hits.extend(node.sink_hits.iter().cloned());
            for callee in &node.callees {
                // Cut at sinks: their own bodies are the machinery whose authority is being
                // classified, not evidence about the caller.
                if sinks.contains(callee.as_str()) || result.visited.contains(callee) {
                    continue;
                }
                queue.push_back(callee.clone());
            }
        }
        result
    }

    /// Expands a discovered loop root the same way commands are expanded.
    pub fn loop_closure(&self, root: &LoopRoot) -> Closure {
        let mut closure = self.closure(root.callees.iter().cloned());
        closure.sink_hits.extend(root.sink_hits.iter().cloned());
        closure
    }
}

/// How a sink hit classifies once target sensitivity is applied.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HitVerdict {
    /// Arms or re-enters agent execution.
    Arming,
    /// Only halts/parks — authority-reducing, exempt (§3.3 exemption decision).
    Halting,
}

pub fn verdict_for(hit: &SinkHit) -> HitVerdict {
    if !TRANSITION_SINKS.contains(&hit.sink.as_str()) {
        return HitVerdict::Arming;
    }
    if hit.targets.is_empty() {
        // Target not statically visible → fail closed.
        return HitVerdict::Arming;
    }
    if hit
        .targets
        .iter()
        .any(|target| ARMING_TRANSITION_TARGETS.contains(&target.as_str()))
    {
        return HitVerdict::Arming;
    }
    if hit
        .targets
        .iter()
        .all(|target| HALTING_TRANSITION_TARGETS.contains(&target.as_str()))
    {
        return HitVerdict::Halting;
    }
    HitVerdict::Arming
}

pub fn closure_is_arming(closure: &Closure) -> bool {
    closure
        .sink_hits
        .iter()
        .any(|hit| verdict_for(hit) == HitVerdict::Arming)
}

// ---------------------------------------------------------------------------------------
// syn visitor
// ---------------------------------------------------------------------------------------

struct FileVisitor<'a> {
    file: String,
    graph: &'a mut CallGraph,
    fn_stack: Vec<String>,
    impl_stack: Vec<String>,
}

impl<'a> FileVisitor<'a> {
    fn new(file: &str, graph: &'a mut CallGraph) -> Self {
        Self {
            file: file.to_string(),
            graph,
            fn_stack: Vec::new(),
            impl_stack: Vec::new(),
        }
    }

    fn current_fn(&self) -> Option<&str> {
        self.fn_stack.last().map(String::as_str)
    }

    fn record_callee(&mut self, callee: &str) {
        let Some(current) = self.current_fn().map(str::to_string) else {
            return;
        };
        self.graph
            .node_mut(&current)
            .callees
            .insert(callee.to_string());
    }

    fn record_token(&mut self, token: String) {
        let Some(current) = self.current_fn().map(str::to_string) else {
            return;
        };
        self.graph.node_mut(&current).tokens.insert(token);
    }

    fn record_sink_hit(&mut self, hit: SinkHit) {
        let Some(current) = self.current_fn().map(str::to_string) else {
            return;
        };
        self.graph.node_mut(&current).sink_hits.insert(hit);
    }

    fn enter_fn(&mut self, bare_name: String, arity: usize, is_method: bool) {
        let owner = self
            .impl_stack
            .last()
            .cloned()
            .unwrap_or_else(|| ":".to_string());
        let name = format!("{}::{}::{}", self.file, owner, bare_name);
        self.graph.node_mut(&name).tokens.insert(bare_name.clone());
        self.graph
            .definitions
            .entry(bare_name)
            .or_default()
            .insert(name.clone());
        self.graph
            .node_files
            .insert(name.clone(), self.file.clone());
        self.graph.definition_arities.insert(name.clone(), arity);
        self.graph.definition_owners.insert(name.clone(), owner);
        self.graph
            .definition_methods
            .insert(name.clone(), is_method);
        self.fn_stack.push(name);
    }

    fn leave_fn(&mut self) {
        self.fn_stack.pop();
    }

    /// Registers a discovered background-loop entry point from a spawn/listen call argument.
    fn record_loop_root(
        &mut self,
        kind: &str,
        args: &syn::punctuated::Punctuated<syn::Expr, syn::token::Comma>,
    ) {
        let enclosing = self.current_fn().unwrap_or("<file-scope>").to_string();
        let mut body = BodyScan::default();
        for arg in args {
            body.visit_expr(arg);
        }
        let id = stable_loop_id(&self.file, &enclosing, &body);
        self.graph.loop_roots.push(LoopRoot {
            id,
            file: self.file.clone(),
            enclosing_fn: enclosing,
            kind: kind.to_string(),
            callees: body.callees,
            sink_hits: body.sink_hits,
        });
    }
}

fn stable_loop_id(file: &str, enclosing: &str, body: &BodyScan) -> String {
    let mut hash = 0xcbf29ce484222325_u64;
    for byte in body
        .callees
        .iter()
        .chain(body.sink_hits.iter().map(|hit| &hit.sink))
        .chain(body.identity_tokens.iter())
        .flat_map(|value| value.bytes().chain(std::iter::once(0)))
    {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    format!("{file}::{enclosing}@{hash:08x}")
}

fn is_architecture_leaf(file: &str) -> bool {
    file.starts_with("domain/")
        || file.starts_with("infrastructure/sqlite/")
        || file.starts_with("infrastructure/services/")
        || file.starts_with("infrastructure/memory/")
        || file == "infrastructure/remote_host_client.rs"
        // Remote settings is a persistence adapter: its generic `get_or_create`/`execute`
        // calls cannot enter application workflows. Keeping it opaque prevents same-arity
        // dispatch from inventing a settings -> agent-workflow edge.
        || file == "remote_server/settings.rs"
}

fn pending_call(kind: &str, name: &str, arity: usize, owner: &str) -> String {
    format!("{kind}|{name}|{arity}|{owner}")
}

fn parse_pending_call(call: &str) -> Option<(&str, &str, usize, &str)> {
    let mut parts = call.split('|');
    let kind = parts.next()?;
    let name = parts.next()?;
    let arity = parts.next()?.parse().ok()?;
    let owner = parts.next()?;
    (parts.next().is_none()).then_some((kind, name, arity, owner))
}

fn dispatch_candidates(
    targets: &BTreeSet<String>,
    arity: usize,
    owner: &str,
    kind: &str,
    definition_arities: &BTreeMap<String, usize>,
    definition_owners: &BTreeMap<String, String>,
    definition_methods: &BTreeMap<String, bool>,
) -> BTreeSet<String> {
    let call_wants_method =
        kind == "method" || owner.chars().next().is_some_and(char::is_uppercase);
    let kind_matching: BTreeSet<String> = targets
        .iter()
        .filter(|target| definition_methods.get(*target) == Some(&call_wants_method))
        .cloned()
        .collect();
    let matching: BTreeSet<String> = kind_matching
        .iter()
        .filter(|target| definition_arities.get(*target) == Some(&arity))
        .cloned()
        .collect();
    let arity_filtered = if matching.is_empty() {
        &kind_matching
    } else {
        &matching
    };
    if owner.is_empty() {
        return arity_filtered.clone();
    }
    let owner_matching: BTreeSet<String> = arity_filtered
        .iter()
        .filter(|target| {
            definition_owners.get(*target).is_some_and(|candidate| {
                candidate == owner || candidate.ends_with(&format!("::{owner}"))
            })
        })
        .cloned()
        .collect();
    if owner_matching.is_empty() {
        if call_wants_method {
            BTreeSet::new()
        } else {
            arity_filtered.clone()
        }
    } else {
        owner_matching
    }
}

fn path_last_segment(path: &syn::Path) -> Option<String> {
    path.segments
        .last()
        .map(|segment| segment.ident.to_string())
}

fn path_string(path: &syn::Path) -> String {
    path.segments
        .iter()
        .map(|segment| segment.ident.to_string())
        .collect::<Vec<_>>()
        .join("::")
}

/// True for `tokio::spawn`, `tokio::task::spawn`, `tauri::async_runtime::spawn`,
/// `std::thread::spawn`, `tokio::task::spawn_blocking` — the loop entry-point forms.
fn is_background_spawn_path(path: &syn::Path) -> Option<&'static str> {
    let segments: Vec<String> = path
        .segments
        .iter()
        .map(|segment| segment.ident.to_string())
        .collect();
    let last = segments.last()?.as_str();
    if !matches!(last, "spawn" | "spawn_blocking") {
        return None;
    }
    let joined = segments.join("::");
    for (needle, kind) in [
        ("tauri::async_runtime::spawn", "async_runtime::spawn"),
        ("async_runtime::spawn", "async_runtime::spawn"),
        ("tokio::task::spawn", "tokio::task::spawn"),
        ("tokio::spawn", "tokio::spawn"),
        ("thread::spawn", "thread::spawn"),
        ("task::spawn_blocking", "spawn_blocking"),
    ] {
        if joined.ends_with(needle) {
            return Some(kind);
        }
    }
    None
}

/// `.spawn("worker", id)` / `.spawn_background("qa-prep", id)` — an `AgentSpawner` call
/// recognised by shape (string-literal agent type first) rather than by receiver name.
pub(super) fn is_agent_spawn_method(
    method: &str,
    args: &syn::punctuated::Punctuated<syn::Expr, syn::token::Comma>,
) -> bool {
    if !matches!(method, "spawn" | "spawn_background") {
        return false;
    }
    matches!(
        args.first(),
        Some(syn::Expr::Lit(syn::ExprLit {
            lit: syn::Lit::Str(_),
            ..
        }))
    )
}

/// Runtime-handle spawn methods whose first argument is executable code. Requiring a closure
/// or async block makes this mutually exclusive with the string-literal-first AgentSpawner
/// shape.
pub(super) fn is_method_background_spawn(
    method: &str,
    args: &syn::punctuated::Punctuated<syn::Expr, syn::token::Comma>,
) -> bool {
    matches!(method, "spawn" | "spawn_blocking")
        && matches!(
            args.first(),
            Some(syn::Expr::Closure(_) | syn::Expr::Async(_))
        )
}

fn is_method_listener(
    method: &str,
    args: &syn::punctuated::Punctuated<syn::Expr, syn::token::Comma>,
) -> bool {
    method == "listen" && args.iter().any(|arg| matches!(arg, syn::Expr::Closure(_)))
}

fn internal_status_targets(
    args: &syn::punctuated::Punctuated<syn::Expr, syn::token::Comma>,
) -> BTreeSet<String> {
    let mut scan = StatusScan::default();
    for arg in args {
        scan.visit_expr(arg);
    }
    scan.targets
}

fn transition_targets(
    name: &str,
    args: &syn::punctuated::Punctuated<syn::Expr, syn::token::Comma>,
) -> BTreeSet<String> {
    let mut targets = internal_status_targets(args);
    if name == "transition_to_stopped_with_context" {
        targets.insert("Stopped".to_string());
    }
    targets
}

#[derive(Default)]
struct StatusScan {
    targets: BTreeSet<String>,
}

impl<'ast> Visit<'ast> for StatusScan {
    fn visit_path(&mut self, path: &'ast syn::Path) {
        let segments: Vec<String> = path
            .segments
            .iter()
            .map(|segment| segment.ident.to_string())
            .collect();
        if let Some(index) = segments.iter().position(|s| s == "InternalStatus") {
            if let Some(variant) = segments.get(index + 1) {
                self.targets.insert(variant.clone());
            }
        }
        syn::visit::visit_path(self, path);
    }
}

/// Collects callees and sink hits from an arbitrary expression (used for loop-root bodies).
#[derive(Default)]
struct BodyScan {
    callees: BTreeSet<String>,
    sink_hits: BTreeSet<SinkHit>,
    identity_tokens: BTreeSet<String>,
}

impl BodyScan {
    fn note(
        &mut self,
        kind: &str,
        name: &str,
        args: &syn::punctuated::Punctuated<syn::Expr, syn::token::Comma>,
    ) {
        self.callees
            .insert(pending_call(kind, name, args.len(), ""));
        if all_cut_sinks().contains(name) {
            self.sink_hits.insert(SinkHit {
                sink: name.to_string(),
                targets: transition_targets(name, args),
            });
        }
    }
}

impl<'ast> Visit<'ast> for BodyScan {
    fn visit_expr_call(&mut self, node: &'ast syn::ExprCall) {
        if let syn::Expr::Path(path) = node.func.as_ref() {
            if let Some(name) = path_last_segment(&path.path) {
                self.note("call", &name, &node.args);
            }
        }
        syn::visit::visit_expr_call(self, node);
    }

    fn visit_expr_method_call(&mut self, node: &'ast syn::ExprMethodCall) {
        let name = node.method.to_string();
        self.note("method", &name, &node.args);
        if is_agent_spawn_method(&name, &node.args) {
            self.sink_hits.insert(SinkHit {
                sink: AGENT_SPAWN_SINK.to_string(),
                targets: BTreeSet::new(),
            });
        }
        syn::visit::visit_expr_method_call(self, node);
    }

    fn visit_path(&mut self, node: &'ast syn::Path) {
        self.identity_tokens.insert(path_string(node));
        syn::visit::visit_path(self, node);
    }

    fn visit_lit_str(&mut self, node: &'ast syn::LitStr) {
        self.identity_tokens.insert(node.value());
        syn::visit::visit_lit_str(self, node);
    }
}

impl<'ast, 'a> Visit<'ast> for FileVisitor<'a> {
    fn visit_item_mod(&mut self, node: &'ast syn::ItemMod) {
        if has_cfg_test(&node.attrs) {
            return;
        }
        syn::visit::visit_item_mod(self, node);
    }

    fn visit_item_fn(&mut self, node: &'ast syn::ItemFn) {
        if has_cfg_test(&node.attrs) {
            return;
        }
        self.enter_fn(node.sig.ident.to_string(), node.sig.inputs.len(), false);
        syn::visit::visit_block(self, &node.block);
        self.leave_fn();
    }

    fn visit_item_impl(&mut self, node: &'ast syn::ItemImpl) {
        let owner = match node.self_ty.as_ref() {
            syn::Type::Path(path) => path_string(&path.path),
            _ => "<impl>".to_string(),
        };
        self.impl_stack.push(owner);
        syn::visit::visit_item_impl(self, node);
        self.impl_stack.pop();
    }

    fn visit_impl_item_fn(&mut self, node: &'ast syn::ImplItemFn) {
        if has_cfg_test(&node.attrs) {
            return;
        }
        let arity = node
            .sig
            .inputs
            .iter()
            .filter(|input| !matches!(input, syn::FnArg::Receiver(_)))
            .count();
        self.enter_fn(node.sig.ident.to_string(), arity, true);
        syn::visit::visit_block(self, &node.block);
        self.leave_fn();
    }

    fn visit_expr_call(&mut self, node: &'ast syn::ExprCall) {
        if let syn::Expr::Path(path) = node.func.as_ref() {
            if let Some(kind) = is_background_spawn_path(&path.path) {
                self.record_loop_root(kind, &node.args);
            }
            if let Some(name) = path_last_segment(&path.path) {
                let owner = path
                    .path
                    .segments
                    .iter()
                    .rev()
                    .nth(1)
                    .map(|segment| segment.ident.to_string())
                    .unwrap_or_default();
                self.record_callee(&pending_call("call", &name, node.args.len(), &owner));
                self.record_token(path_string(&path.path));
                if all_cut_sinks().contains(name.as_str()) {
                    let targets = transition_targets(&name, &node.args);
                    self.record_sink_hit(SinkHit {
                        sink: name,
                        targets,
                    });
                }
            }
        }
        syn::visit::visit_expr_call(self, node);
    }

    fn visit_expr_method_call(&mut self, node: &'ast syn::ExprMethodCall) {
        let name = node.method.to_string();
        self.record_callee(&pending_call("method", &name, node.args.len(), ""));
        self.record_token(name.clone());
        if name == "listen_any" || name == "listen_global" {
            self.record_loop_root("listen_any", &node.args);
        } else if is_method_background_spawn(&name, &node.args) {
            self.record_loop_root(&format!("method::{name}"), &node.args);
        } else if is_method_listener(&name, &node.args) {
            self.record_loop_root("listen", &node.args);
        }
        if all_cut_sinks().contains(name.as_str()) {
            let targets = transition_targets(&name, &node.args);
            self.record_sink_hit(SinkHit {
                sink: name.clone(),
                targets,
            });
        }
        if is_agent_spawn_method(&name, &node.args) {
            self.record_sink_hit(SinkHit {
                sink: AGENT_SPAWN_SINK.to_string(),
                targets: BTreeSet::new(),
            });
        }
        syn::visit::visit_expr_method_call(self, node);
    }

    fn visit_path(&mut self, node: &'ast syn::Path) {
        if node.segments.len() > 1 {
            self.record_token(path_string(node));
        } else if let Some(segment) = node.segments.first() {
            self.record_token(segment.ident.to_string());
        }
        syn::visit::visit_path(self, node);
    }

    fn visit_member(&mut self, node: &'ast syn::Member) {
        if let syn::Member::Named(ident) = node {
            self.record_token(ident.to_string());
        }
        syn::visit::visit_member(self, node);
    }

    fn visit_lit_str(&mut self, node: &'ast syn::LitStr) {
        self.record_token(node.value());
        syn::visit::visit_lit_str(self, node);
    }
}

fn has_cfg_test(attrs: &[syn::Attribute]) -> bool {
    attrs.iter().any(|attr| {
        attr.path().is_ident("cfg")
            && attr
                .parse_args::<syn::Meta>()
                .map(|meta| meta.path().is_ident("test"))
                .unwrap_or(false)
    })
}

// ---------------------------------------------------------------------------------------
// Source loading
// ---------------------------------------------------------------------------------------

/// Crate root, baked at compile time by cargo — never read from the process environment, so
/// no runtime-tainted value reaches a filesystem sink here.
pub fn crate_src_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("src")
}

pub fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("src-tauri always has a parent")
        .to_path_buf()
}

/// Fail-closed floor on the size of the loaded production graph.
///
/// The detectors are only a floor if the graph they run over is the whole program. A load that
/// silently returns a fraction of the tree would collapse every downstream classification to
/// "nothing is authority-bearing" and could be baked into the checked-in manifest through
/// `RALPHX_REGENERATE_REMOTE_MANIFEST`. The tree has ~1060 production files; anything under
/// this floor means the walk lost the tree, not that the tree shrank.
pub const MIN_PRODUCTION_SOURCE_FILES: usize = 900;

/// Loads every production `.rs` file under `src-tauri/src`.
///
/// `*_tests.rs` files are excluded: test bodies would inject edges no production caller has.
///
/// Every I/O failure is a hard error. An unreadable directory or file shrinks the call graph
/// exactly the way an unparseable file does, and the parse path already panics rather than
/// skip (see [`CallGraph::build`]); silent skipping here would have been the same
/// silent-graph-shrinkage with a quieter failure mode.
pub fn load_production_sources() -> Vec<(String, String)> {
    let root = crate_src_root();
    let mut files = Vec::new();
    collect_rs_files(&root, &root, &mut files);
    files.sort_by(|a, b| a.0.cmp(&b.0));
    assert!(
        files.len() >= MIN_PRODUCTION_SOURCE_FILES,
        "authority audit loaded only {} production sources, below the {MIN_PRODUCTION_SOURCE_FILES} floor: the graph collapsed",
        files.len()
    );
    files
}

/// Recursive `.rs` walk. Public so the fail-closed behaviour is testable against a fixture
/// tree; production callers go through [`load_production_sources`].
pub fn collect_rs_files(root: &Path, dir: &Path, out: &mut Vec<(String, String)>) {
    let entries = std::fs::read_dir(dir).unwrap_or_else(|error| {
        panic!(
            "authority audit could not read directory {}: {error}",
            dir.display()
        )
    });
    for entry in entries {
        let entry = entry.unwrap_or_else(|error| {
            panic!(
                "authority audit could not read a directory entry under {}: {error}",
                dir.display()
            )
        });
        let path = entry.path();
        let file_type = entry.file_type().unwrap_or_else(|error| {
            panic!("authority audit could not stat {}: {error}", path.display())
        });
        if file_type.is_dir() {
            if matches!(
                path.file_name().and_then(|name| name.to_str()),
                Some("tests" | "testing")
            ) {
                continue;
            }
            collect_rs_files(root, &path, out);
            continue;
        }
        let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        if !name.ends_with(".rs")
            || name.ends_with("_tests.rs")
            || matches!(name, "tests.rs" | "mocks.rs")
        {
            continue;
        }
        let source = std::fs::read_to_string(&path).unwrap_or_else(|error| {
            panic!("authority audit could not read {}: {error}", path.display())
        });
        let relative = path
            .strip_prefix(root)
            .unwrap_or(&path)
            .to_string_lossy()
            .replace('\\', "/");
        out.push((relative, source));
    }
}

/// Reads the leaf command names out of `commands/registry.rs`.
///
/// The Tauri registry is the census: the ledger is exhaustive over exactly this list.
pub fn registered_command_names() -> Vec<String> {
    let source = std::fs::read_to_string(crate_src_root().join("commands/registry.rs"))
        .expect("commands/registry.rs must be readable");
    parse_registered_commands(&source)
        .into_iter()
        .map(|(command, _)| command)
        .collect()
}

pub fn parse_registered_commands(source: &str) -> Vec<(String, String)> {
    const MARKER: &str = "tauri::generate_handler![";
    let start = source
        .find(MARKER)
        .expect("registry.rs must contain generate_handler!");
    let body = &source[start + MARKER.len()..];
    let uncommented = body
        .lines()
        .map(|line| line.split_once("//").map_or(line, |(code, _)| code))
        .collect::<Vec<_>>()
        .join("\n");
    let mut bracket_depth = 1usize;
    let end = uncommented
        .char_indices()
        .find_map(|(index, ch)| match ch {
            '[' => {
                bracket_depth += 1;
                None
            }
            ']' => {
                bracket_depth -= 1;
                (bracket_depth == 0).then_some(index)
            }
            _ => None,
        })
        .expect("generate_handler! body must have a closing bracket");

    uncommented[..end]
        .split(',')
        .filter_map(|raw_segment| {
            let mut segment = raw_segment.trim();
            while segment.starts_with("#[") {
                let attribute_end = segment.find(']').unwrap_or_else(|| {
                    panic!("malformed command census segment `{segment}`: unterminated attribute")
                });
                segment = segment[attribute_end + 1..].trim();
            }
            if segment.is_empty() {
                return None;
            }
            let parts = segment.split("::").collect::<Vec<_>>();
            let valid_ident = |part: &&str| {
                let mut chars = part.chars();
                chars
                    .next()
                    .is_some_and(|first| first == '_' || first.is_ascii_alphabetic())
                    && chars.all(|ch| ch == '_' || ch.is_ascii_alphanumeric())
            };
            if parts.is_empty() || !parts.iter().all(valid_ident) {
                panic!("malformed command census segment `{segment}`");
            }
            let (command, module) = match parts.as_slice() {
                [command] => ((*command).to_string(), "root".to_string()),
                ["commands", module, .., command] => {
                    ((*command).to_string(), (*module).to_string())
                }
                _ => panic!("malformed command census segment `{segment}`"),
            };
            Some((command, module))
        })
        .collect()
}
