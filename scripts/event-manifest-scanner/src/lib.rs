use anyhow::{bail, Context, Result};
use ralphx_remote_protocol::{EventClassification, EVENT_CLASSIFICATIONS};
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use syn::spanned::Spanned;
use syn::visit::{self, Visit};
use syn::{Expr, ExprCall, ExprMethodCall, File, ImplItem, Item, ItemConst, ItemFn, Lit, Pat};
use tree_sitter::{Node, Parser};
use tree_sitter_typescript::LANGUAGE_TSX;
use walkdir::WalkDir;

/// Event abstraction functions/methods which are allowed to forward an event-name parameter.
/// The list is deliberately explicit: a newly introduced forwarding wrapper is a CI failure.
const WRAPPERS: &[Wrapper] = &[
    Wrapper::function("emit_app_event", Some(1)),
    Wrapper::function("emit_http_event", Some(1)),
    Wrapper::function("emit_serialized", Some(1)),
    Wrapper::function("emit_serialized_http_event", Some(1)),
    Wrapper::function("emit_queue_changed", None),
    Wrapper::function("emit_task_lifecycle_event", Some(1)),
    Wrapper::function("emit_ticketing_operation_event", None),
    Wrapper::method("TauriEventSink::emit", Some(1)),
    Wrapper::method("TauriFrameSink::emit", Some(1)),
    Wrapper::method("TauriLifecycleSink::emit", Some(1)),
    // `test-utils`-gated remote host fixture; same forwarding shape as `TauriEventSink::emit`,
    // and it lives outside a `_tests.rs` file only because the harness is a shared fixture.
    Wrapper::method("RemoteHostHarness::emit", Some(1)),
    Wrapper::method("ThrottledEmitter::new", None),
    Wrapper::method("ThrottledEmitter::emit", Some(0)),
    Wrapper::method("AppChatService::emit_event", Some(0)),
    Wrapper::method("EnrichedEventEmitter::emit", Some(0)),
    Wrapper::method("EnrichedEventEmitter::emit_with_payload", Some(0)),
];

/// Non-Tauri `.emit` sites that the AST scanner intentionally over-approximates.
/// Every entry names one stable receiver/function shape and records why it cannot carry a Tauri
/// event name. Additions are reviewed source changes rather than silent scanner exclusions.
const FALSE_POSITIVE_ALLOWLIST: &[FalsePositive] = &[
    FalsePositive {
        function: "TauriTicketingEventSink::emit_ticketing_operation_event",
        reason: "the event argument is a TicketingOperationEvent payload; the inner AppHandle emit uses TICKETING_OPERATION_EVENT",
    },
    FalsePositive {
        function: "RecordingEventSink::emit_ticketing_operation_event",
        reason: "test double records a typed TicketingOperationEvent payload and does not emit to Tauri",
    },
];

/// Exact source/receiver pairs for typed internal event buses. These calls do not emit a Tauri
/// event themselves; their `AutomationEvent` is translated by `AutomationEventEmitter`.
const RECEIVER_FALSE_POSITIVE_ALLOWLIST: &[ReceiverFalsePositive] = &[
    ReceiverFalsePositive {
        file: "src-tauri/src/application/automation/provisioning.rs",
        receiver: "self.event_emitter",
        reason: "typed AutomationEvent bus; the Tauri adapter emits a fixed classified name",
    },
    ReceiverFalsePositive {
        file: "src-tauri/src/application/automation/service.rs",
        receiver: "self.event_emitter",
        reason: "typed AutomationEvent bus; the Tauri adapter emits a fixed classified name",
    },
    ReceiverFalsePositive {
        file: "src-tauri/src/application/automation/transition.rs",
        receiver: "self.event_emitter",
        reason: "typed AutomationEvent bus; the Tauri adapter emits a fixed classified name",
    },
];

/// Functions whose closed, literal/const return vocabulary is intentionally used as an event
/// name. This is visible in the manifest rather than treating the value as a dynamic exemption.
const STATIC_EVENT_FUNCTIONS: &[StaticEventFunction] = &[StaticEventFunction {
    function: "menu_event_name_for_id",
    names: &["ralphx://check-for-updates", "ralphx://show-release-notes"],
    reason: "native-menu ID mapping returns one of two local chrome event constants",
}];

/// Reviewed exceptions for names classified for remote delivery but which have no Tauri emit
/// producer in the current application. This is deliberately a closed list: either adding a
/// classification or landing its source emit requires a corresponding reviewed ledger update.
const UNMATCHED_EVENT_GAPS: &[ReviewedUnmatchedEvent] = &[
    ReviewedUnmatchedEvent::new(
        "execution:error",
        "no-tauri-emitter",
        "frontend consumer exists; no current Tauri event producer",
    ),
    ReviewedUnmatchedEvent::new(
        "file:change",
        "no-tauri-emitter",
        "frontend consumer exists; no current Tauri event producer",
    ),
    ReviewedUnmatchedEvent::new(
        "proposal:deleted",
        "no-tauri-emitter",
        "frontend consumer exists; no current Tauri event producer",
    ),
    ReviewedUnmatchedEvent::new(
        "qa:prep",
        "no-tauri-emitter",
        "frontend consumer exists; no current Tauri event producer",
    ),
    ReviewedUnmatchedEvent::new(
        "qa:test",
        "no-tauri-emitter",
        "frontend consumer exists; no current Tauri event producer",
    ),
    ReviewedUnmatchedEvent::new(
        "step:deleted",
        "no-tauri-emitter",
        "frontend consumer exists; no current Tauri event producer",
    ),
    ReviewedUnmatchedEvent::new(
        "step:status_changed",
        "no-tauri-emitter",
        "frontend consumer exists; no current Tauri event producer",
    ),
    ReviewedUnmatchedEvent::new(
        "steps:reordered",
        "no-tauri-emitter",
        "frontend consumer exists; no current Tauri event producer",
    ),
    ReviewedUnmatchedEvent::new(
        "supervisor:alert",
        "no-tauri-bridge",
        "frontend consumer exists; backend has internal Supervisor EventBus but no Tauri bridge",
    ),
    ReviewedUnmatchedEvent::new(
        "supervisor:event",
        "no-tauri-bridge",
        "frontend consumer exists; backend has internal Supervisor EventBus but no Tauri bridge",
    ),
    ReviewedUnmatchedEvent::new(
        "execution:stderr",
        "no-tauri-emitter",
        "frontend consumer exists; no current Tauri event producer",
    ),
];

#[derive(Clone, Copy)]
struct Wrapper {
    name: &'static str,
    event_arg: Option<usize>,
}

impl Wrapper {
    const fn function(name: &'static str, event_arg: Option<usize>) -> Self {
        Self { name, event_arg }
    }

    const fn method(name: &'static str, event_arg: Option<usize>) -> Self {
        Self { name, event_arg }
    }
}

#[derive(Clone, Copy)]
struct FalsePositive {
    function: &'static str,
    reason: &'static str,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
pub struct ReceiverFalsePositive {
    file: &'static str,
    receiver: &'static str,
    reason: &'static str,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
pub struct StaticEventFunction {
    function: &'static str,
    names: &'static [&'static str],
    reason: &'static str,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
pub struct ReviewedUnmatchedEvent {
    name: &'static str,
    reason_code: &'static str,
    reason: &'static str,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct ManifestFalsePositive {
    kind: String,
    target: String,
    reason: String,
}

impl ReviewedUnmatchedEvent {
    const fn new(name: &'static str, reason_code: &'static str, reason: &'static str) -> Self {
        Self {
            name,
            reason_code,
            reason,
        }
    }

    pub fn name(&self) -> &'static str {
        self.name
    }
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq, PartialOrd, Ord)]
pub struct EmitSite {
    pub name: String,
    pub file: String,
    pub line: usize,
    pub kind: &'static str,
}

#[derive(Debug, Serialize, PartialEq, Eq)]
pub struct Manifest {
    pub schema_version: u8,
    pub emitted: Vec<EmitSite>,
    pub consumed: Vec<String>,
    pub classified: Vec<String>,
    pub false_positive_allowlist: Vec<ManifestFalsePositive>,
    pub static_event_functions: Vec<StaticEventFunction>,
    pub unmatched_classified_events: Vec<ReviewedUnmatchedEvent>,
    /// Backend emit names the classification table deliberately does not carry into v1.
    ///
    /// `verify_manifest` only proves consumed ⊆ classified and classified ⇒ emitted, so without
    /// this section the emitted→classified direction is invisible: §3.4 mandates glob expansion
    /// (`task:*`, `team:*`, `ticketing:*` … Durable) while the seeded table narrows to
    /// UI-consumed ∪ explicitly enumerated names. Publishing the residue makes the narrowing
    /// auditable and diffable — the regenerate-and-diff staleness check turns any NEW
    /// unclassified backend emit into a CI failure that forces a recorded decision.
    pub unclassified_backend_emits: Vec<String>,
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum ScanError {
    #[error("{file}:{line}: unresolved event name in {function}")]
    UnresolvedEmit {
        file: String,
        line: usize,
        function: String,
    },
    #[error("{file}:{line}: wrapper {function} forwards an event-name parameter but is absent from WRAPPERS")]
    UnregisteredWrapper {
        file: String,
        line: usize,
        function: String,
    },
    #[error("{file}:{line}: wrapper {function} is called with an unresolved event-name argument")]
    UnresolvedWrapperCall {
        file: String,
        line: usize,
        function: String,
    },
    #[error("{file}:{line}: emit has no event-name argument")]
    MissingEventArgument { file: String, line: usize },
    #[error("{0}")]
    Parse(String),
}

pub fn scan_rust_source(file: impl Into<String>, source: &str) -> Result<Vec<EmitSite>, ScanError> {
    let file = file.into();
    let syntax =
        syn::parse_file(source).map_err(|error| ScanError::Parse(format!("{file}: {error}")))?;
    let constants = collect_constants_from_file(&syntax, "");
    let functions = collect_functions(&syntax, &file);
    let calls = collect_call_sites(&syntax, &file);
    verify_wrapper_contract(&constants, &functions, &calls)?;
    let mut visitor = EmitVisitor {
        file,
        constants: &constants,
        current_function: None,
        current_locals: BTreeMap::new(),
        sites: Vec::new(),
        error: None,
    };
    visitor.visit_file(&syntax);
    visitor.error.map_or_else(|| Ok(visitor.sites), Err)
}

pub fn build_manifest(root: &Path) -> Result<Manifest> {
    let rust_root = root.join("src-tauri/src");
    let emitted = scan_production_rust_tree(&rust_root)?
        .into_iter()
        .map(|mut site| {
            site.file = relative(root, &rust_root.join(&site.file));
            site
        })
        .collect::<Vec<_>>();
    let mut emitted = emitted;
    emitted.sort();
    emitted.dedup();

    let consumed = consumed_names(&root.join("frontend/src"))?;
    let classified = EVENT_CLASSIFICATIONS
        .iter()
        .map(|entry| entry.name.to_owned())
        .collect::<Vec<_>>();
    verify_manifest(&emitted, &consumed, EVENT_CLASSIFICATIONS)?;
    let classified_set = classified
        .iter()
        .map(String::as_str)
        .collect::<BTreeSet<_>>();
    let unclassified_backend_emits = emitted
        .iter()
        .map(|site| site.name.as_str())
        .filter(|name| !classified_set.contains(name))
        .map(ToOwned::to_owned)
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>();
    Ok(Manifest {
        schema_version: 1,
        emitted,
        consumed,
        classified,
        false_positive_allowlist: manifest_false_positives(),
        static_event_functions: STATIC_EVENT_FUNCTIONS.to_vec(),
        unmatched_classified_events: reviewed_unmatched_events(),
        unclassified_backend_emits,
    })
}

pub fn scan_production_rust_tree(root: &Path) -> Result<Vec<EmitSite>> {
    let mut source_files = Vec::new();
    let mut constants = ConstantTable::default();
    let mut functions = Vec::new();
    let mut calls = Vec::new();
    for path in production_rust_files(root)? {
        let file = relative(root, &path);
        let source =
            fs::read_to_string(&path).with_context(|| format!("read {}", path.display()))?;
        let syntax =
            syn::parse_file(&source).with_context(|| format!("parse {}", path.display()))?;
        constants.extend(collect_constants_from_file(
            &syntax,
            &module_path_for_file(root, &path),
        ));
        functions.extend(collect_functions(&syntax, &file));
        calls.extend(collect_call_sites(&syntax, &file));
        source_files.push((file, syntax));
    }
    verify_wrapper_contract(&constants, &functions, &calls).map_err(anyhow::Error::new)?;

    let mut emitted = Vec::new();
    for (file, syntax) in source_files {
        let mut visitor = EmitVisitor {
            file,
            constants: &constants,
            current_function: None,
            current_locals: BTreeMap::new(),
            sites: Vec::new(),
            error: None,
        };
        visitor.visit_file(&syntax);
        if let Some(error) = visitor.error {
            return Err(error.into());
        }
        emitted.extend(visitor.sites);
    }
    Ok(emitted)
}

/// Fails when a UI-consumed event name is absent from the classification table.
///
/// Exposed (rather than inlined into `verify_manifest`) so the failure direction PR 0.1's
/// acceptance criterion #5 claims — "removing any UI-consumed name from the classification table
/// makes the manifest test fail" — is unit-testable with injected slices.
pub fn verify_consumed_classification(
    consumed: &[String],
    classifications: &[EventClassification],
) -> Result<()> {
    let classified = classifications
        .iter()
        .map(|entry| entry.name)
        .collect::<BTreeSet<_>>();
    let missing_classifications = consumed
        .iter()
        .filter(|name| !classified.contains(name.as_str()))
        .cloned()
        .collect::<Vec<_>>();
    if !missing_classifications.is_empty() {
        bail!(
            "UI-consumed event names are unclassified: {}",
            missing_classifications.join(", ")
        );
    }
    Ok(())
}

fn verify_manifest(
    emitted: &[EmitSite],
    consumed: &[String],
    classifications: &[EventClassification],
) -> Result<()> {
    verify_consumed_classification(consumed, classifications)?;

    let emitted_names = emitted
        .iter()
        .map(|site| site.name.as_str())
        .collect::<BTreeSet<_>>();
    let missing_emits = classifications
        .iter()
        .filter(|entry| {
            !matches!(
                entry.delivery,
                ralphx_remote_protocol::EventDelivery::LocalOnly
            )
        })
        .filter(|entry| !emitted_names.contains(entry.name))
        .map(|entry| entry.name)
        .collect::<Vec<_>>();
    verify_unmatched_event_coverage(&missing_emits)?;
    Ok(())
}

pub fn reviewed_unmatched_events() -> Vec<ReviewedUnmatchedEvent> {
    UNMATCHED_EVENT_GAPS.to_vec()
}

fn manifest_false_positives() -> Vec<ManifestFalsePositive> {
    let mut entries = FALSE_POSITIVE_ALLOWLIST
        .iter()
        .map(|entry| ManifestFalsePositive {
            kind: "function".to_owned(),
            target: entry.function.to_owned(),
            reason: entry.reason.to_owned(),
        })
        .chain(
            RECEIVER_FALSE_POSITIVE_ALLOWLIST
                .iter()
                .map(|entry| ManifestFalsePositive {
                    kind: "receiver".to_owned(),
                    target: format!("{}::{}", entry.file, entry.receiver),
                    reason: entry.reason.to_owned(),
                }),
        )
        .collect::<Vec<_>>();
    entries.sort_by(|left, right| left.target.cmp(&right.target));
    entries
}

pub fn verify_unmatched_event_coverage(missing: &[&str]) -> Result<()> {
    let expected = UNMATCHED_EVENT_GAPS
        .iter()
        .map(|gap| gap.name)
        .collect::<BTreeSet<_>>();
    let actual = missing.iter().copied().collect::<BTreeSet<_>>();
    let unexpected = actual.difference(&expected).copied().collect::<Vec<_>>();
    let resolved = expected.difference(&actual).copied().collect::<Vec<_>>();
    if !unexpected.is_empty() {
        bail!(
            "classified Durable/Transient event names have no resolved Rust emit site and no reviewed gap entry: {}",
            unexpected.join(", ")
        );
    }
    if !resolved.is_empty() {
        bail!(
            "reviewed unmatched event entries now have a resolved Rust emit site; remove them from the ledger: {}",
            resolved.join(", ")
        );
    }
    Ok(())
}

#[derive(Debug, Clone, Default)]
struct ConstantTable {
    bindings: Vec<ConstantBinding>,
}

#[derive(Debug, Clone)]
struct ConstantBinding {
    qualified_name: String,
    value: String,
}

impl ConstantTable {
    fn extend(&mut self, other: Self) {
        self.bindings.extend(other.bindings);
    }

    fn resolve_path(&self, path: &syn::Path) -> Option<String> {
        let requested = path
            .segments
            .iter()
            .map(|segment| segment.ident.to_string())
            .filter(|segment| !matches!(segment.as_str(), "crate" | "self" | "super"))
            .collect::<Vec<_>>()
            .join("::");
        if requested.is_empty() {
            return None;
        }
        let exact = self
            .bindings
            .iter()
            .filter(|binding| binding.qualified_name == requested)
            .collect::<Vec<_>>();
        if exact.len() == 1 {
            return Some(exact[0].value.clone());
        }
        if exact.len() > 1 {
            return None;
        }

        let suffix = format!("::{requested}");
        let matches = self
            .bindings
            .iter()
            .filter(|binding| {
                binding.qualified_name.ends_with(&suffix)
                    || (requested.contains("::")
                        && requested.ends_with(&format!("::{}", binding.qualified_name)))
            })
            .collect::<Vec<_>>();
        (matches.len() == 1).then(|| matches[0].value.clone())
    }
}

fn collect_constants_from_file(file: &File, module_path: &str) -> ConstantTable {
    fn collect_items(items: &[Item], prefix: &str, table: &mut ConstantTable) {
        for item in items {
            match item {
                Item::Const(ItemConst { ident, expr, .. }) => {
                    if let Some(value) = literal(expr) {
                        let qualified_name = if prefix.is_empty() {
                            ident.to_string()
                        } else {
                            format!("{prefix}::{ident}")
                        };
                        table.bindings.push(ConstantBinding {
                            qualified_name,
                            value,
                        });
                    }
                }
                Item::Mod(module) if !is_cfg_test(&module.attrs) => {
                    if let Some((_, contents)) = &module.content {
                        let nested = if prefix.is_empty() {
                            module.ident.to_string()
                        } else {
                            format!("{prefix}::{}", module.ident)
                        };
                        collect_items(contents, &nested, table);
                    }
                }
                _ => {}
            }
        }
    }

    let mut table = ConstantTable::default();
    collect_items(&file.items, module_path, &mut table);
    table
}

#[derive(Debug, Clone)]
struct FunctionInfo {
    file: String,
    name: String,
    simple_name: String,
    emitted_param_indexes: BTreeSet<usize>,
}

fn collect_functions(file: &File, source_file: &str) -> Vec<FunctionInfo> {
    let mut functions = Vec::new();
    for item in &file.items {
        match item {
            Item::Fn(function) => functions.push(function_info(function, None, source_file)),
            Item::Impl(implementation) => {
                let type_name = impl_type_name(&implementation.self_ty);
                for member in &implementation.items {
                    if let ImplItem::Fn(function) = member {
                        functions.push(impl_function_info(
                            function,
                            type_name.as_deref(),
                            source_file,
                        ));
                    }
                }
            }
            _ => {}
        }
    }
    functions
}

fn function_info(function: &ItemFn, owner: Option<&str>, source_file: &str) -> FunctionInfo {
    function_info_from_parts(
        owner,
        function.sig.ident.to_string(),
        &function.sig.inputs,
        &function.block,
        function.span().start().line,
        source_file,
    )
}

fn impl_function_info(
    function: &syn::ImplItemFn,
    owner: Option<&str>,
    source_file: &str,
) -> FunctionInfo {
    function_info_from_parts(
        owner,
        function.sig.ident.to_string(),
        &function.sig.inputs,
        &function.block,
        function.span().start().line,
        source_file,
    )
}

fn function_info_from_parts(
    owner: Option<&str>,
    simple_name: String,
    inputs: &syn::punctuated::Punctuated<syn::FnArg, syn::token::Comma>,
    body: &syn::Block,
    line: usize,
    source_file: &str,
) -> FunctionInfo {
    let param_names = inputs
        .iter()
        .map(|input| match input {
            syn::FnArg::Receiver(_) => None,
            syn::FnArg::Typed(argument) => match peel_pat(&argument.pat) {
                Pat::Ident(identifier) => Some(identifier.ident.to_string()),
                _ => None,
            },
        })
        .collect::<Vec<_>>();
    let param_set = param_names
        .iter()
        .flatten()
        .cloned()
        .collect::<BTreeSet<_>>();
    let mut flow = ParameterEmitFlow {
        params: &param_set,
        names: BTreeSet::new(),
    };
    flow.visit_block(body);
    let emitted_param_indexes = param_names
        .iter()
        .enumerate()
        .filter_map(|(index, name)| {
            name.as_ref()
                .filter(|name| flow.names.contains(*name))
                .map(|_| index)
        })
        .collect();
    let name = owner.map_or_else(
        || simple_name.clone(),
        |owner| format!("{owner}::{simple_name}"),
    );
    let _ = line;
    FunctionInfo {
        file: source_file.to_owned(),
        name,
        simple_name,
        emitted_param_indexes,
    }
}

fn impl_type_name(ty: &syn::Type) -> Option<String> {
    match ty {
        syn::Type::Path(path) => path
            .path
            .segments
            .last()
            .map(|segment| segment.ident.to_string()),
        _ => None,
    }
}

struct ParameterEmitFlow<'a> {
    params: &'a BTreeSet<String>,
    names: BTreeSet<String>,
}

impl<'ast> Visit<'ast> for ParameterEmitFlow<'_> {
    fn visit_expr_method_call(&mut self, node: &'ast ExprMethodCall) {
        if node.method == "emit" {
            if let Some(Expr::Path(path)) = node.args.first().map(peel) {
                if let Some(identifier) = path.path.get_ident() {
                    if self.params.contains(&identifier.to_string()) {
                        self.names.insert(identifier.to_string());
                    }
                }
            }
        }
        visit::visit_expr_method_call(self, node);
    }
}

fn verify_wrapper_contract(
    constants: &ConstantTable,
    functions: &[FunctionInfo],
    calls: &[CallSite],
) -> Result<(), ScanError> {
    for function in functions
        .iter()
        .filter(|function| !function.emitted_param_indexes.is_empty())
    {
        for call in calls
            .iter()
            .filter(|call| call.name == function.simple_name)
        {
            for index in &function.emitted_param_indexes {
                let Some(argument) = call.args.get(*index) else {
                    continue;
                };
                let line = call.line;
                if wrapper_for_function(function).is_some() {
                    continue;
                }
                if resolve_name(argument, constants).is_none() {
                    return Err(ScanError::UnresolvedWrapperCall {
                        file: call.file.clone(),
                        line,
                        function: function.name.clone(),
                    });
                }
                if !is_false_positive(function) {
                    return Err(ScanError::UnregisteredWrapper {
                        file: function.file.clone(),
                        line,
                        function: function.name.clone(),
                    });
                }
            }
        }
    }
    Ok(())
}

struct CallSite {
    file: String,
    name: String,
    args: Vec<Expr>,
    line: usize,
}

struct CallCollector<'a> {
    file: &'a str,
    calls: &'a mut Vec<CallSite>,
}

impl<'ast> Visit<'ast> for CallCollector<'ast> {
    fn visit_item_mod(&mut self, node: &'ast syn::ItemMod) {
        if !is_cfg_test(&node.attrs) {
            visit::visit_item_mod(self, node);
        }
    }

    fn visit_expr_call(&mut self, node: &'ast ExprCall) {
        if let Expr::Path(path) = peel(&node.func) {
            if let Some(segment) = path.path.segments.last() {
                self.calls.push(CallSite {
                    file: self.file.to_owned(),
                    name: segment.ident.to_string(),
                    args: node.args.iter().cloned().collect(),
                    line: node.span().start().line,
                });
            }
        }
        visit::visit_expr_call(self, node);
    }
}

fn collect_call_sites(syntax: &File, file: &str) -> Vec<CallSite> {
    let mut calls = Vec::new();
    CallCollector {
        file,
        calls: &mut calls,
    }
    .visit_file(syntax);
    calls
}

/// Any registered method wrapper whose method name matches, ignoring the owning type.
///
/// `syn` cannot resolve a receiver's type, so the wrapper match is by method name plus the
/// enclosing-impl guard below. This helper exposes the name-only half so a call that matches the
/// name but fails the guard can be treated as ambiguous instead of silently skipped.
fn method_wrapper_by_name(method: &str) -> Option<&'static Wrapper> {
    WRAPPERS.iter().find(|wrapper| {
        wrapper
            .name
            .rsplit_once("::")
            .is_some_and(|(_, name)| name == method)
    })
}

fn wrapper_for_method(method: &str, current_function: Option<&str>) -> Option<&'static Wrapper> {
    method_wrapper_by_name(method).filter(|_| {
        // `emit_event` is not unique: `ExternalMcpSupervisor` and the rule-ingestion service
        // both define one whose first argument is a status string, not an event name. Only
        // `AppChatService`'s forwards an event name, so the wrapper applies inside that impl.
        method != "emit_event"
            || current_function.is_some_and(|function| function.starts_with("AppChatService::"))
    })
}

/// True for a `self.…` / `Self::…` receiver.
fn is_self_receiver(receiver: &Expr) -> bool {
    matches!(peel(receiver), Expr::Path(path) if path.path.is_ident("self"))
}

fn wrapper_for_function(function: &FunctionInfo) -> Option<&'static Wrapper> {
    WRAPPERS
        .iter()
        .find(|wrapper| wrapper.name == function.name || wrapper.name == function.simple_name)
}

fn is_false_positive(function: &FunctionInfo) -> bool {
    FALSE_POSITIVE_ALLOWLIST.iter().any(|entry| {
        debug_assert!(!entry.reason.is_empty());
        entry.function == function.name
    })
}

struct EmitVisitor<'a> {
    file: String,
    constants: &'a ConstantTable,
    current_function: Option<String>,
    current_locals: BTreeMap<String, BTreeSet<String>>,
    sites: Vec<EmitSite>,
    error: Option<ScanError>,
}

impl EmitVisitor<'_> {
    fn record(
        &mut self,
        expr: &Expr,
        receiver: Option<&Expr>,
        span: proc_macro2::Span,
        kind: &'static str,
    ) {
        if self.error.is_some() {
            return;
        }
        let names = resolve_names(expr, self.constants, &self.current_locals);
        if !names.is_empty() {
            for name in names {
                self.sites.push(EmitSite {
                    name,
                    file: self.file.clone(),
                    line: span.start().line,
                    kind,
                });
            }
        } else if !self.current_function_is_known_wrapper_or_false_positive()
            && !receiver.is_some_and(|receiver| self.is_allowlisted_receiver(receiver))
        {
            self.error = Some(ScanError::UnresolvedEmit {
                file: self.file.clone(),
                line: span.start().line,
                function: self
                    .current_function
                    .clone()
                    .unwrap_or_else(|| "<module>".into()),
            });
        }
    }

    fn current_function_is_known_wrapper_or_false_positive(&self) -> bool {
        self.current_function.as_deref().is_some_and(|name| {
            WRAPPERS.iter().any(|wrapper| wrapper.name == name)
                || FALSE_POSITIVE_ALLOWLIST
                    .iter()
                    .any(|entry| entry.function == name && !entry.reason.is_empty())
        })
    }

    fn is_allowlisted_receiver(&self, receiver: &Expr) -> bool {
        let Some(identity) = receiver_identity(receiver) else {
            return false;
        };
        RECEIVER_FALSE_POSITIVE_ALLOWLIST.iter().any(|entry| {
            debug_assert!(!entry.reason.is_empty());
            entry.file.ends_with(&self.file) && entry.receiver == identity
        })
    }

    fn enter_function(
        &mut self,
        name: String,
        locals: BTreeMap<String, BTreeSet<String>>,
        visit: impl FnOnce(&mut Self),
    ) {
        let previous = self.current_function.replace(name);
        let previous_locals = std::mem::replace(&mut self.current_locals, locals);
        visit(self);
        self.current_function = previous;
        self.current_locals = previous_locals;
    }
}

impl<'ast> Visit<'ast> for EmitVisitor<'_> {
    fn visit_item_mod(&mut self, node: &'ast syn::ItemMod) {
        if !is_cfg_test(&node.attrs) {
            visit::visit_item_mod(self, node);
        }
    }

    fn visit_item_fn(&mut self, node: &'ast ItemFn) {
        self.enter_function(
            node.sig.ident.to_string(),
            static_local_names(&node.block, self.constants),
            |visitor| visit::visit_item_fn(visitor, node),
        );
    }

    fn visit_item_impl(&mut self, node: &'ast syn::ItemImpl) {
        let owner = impl_type_name(&node.self_ty);
        for member in &node.items {
            if let ImplItem::Fn(function) = member {
                let name = owner.as_ref().map_or_else(
                    || function.sig.ident.to_string(),
                    |owner| format!("{owner}::{}", function.sig.ident),
                );
                self.enter_function(
                    name,
                    static_local_names(&function.block, self.constants),
                    |visitor| visit::visit_impl_item_fn(visitor, function),
                );
            } else {
                self.visit_impl_item(member);
            }
        }
    }

    fn visit_expr_method_call(&mut self, node: &'ast ExprMethodCall) {
        if node.method == "emit" {
            if let Some(event) = node.args.first() {
                self.record(event, Some(&node.receiver), node.span(), "method");
            } else if self.error.is_none() {
                self.error = Some(ScanError::MissingEventArgument {
                    file: self.file.clone(),
                    line: node.span().start().line,
                });
            }
        } else if let Some(wrapper) =
            wrapper_for_method(&node.method.to_string(), self.current_function.as_deref())
        {
            if let Some(index) = wrapper.event_arg {
                if let Some(event) = node.args.iter().nth(index) {
                    self.record(event, Some(&node.receiver), node.span(), "wrapper_method");
                } else if self.error.is_none() {
                    self.error = Some(ScanError::MissingEventArgument {
                        file: self.file.clone(),
                        line: node.span().start().line,
                    });
                }
            }
        } else if let Some(wrapper) = method_wrapper_by_name(&node.method.to_string()) {
            // The method name matches a registered event-forwarding wrapper but the
            // enclosing-impl guard rejected it. On a `self` receiver that is the intended
            // same-name-different-type case (`ExternalMcpSupervisor::emit_event`). On any other
            // receiver the callee's type is unknown, so a genuine cross-object wrapper call
            // would silently vanish from the census — fail closed instead, matching the
            // over-approximate-or-fail posture of every other emit shape.
            if wrapper.event_arg.is_some()
                && !is_self_receiver(&node.receiver)
                && self.error.is_none()
            {
                self.error = Some(ScanError::UnresolvedEmit {
                    file: self.file.clone(),
                    line: node.span().start().line,
                    function: format!(
                        "cross-object `{}` call: wrapper receiver type is unresolvable",
                        node.method
                    ),
                });
            }
        }
        visit::visit_expr_method_call(self, node);
    }

    fn visit_expr_call(&mut self, node: &'ast ExprCall) {
        if let Expr::Path(path) = peel(&node.func) {
            if let Some(name) = path
                .path
                .segments
                .last()
                .map(|segment| segment.ident.to_string())
            {
                if let Some(wrapper) = WRAPPERS.iter().find(|wrapper| wrapper.name == name) {
                    if let Some(index) = wrapper.event_arg {
                        if let Some(event) = node.args.iter().nth(index) {
                            self.record(event, None, node.span(), "wrapper");
                        } else if self.error.is_none() {
                            self.error = Some(ScanError::MissingEventArgument {
                                file: self.file.clone(),
                                line: node.span().start().line,
                            });
                        }
                    }
                }
            }
        }
        visit::visit_expr_call(self, node);
    }
}

fn resolve_name(expr: &Expr, constants: &ConstantTable) -> Option<String> {
    literal(expr).or_else(|| match peel(expr) {
        Expr::Path(path) => (!path.path.segments.is_empty())
            .then(|| constants.resolve_path(&path.path))
            .flatten(),
        Expr::Reference(reference) => resolve_name(&reference.expr, constants),
        _ => None,
    })
}

fn resolve_names(
    expr: &Expr,
    constants: &ConstantTable,
    locals: &BTreeMap<String, BTreeSet<String>>,
) -> BTreeSet<String> {
    if let Expr::Path(path) = peel(expr) {
        if let Some(identifier) = path.path.get_ident() {
            if let Some(names) = locals.get(&identifier.to_string()) {
                return names.clone();
            }
        }
    }
    resolve_name(expr, constants).into_iter().collect()
}

fn static_local_names(
    block: &syn::Block,
    constants: &ConstantTable,
) -> BTreeMap<String, BTreeSet<String>> {
    struct Locals {
        bindings: Vec<(Pat, Expr)>,
    }
    impl<'ast> Visit<'ast> for Locals {
        fn visit_local(&mut self, node: &'ast syn::Local) {
            if let Some(initializer) = &node.init {
                self.bindings
                    .push((node.pat.clone(), (*initializer.expr).clone()));
            }
            visit::visit_local(self, node);
        }

        fn visit_expr_let(&mut self, node: &'ast syn::ExprLet) {
            self.bindings
                .push(((*node.pat).clone(), (*node.expr).clone()));
            visit::visit_expr_let(self, node);
        }
    }
    let mut collector = Locals {
        bindings: Vec::new(),
    };
    collector.visit_block(block);
    let mut values = BTreeMap::new();
    for _ in 0..collector.bindings.len() {
        let mut changed = false;
        for (pattern, expression) in &collector.bindings {
            for (identifier, names) in
                static_pattern_bindings(pattern, expression, constants, &values)
            {
                if names.is_empty() {
                    continue;
                }
                changed |= values.get(&identifier) != Some(&names);
                values.insert(identifier, names.clone());
            }
        }
        if !changed {
            break;
        }
    }
    values
}

fn static_expression_names(
    expression: &Expr,
    constants: &ConstantTable,
    locals: &BTreeMap<String, BTreeSet<String>>,
) -> BTreeSet<String> {
    match peel(expression) {
        Expr::Lit(_) | Expr::Reference(_) | Expr::Path(_) => {
            resolve_names(expression, constants, locals)
        }
        Expr::If(expression) => {
            let mut names = static_block_names(&expression.then_branch, constants, locals);
            if let Some((_, otherwise)) = &expression.else_branch {
                names.extend(static_expression_names(otherwise, constants, locals));
            }
            names
        }
        Expr::Block(expression) => static_block_names(&expression.block, constants, locals),
        Expr::Match(expression) => expression
            .arms
            .iter()
            .flat_map(|arm| static_expression_names(&arm.body, constants, locals))
            .collect(),
        Expr::Tuple(expression) => expression
            .elems
            .first()
            .map(|element| static_expression_names(element, constants, locals))
            .unwrap_or_default(),
        Expr::Call(expression) if expression.args.len() == 1 => {
            let function = match peel(&expression.func) {
                Expr::Path(path) => path
                    .path
                    .segments
                    .last()
                    .map(|segment| segment.ident.to_string()),
                _ => None,
            };
            if function.as_deref() == Some("Some") {
                static_expression_names(
                    expression.args.first().expect("one checked argument"),
                    constants,
                    locals,
                )
            } else if let Some(output) = STATIC_EVENT_FUNCTIONS
                .iter()
                .find(|output| Some(output.function) == function.as_deref())
            {
                debug_assert!(!output.reason.is_empty());
                output.names.iter().map(|name| (*name).to_owned()).collect()
            } else {
                BTreeSet::new()
            }
        }
        Expr::MethodCall(expression)
            if expression.method == "map" && expression.args.len() == 1 =>
        {
            static_expression_names(
                expression.args.first().expect("one checked argument"),
                constants,
                locals,
            )
        }
        Expr::Closure(expression) => static_expression_names(&expression.body, constants, locals),
        _ => BTreeSet::new(),
    }
}

fn static_block_names(
    block: &syn::Block,
    constants: &ConstantTable,
    locals: &BTreeMap<String, BTreeSet<String>>,
) -> BTreeSet<String> {
    block
        .stmts
        .last()
        .map_or_else(BTreeSet::new, |statement| match statement {
            syn::Stmt::Expr(expression, _) => {
                static_expression_names(expression, constants, locals)
            }
            _ => BTreeSet::new(),
        })
}

fn static_pattern_bindings(
    pattern: &Pat,
    expression: &Expr,
    constants: &ConstantTable,
    locals: &BTreeMap<String, BTreeSet<String>>,
) -> Vec<(String, BTreeSet<String>)> {
    match pattern {
        Pat::Ident(identifier) => vec![(
            identifier.ident.to_string(),
            static_expression_names(expression, constants, locals),
        )],
        Pat::Type(pattern) => static_pattern_bindings(&pattern.pat, expression, constants, locals),
        Pat::Paren(pattern) => static_pattern_bindings(&pattern.pat, expression, constants, locals),
        Pat::Reference(pattern) => {
            static_pattern_bindings(&pattern.pat, expression, constants, locals)
        }
        Pat::Or(pattern) => pattern
            .cases
            .iter()
            .flat_map(|case| static_pattern_bindings(case, expression, constants, locals))
            .collect(),
        Pat::Tuple(tuple) => match peel(expression) {
            Expr::Tuple(values) => tuple
                .elems
                .iter()
                .zip(values.elems.iter())
                .flat_map(|(pattern, value)| {
                    static_pattern_bindings(pattern, value, constants, locals)
                })
                .collect(),
            _ => tuple
                .elems
                .iter()
                .flat_map(|pattern| static_pattern_bindings(pattern, expression, constants, locals))
                .collect(),
        },
        Pat::TupleStruct(tuple) => match peel(expression) {
            Expr::Tuple(values) => tuple
                .elems
                .iter()
                .zip(values.elems.iter())
                .flat_map(|(pattern, value)| {
                    static_pattern_bindings(pattern, value, constants, locals)
                })
                .collect(),
            _ => tuple
                .elems
                .iter()
                .flat_map(|pattern| static_pattern_bindings(pattern, expression, constants, locals))
                .collect(),
        },
        _ => Vec::new(),
    }
}

fn peel(expr: &Expr) -> &Expr {
    match expr {
        Expr::Paren(expression) => peel(&expression.expr),
        Expr::Group(expression) => peel(&expression.expr),
        _ => expr,
    }
}

fn peel_pat(pattern: &Pat) -> &Pat {
    match pattern {
        Pat::Type(pattern) => peel_pat(&pattern.pat),
        _ => pattern,
    }
}

fn receiver_identity(expr: &Expr) -> Option<&'static str> {
    match peel(expr) {
        Expr::Field(field)
            if matches!(peel(&field.base), Expr::Path(path) if path.path.is_ident("self"))
                && matches!(&field.member, syn::Member::Named(name) if name == "event_emitter") =>
        {
            Some("self.event_emitter")
        }
        _ => None,
    }
}

fn literal(expr: &Expr) -> Option<String> {
    match peel(expr) {
        Expr::Lit(expression) => match &expression.lit {
            Lit::Str(value) => Some(value.value()),
            _ => None,
        },
        _ => None,
    }
}

fn files_with_extension(root: &Path, extension: &str) -> Result<Vec<PathBuf>> {
    let mut paths = WalkDir::new(root)
        .into_iter()
        .filter_map(|entry| entry.ok())
        .filter(|entry| entry.file_type().is_file())
        .filter(|entry| {
            entry
                .path()
                .extension()
                .is_some_and(|value| value == extension)
        })
        .map(|entry| entry.into_path())
        .collect::<Vec<_>>();
    paths.sort();
    Ok(paths)
}

fn production_rust_files(root: &Path) -> Result<Vec<PathBuf>> {
    Ok(files_with_extension(root, "rs")?
        .into_iter()
        .filter(|path| {
            let file_name = path
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or_default();
            !file_name.ends_with("_tests.rs")
                && !path
                    .components()
                    .any(|component| component.as_os_str() == "tests")
        })
        .collect())
}

fn is_cfg_test(attributes: &[syn::Attribute]) -> bool {
    attributes.iter().any(|attribute| {
        attribute.path().is_ident("cfg")
            && attribute
                .parse_args::<syn::Ident>()
                .is_ok_and(|identifier| identifier == "test")
    })
}

fn relative(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .display()
        .to_string()
}

fn module_path_for_file(root: &Path, path: &Path) -> String {
    let mut parts = path
        .strip_prefix(root)
        .unwrap_or(path)
        .components()
        .filter_map(|component| component.as_os_str().to_str())
        .map(ToOwned::to_owned)
        .collect::<Vec<_>>();
    let Some(file_name) = parts.pop() else {
        return String::new();
    };
    let stem = Path::new(&file_name)
        .file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or_default();
    if !matches!(stem, "lib" | "main" | "mod") {
        parts.push(stem.to_owned());
    }
    parts.join("::")
}

/// Scans a frontend source root (`frontend/src`) for UI-consumed event names.
///
/// Exposed so the import-resolution and fail-closed behaviours are testable against a real
/// directory layout rather than only through the full manifest build.
pub fn scan_consumed_tree(frontend_src: &Path) -> Result<Vec<String>> {
    consumed_names(frontend_src)
}

fn consumed_names(root: &Path) -> Result<Vec<String>> {
    let mut names = BTreeSet::new();
    let shared_constants = frontend_event_constants(root)?;
    let mut module_cache = BTreeMap::new();
    for extension in ["ts", "tsx"] {
        for path in files_with_extension(root, extension)? {
            if path
                .file_name()
                .is_some_and(|name| name.to_string_lossy().contains(".test."))
            {
                continue;
            }
            let source = fs::read_to_string(&path)?;
            if !source.contains(".subscribe") {
                continue;
            }
            let mut constants = shared_constants.clone();
            constants.extend(
                imported_event_constants(root, &path, &source, &mut module_cache)
                    .with_context(|| format!("resolve imports of {}", path.display()))?,
            );
            names.extend(
                scan_consumed_source_with_constants(
                    &path.display().to_string(),
                    &source,
                    &constants,
                )
                .with_context(|| format!("scan {}", path.display()))?,
            );
        }
    }
    Ok(names.into_iter().collect())
}

/// Resolves event-name constants a consumer imports from another frontend module.
///
/// The consumed scan is fail-closed: an unresolvable `subscribe(<ident>)` argument is a hard
/// error, not a skip. Before the bus migration every consumed name was a string literal, a
/// file-local const, or a `lib/events.ts` const; migrated consumers now import them from feature
/// modules (`AgentTerminalDrawer.tsx` takes `AGENT_TERMINAL_EVENT` from `api/terminal.ts`), so
/// the scan follows one level of import rather than turning a legal migration into a build break.
fn imported_event_constants(
    root: &Path,
    file: &Path,
    source: &str,
    cache: &mut BTreeMap<PathBuf, BTreeMap<String, Vec<String>>>,
) -> Result<BTreeMap<String, Vec<String>>> {
    let mut resolved = BTreeMap::new();
    let tree = parse_tsx(source, &file.display().to_string())?;
    let mut imports = Vec::new();
    collect_import_bindings(tree.root_node(), source, &mut imports);
    for (specifier, bindings) in imports {
        let Some(module_path) = resolve_frontend_module(root, file, &specifier) else {
            continue;
        };
        if !cache.contains_key(&module_path) {
            let module_source = fs::read_to_string(&module_path)
                .with_context(|| format!("read {}", module_path.display()))?;
            let module_tree = parse_tsx(&module_source, &module_path.display().to_string())?;
            let constants = collect_ts_constants(module_tree.root_node(), &module_source);
            cache.insert(module_path.clone(), constants);
        }
        let constants = &cache[&module_path];
        for (exported, local) in bindings {
            if let Some(values) = constants.get(&exported) {
                resolved.insert(local, values.clone());
            }
        }
    }
    Ok(resolved)
}

/// Maps a TS module specifier to a file under the frontend source root.
///
/// Handles the `@/` alias and relative specifiers only; bare package specifiers resolve to
/// `node_modules` and can never define a RalphX event name.
fn resolve_frontend_module(root: &Path, file: &Path, specifier: &str) -> Option<PathBuf> {
    let base = if let Some(rest) = specifier.strip_prefix("@/") {
        root.join(rest)
    } else if specifier.starts_with("./") || specifier.starts_with("../") {
        file.parent()?.join(specifier)
    } else {
        return None;
    };
    ["ts", "tsx"]
        .into_iter()
        .map(|extension| base.with_extension(extension))
        .chain(
            ["index.ts", "index.tsx"]
                .into_iter()
                .map(|entry| base.join(entry)),
        )
        .find(|candidate| candidate.is_file())
}

/// Collects `(module specifier, [(exported name, local binding)])` for every named import.
fn collect_import_bindings(node: Node<'_>, source: &str, output: &mut Vec<(String, Vec<Names>)>) {
    if node.kind() == "import_statement" {
        if let Some(specifier) = node
            .child_by_field_name("source")
            .and_then(|child| string_value(child, source))
        {
            let mut bindings = Vec::new();
            collect_import_specifiers(node, source, &mut bindings);
            if !bindings.is_empty() {
                output.push((specifier, bindings));
            }
        }
    }
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        collect_import_bindings(child, source, output);
    }
}

/// `(exported name, local binding)` — they differ under `import { A as B }`.
type Names = (String, String);

fn collect_import_specifiers(node: Node<'_>, source: &str, output: &mut Vec<Names>) {
    if node.kind() == "import_specifier" {
        if let Some(name) = node.child_by_field_name("name") {
            let exported = node_text(name, source).to_owned();
            let local = node.child_by_field_name("alias").map_or_else(
                || exported.clone(),
                |alias| node_text(alias, source).to_owned(),
            );
            output.push((exported, local));
        }
        return;
    }
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        collect_import_specifiers(child, source, output);
    }
}

fn parse_tsx(source: &str, label: &str) -> Result<tree_sitter::Tree> {
    let mut parser = Parser::new();
    parser
        .set_language(&LANGUAGE_TSX.into())
        .map_err(|error| anyhow::anyhow!("{label}: configure TSX parser: {error}"))?;
    let tree = parser
        .parse(source, None)
        .ok_or_else(|| anyhow::anyhow!("{label}: TSX parser returned no tree"))?;
    if tree.root_node().has_error() {
        bail!("{label}: invalid TS/TSX source")
    }
    Ok(tree)
}

pub fn scan_consumed_source(file: &str, source: &str) -> Result<Vec<String>> {
    scan_consumed_source_with_constants(file, source, &BTreeMap::new())
}

fn scan_consumed_source_with_constants(
    file: &str,
    source: &str,
    shared_constants: &BTreeMap<String, Vec<String>>,
) -> Result<Vec<String>> {
    let tree = parse_tsx(source, file)?;
    let mut constants = shared_constants.clone();
    constants.extend(collect_ts_constants(tree.root_node(), source));
    let mut values = BTreeSet::new();
    collect_subscriptions(
        tree.root_node(),
        source,
        file,
        &constants,
        &BTreeMap::new(),
        &mut values,
    )?;
    Ok(values.into_iter().collect())
}

fn frontend_event_constants(root: &Path) -> Result<BTreeMap<String, Vec<String>>> {
    let path = root.join("lib/events.ts");
    let source = fs::read_to_string(&path).with_context(|| format!("read {}", path.display()))?;
    let tree = parse_tsx(&source, &path.display().to_string())?;
    Ok(collect_ts_constants(tree.root_node(), &source))
}

fn collect_ts_constants(root: Node<'_>, source: &str) -> BTreeMap<String, Vec<String>> {
    let mut values = BTreeMap::new();
    let mut cursor = root.walk();
    for child in root.children(&mut cursor) {
        collect_ts_constants_inner(child, source, &mut values);
    }
    values
}

fn collect_ts_constants_inner(
    node: Node<'_>,
    source: &str,
    values: &mut BTreeMap<String, Vec<String>>,
) {
    if node.kind() == "variable_declarator" {
        if let (Some(name), Some(value)) = (
            node.child_by_field_name("name"),
            node.child_by_field_name("value"),
        ) {
            if name.kind() == "identifier" {
                if let Some(events) = static_event_values(value, source) {
                    values.insert(node_text(name, source).to_owned(), events);
                }
            }
        }
    }
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        collect_ts_constants_inner(child, source, values);
    }
}

fn collect_subscriptions(
    node: Node<'_>,
    source: &str,
    file: &str,
    constants: &BTreeMap<String, Vec<String>>,
    mapped_values: &BTreeMap<String, Vec<String>>,
    output: &mut BTreeSet<String>,
) -> Result<()> {
    if node.kind() == "call_expression" {
        if let Some((list_name, callback)) = static_map_call(node, source) {
            if !contains_event_bus_subscribe(callback, source) {
                let mut cursor = node.walk();
                for child in node.children(&mut cursor) {
                    collect_subscriptions(child, source, file, constants, mapped_values, output)?;
                }
                return Ok(());
            }
            let events = constants.get(list_name).ok_or_else(|| {
                anyhow::anyhow!(
                    "{file}:{} mapped event list `{list_name}` is not a static literal array",
                    node.start_position().row + 1
                )
            })?;
            let parameter = callback_parameter(callback, source).ok_or_else(|| {
                anyhow::anyhow!(
                    "{file}:{} mapped event callback must declare one identifier parameter",
                    callback.start_position().row + 1
                )
            })?;
            let mut mapped = mapped_values.clone();
            mapped.insert(parameter, events.clone());
            let mut cursor = callback.walk();
            for child in callback.children(&mut cursor) {
                collect_subscriptions(child, source, file, constants, &mapped, output)?;
            }
            return Ok(());
        }
        if subscribe_receiver(node, source) == Some(SubscribeReceiver::Unknown) {
            let receiver = node
                .child_by_field_name("function")
                .and_then(|function| function.child_by_field_name("object"))
                .map_or_else(String::new, |object| node_text(object, source).to_owned());
            bail!(
                "{file}:{} unrecognised `.subscribe(` receiver `{receiver}`: name event-bus \
                 receivers `bus`/`eventBus` (or call `useEventBus()` inline) so consumed event \
                 names stay auditable, or add the receiver to FOREIGN_SUBSCRIBE_ALLOWLIST with \
                 a reason",
                node.start_position().row + 1
            );
        }
        if is_event_bus_subscribe(node, source) {
            let argument = first_argument(node).ok_or_else(|| {
                anyhow::anyhow!(
                    "{file}:{} EventBus.subscribe requires an event-name argument",
                    node.start_position().row + 1
                )
            })?;
            let events =
                event_values(argument, source, constants, mapped_values).ok_or_else(|| {
                    anyhow::anyhow!(
                        "{file}:{} unresolved EventBus subscription argument `{}`",
                        node.start_position().row + 1,
                        node_text(argument, source)
                    )
                })?;
            output.extend(events);
        }
    }

    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        collect_subscriptions(child, source, file, constants, mapped_values, output)?;
    }
    Ok(())
}

fn contains_event_bus_subscribe(node: Node<'_>, source: &str) -> bool {
    if node.kind() == "call_expression" && is_event_bus_subscribe(node, source) {
        return true;
    }
    let mut cursor = node.walk();
    let contains = node
        .children(&mut cursor)
        .any(|child| contains_event_bus_subscribe(child, source));
    contains
}

fn static_map_call<'a>(node: Node<'a>, source: &'a str) -> Option<(&'a str, Node<'a>)> {
    let function = node.child_by_field_name("function")?;
    if function.kind() != "member_expression" || member_property(function, source) != Some("map") {
        return None;
    }
    let object = function.child_by_field_name("object")?;
    if object.kind() != "identifier" {
        return None;
    }
    let arguments = node.child_by_field_name("arguments")?;
    let callback = named_children(arguments)
        .into_iter()
        .find(|child| child.kind() == "arrow_function")?;
    Some((node_text(object, source), callback))
}

fn callback_parameter(callback: Node<'_>, source: &str) -> Option<String> {
    let parameters = callback.child_by_field_name("parameters")?;
    let parameter = named_children(parameters).into_iter().next()?;
    let pattern = if parameter.kind() == "required_parameter" {
        parameter.child_by_field_name("pattern")?
    } else {
        parameter
    };
    (pattern.kind() == "identifier").then(|| node_text(pattern, source).to_owned())
}

/// Receiver expressions that are known NOT to be the app event bus.
///
/// Reviewed entries only. A genuine non-event-bus `subscribe` (a store, an observable) belongs
/// here with a reason; anything else hard-fails so a renamed or destructured bus cannot silently
/// escape the consumed-name census.
const FOREIGN_SUBSCRIBE_ALLOWLIST: &[(&str, &str)] = &[
    (
        "queryClient.getQueryCache()",
        "TanStack Query cache subscription (useSyncExternalStore), carries no event name",
    ),
    (
        "useUiStore",
        "Zustand store selector subscription; the listener receives (state, previous), never an event name",
    ),
    (
        "useEnvironmentStore",
        "Zustand store selector subscription; the listener receives (state, previous), never an event name",
    ),
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SubscribeReceiver {
    /// The app event bus (`bus`/`eventBus`, or a direct `useEventBus()`/`getEventBus()` call).
    EventBus,
    /// A reviewed non-event-bus `subscribe` receiver; deliberately ignored.
    Foreign,
    /// A `.subscribe(` whose receiver the scan cannot prove is not the event bus.
    Unknown,
}

/// Classifies a `.subscribe(` call site.
///
/// Fail-closed by design (mirrors the emit side's over-approximate-or-fail contract): an
/// unrecognised receiver is reported as [`SubscribeReceiver::Unknown`] and hard-fails the scan,
/// because a renamed/destructured bus that silently escaped the census is exactly the
/// "unclassified UI-consumed event name = undefined behaviour" hole this gate exists to close.
fn subscribe_receiver(node: Node<'_>, source: &str) -> Option<SubscribeReceiver> {
    let function = node.child_by_field_name("function")?;
    if function.kind() != "member_expression"
        || member_property(function, source) != Some("subscribe")
    {
        return None;
    }
    let object = function.child_by_field_name("object")?;
    let text = node_text(object, source);
    if matches!(text, "bus" | "eventBus" | "useEventBus()" | "getEventBus()") {
        return Some(SubscribeReceiver::EventBus);
    }
    if FOREIGN_SUBSCRIBE_ALLOWLIST
        .iter()
        .any(|(receiver, reason)| {
            debug_assert!(!reason.is_empty());
            *receiver == text
        })
    {
        return Some(SubscribeReceiver::Foreign);
    }
    Some(SubscribeReceiver::Unknown)
}

fn is_event_bus_subscribe(node: Node<'_>, source: &str) -> bool {
    subscribe_receiver(node, source) == Some(SubscribeReceiver::EventBus)
}

fn member_property<'a>(node: Node<'a>, source: &'a str) -> Option<&'a str> {
    node.child_by_field_name("property")
        .map(|property| node_text(property, source))
}

fn first_argument(node: Node<'_>) -> Option<Node<'_>> {
    let arguments = node.child_by_field_name("arguments")?;
    named_children(arguments).into_iter().next()
}

fn event_values(
    argument: Node<'_>,
    source: &str,
    constants: &BTreeMap<String, Vec<String>>,
    mapped_values: &BTreeMap<String, Vec<String>>,
) -> Option<Vec<String>> {
    static_event_values(argument, source).or_else(|| {
        (argument.kind() == "identifier").then(|| {
            let name = node_text(argument, source);
            mapped_values
                .get(name)
                .or_else(|| constants.get(name))
                .cloned()
        })?
    })
}

fn static_event_values(node: Node<'_>, source: &str) -> Option<Vec<String>> {
    match node.kind() {
        "string" => string_value(node, source).map(|value| vec![value]),
        "array" => named_children(node)
            .into_iter()
            .map(|child| string_value(child, source))
            .collect(),
        "as_expression" | "parenthesized_expression" => named_children(node)
            .into_iter()
            .next()
            .and_then(|expression| static_event_values(expression, source)),
        _ => None,
    }
}

fn string_value(node: Node<'_>, source: &str) -> Option<String> {
    if node.kind() != "string" {
        return None;
    }
    let text = node_text(node, source);
    let quote = text.chars().next()?;
    let end = text.strip_prefix(quote)?.strip_suffix(quote)?;
    (!end.contains("${")).then(|| end.to_owned())
}

fn named_children(node: Node<'_>) -> Vec<Node<'_>> {
    let mut cursor = node.walk();
    node.named_children(&mut cursor).collect()
}

fn node_text<'a>(node: Node<'_>, source: &'a str) -> &'a str {
    &source[node.byte_range()]
}
