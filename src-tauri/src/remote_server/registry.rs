//! The remote invoke facade registry (PR 1.3, §3.3).
//!
//! The remote command surface is an **explicit, hand-audited allowlist** — never derived from
//! the frontend. A command absent from [`remote_commands!`] is unreachable remotely by
//! construction: `dispatch` returns `REMOTE_COMMAND_UNAVAILABLE` and there is no opt-out.
//!
//! # What the compile gate proves (and what it does not)
//!
//! Every registration emits `const _: () = assert!(class_permits(CLASS, CAPS));`. That proves
//! **declared consistency only**: a command declared `SpawnsProcess` cannot also be declared
//! `Read`/`Operate`. It does **not** catch under-labeling — `class_permits(Operate, &[])`
//! succeeds for a command mislabeled capability-free. Correctness against under-labeling rests
//! on the hand-audited [`super::capability_ledger`] plus the independent
//! [`super::authority_audit`] detectors, whose output is a CI-enforced floor (P-17, N3-H1).
//!
//! # Runtime parameterisation (PR 1.5 resolution of the 1.3 deviation)
//!
//! [`dispatch`] stays generic over `R: tauri::Runtime` so the P-4 parity suite and the whole
//! P-17b scope suite can drive the *production* dispatch path under `tauri::test::MockRuntime`
//! rather than a test-only fork of it (A-7: no forked command fns).
//!
//! PR 1.3 recorded the consequence as a blocker: the `(app_handle)` injection form yields
//! `AppHandle<R>`, so the ~115 commands whose signature demands the Wry-monomorphic
//! `tauri::AppHandle` — including the `block_task`/`pause_tasks_in_group` brakes — were
//! unregistrable. PR 1.5 needs them, and **monomorphising `dispatch` on `Wry` is not a usable
//! resolution on this platform**: building a Wry `AppHandle` in a test panics with
//! `On macOS, EventLoop must be created on the main thread!` (this is the standing
//! `remote_server::listener_tests` failure — it reproduces under `cargo nextest` too, because
//! libtest runs the test body on a spawned thread). Monomorphising would therefore make every
//! dispatch-path test unrunnable and silently delete the facade's only authorization coverage.
//!
//! The resolution is the [`(host_app_handle)`](remote_commands) injection arm: the facade keeps
//! its generic dispatch and resolves the concrete Wry handle from `AppState::app_handle`, which
//! the host populates at startup. This mirrors the `:3847` HTTP surface, which already resolves
//! the same handle the same way (`http_server/handlers/git.rs` `build_transition_service`), and
//! it fails CLOSED: when no handle is managed the request is refused with
//! `REMOTE_INTERNAL_ERROR` instead of taking a degraded path.

use ralphx_remote_protocol::{Capability, ErrorCode, RiskClass, Scope};
use serde_json::Value;

/// An argument-sensitive authorization predicate, evaluated on parsed args BEFORE dispatch.
///
/// Returns the scope the *specific request* requires, which may exceed the command's class
/// scope (the `update_task` field-level predicate is the canonical user, §3.3).
pub type AuthzPredicate = fn(&Value) -> Scope;

/// A validation predicate, evaluated after authorization and before dispatch.
pub type ValidatePredicate = fn(&Value) -> Result<(), String>;

/// A server-minted constant bound into a target fn's input, replacing whatever the client sent.
///
/// This is what makes one dual-decision command safe to expose as two single-purpose facade ops
/// (P-1): `deny_permission_request` pins `decision = "deny"` into `ResolvePermissionArgs`, so a
/// request carrying `"decision": "allow"` still denies. The declaration below is the ONLY source
/// of the pinned value — [`extract_pinned_arg`] reads `spec.pins` at dispatch time, so a pin
/// cannot be declared in the manifest yet absent from the wire path, or vice versa.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PinnedField {
    /// The target fn's parameter whose (struct) input carries the field.
    pub param: &'static str,
    /// The field inside that input which is server-controlled.
    pub field: &'static str,
    /// The constant written into it.
    pub value: &'static str,
}

/// One row of the remote allowlist.
#[derive(Clone)]
pub struct RemoteCommandSpec {
    pub name: &'static str,
    /// Fully-qualified path of the existing Tauri command fn. The facade references existing
    /// fns only — never a fork (A-7) — and P-3 walks these paths against the transition
    /// denylist.
    pub target: &'static str,
    pub class: RiskClass,
    pub capabilities: &'static [Capability],
    pub authz: Option<AuthzPredicate>,
    pub validate: Option<ValidatePredicate>,
    /// Server-pinned input fields (see [`PinnedField`]). Empty for ordinary registrations.
    pub pins: &'static [PinnedField],
}

/// The scope a risk class requires before any dispatch happens.
pub const fn scope_for_class(class: RiskClass) -> Option<Scope> {
    match class {
        RiskClass::Read => Some(Scope::UiRead),
        RiskClass::Operate | RiskClass::PathScoped => Some(Scope::UiOperate),
        RiskClass::AgentControl => Some(Scope::UiAgent),
        RiskClass::Elevated => Some(Scope::UiElevated),
        RiskClass::Denied => None,
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoteInvokeError {
    pub code: ErrorCode,
    pub message: String,
}

impl RemoteInvokeError {
    pub fn unavailable(command: &str) -> Self {
        Self {
            code: ErrorCode::RemoteCommandUnavailable,
            message: format!("Command `{command}` is not available remotely."),
        }
    }

    pub fn forbidden(message: impl Into<String>) -> Self {
        Self {
            code: ErrorCode::RemoteForbidden,
            message: message.into(),
        }
    }

    /// A registered command whose arguments would not deserialize. NOT
    /// `RemoteCommandUnavailable`: that code is reserved for a `find_spec` miss and the client
    /// treats it as "this host does not support the command at all", which is terminal and
    /// about to gate remote affordances.
    fn bad_args(message: impl Into<String>) -> Self {
        Self {
            code: ErrorCode::RemoteInvalidArguments,
            message: message.into(),
        }
    }

    /// A host-side fault: the request was well-formed and authorized, but the host could not
    /// assemble what the target fn needs. Never a statement about the client's request, and
    /// never a path that lets the command run with a substitute.
    pub fn internal(message: impl Into<String>) -> Self {
        Self {
            code: ErrorCode::RemoteInternalError,
            message: message.into(),
        }
    }
}

/// Outcome of a dispatch, mirroring the wire envelope.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DispatchOutcome {
    /// The command returned `Ok(T)`; the value is `serde_json::to_value(T)` — byte-identical
    /// to what Tauri's IPC layer serialises for the same return type.
    Ok(Value),
    /// The command returned `Err(E)`; `E` is rendered exactly as Tauri renders it.
    Err(Value),
}

// ---------------------------------------------------------------------------------------
// Argument extraction — parity with Tauri IPC deserialization (P-4 / C-11)
// ---------------------------------------------------------------------------------------

/// `project_id` → `projectId`. Tauri accepts either form for flat params, so the facade must
/// too, or a client written against the local IPC contract breaks remotely.
pub fn camel_case(name: &str) -> String {
    let mut out = String::with_capacity(name.len());
    let mut upper_next = false;
    for ch in name.chars() {
        if ch == '_' {
            upper_next = true;
            continue;
        }
        if upper_next {
            out.extend(ch.to_uppercase());
            upper_next = false;
        } else {
            out.push(ch);
        }
    }
    out
}

/// Extracts one argument by its exact declared name, falling back to the camelCase alias.
///
/// A missing key is deserialized from `null`, which is what makes `Option<T>` params behave
/// identically to the local IPC path (absent ⇒ `None`) while a missing required param still
/// produces a typed error rather than a panic.
pub fn extract_arg<T: serde::de::DeserializeOwned>(
    args: &Value,
    name: &str,
) -> Result<T, RemoteInvokeError> {
    let raw = args
        .get(name)
        .or_else(|| args.get(camel_case(name)))
        .cloned()
        .unwrap_or(Value::Null);
    serde_json::from_value(raw)
        .map_err(|error| RemoteInvokeError::bad_args(format!("Invalid argument `{name}`: {error}")))
}

/// Extracts a struct argument, then OVERWRITES its server-pinned fields.
///
/// The overwrite is unconditional and happens after the client's object is taken, so a
/// client-supplied value for a pinned field is discarded rather than merged. `pins` is filtered
/// to the requested parameter, so one registration can pin fields in several inputs.
///
/// A missing input deserializes from an empty object rather than `null`, which is what lets a
/// fully-pinned struct be invoked with no client args at all.
pub fn extract_pinned_arg<T: serde::de::DeserializeOwned>(
    args: &Value,
    name: &str,
    pins: &[PinnedField],
) -> Result<T, RemoteInvokeError> {
    let mut raw = args
        .get(name)
        .or_else(|| args.get(camel_case(name)))
        .cloned()
        .unwrap_or_else(|| Value::Object(serde_json::Map::new()));
    let object = raw.as_object_mut().ok_or_else(|| {
        RemoteInvokeError::bad_args(format!("Argument `{name}` must be an object"))
    })?;
    let mut applied = 0usize;
    for pin in pins.iter().filter(|pin| pin.param == name) {
        object.insert(pin.field.to_string(), Value::String(pin.value.to_string()));
        applied += 1;
    }
    if applied == 0 {
        // A `pinned_arg` param whose spec declares no pin for it would silently degrade into an
        // ordinary client-controlled argument — exactly the dual-decision shape this mechanism
        // exists to remove. Refuse instead.
        return Err(RemoteInvokeError::internal(format!(
            "Argument `{name}` is registered as pinned but the specification declares no pin for it"
        )));
    }
    serde_json::from_value(raw)
        .map_err(|error| RemoteInvokeError::bad_args(format!("Invalid argument `{name}`: {error}")))
}

/// Serialises a command's success value exactly as the Tauri IPC layer does.
pub fn serialize_ok<T: serde::Serialize>(value: T) -> Result<Value, RemoteInvokeError> {
    // A host-side serialization fault, not a statement about the command's availability.
    serde_json::to_value(value).map_err(|error| RemoteInvokeError {
        code: ErrorCode::RemoteInternalError,
        message: format!("Response could not be serialized: {error}"),
    })
}

// ---------------------------------------------------------------------------------------
// The registration macro
// ---------------------------------------------------------------------------------------

/// Registers Tauri commands on the remote invoke facade.
///
/// Expansion contract (§3.3):
/// * (a) args are extracted by exact name AND camelCase form; struct params bind under the
///   param name (`invoke("cmd", { input: {...} })`);
/// * (b) injected params resolve against the **fixed injection table** — any other extractor
///   form has no macro arm and fails compilation;
/// * (c) `const _: () = assert!(class_permits(class, caps))`;
/// * (d) fail-closed scope enforcement (incl. `ui:agent` for `AgentControl`) then `authz:`
///   then `validate:` — all strictly before dispatch;
/// * (e) `Channel`/request-body params are rejected structurally at macro-expansion time;
/// * (f) `Ok(T)` serialises byte-identically to Tauri IPC;
/// * (g) `Err(E)` maps to `{ ok: false, error }`.
///
/// A legal class/capability pair compiles through the complete registration shape:
///
/// ```
/// async fn legal_target() {}
///
/// ralphx_lib::remote_commands! {
///     "legal_fixture" => legal_target {
///         class: Elevated,
///         caps: [ConfiguresFutureProcessAuthority],
///         params: [],
///         call: async,
///         result: infallible,
///     },
/// }
/// ```
///
/// The macro-emitted const assertion rejects capabilities unavailable to the declared class:
///
/// ```compile_fail
/// async fn capability_mismatch_target() {}
///
/// ralphx_lib::remote_commands! {
///     "capability_mismatch_fixture" => capability_mismatch_target {
///         class: Operate,
///         caps: [ConfiguresFutureProcessAuthority],
///         params: [],
///         call: async,
///         result: infallible,
///     },
/// }
/// ```
///
/// `Denied` is unregistrable even with an empty capability set:
///
/// ```compile_fail
/// async fn denied_target() {}
///
/// ralphx_lib::remote_commands! {
///     "denied_fixture" => denied_target {
///         class: Denied,
///         caps: [],
///         params: [],
///         call: async,
///         result: infallible,
///     },
/// }
/// ```
#[macro_export]
macro_rules! remote_commands {
    (
        $(
            $name:literal => $target:path {
                class: $class:ident,
                caps: [ $( $cap:ident ),* $(,)? ],
                params: [ $( $param:tt ),* $(,)? ],
                call: $call:ident,
                result: $result:ident
                $(, pins: [ $( ($pin_param:literal, $pin_field:literal, $pin_value:literal) ),* $(,)? ] )?
                $(, authz: $authz:expr )?
                $(, validate: $validate:expr )?
                $(,)?
            }
        ),* $(,)?
    ) => {
        /// Every registered command, in declaration order.
        pub static REMOTE_COMMANDS: &[$crate::remote_server::registry::RemoteCommandSpec] = &[
            $(
                {
                    // (c) Declared-consistency gate. `class_permits` is a `const fn`, so a
                    // forbidden class/capability pair is a compile error, not a test failure.
                    const _: () = assert!(
                        ::ralphx_remote_protocol::class_permits(
                            ::ralphx_remote_protocol::RiskClass::$class,
                            &[ $( ::ralphx_remote_protocol::Capability::$cap ),* ],
                        ),
                        concat!(
                            "remote_commands!: `", $name,
                            "` declares a capability its risk class does not permit"
                        )
                    );
                    // A `Denied` command must never be registrable at all.
                    const _: () = assert!(
                        $crate::remote_server::registry::scope_for_class(
                            ::ralphx_remote_protocol::RiskClass::$class
                        ).is_some(),
                        concat!("remote_commands!: `", $name, "` is Denied and cannot be registered")
                    );
                    $( $crate::remote_commands!(@reject_forbidden_param $param); )*
                    $crate::remote_server::registry::RemoteCommandSpec {
                        name: $name,
                        target: stringify!($target),
                        class: ::ralphx_remote_protocol::RiskClass::$class,
                        capabilities: &[ $( ::ralphx_remote_protocol::Capability::$cap ),* ],
                        authz: $crate::remote_commands!(@authz $( $authz )?),
                        validate: $crate::remote_commands!(@validate $( $validate )?),
                        pins: &[ $($(
                            $crate::remote_server::registry::PinnedField {
                                param: $pin_param,
                                field: $pin_field,
                                value: $pin_value,
                            }
                        ),*)? ],
                    }
                }
            ),*
        ];

        /// Dispatches one `{ cmd, args }` pair against the registered allowlist.
        ///
        /// `granted` is the device's scope set; enforcement happens here, before the target
        /// fn is reached, so an unauthorized request never touches business state.
        pub async fn dispatch<R: ::tauri::Runtime>(
            app: &::tauri::AppHandle<R>,
            granted: &[::ralphx_remote_protocol::Scope],
            cmd: &str,
            args: &::serde_json::Value,
        ) -> ::std::result::Result<
            $crate::remote_server::registry::DispatchOutcome,
            $crate::remote_server::registry::RemoteInvokeError,
        > {
            #[allow(unused_imports)]
            use ::tauri::Manager as _;

            let spec = $crate::remote_server::registry::find_spec(cmd)
                .ok_or_else(|| $crate::remote_server::registry::RemoteInvokeError::unavailable(cmd))?;
            // (d) scope → authz → validate, all before dispatch.
            $crate::remote_server::registry::enforce_scope(spec, granted, args)?;
            if let Some(validate) = spec.validate {
                validate(args).map_err(|message| {
                    $crate::remote_server::registry::RemoteInvokeError::forbidden(message)
                })?;
            }

            match cmd {
                $(
                    $name => {
                        let outcome = $crate::remote_commands!(
                            @invoke app, args, spec, $target, $call, $result, [ $( $param ),* ]
                        );
                        outcome
                    }
                )*
                _ => Err($crate::remote_server::registry::RemoteInvokeError::unavailable(cmd)),
            }
        }
    };

    // --- (e) structural rejection of channel / raw-body params -------------------------
    (@reject_forbidden_param (arg $n:ident : Channel $($rest:tt)*)) => {
        compile_error!(concat!(
            "remote_commands!: `", stringify!($n),
            "` is a Channel param; streaming channels are not dispatchable over the facade"
        ));
    };
    (@reject_forbidden_param (arg $n:ident : tauri::ipc::Channel $($rest:tt)*)) => {
        compile_error!(concat!(
            "remote_commands!: `", stringify!($n),
            "` is a Channel param; streaming channels are not dispatchable over the facade"
        ));
    };
    (@reject_forbidden_param (arg $n:ident : ::tauri::ipc::Channel $($rest:tt)*)) => {
        compile_error!(concat!(
            "remote_commands!: `", stringify!($n),
            "` is a Channel param; streaming channels are not dispatchable over the facade"
        ));
    };
    (@reject_forbidden_param (arg $n:ident : tauri::ipc::Request $($rest:tt)*)) => {
        compile_error!("remote_commands!: raw request bodies are not dispatchable over the facade");
    };
    (@reject_forbidden_param (arg $n:ident : tauri::ipc::Response $($rest:tt)*)) => {
        compile_error!("remote_commands!: raw response bodies are not dispatchable over the facade");
    };
    (@reject_forbidden_param (arg $n:ident : $t:ty)) => {};
    (@reject_forbidden_param (pinned_arg $n:ident : $t:ty)) => {};
    (@reject_forbidden_param (app_state)) => {};
    (@reject_forbidden_param (execution_state)) => {};
    (@reject_forbidden_param (active_project_state)) => {};
    (@reject_forbidden_param (app_handle)) => {};
    (@reject_forbidden_param (host_app_handle)) => {};

    // --- (b) the FIXED injection table -------------------------------------------------
    // These five arms ARE the table. An extractor form absent here has no matching arm, so
    // registering a command that needs it is a compile error (C-12, X-11).
    (@bind $app:ident, $args:ident, $spec:ident, (app_state)) => {
        $app.state::<$crate::application::AppState>()
    };
    (@bind $app:ident, $args:ident, $spec:ident, (execution_state)) => {
        $app.state::<::std::sync::Arc<$crate::commands::execution_commands::ExecutionState>>()
    };
    (@bind $app:ident, $args:ident, $spec:ident, (active_project_state)) => {
        $app.state::<::std::sync::Arc<$crate::commands::execution_commands::ActiveProjectState>>()
    };
    (@bind $app:ident, $args:ident, $spec:ident, (app_handle)) => {
        $app.clone()
    };
    // The Wry-monomorphic handle, resolved from the managed `AppState` rather than from the
    // generic dispatch handle (see the module docs). Fails closed when the host has not
    // populated it — a command demanding an `AppHandle` never runs without one.
    (@bind $app:ident, $args:ident, $spec:ident, (host_app_handle)) => {
        match $app
            .state::<$crate::application::AppState>()
            .app_handle
            .clone()
        {
            Some(handle) => handle,
            None => {
                return Err($crate::remote_server::registry::RemoteInvokeError::internal(
                    "The host application handle is unavailable; the command was not executed.",
                ))
            }
        }
    };
    (@bind $app:ident, $args:ident, $spec:ident, (arg $n:ident : $t:ty)) => {
        match $crate::remote_server::registry::extract_arg::<$t>($args, stringify!($n)) {
            Ok(value) => value,
            Err(error) => return Err(error),
        }
    };
    (@bind $app:ident, $args:ident, $spec:ident, (pinned_arg $n:ident : $t:ty)) => {
        match $crate::remote_server::registry::extract_pinned_arg::<$t>(
            $args,
            stringify!($n),
            $spec.pins,
        ) {
            Ok(value) => value,
            Err(error) => return Err(error),
        }
    };

    // --- call + result shaping ---------------------------------------------------------
    (@invoke $app:ident, $args:ident, $spec:ident, $target:path, async, fallible, [ $( $param:tt ),* ]) => {{
        match $target( $( $crate::remote_commands!(@bind $app, $args, $spec, $param) ),* ).await {
            Ok(value) => $crate::remote_server::registry::serialize_ok(value)
                .map($crate::remote_server::registry::DispatchOutcome::Ok),
            Err(error) => Ok($crate::remote_server::registry::DispatchOutcome::Err(
                $crate::remote_server::registry::serialize_ok(error)?,
            )),
        }
    }};
    (@invoke $app:ident, $args:ident, $spec:ident, $target:path, async, infallible, [ $( $param:tt ),* ]) => {{
        let value = $target( $( $crate::remote_commands!(@bind $app, $args, $spec, $param) ),* ).await;
        $crate::remote_server::registry::serialize_ok(value)
            .map($crate::remote_server::registry::DispatchOutcome::Ok)
    }};
    (@invoke $app:ident, $args:ident, $spec:ident, $target:path, sync, fallible, [ $( $param:tt ),* ]) => {{
        match $target( $( $crate::remote_commands!(@bind $app, $args, $spec, $param) ),* ) {
            Ok(value) => $crate::remote_server::registry::serialize_ok(value)
                .map($crate::remote_server::registry::DispatchOutcome::Ok),
            Err(error) => Ok($crate::remote_server::registry::DispatchOutcome::Err(
                $crate::remote_server::registry::serialize_ok(error)?,
            )),
        }
    }};
    (@invoke $app:ident, $args:ident, $spec:ident, $target:path, sync, infallible, [ $( $param:tt ),* ]) => {{
        let value = $target( $( $crate::remote_commands!(@bind $app, $args, $spec, $param) ),* );
        $crate::remote_server::registry::serialize_ok(value)
            .map($crate::remote_server::registry::DispatchOutcome::Ok)
    }};

    (@authz) => { None };
    (@authz $expr:expr) => { Some($expr as $crate::remote_server::registry::AuthzPredicate) };
    (@validate) => { None };
    (@validate $expr:expr) => { Some($expr as $crate::remote_server::registry::ValidatePredicate) };
}

/// Fail-closed scope enforcement.
///
/// Class scope first, then the per-request `authz:` predicate, whose answer may exceed the
/// class scope (`update_task` touching `title`/`description` demands `ui:agent` even though
/// the command's class is `Operate`).
pub fn enforce_scope(
    spec: &RemoteCommandSpec,
    granted: &[Scope],
    args: &Value,
) -> Result<(), RemoteInvokeError> {
    let Some(class_scope) = scope_for_class(spec.class) else {
        return Err(RemoteInvokeError::forbidden(format!(
            "`{}` is denied on the remote facade.",
            spec.name
        )));
    };
    if !granted.contains(&class_scope) {
        return Err(RemoteInvokeError::forbidden(format!(
            "`{}` requires a scope this device was not granted.",
            spec.name
        )));
    }
    if let Some(authz) = spec.authz {
        let required = authz(args);
        if !granted.contains(&required) {
            return Err(RemoteInvokeError::forbidden(format!(
                "`{}` requires a scope this device was not granted for these arguments.",
                spec.name
            )));
        }
    }
    Ok(())
}

pub fn find_spec(name: &str) -> Option<&'static RemoteCommandSpec> {
    REMOTE_COMMANDS.iter().find(|spec| spec.name == name)
}

// ---------------------------------------------------------------------------------------
// Argument-sensitive predicates
// ---------------------------------------------------------------------------------------

/// Fields of `UpdateTaskInput` that are worker-consumed content (R4-C2).
///
/// `title` feeds the imperative `SCOPE: Execute ONLY work for: "{title}"` directive and the
/// sibling dependency hints; `description` is the plan body. Writing either is deferred spawn
/// authority, so those requests demand `ui:agent`.
///
/// `category`/`priority` stay `ui:operate`, but NOT because no worker payload carries them —
/// that claim was false. The `WorkerTaskView` projection behind `get_task_context` and
/// `get_step_context` does exclude them, yet `/api/get_task_details` serialises both through
/// `task_to_response`. They are inert for a different and stronger reason: `category` is a
/// closed `TaskCategory` enum and `priority` is an `i32`, so neither can carry attacker-chosen
/// text into a prompt regardless of which projection renders it. `remote_server::registry_tests`
/// pins both halves — the `WorkerTaskView` exclusion and the `task_to_response` inclusion —
/// against poison sentinels.
///
/// `internal_status` never reaches here: `validate_update_task_input` rejects it.
pub const UPDATE_TASK_CONTENT_FIELDS: &[&str] = &["title", "description"];

/// The `update_task` field-level predicate (§3.3).
pub fn update_task_authz(args: &Value) -> Scope {
    let input = args.get("input").unwrap_or(args);
    let touches_content = UPDATE_TASK_CONTENT_FIELDS.iter().any(|field| {
        input
            .get(field)
            .map(|value| !value.is_null())
            .unwrap_or(false)
    });
    if touches_content {
        Scope::UiAgent
    } else {
        Scope::UiOperate
    }
}

// ---------------------------------------------------------------------------------------
// The v1 registered surface
// ---------------------------------------------------------------------------------------

// PR 1.3 registers the `Read` class only. Every entry below was individually checked against
// the capability ledger: none spawns a process, and none is one of the "getter that shells out"
// commands (`list_projects`/`get_project`, `get_git_branches`,
// `get_task_file_changes`/`get_file_diff`, `get_codex_cli_diagnostics`,
// `build_agent_issue_report`) which are NOT `Read`. Mutating
// classes land in PR 1.5 (`ui:agent` suite) and PR 3.1 (full coverage).
crate::remote_commands! {
    "health_check" => crate::commands::health::health_check {
        class: Read,
        caps: [],
        params: [],
        call: sync,
        result: infallible,
    },
    "list_tasks" => crate::commands::task_commands::query::list_tasks {
        class: Read,
        caps: [],
        params: [
            (arg project_id: String),
            (arg statuses: Option<Vec<String>>),
            (arg offset: Option<u32>),
            (arg limit: Option<u32>),
            (arg include_archived: Option<bool>),
            (arg ideation_session_id: Option<String>),
            (arg execution_plan_id: Option<String>),
            (arg categories: Option<Vec<String>>),
            (app_state),
        ],
        call: async,
        result: fallible,
    },
    "get_task" => crate::commands::task_commands::query::get_task {
        class: Read,
        caps: [],
        params: [(arg id: String), (app_state)],
        call: async,
        result: fallible,
    },
    "search_tasks" => crate::commands::task_commands::query::search_tasks {
        class: Read,
        caps: [],
        params: [
            (arg project_id: String),
            (arg query: String),
            (arg include_archived: Option<bool>),
            (arg ideation_session_id: Option<String>),
            (app_state),
        ],
        call: async,
        result: fallible,
    },
    "get_valid_transitions" => crate::commands::task_commands::query::get_valid_transitions {
        class: Read,
        caps: [],
        params: [(arg task_id: String), (app_state)],
        call: async,
        result: fallible,
    },

    // -----------------------------------------------------------------------------------
    // PR 3.1-b batch 1 — the 2.7 reconnect gate reads, at `ui:read`.
    //
    // These are the flag-on precondition for remote P-21: a client that reconnects while a
    // permission or question gate is open learns about it ONLY through these two reads, and
    // `pending-gate-reconcile.ts` treats `REMOTE_COMMAND_UNAVAILABLE` as "cannot
    // reconcile". Their `get_pending_info_strict` targets are the fail-closed halves of the
    // pair — a repository error propagates instead of collapsing into an empty gate list,
    // which is what makes them safe to expose: the remote client can never be told "no
    // gates are open" because a read failed.
    //
    // Both are enumerations, not resolutions. The sibling commands that ANSWER a gate
    // (`resolve_permission_request`, `resolve_user_question`,
    // `approve_permission_request`) stay at `AgentControl` — see the overrides below.
    // -----------------------------------------------------------------------------------
    "list_pending_permission_gates"
        => crate::commands::permission_commands::list_pending_permission_gates {
        class: Read,
        caps: [],
        params: [(app_state)],
        call: async,
        result: fallible,
    },
    "list_pending_question_gates"
        => crate::commands::question_commands::list_pending_question_gates {
        class: Read,
        caps: [],
        params: [(app_state)],
        call: async,
        result: fallible,
    },

    // -----------------------------------------------------------------------------------
    // PR 3.1-b batch 2 — census `B1`, the `task_commands` read cluster, at `ui:read`.
    //
    // These are reclassifications, not newly-permissive rows: each sat at `AgentControl`
    // only because `task_commands` defaults there, and the default is conservative because
    // the module also holds `move_task`, `unblock_task` and the execution-plan controls.
    // The per-command audit (detectors a/b/c silent, bodies hand-traced to repository
    // reads with propagated errors) is recorded in `capability_ledger` and pinned by the
    // detector calibration lists.
    // -----------------------------------------------------------------------------------
    "get_archived_count" => crate::commands::task_commands::query::get_archived_count {
        class: Read,
        caps: [],
        params: [
            (arg project_id: String),
            (arg ideation_session_id: Option<String>),
            (app_state),
        ],
        call: async,
        result: fallible,
    },
    "get_tasks_awaiting_review"
        => crate::commands::task_commands::query::get_tasks_awaiting_review {
        class: Read,
        caps: [],
        params: [(arg project_id: String), (app_state)],
        call: async,
        result: fallible,
    },
    "get_session_task_history_availability"
        => crate::commands::task_commands::query::get_session_task_history_availability {
        class: Read,
        caps: [],
        params: [
            (arg project_id: String),
            (arg ideation_session_id: String),
            (app_state),
        ],
        call: async,
        result: fallible,
    },
    "get_task_state_transitions"
        => crate::commands::task_commands::query::get_task_state_transitions {
        class: Read,
        caps: [],
        params: [(arg task_id: String), (app_state)],
        call: async,
        result: fallible,
    },
    "get_task_dependency_graph"
        => crate::commands::task_commands::query::get_task_dependency_graph {
        class: Read,
        caps: [],
        params: [
            (arg project_id: String),
            (arg include_archived: Option<bool>),
            (arg session_id: Option<String>),
            (arg execution_plan_id: Option<String>),
            (app_state),
        ],
        call: async,
        result: fallible,
    },
    "get_task_timeline_events"
        => crate::commands::task_commands::query::get_task_timeline_events {
        class: Read,
        caps: [],
        params: [
            (arg project_id: String),
            (arg offset: Option<u32>),
            (arg limit: Option<u32>),
            (arg session_id: Option<String>),
            (arg execution_plan_id: Option<String>),
            (app_state),
        ],
        call: async,
        result: fallible,
    },
    "get_task_agent_workspace"
        => crate::commands::task_commands::query::get_task_agent_workspace {
        class: Read,
        caps: [],
        params: [(arg task_id: String), (app_state)],
        call: async,
        result: fallible,
    },

    // -----------------------------------------------------------------------------------
    // PR 3.1-b batch 2 — census `B1`, the step + execution read clusters, at `ui:read`.
    //
    // The execution cluster is three getters, not the module's eight: detector (c) fires on
    // `get_execution_status` and `get_running_processes` (both resolve a process-inspection
    // CLI), and `set_active_project` syncs the runtime scheduler quota. All three stay
    // unregistered, pinned by `the_b1_step_and_execution_reads_are_refused_below_ui_read`.
    // -----------------------------------------------------------------------------------
    "get_task_steps" => crate::commands::task_step_commands::get_task_steps {
        class: Read,
        caps: [],
        params: [(arg task_id: String), (app_state)],
        call: async,
        result: fallible,
    },
    "get_step_progress" => crate::commands::task_step_commands::get_step_progress {
        class: Read,
        caps: [],
        params: [(arg task_id: String), (app_state)],
        call: async,
        result: fallible,
    },
    "get_execution_settings"
        => crate::commands::execution_commands::get_execution_settings {
        class: Read,
        caps: [],
        params: [(arg project_id: Option<String>), (app_state)],
        call: async,
        result: fallible,
    },
    "get_global_execution_settings"
        => crate::commands::execution_commands::get_global_execution_settings {
        class: Read,
        caps: [],
        params: [(app_state)],
        call: async,
        result: fallible,
    },
    "get_active_project" => crate::commands::execution_commands::get_active_project {
        class: Read,
        caps: [],
        params: [(active_project_state)],
        call: async,
        result: fallible,
    },

    // -----------------------------------------------------------------------------------
    // PR 1.5-A — `ui:operate`: watch + brakes + inert edits, and NOTHING that can start,
    // resume, restart, or steer an agent. This is the default pairing's entire mutating
    // surface (the "viewer with brakes" boundary, §3.3/§4.3).
    // -----------------------------------------------------------------------------------

    // Argument-sensitive: `category`/`priority` are inert (closed enum + i32, so neither can
    // carry attacker-chosen text into a prompt); `title`/`description` are worker-consumed
    // content and `update_task_authz` escalates those requests to `ui:agent`. The conditional
    // `MutatesAgentConsumedContent` capability cannot live in `caps:` — `class_permits` gives
    // `Operate` no capabilities at all — so it is carried as a ledger annotation whose CI guard
    // requires this predicate to exist (`capability_ledger::CONDITIONAL_CAPABILITIES`).
    "update_task" => crate::commands::task_commands::mutation::update_task {
        class: Operate,
        caps: [],
        params: [
            (arg task_id: String),
            (arg input: crate::commands::task_commands::types::UpdateTaskInput),
            (app_state),
        ],
        call: async,
        result: fallible,
        authz: crate::remote_server::registry::update_task_authz,
    },
    // Backlog-only by construction: `CreateTaskInput` carries no status field and every
    // construction path runs `Task::new_with_category`, which sets `InternalStatus::Backlog`.
    // A created task therefore cannot be born in a spawn-triggering state.
    "create_task" => crate::commands::task_commands::mutation::create_task {
        class: Operate,
        caps: [],
        params: [
            (arg input: crate::commands::task_commands::types::CreateTaskInput),
            (app_state),
        ],
        call: async,
        result: fallible,
    },
    "pause_task" => crate::commands::task_commands::mutation::pause_task {
        class: Operate,
        caps: [],
        params: [(arg task_id: String), (app_state), (execution_state)],
        call: async,
        result: fallible,
    },
    "block_task" => crate::commands::task_commands::mutation::block_task {
        class: Operate,
        caps: [],
        params: [
            (arg task_id: String),
            (arg reason: Option<String>),
            (app_state),
            (execution_state),
            (host_app_handle),
        ],
        call: async,
        result: fallible,
    },
    "stop_task" => crate::commands::task_commands::mutation::stop_task {
        class: Operate,
        caps: [],
        params: [
            (arg task_id: String),
            (arg reason: Option<String>),
            (app_state),
            (execution_state),
        ],
        call: async,
        result: fallible,
    },
    "pause_tasks_in_group" => crate::commands::task_commands::mutation::pause_tasks_in_group {
        class: Operate,
        caps: [],
        params: [
            (arg group_kind: String),
            (arg group_id: String),
            (arg project_id: String),
            (app_state),
            (execution_state),
            (host_app_handle),
        ],
        call: async,
        result: fallible,
    },
    // The deny half of the dual-decision `resolve_permission_request`. The raw command is
    // NEVER registered; `decision` is server-pinned, so a client sending `"allow"` still denies.
    "deny_permission_request" => crate::commands::permission_commands::resolve_permission_request {
        class: Operate,
        caps: [],
        params: [
            (app_state),
            (pinned_arg args: crate::commands::permission_commands::ResolvePermissionArgs),
        ],
        call: async,
        result: fallible,
        pins: [("args", "decision", "deny")],
    },

    // -----------------------------------------------------------------------------------
    // PR 1.5-A — `ui:agent`: everything that can start, resume, restart or steer an agent,
    // whether directly (detector a), by seeding state a background loop consumes
    // (detector b), by mutating agent-consumed content, or as a declared membership.
    // Off by default; granted per device.
    // -----------------------------------------------------------------------------------

    // Detector (a).
    "move_task" => crate::commands::task_commands::mutation::move_task {
        class: AgentControl,
        caps: [AgentControl, MutatesAgentConsumedContent],
        params: [
            (arg task_id: String),
            (arg to_status: String),
            (arg note: Option<String>),
            (app_state),
            (execution_state),
            (host_app_handle),
        ],
        call: async,
        result: fallible,
    },
    // NOT registered — detector (c) proves `resume_task`, `apply_proposals_to_kanban` and
    // `set_agent_conversation_workspace_auto_publish` reach a process-launch sink, and a
    // command carrying `SpawnsProcess` authority is not exposable on the v1 facade at any
    // scope. They stay AgentControl-floor members and answer `REMOTE_COMMAND_UNAVAILABLE`;
    // the generated P-17b suite asserts exactly that.
    "unblock_task" => crate::commands::task_commands::mutation::unblock_task {
        class: AgentControl,
        caps: [AgentControl],
        params: [
            (arg task_id: String),
            (app_state),
            (execution_state),
            (host_app_handle),
        ],
        call: async,
        result: fallible,
    },
    "answer_user_question" => crate::commands::task_commands::mutation::answer_user_question {
        class: AgentControl,
        caps: [AgentControl],
        params: [
            (arg input: crate::commands::task_commands::types::AnswerUserQuestionInput),
            (app_state),
            (execution_state),
            (host_app_handle),
        ],
        call: async,
        result: fallible,
    },
    "approve_task_for_review" => crate::commands::review_commands::approve_task_for_review {
        class: AgentControl,
        caps: [MutatesAgentConsumedContent],
        params: [
            (arg input: crate::commands::review_commands_types::ApproveTaskInput),
            (app_state),
            (execution_state),
            (host_app_handle),
        ],
        call: async,
        result: fallible,
    },
    "reanalyze_project" => crate::commands::project_commands::reanalyze_project {
        class: AgentControl,
        caps: [AgentControl],
        params: [(arg id: String), (app_state)],
        call: async,
        result: fallible,
    },

    // Detector (b) — seeds state a registered background loop consumes.
    "inject_task" => crate::commands::task_commands::mutation::inject_task {
        class: AgentControl,
        caps: [SeedsSpawnTriggeringState],
        params: [
            (arg input: crate::commands::task_commands::types::InjectTaskInput),
            (app_state),
            (host_app_handle),
        ],
        call: async,
        result: fallible,
    },
    "resume_automation" => crate::commands::automation_commands::resume_automation {
        class: AgentControl,
        caps: [SeedsSpawnTriggeringState],
        params: [
            (arg input: crate::commands::automation_commands::AutomationIdInput),
            (app_state),
        ],
        call: async,
        result: fallible,
    },
    "finalize_automation" => crate::commands::automation_commands::finalize_automation {
        class: AgentControl,
        caps: [SeedsSpawnTriggeringState],
        params: [
            (arg input: crate::commands::automation_commands::AutomationIdInput),
            (app_state),
        ],
        call: async,
        result: fallible,
    },

    // Spawn-free chat send. This is the ONLY registered way to put words into an agent
    // conversation from a paired device: `send_agent_message` and `start_agent_conversation`
    // both fire detector (c) and stay unregistered, so remote participation is confined to
    // steering a run the host already started.
    //
    // `role` is server-pinned to `"user"`. The transcript's role field is what downstream
    // prompt assembly trusts to tell instruction from user input, so a client that could
    // author an `orchestrator` turn could put words in the agent's own mouth. The pin is
    // read from `spec.pins` at dispatch, and the command independently rejects any other
    // role — the declaration cannot drift from the behaviour.
    "send_remote_chat_message" => crate::commands::remote_chat_commands::send_remote_chat_message {
        class: AgentControl,
        caps: [MutatesAgentConsumedContent],
        params: [
            (pinned_arg input: crate::commands::remote_chat_commands::SendRemoteChatMessageInput),
            (app_state),
        ],
        call: async,
        result: fallible,
        pins: [("input", "role", "user")],
    },

    // Agent-consumed content surface.
    "create_task_step" => crate::commands::task_step_commands::create_task_step {
        class: AgentControl,
        caps: [MutatesAgentConsumedContent],
        params: [
            (arg task_id: String),
            (arg input: crate::commands::task_step_commands_types::CreateTaskStepInput),
            (app_state),
        ],
        call: async,
        result: fallible,
    },
    "update_task_step" => crate::commands::task_step_commands::update_task_step {
        class: AgentControl,
        caps: [MutatesAgentConsumedContent],
        params: [
            (arg step_id: String),
            (arg input: crate::commands::task_step_commands_types::UpdateTaskStepInput),
            (app_state),
        ],
        call: async,
        result: fallible,
    },
    "create_artifact" => crate::commands::artifact_commands::create_artifact {
        class: AgentControl,
        caps: [MutatesAgentConsumedContent],
        params: [
            (arg input: crate::commands::artifact_commands::CreateArtifactInput),
            (app_state),
        ],
        call: async,
        result: fallible,
    },
    "update_artifact" => crate::commands::artifact_commands::update_artifact {
        class: AgentControl,
        caps: [MutatesAgentConsumedContent],
        params: [
            (arg id: String),
            (arg input: crate::commands::artifact_commands::UpdateArtifactInput),
            (app_state),
        ],
        call: async,
        result: fallible,
    },
    "add_artifact_relation" => crate::commands::artifact_commands::add_artifact_relation {
        class: AgentControl,
        caps: [MutatesAgentConsumedContent],
        params: [
            (arg input: crate::commands::artifact_commands::AddRelationInput),
            (app_state),
        ],
        call: async,
        result: fallible,
    },
    "update_task_proposal" => crate::commands::ideation_commands::update_task_proposal {
        class: AgentControl,
        caps: [MutatesAgentConsumedContent],
        params: [
            (arg id: String),
            (arg input: crate::commands::ideation_commands::UpdateProposalInput),
            (app_state),
        ],
        call: async,
        result: fallible,
    },

    // Declared membership: authorising a live tool call is not inferable from a transition or
    // process sink, so it is declared. Same target fn as `deny_permission_request`, opposite
    // server-pinned decision — and a whole class higher.
    "approve_permission_request"
        => crate::commands::permission_commands::resolve_permission_request {
        class: AgentControl,
        caps: [AgentControl],
        params: [
            (app_state),
            (pinned_arg args: crate::commands::permission_commands::ResolvePermissionArgs),
        ],
        call: async,
        result: fallible,
        pins: [("args", "decision", "allow")],
    },
}
