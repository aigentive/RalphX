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
//! The graph is **name-keyed**: a node is a function/method *name*, and every definition of
//! that name unions into one node. This is a deliberate over-approximation and it is exactly
//! what makes dyn-dispatch safe here — `Arc<dyn AgenticClient>::send_message`, the mock, and
//! every concrete impl collapse into the same node, so a trait-object edge is never lost.
//! The cost is false positives (an unrelated `send_message` on another type is also an edge).
//! Over-approximation is the correct direction: the audit output is a CI-enforced *subset* of
//! the shipped `AgentControl` set, so a false positive costs a ledger row, while a false
//! negative would cost an unclassified spawn path.
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

/// `InternalStatus` targets that ARM or re-enter scheduling → classify.
pub const ARMING_TRANSITION_TARGETS: &[&str] = &[
    "Ready",
    "Executing",
    "Reviewing",
    "Merging",
    "QaTesting",
    "QaPrep",
    "PendingReview",
];

/// `InternalStatus` targets that only halt/park → authority-reducing, exempt.
pub const HALTING_TRANSITION_TARGETS: &[&str] =
    &["Paused", "Blocked", "Stopped", "Failed", "Cancelled", "Archived"];

fn all_cut_sinks() -> BTreeSet<&'static str> {
    TRANSITION_SINKS
        .iter()
        .chain(SCHEDULER_SINKS.iter())
        .chain(STEER_SINKS.iter())
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
}

/// Result of expanding a set of roots through the graph, stopping at sinks.
#[derive(Debug, Default)]
pub struct Closure {
    pub visited: BTreeSet<String>,
    pub tokens: BTreeSet<String>,
    pub sink_hits: BTreeSet<SinkHit>,
}

impl CallGraph {
    pub fn build(files: &[(String, String)]) -> Self {
        let mut graph = CallGraph::default();
        for (path, source) in files {
            let Ok(parsed) = syn::parse_file(source) else {
                // A file this crate cannot parse would silently shrink the graph, so it is a
                // hard failure rather than a skipped file.
                panic!("authority audit could not parse {path}");
            };
            let mut visitor = FileVisitor::new(path, &mut graph);
            visitor.visit_file(&parsed);
        }
        graph
    }

    fn node_mut(&mut self, name: &str) -> &mut FnNode {
        self.nodes.entry(name.to_string()).or_default()
    }

    /// Expands `roots` transitively, stopping at (but recording) sinks.
    pub fn closure(&self, roots: impl IntoIterator<Item = String>) -> Closure {
        let sinks = all_cut_sinks();
        let mut result = Closure::default();
        let mut queue: VecDeque<String> = roots.into_iter().collect();
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
    loop_ordinal: usize,
}

impl<'a> FileVisitor<'a> {
    fn new(file: &str, graph: &'a mut CallGraph) -> Self {
        Self {
            file: file.to_string(),
            graph,
            fn_stack: Vec::new(),
            loop_ordinal: 0,
        }
    }

    fn current_fn(&self) -> Option<&str> {
        self.fn_stack.last().map(String::as_str)
    }

    fn record_callee(&mut self, callee: &str) {
        let Some(current) = self.current_fn().map(str::to_string) else {
            return;
        };
        self.graph.node_mut(&current).callees.insert(callee.to_string());
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

    fn enter_fn(&mut self, name: String) {
        self.graph.node_mut(&name);
        self.fn_stack.push(name);
    }

    fn leave_fn(&mut self) {
        self.fn_stack.pop();
    }

    /// Registers a discovered background-loop entry point from a spawn/listen call argument.
    fn record_loop_root(&mut self, kind: &str, args: &syn::punctuated::Punctuated<syn::Expr, syn::token::Comma>) {
        let enclosing = self.current_fn().unwrap_or("<file-scope>").to_string();
        self.loop_ordinal += 1;
        let id = format!("{}::{}#{}", self.file, enclosing, self.loop_ordinal);
        let mut body = BodyScan::default();
        for arg in args {
            body.visit_expr(arg);
        }
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

fn path_last_segment(path: &syn::Path) -> Option<String> {
    path.segments.last().map(|segment| segment.ident.to_string())
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
fn is_agent_spawn_method(
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

fn internal_status_targets(
    args: &syn::punctuated::Punctuated<syn::Expr, syn::token::Comma>,
) -> BTreeSet<String> {
    let mut scan = StatusScan::default();
    for arg in args {
        scan.visit_expr(arg);
    }
    scan.targets
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
}

impl BodyScan {
    fn note(&mut self, name: &str, args: &syn::punctuated::Punctuated<syn::Expr, syn::token::Comma>) {
        self.callees.insert(name.to_string());
        if all_cut_sinks().contains(name) {
            self.sink_hits.insert(SinkHit {
                sink: name.to_string(),
                targets: internal_status_targets(args),
            });
        }
    }
}

impl<'ast> Visit<'ast> for BodyScan {
    fn visit_expr_call(&mut self, node: &'ast syn::ExprCall) {
        if let syn::Expr::Path(path) = node.func.as_ref() {
            if let Some(name) = path_last_segment(&path.path) {
                self.note(&name, &node.args);
            }
        }
        syn::visit::visit_expr_call(self, node);
    }

    fn visit_expr_method_call(&mut self, node: &'ast syn::ExprMethodCall) {
        let name = node.method.to_string();
        self.note(&name, &node.args);
        if is_agent_spawn_method(&name, &node.args) {
            self.sink_hits.insert(SinkHit {
                sink: AGENT_SPAWN_SINK.to_string(),
                targets: BTreeSet::new(),
            });
        }
        syn::visit::visit_expr_method_call(self, node);
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
        self.enter_fn(node.sig.ident.to_string());
        syn::visit::visit_block(self, &node.block);
        self.leave_fn();
    }

    fn visit_impl_item_fn(&mut self, node: &'ast syn::ImplItemFn) {
        if has_cfg_test(&node.attrs) {
            return;
        }
        self.enter_fn(node.sig.ident.to_string());
        syn::visit::visit_block(self, &node.block);
        self.leave_fn();
    }

    fn visit_expr_call(&mut self, node: &'ast syn::ExprCall) {
        if let syn::Expr::Path(path) = node.func.as_ref() {
            if let Some(kind) = is_background_spawn_path(&path.path) {
                self.record_loop_root(kind, &node.args);
            }
            if let Some(name) = path_last_segment(&path.path) {
                self.record_callee(&name);
                self.record_token(path_string(&path.path));
                if all_cut_sinks().contains(name.as_str()) {
                    let targets = internal_status_targets(&node.args);
                    self.record_sink_hit(SinkHit { sink: name, targets });
                }
            }
        }
        syn::visit::visit_expr_call(self, node);
    }

    fn visit_expr_method_call(&mut self, node: &'ast syn::ExprMethodCall) {
        let name = node.method.to_string();
        self.record_callee(&name);
        self.record_token(name.clone());
        if name == "listen_any" || name == "listen_global" {
            self.record_loop_root("listen_any", &node.args);
        }
        if all_cut_sinks().contains(name.as_str()) {
            let targets = internal_status_targets(&node.args);
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

/// Loads every production `.rs` file under `src-tauri/src`.
///
/// `*_tests.rs` files are excluded: test bodies would inject edges no production caller has.
pub fn load_production_sources() -> Vec<(String, String)> {
    let root = crate_src_root();
    let mut files = Vec::new();
    collect_rs_files(&root, &root, &mut files);
    files.sort_by(|a, b| a.0.cmp(&b.0));
    files
}

fn collect_rs_files(root: &Path, dir: &Path, out: &mut Vec<(String, String)>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_rs_files(root, &path, out);
            continue;
        }
        let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        if !name.ends_with(".rs") || name.ends_with("_tests.rs") || name == "mocks.rs" {
            continue;
        }
        let Ok(source) = std::fs::read_to_string(&path) else {
            continue;
        };
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
    parse_registered_command_names(&source)
}

pub fn parse_registered_command_names(source: &str) -> Vec<String> {
    let start = source
        .find("tauri::generate_handler![")
        .expect("registry.rs must contain generate_handler!");
    let body = &source[start..];
    let mut names = Vec::new();
    for line in body.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with("//") || line.starts_with("#[") {
            continue;
        }
        if line.starts_with(']') {
            break;
        }
        let line = line.trim_start_matches("tauri::generate_handler![").trim();
        let candidate = line.trim_end_matches(',').trim();
        if candidate.is_empty() {
            continue;
        }
        let leaf = candidate.rsplit("::").next().unwrap_or(candidate);
        if leaf
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_')
            && !leaf.is_empty()
        {
            names.push(leaf.to_string());
        }
    }
    names
}
