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
    ReviewedUnmatchedEvent::new("execution:error", "no-tauri-emitter", "execution error state is surfaced through query invalidation, not a Tauri emit"),
    ReviewedUnmatchedEvent::new("file:change", "no-tauri-emitter", "file changes are consumed from watcher state without a Tauri emit"),
    ReviewedUnmatchedEvent::new("proposal:deleted", "no-tauri-emitter", "proposal deletion has no current Tauri event producer"),
    ReviewedUnmatchedEvent::new("qa:prep", "no-tauri-emitter", "QA preparation state has no current Tauri event producer"),
    ReviewedUnmatchedEvent::new("qa:test", "no-tauri-emitter", "QA test state has no current Tauri event producer"),
    ReviewedUnmatchedEvent::new("step:deleted", "no-tauri-emitter", "step deletion has no current Tauri event producer"),
    ReviewedUnmatchedEvent::new("step:status_changed", "no-tauri-emitter", "step status changes have no current Tauri event producer"),
    ReviewedUnmatchedEvent::new("steps:reordered", "no-tauri-emitter", "step reordering has no current Tauri event producer"),
    ReviewedUnmatchedEvent::new("supervisor:alert", "no-tauri-emitter", "supervisor alerts have no current Tauri event producer"),
    ReviewedUnmatchedEvent::new("supervisor:event", "no-tauri-emitter", "supervisor events have no current Tauri event producer"),
    ReviewedUnmatchedEvent::new("execution:stderr", "no-tauri-emitter", "execution stderr is consumed from process state without a Tauri emit"),
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
        Self { name, reason_code, reason }
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
    let syntax = syn::parse_file(source).map_err(|error| ScanError::Parse(format!("{file}: {error}")))?;
    let constants = collect_constants_from_file(&syntax);
    let functions = collect_functions(&syntax);
    verify_wrapper_contract(&file, &syntax, &constants, &functions)?;
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
    let constants = collect_rust_constants(&rust_root)?;
    let mut emitted = Vec::new();
    for path in files_with_extension(&rust_root, "rs")? {
        let source = fs::read_to_string(&path).with_context(|| format!("read {}", path.display()))?;
        let syntax = syn::parse_file(&source).with_context(|| format!("parse {}", path.display()))?;
        let functions = collect_functions(&syntax);
        let file = relative(root, &path);
        verify_wrapper_contract(&file, &syntax, &constants, &functions).map_err(anyhow::Error::new)?;
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
    emitted.sort();
    emitted.dedup();

    let consumed = consumed_names(&root.join("frontend/src"))?;
    let classified = EVENT_CLASSIFICATIONS
        .iter()
        .map(|entry| entry.name.to_owned())
        .collect::<Vec<_>>();
    verify_manifest(&emitted, &consumed, EVENT_CLASSIFICATIONS)?;
    Ok(Manifest {
        schema_version: 1,
        emitted,
        consumed,
        classified,
        false_positive_allowlist: manifest_false_positives(),
        static_event_functions: STATIC_EVENT_FUNCTIONS.to_vec(),
        unmatched_classified_events: reviewed_unmatched_events(),
    })
}

fn verify_manifest(
    emitted: &[EmitSite],
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
        bail!("UI-consumed event names are unclassified: {}", missing_classifications.join(", "));
    }

    let emitted_names = emitted.iter().map(|site| site.name.as_str()).collect::<BTreeSet<_>>();
    let missing_emits = classifications
        .iter()
        .filter(|entry| !matches!(entry.delivery, ralphx_remote_protocol::EventDelivery::LocalOnly))
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
        .chain(RECEIVER_FALSE_POSITIVE_ALLOWLIST.iter().map(|entry| ManifestFalsePositive {
            kind: "receiver".to_owned(),
            target: format!("{}::{}", entry.file, entry.receiver),
            reason: entry.reason.to_owned(),
        }))
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

fn collect_rust_constants(root: &Path) -> Result<BTreeMap<String, String>> {
    let mut values = BTreeMap::new();
    for path in files_with_extension(root, "rs")? {
        let source = fs::read_to_string(&path)?;
        let syntax = syn::parse_file(&source)?;
        merge_constants(&mut values, collect_constants_from_file(&syntax));
    }
    Ok(values)
}

fn collect_constants_from_file(file: &File) -> BTreeMap<String, String> {
    struct Constants(BTreeMap<String, String>);
    impl<'ast> Visit<'ast> for Constants {
        fn visit_item_const(&mut self, node: &'ast ItemConst) {
            if let Some(value) = literal(&node.expr) {
                self.0.insert(node.ident.to_string(), value);
            }
            visit::visit_item_const(self, node);
        }
    }
    let mut visitor = Constants(BTreeMap::new());
    visitor.visit_file(file);
    visitor.0
}

fn merge_constants(into: &mut BTreeMap<String, String>, next: BTreeMap<String, String>) {
    for (name, value) in next {
        if let Some(previous) = into.get(&name) {
            if previous != &value {
                into.remove(&name);
            }
        } else {
            into.insert(name, value);
        }
    }
}

#[derive(Debug, Clone)]
struct FunctionInfo {
    name: String,
    simple_name: String,
    emitted_param_indexes: BTreeSet<usize>,
}

fn collect_functions(file: &File) -> Vec<FunctionInfo> {
    let mut functions = Vec::new();
    for item in &file.items {
        match item {
            Item::Fn(function) => functions.push(function_info(function, None)),
            Item::Impl(implementation) => {
                let type_name = impl_type_name(&implementation.self_ty);
                for member in &implementation.items {
                    if let ImplItem::Fn(function) = member {
                        functions.push(impl_function_info(function, type_name.as_deref()));
                    }
                }
            }
            _ => {}
        }
    }
    functions
}

fn function_info(function: &ItemFn, owner: Option<&str>) -> FunctionInfo {
    function_info_from_parts(
        owner,
        function.sig.ident.to_string(),
        &function.sig.inputs,
        &function.block,
        function.span().start().line,
    )
}

fn impl_function_info(function: &syn::ImplItemFn, owner: Option<&str>) -> FunctionInfo {
    function_info_from_parts(
        owner,
        function.sig.ident.to_string(),
        &function.sig.inputs,
        &function.block,
        function.span().start().line,
    )
}

fn function_info_from_parts(
    owner: Option<&str>,
    simple_name: String,
    inputs: &syn::punctuated::Punctuated<syn::FnArg, syn::token::Comma>,
    body: &syn::Block,
    line: usize,
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
    let param_set = param_names.iter().flatten().cloned().collect::<BTreeSet<_>>();
    let mut flow = ParameterEmitFlow {
        params: &param_set,
        names: BTreeSet::new(),
    };
    flow.visit_block(body);
    let emitted_param_indexes = param_names
        .iter()
        .enumerate()
        .filter_map(|(index, name)| name.as_ref().filter(|name| flow.names.contains(*name)).map(|_| index))
        .collect();
    let name = owner.map_or_else(|| simple_name.clone(), |owner| format!("{owner}::{simple_name}"));
    let _ = line;
    FunctionInfo { name, simple_name, emitted_param_indexes }
}

fn impl_type_name(ty: &syn::Type) -> Option<String> {
    match ty {
        syn::Type::Path(path) => path.path.segments.last().map(|segment| segment.ident.to_string()),
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
    file: &str,
    syntax: &File,
    constants: &BTreeMap<String, String>,
    functions: &[FunctionInfo],
) -> Result<(), ScanError> {
    let mut calls = Vec::new();
    {
        let mut collector = CallCollector { calls: &mut calls };
        collector.visit_file(syntax);
    }
    for function in functions.iter().filter(|function| !function.emitted_param_indexes.is_empty()) {
        for call in calls.iter().filter(|call| call.name == function.simple_name) {
            for index in &function.emitted_param_indexes {
                let Some(argument) = call.args.get(*index) else { continue };
                let line = call.line;
                if wrapper_for_function(function).is_some() {
                    continue;
                }
                if resolve_name(argument, constants).is_none() {
                    return Err(ScanError::UnresolvedWrapperCall {
                        file: file.to_owned(),
                        line,
                        function: function.name.clone(),
                    });
                }
                if !is_false_positive(function) {
                    return Err(ScanError::UnregisteredWrapper {
                        file: file.to_owned(),
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
    name: String,
    args: Vec<Expr>,
    line: usize,
}

struct CallCollector<'a> {
    calls: &'a mut Vec<CallSite>,
}

impl<'ast> Visit<'ast> for CallCollector<'ast> {
    fn visit_expr_call(&mut self, node: &'ast ExprCall) {
        if let Expr::Path(path) = peel(&node.func) {
            if let Some(segment) = path.path.segments.last() {
                self.calls.push(CallSite {
                    name: segment.ident.to_string(),
                    args: node.args.iter().cloned().collect(),
                    line: node.span().start().line,
                });
            }
        }
        visit::visit_expr_call(self, node);
    }
}

fn wrapper_for_method(method: &str, current_function: Option<&str>) -> Option<&'static Wrapper> {
    WRAPPERS.iter().find(|wrapper| {
        wrapper
            .name
            .rsplit_once("::")
            .is_some_and(|(_, name)| name == method)
            && (method != "emit_event"
                || current_function.is_some_and(|function| function.starts_with("AppChatService::")))
    })
}

fn wrapper_for_function(function: &FunctionInfo) -> Option<&'static Wrapper> {
    WRAPPERS.iter().find(|wrapper| wrapper.name == function.name || wrapper.name == function.simple_name)
}

fn is_false_positive(function: &FunctionInfo) -> bool {
    FALSE_POSITIVE_ALLOWLIST.iter().any(|entry| {
        debug_assert!(!entry.reason.is_empty());
        entry.function == function.name
    })
}

struct EmitVisitor<'a> {
    file: String,
    constants: &'a BTreeMap<String, String>,
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
                function: self.current_function.clone().unwrap_or_else(|| "<module>".into()),
            });
        }
    }

    fn current_function_is_known_wrapper_or_false_positive(&self) -> bool {
        self.current_function.as_deref().is_some_and(|name| {
            WRAPPERS.iter().any(|wrapper| wrapper.name == name)
                || FALSE_POSITIVE_ALLOWLIST.iter().any(|entry| entry.function == name && !entry.reason.is_empty())
        })
    }

    fn is_allowlisted_receiver(&self, receiver: &Expr) -> bool {
        let Some(identity) = receiver_identity(receiver) else {
            return false;
        };
        RECEIVER_FALSE_POSITIVE_ALLOWLIST.iter().any(|entry| {
            debug_assert!(!entry.reason.is_empty());
            entry.file == self.file && entry.receiver == identity
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
        }
        visit::visit_expr_method_call(self, node);
    }

    fn visit_expr_call(&mut self, node: &'ast ExprCall) {
        if let Expr::Path(path) = peel(&node.func) {
            if let Some(name) = path.path.segments.last().map(|segment| segment.ident.to_string()) {
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

fn resolve_name(expr: &Expr, constants: &BTreeMap<String, String>) -> Option<String> {
    literal(expr).or_else(|| match peel(expr) {
        Expr::Path(path) => path
            .path
            .segments
            .last()
            .and_then(|segment| constants.get(&segment.ident.to_string()))
            .cloned(),
        Expr::Reference(reference) => resolve_name(&reference.expr, constants),
        _ => None,
    })
}

fn resolve_names(
    expr: &Expr,
    constants: &BTreeMap<String, String>,
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
    constants: &BTreeMap<String, String>,
) -> BTreeMap<String, BTreeSet<String>> {
    struct Locals {
        bindings: Vec<(Pat, Expr)>,
    }
    impl<'ast> Visit<'ast> for Locals {
        fn visit_local(&mut self, node: &'ast syn::Local) {
            if let Some(initializer) = &node.init {
                self.bindings.push((node.pat.clone(), (*initializer.expr).clone()));
            }
            visit::visit_local(self, node);
        }

        fn visit_expr_let(&mut self, node: &'ast syn::ExprLet) {
            self.bindings.push(((*node.pat).clone(), (*node.expr).clone()));
            visit::visit_expr_let(self, node);
        }
    }
    let mut collector = Locals { bindings: Vec::new() };
    collector.visit_block(block);
    let mut values = BTreeMap::new();
    for _ in 0..collector.bindings.len() {
        let mut changed = false;
        for (pattern, expression) in &collector.bindings {
            for (identifier, names) in static_pattern_bindings(pattern, expression, constants, &values) {
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
    constants: &BTreeMap<String, String>,
    locals: &BTreeMap<String, BTreeSet<String>>,
) -> BTreeSet<String> {
    match peel(expression) {
        Expr::Lit(_) | Expr::Reference(_) | Expr::Path(_) => resolve_names(expression, constants, locals),
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
                Expr::Path(path) => path.path.segments.last().map(|segment| segment.ident.to_string()),
                _ => None,
            };
            if function.as_deref() == Some("Some") {
                static_expression_names(expression.args.first().expect("one checked argument"), constants, locals)
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
        Expr::MethodCall(expression) if expression.method == "map" && expression.args.len() == 1 => {
            static_expression_names(expression.args.first().expect("one checked argument"), constants, locals)
        }
        Expr::Closure(expression) => static_expression_names(&expression.body, constants, locals),
        _ => BTreeSet::new(),
    }
}

fn static_block_names(
    block: &syn::Block,
    constants: &BTreeMap<String, String>,
    locals: &BTreeMap<String, BTreeSet<String>>,
) -> BTreeSet<String> {
    block.stmts.last().map_or_else(BTreeSet::new, |statement| match statement {
        syn::Stmt::Expr(expression, _) => static_expression_names(expression, constants, locals),
        _ => BTreeSet::new(),
    })
}

fn static_pattern_bindings(
    pattern: &Pat,
    expression: &Expr,
    constants: &BTreeMap<String, String>,
    locals: &BTreeMap<String, BTreeSet<String>>,
) -> Vec<(String, BTreeSet<String>)> {
    match pattern {
        Pat::Ident(identifier) => vec![(
            identifier.ident.to_string(),
            static_expression_names(expression, constants, locals),
        )],
        Pat::Type(pattern) => static_pattern_bindings(&pattern.pat, expression, constants, locals),
        Pat::Paren(pattern) => static_pattern_bindings(&pattern.pat, expression, constants, locals),
        Pat::Reference(pattern) => static_pattern_bindings(&pattern.pat, expression, constants, locals),
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
                .flat_map(|(pattern, value)| static_pattern_bindings(pattern, value, constants, locals))
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
                .flat_map(|(pattern, value)| static_pattern_bindings(pattern, value, constants, locals))
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
        .filter(|entry| entry.path().extension().is_some_and(|value| value == extension))
        .map(|entry| entry.into_path())
        .collect::<Vec<_>>();
    paths.sort();
    Ok(paths)
}

fn relative(root: &Path, path: &Path) -> String {
    path.strip_prefix(root).unwrap_or(path).display().to_string()
}

fn consumed_names(root: &Path) -> Result<Vec<String>> {
    let mut names = BTreeSet::new();
    let shared_constants = frontend_event_constants(root)?;
    for extension in ["ts", "tsx"] {
        for path in files_with_extension(root, extension)? {
            if path.file_name().is_some_and(|name| name.to_string_lossy().contains(".test.")) {
                continue;
            }
            let source = fs::read_to_string(&path)?;
            if !source.contains(".subscribe") {
                continue;
            }
            names.extend(
                scan_consumed_source_with_constants(&path.display().to_string(), &source, &shared_constants)
                    .with_context(|| format!("scan {}", path.display()))?,
            );
        }
    }
    Ok(names.into_iter().collect())
}

pub fn scan_consumed_source(file: &str, source: &str) -> Result<Vec<String>> {
    scan_consumed_source_with_constants(file, source, &BTreeMap::new())
}

fn scan_consumed_source_with_constants(
    file: &str,
    source: &str,
    shared_constants: &BTreeMap<String, Vec<String>>,
) -> Result<Vec<String>> {
    let mut parser = Parser::new();
    parser
        .set_language(&LANGUAGE_TSX.into())
        .map_err(|error| anyhow::anyhow!("{file}: configure TSX parser: {error}"))?;
    let tree = parser
        .parse(source, None)
        .ok_or_else(|| anyhow::anyhow!("{file}: TSX parser returned no tree"))?;
    if tree.root_node().has_error() {
        bail!("{file}: invalid TS/TSX source")
    }
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
    let mut parser = Parser::new();
    parser
        .set_language(&LANGUAGE_TSX.into())
        .map_err(|error| anyhow::anyhow!("configure TSX parser: {error}"))?;
    let tree = parser
        .parse(&source, None)
        .ok_or_else(|| anyhow::anyhow!("parse {} returned no tree", path.display()))?;
    if tree.root_node().has_error() {
        bail!("{}: invalid TypeScript event constants", path.display());
    }
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
        if let (Some(name), Some(value)) = (node.child_by_field_name("name"), node.child_by_field_name("value")) {
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
                anyhow::anyhow!("{file}:{} mapped event list `{list_name}` is not a static literal array", node.start_position().row + 1)
            })?;
            let parameter = callback_parameter(callback, source).ok_or_else(|| {
                anyhow::anyhow!("{file}:{} mapped event callback must declare one identifier parameter", callback.start_position().row + 1)
            })?;
            let mut mapped = mapped_values.clone();
            mapped.insert(parameter, events.clone());
            let mut cursor = callback.walk();
            for child in callback.children(&mut cursor) {
                collect_subscriptions(child, source, file, constants, &mapped, output)?;
            }
            return Ok(());
        }
        if is_event_bus_subscribe(node, source) {
            let argument = first_argument(node).ok_or_else(|| {
                anyhow::anyhow!("{file}:{} EventBus.subscribe requires an event-name argument", node.start_position().row + 1)
            })?;
            let events = event_values(argument, source, constants, mapped_values).ok_or_else(|| {
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
    let callback = named_children(arguments).into_iter().find(|child| child.kind() == "arrow_function")?;
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

fn is_event_bus_subscribe(node: Node<'_>, source: &str) -> bool {
    let Some(function) = node.child_by_field_name("function") else { return false };
    if function.kind() != "member_expression" || member_property(function, source) != Some("subscribe") {
        return false;
    }
    let Some(object) = function.child_by_field_name("object") else { return false };
    matches!(node_text(object, source), "bus" | "eventBus")
}

fn member_property<'a>(node: Node<'a>, source: &'a str) -> Option<&'a str> {
    node.child_by_field_name("property").map(|property| node_text(property, source))
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
            mapped_values.get(name).or_else(|| constants.get(name)).cloned()
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
