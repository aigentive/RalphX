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
    pub fn bad_args(message: impl Into<String>) -> Self {
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
                // An argument-shape refusal, NOT `forbidden`: the device's grant is sufficient
                // and re-pairing at a higher scope would not help. Saying `forbidden` here
                // would send the client to the pairing flow for a fixable request.
                validate(args).map_err(|message| {
                    $crate::remote_server::registry::RemoteInvokeError::bad_args(message)
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
/// that claim was false. The remote-facade `WorkerTaskView` projection behind `get_task_context`
/// and the `get_step_context` task summary do exclude them, yet `/api/get_task_details`
/// serialises both through
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

/// Refusal message for a brake dispatched without an explicit project scope.
pub const BRAKE_REQUIRES_PROJECT_SCOPE: &str =
    "This command requires an explicit non-null `projectId` when invoked remotely.";

/// The brake-scope predicate: a remote halt must name the project it halts.
///
/// `pause_execution`/`stop_execution` accept `project_id: Option<String>`. Locally the `None`
/// arm is a convenience — it falls back to the active project, and then to *every* project via
/// `project_repo.get_all()`. Remotely that same arm is a one-call sweep of the whole host from a
/// device in the DEFAULT `ui:operate` pairing, so the facade requires the argument.
///
/// # What this confines, and what it does not
///
/// Confined: the per-task transition sweep. With an explicit `projectId` the command transitions
/// only that project's agent-active tasks to `Paused`/`Stopped`.
///
/// NOT confined, and deliberately recorded rather than implied: `execution_state.pause()`,
/// `persist_execution_halt_mode`, `running_agent_registry.stop_all()` and
/// `interactive_process_registry.clear()` are process-global in `execution_commands::lifecycle`
/// and run before the project is ever resolved. A scoped remote brake still halts host-wide
/// scheduling. The predicate narrows blast radius and forces the caller to state an intent; it
/// is not a per-project halt, and
/// `the_brake_scope_predicate_does_not_confine_the_global_pause_flag` pins that limit so the
/// confinement cannot be read as stronger than it is.
///
/// This is facade-layer only: local callers reach the command fn directly and are unaffected.
pub fn require_explicit_project_scope(args: &Value) -> Result<(), String> {
    match args.get("projectId").or_else(|| args.get("project_id")) {
        // Absent or explicitly null are the SAME failure: both deserialize to `None` and reach
        // the all-projects arm, so accepting one would leave the sweep open.
        None | Some(Value::Null) => Err(BRAKE_REQUIRES_PROJECT_SCOPE.to_string()),
        Some(Value::String(id)) if id.trim().is_empty() => {
            Err(BRAKE_REQUIRES_PROJECT_SCOPE.to_string())
        }
        Some(Value::String(_)) => Ok(()),
        // A non-string is an argument-shape error; refusing here keeps the predicate from
        // passing a value the target fn would reject anyway.
        Some(_) => Err(BRAKE_REQUIRES_PROJECT_SCOPE.to_string()),
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
    // `get_remote_execution_status` is the separately audited spawn-free read twin.
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
    // PR 3.1-b batch 3 — census `B2`, the conversation-stats read cluster, at `ui:read`.
    //
    // The smallest complete module in the census's highest-risk batch. Detectors (a)/(b)/(c)
    // and (d) are silent on all four; bodies hand-traced to repository reads with propagated
    // errors. Payloads are token/cost AGGREGATES only — no message text, prompt, or tool
    // input — so this is the usage-reporting surface, not the transcript surface.
    //
    // The rest of `B2` is NOT here and the probe says why: `get_agent_conversation`,
    // `get_agent_conversation_messages_page` and `get_agent_conversation_timeline_page` all
    // fire detector (a), and the workspace/publish surface fires (a), (b) and (c) together.
    // -----------------------------------------------------------------------------------
    "get_agent_conversation_stats"
        => crate::commands::conversation_stats_commands::get_agent_conversation_stats {
        class: Read,
        caps: [],
        params: [(arg conversation_id: String), (app_state)],
        call: async,
        result: fallible,
    },
    "get_project_chat_usage_stats"
        => crate::commands::conversation_stats_commands::get_project_chat_usage_stats {
        class: Read,
        caps: [],
        params: [(arg project_id: String), (app_state)],
        call: async,
        result: fallible,
    },
    "get_task_chat_usage_stats"
        => crate::commands::conversation_stats_commands::get_task_chat_usage_stats {
        class: Read,
        caps: [],
        params: [(arg task_id: String), (app_state)],
        call: async,
        result: fallible,
    },
    "get_insights_chat_usage_stats"
        => crate::commands::conversation_stats_commands::get_insights_chat_usage_stats {
        class: Read,
        caps: [],
        params: [(arg project_id: Option<String>), (app_state)],
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
        class: AgentControl,
        caps: [AgentControl],
        params: [(arg task_id: String), (app_state), (execution_state)],
        call: async,
        result: fallible,
    },
    "block_task" => crate::commands::task_commands::mutation::block_task {
        class: AgentControl,
        caps: [AgentControl],
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
        class: AgentControl,
        caps: [AgentControl],
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
        class: AgentControl,
        caps: [AgentControl],
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
    // -----------------------------------------------------------------------------------
    // PR 3.1-b batch 3 — the global Operate brakes, at `ui:operate`.
    //
    // Until this batch a paired device could WATCH execution it had no way to stop: the
    // global halt was not. The two global commands below move the system strictly toward less
    // autonomous work and set the process-wide pause gate before task transitions.
    //
    // `pause_execution` / `stop_execution` open with `sync_quota_from_project`, the runtime
    // scheduler-quota write that disqualified `set_active_project` in batch 2. They are safe
    // where it was not, for a reason that is tested rather than asserted: the quota is written
    // and then IMMEDIATELY dominated by the pause flag, `can_start_task` short-circuits on
    // `is_paused()` before reading any quota, and the single production path that clears the
    // pause flag (`resume_execution`) re-syncs the quota before it does. A quota raised while
    // halting can therefore never arm the scheduler.
    //
    // `archive_tasks_in_group` is deliberately ABSENT — see the ledger comment; it hides
    // running agents instead of stopping them.
    // -----------------------------------------------------------------------------------
    "pause_execution" => crate::commands::execution_commands::pause_execution {
        class: Operate,
        caps: [],
        params: [
            (arg project_id: Option<String>),
            (active_project_state),
            (execution_state),
            (app_state),
        ],
        call: async,
        result: fallible,
        validate: crate::remote_server::registry::require_explicit_project_scope,
    },
    "stop_execution" => crate::commands::execution_commands::stop_execution {
        class: Operate,
        caps: [],
        params: [
            (arg project_id: Option<String>),
            (active_project_state),
            (execution_state),
            (app_state),
        ],
        call: async,
        result: fallible,
        validate: crate::remote_server::registry::require_explicit_project_scope,
    },
    "cancel_tasks_in_group"
        => crate::commands::task_commands::mutation::cancel_tasks_in_group {
        class: AgentControl,
        caps: [AgentControl],
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

    // Spawn-free conversation START. The registrable half of "create remotely, start on the
    // host": this closure only PERSISTS a start intent (seeded draft conversation + intent row);
    // the host-owned `spawn_remote_conversation_start_dispatcher` loop is the sole spawner.
    // Detector-silent on (a) and (c) — it reaches no scheduler and no CLI path — and detector (b)
    // flags it MECHANICALLY (not via a declared writer) through the
    // `remote-conversation-start` state-surface row, which is the honest classification the
    // `SeedsSpawnTriggeringState` capability expresses. `mode` is host-pinned to "chat"; the
    // command independently rejects any other mode, and there is no role/team/base field to forge.
    "request_remote_agent_conversation_start"
        => crate::commands::remote_conversation_start_commands::request_remote_agent_conversation_start {
        class: AgentControl,
        caps: [MutatesAgentConsumedContent, SeedsSpawnTriggeringState],
        params: [
            (pinned_arg input: crate::commands::remote_conversation_start_commands::RequestRemoteAgentConversationStartInput),
            (app_state),
        ],
        call: async,
        result: fallible,
        pins: [("input", "mode", "chat")],
    },

    // Spawn-free conversation CONTINUATION (WP1) — the fix for the one-shot remote surface.
    // `send_remote_chat_message` above only works while a run is LIVE; once the agent finished
    // its turn a paired device hit a dead end. This closure only PERSISTS a continuation intent;
    // the host-owned `spawn_remote_conversation_message_dispatcher` loop is the sole sender, and
    // its terminal call is `ChatService::send_message` (the provider-session resume seam), NOT
    // `AgentConversationStartService::start` — starting would mint a fresh run and abandon the
    // session. Detector-silent on (a) and (c); detector (b) flags it MECHANICALLY through the
    // `remote-conversation-message` state-surface row, which is what `SeedsSpawnTriggeringState`
    // expresses honestly.
    //
    // There is NO `role` field to pin: first-turn authorship is host-forced by FIELD ABSENCE,
    // which is stronger than a pinned value because there is nothing on the wire to forge. The
    // command additionally REFUSES when a run is already live, so this surface and
    // `send_remote_chat_message` are disjoint by construction and a turn can never be doubled.
    "request_remote_agent_conversation_message"
        => crate::commands::remote_conversation_message_commands::request_remote_agent_conversation_message {
        class: AgentControl,
        caps: [MutatesAgentConsumedContent, SeedsSpawnTriggeringState],
        params: [
            (arg input: crate::commands::remote_conversation_message_commands::RequestRemoteAgentConversationMessageInput),
            (app_state),
        ],
        call: async,
        result: fallible,
    },

    // The client's post-submit poll target for the continuation intent. Pure repository read;
    // the client MUST reach a terminal status here before it may render the turn as delivered.
    "get_remote_conversation_message_request"
        => crate::commands::remote_conversation_message_commands::get_remote_conversation_message_request {
        class: Read,
        caps: [],
        params: [
            (arg message_request_id: String),
            (app_state),
        ],
        call: async,
        result: fallible,
    },

    // The client's post-submit poll target for the intent above. Pure repository read.
    "get_remote_conversation_start_request"
        => crate::commands::remote_conversation_start_commands::get_remote_conversation_start_request {
        class: Read,
        caps: [],
        params: [
            (arg start_request_id: String),
            (app_state),
        ],
        call: async,
        result: fallible,
    },

    // Spawn-free agent STOP, at `ui:operate` — a brake on the DEFAULT pairing.
    //
    // `stop_agent` reaches `Command::new(resolve_pkill_cli_path())` and stays unregistered by
    // the absolute process floor. Registering the BRAKE anyway is not a relaxation of that
    // floor: this closure persists one conversation-scoped intent row and returns, and the
    // host-owned `spawn_remote_agent_stop_dispatcher` loop is the sole holder of the
    // terminating path. The `Operate` class is the honest one because the intent is
    // authority-REDUCING — the loop that consumes it can only end a run, never start, resume or
    // steer one — which is exactly the `pause_execution`/`stop_execution` shape, and it carries
    // an `AUTHORITY_REDUCING_EXEMPTIONS` row for the gap to its `stop_agent` sibling.
    //
    // There is deliberately NO `contextType`, run id, or pid on the wire: the host resolves what
    // to terminate from the conversation row at drain time, so a client cannot aim the brake
    // outside the Agents surface. Field absence, not a pin.
    "request_remote_agent_stop"
        => crate::commands::remote_agent_stop_commands::request_remote_agent_stop {
        class: Operate,
        caps: [],
        params: [
            (arg input: crate::commands::remote_agent_stop_commands::RequestRemoteAgentStopInput),
            (app_state),
        ],
        call: async,
        result: fallible,
    },

    // Spawn-free conversation MODE SWITCH (WP5a) — the fix for a remote surface stuck in chat.
    //
    // `switch_agent_conversation_mode` is `host-denied-spawns-process` (its body reaches
    // `GitService::ref_exists` and `inspect_repository_capability` -> `ensure_git_worktree`), and
    // the process floor is absolute. Because the conversation-start intent host-pins `mode` to
    // "chat", that left a paired device able to reach chat and NOTHING else — Edit, Plan and
    // Ideation were unreachable, not merely slower. This closure only PERSISTS a switch intent;
    // the host-owned `spawn_remote_conversation_mode_switch_dispatcher` loop is the sole holder
    // of the worktree-preparing path, and its terminal call is
    // `switch_agent_conversation_mode_for_state` (the REJECT-on-running-agent variant), NOT the
    // `..._stopping_running_agent` one the local command uses — stopping stays WP2's separate,
    // explicitly user-initiated intent rather than a side effect of moving a dropdown.
    //
    // Detector-silent on (a) and (c); detector (b) flags it MECHANICALLY through the
    // `remote-conversation-mode-switch` state-surface row, which is what
    // `SeedsSpawnTriggeringState` expresses honestly: the row this command writes causes the host
    // to prepare a workspace a later agent process runs in.
    //
    // There is NO base/branch/runtime-override field to pin: every one of them steers real
    // workspace preparation, and they are ABSENT from the wire rather than pinned, which is
    // stronger because there is nothing to forge. The command additionally REFUSES when a run is
    // live, so a switch can never race a running agent's workspace.
    "request_remote_agent_conversation_mode_switch"
        => crate::commands::remote_conversation_mode_switch_commands::request_remote_agent_conversation_mode_switch {
        class: AgentControl,
        caps: [SeedsSpawnTriggeringState],
        params: [
            (arg input: crate::commands::remote_conversation_mode_switch_commands::RequestRemoteAgentConversationModeSwitchInput),
            (app_state),
        ],
        call: async,
        result: fallible,
    },

    // The client's post-submit poll target for the mode-switch intent. Pure repository read.
    "get_remote_conversation_mode_switch_request"
        => crate::commands::remote_conversation_mode_switch_commands::get_remote_conversation_mode_switch_request {
        class: Read,
        caps: [],
        params: [
            (arg mode_switch_request_id: String),
            (app_state),
        ],
        call: async,
        result: fallible,
    },

    // The client's post-submit poll target for the stop intent. Pure repository read.
    "get_remote_agent_stop_request"
        => crate::commands::remote_agent_stop_commands::get_remote_agent_stop_request {
        class: Read,
        caps: [],
        params: [
            (arg stop_request_id: String),
            (app_state),
        ],
        call: async,
        result: fallible,
    },

    // -----------------------------------------------------------------------------------
    // PR 3.1-b batch 10 — the `ui:agent` registration decisions.
    //
    // Batch 9 left 25 audited-and-refused commands on the ratchet with a precise diagnosis:
    // every recorded finding was arming, steering, or an unaudited write, and the facade
    // already serves 16 `agentControl` ops of exactly that shape. So the refusals recorded
    // which batch ran out of scope, not a property of the command, and what they needed was
    // the `ui:agent` registration audit nobody had done. Batch 10 did it.
    //
    // The floor held first: `probe_batch9_retroactive_closure_candidates` was re-run against
    // the CURRENT graph and detector (c) is SILENT on all 25 — not one reaches a
    // `PROCESS_LAUNCH_SINKS` resolver. Nothing below is registered over a CLI launch.
    //
    // Detector silence was necessary and never sufficient, and it did not carry the batch:
    // seven of the 25 audit DIRTY and are in `AUDIT_REFUSALS` rather than here — three
    // fail-open writes and four surfaces the facade already answers under another name.
    //
    // Every row below was hand-traced to its repository call. The shared structural property,
    // checked rather than assumed: a status/enum guard that returns `Err` on violation, and
    // repository errors propagated with `?`/`map_err(...)?` — never collapsed into a default,
    // an empty result, or a discarded write. The only `let _ =` in any of these bodies is a
    // post-transition `app.emit`, i.e. a UI notification AFTER the backend has already
    // accepted the write, which is the authority-before-effects ordering rather than a breach
    // of it.
    // -----------------------------------------------------------------------------------

    // --- Review lifecycle: human gate decisions that resume or redirect agent work. These are
    //     the closest siblings of the already-registered `approve_task_for_review`, and two of
    //     them reach the very same `TaskTransitionService`.
    //
    // NOT registered — the fix-task repair pair. Batch 10 audited `approve_fix_task` clean (a
    // Blocked→Ready transition in the registered `unblock_task` shape) and intended to register
    // it, and `no_registered_facade_target_reaches_a_corrective_transition` refused the other
    // half: `reject_fix_task` reaches `transition_task_corrective`, the nonstandard repair jump
    // that is repair-path-only and must never be remotely reachable. That is a hard invariant,
    // not a scope call, so `reject_fix_task` cannot be registered at any scope as it stands.
    //
    // `approve_fix_task` is then withheld on the pre-existing PAIR argument rather than on any
    // finding of its own, and batch 10 upholds it: registering the approve half alone would let
    // a paired device unblock fix tasks with no remote way to reject one — the same
    // brake-less asymmetry batch 3 closed for execution. Both stay on the ratchet, and both
    // reasons are recorded in `b3_members_that_audit_dirty_stay_unregistered`.
    // Registered where its near-twin `request_task_changes_from_reviewing` is REFUSED, and the
    // split is the batch's sharpest single finding. Both reach the same `RevisionNeeded`
    // transition; only the `_from_reviewing` variant first rewrites `task.metadata` through two
    // `unwrap_or_else` fallbacks that replace an unparseable blob with a stub and drop every
    // other field while still returning `Ok`. This one carries no such write.
    "request_task_changes_for_review"
        => crate::commands::review_commands::request_task_changes_for_review {
        class: AgentControl,
        caps: [MutatesAgentConsumedContent],
        params: [
            (arg input: crate::commands::review_commands_types::RequestTaskChangesInput),
            (app_state),
            (execution_state),
            (host_app_handle),
        ],
        call: async,
        result: fallible,
    },
    "re_review_task_from_escalated"
        => crate::commands::review_commands::re_review_task_from_escalated {
        class: AgentControl,
        caps: [AgentControl],
        params: [
            (arg input: crate::commands::review_commands_types::ReReviewTaskInput),
            (app_state),
            (execution_state),
            (host_app_handle),
        ],
        call: async,
        result: fallible,
    },
    // Detector (b): arms `require_workspace_review`, which the auto-review spawner consumes.
    // The registered `inject_task`/`resume_automation`/`finalize_automation` carry the same
    // `SeedsSpawnTriggeringState` capability, which is exactly why batch 9 refused to call this
    // shape host-denied.
    "update_review_settings" => crate::commands::review_commands::update_review_settings {
        class: AgentControl,
        caps: [AgentControl, SeedsSpawnTriggeringState],
        params: [
            (arg input: crate::commands::review_commands_types::UpdateReviewSettingsInput),
            (app_state),
        ],
        call: async,
        result: fallible,
    },

    // --- Review ISSUE bookkeeping. Each is `review_issue_repo.get_by_id` → an explicit status
    //     guard → `review_issue_repo.update`, taking only `&AppState`: no transition service,
    //     no `AppHandle`, no `ExecutionState`. They carry `MutatesAgentConsumedContent` because
    //     a reviewing agent reads issue state, which is the whole reason they are `ui:agent`
    //     rather than `ui:operate`.
    "reopen_issue" => crate::commands::review_commands::reopen_issue {
        class: AgentControl,
        caps: [AgentControl, MutatesAgentConsumedContent],
        params: [
            (arg input: crate::commands::review_commands_types::ReopenIssueInput),
            (app_state),
        ],
        call: async,
        result: fallible,
    },
    "verify_issue" => crate::commands::review_commands::verify_issue {
        class: AgentControl,
        caps: [AgentControl, MutatesAgentConsumedContent],
        params: [
            (arg input: crate::commands::review_commands_types::VerifyIssueInput),
            (app_state),
        ],
        call: async,
        result: fallible,
    },
    "mark_issue_in_progress" => crate::commands::review_commands::mark_issue_in_progress {
        class: AgentControl,
        caps: [AgentControl, MutatesAgentConsumedContent],
        params: [
            (arg input: crate::commands::review_commands_types::MarkIssueInProgressInput),
            (app_state),
        ],
        call: async,
        result: fallible,
    },
    "mark_issue_addressed" => crate::commands::review_commands::mark_issue_addressed {
        class: AgentControl,
        caps: [AgentControl, MutatesAgentConsumedContent],
        params: [
            (arg input: crate::commands::review_commands_types::MarkIssueAddressedInput),
            (app_state),
        ],
        call: async,
        result: fallible,
    },

    // --- Review ROW verdicts. Recorded precisely, because batch 9's pairing table guessed
    //     these were duplicates of `approve_task_for_review` and the body audit refuted that:
    //     they write the `reviews` row only (`review_repo.update` on status/notes/completed_at)
    //     and never touch `Task::internal_status` or any transition service. So they are NOT a
    //     second path to the registered task approval — they are a different, narrower write,
    //     which is why they are registered on their own audit rather than twin-classified.
    "approve_review" => crate::commands::review_commands::approve_review {
        class: AgentControl,
        caps: [MutatesAgentConsumedContent],
        params: [
            (arg input: crate::commands::review_commands_types::ApproveReviewInput),
            (app_state),
        ],
        call: async,
        result: fallible,
    },
    "reject_review" => crate::commands::review_commands::reject_review {
        class: AgentControl,
        caps: [MutatesAgentConsumedContent],
        params: [
            (arg input: crate::commands::review_commands_types::RejectReviewInput),
            (app_state),
        ],
        call: async,
        result: fallible,
    },
    "request_changes" => crate::commands::review_commands::request_changes {
        class: AgentControl,
        caps: [MutatesAgentConsumedContent],
        params: [
            (arg input: crate::commands::review_commands_types::RequestChangesInput),
            (app_state),
        ],
        call: async,
        result: fallible,
    },

    // --- QA. `retry_qa` writes an unambiguous all-Pending reset; its sibling `skip_qa` is
    //     REFUSED because the verdict it writes does not mean what its name promises.
    //     `update_qa_settings` is an arming write with a DECLARED_MEMBERSHIPS row.
    "retry_qa" => crate::commands::qa_commands::retry_qa {
        class: AgentControl,
        caps: [AgentControl],
        params: [
            (arg task_id: String),
            (app_state),
        ],
        call: async,
        result: fallible,
    },
    "update_qa_settings" => crate::commands::qa_commands::update_qa_settings {
        class: AgentControl,
        caps: [AgentControl],
        params: [
            (arg input: crate::commands::qa_commands::UpdateQASettingsInput),
            (app_state),
        ],
        call: async,
        result: fallible,
    },

    // --- Plan / workflow / research writes.
    //
    // `clear_active_plan` is registered while its WRITE sibling `set_active_plan` stays
    // refused, and the asymmetry is the finding, not an oversight: set_active_plan swallows an
    // execution-plan lookup behind `if let Ok(Some(ep))` and discards the follow-up write with
    // `let _ =`; clear is one `active_plan_repo.clear` with its error propagated.
    "clear_active_plan" => crate::commands::plan_commands::clear_active_plan {
        class: AgentControl,
        caps: [AgentControl],
        params: [
            (arg project_id: String),
            (app_state),
        ],
        call: async,
        result: fallible,
    },
    "seed_builtin_workflows" => crate::commands::workflow_commands::seed_builtin_workflows {
        class: AgentControl,
        caps: [AgentControl],
        params: [(app_state)],
        call: async,
        result: fallible,
    },
    "start_research" => crate::commands::research_commands::start_research {
        class: AgentControl,
        caps: [AgentControl],
        params: [
            (arg input: crate::commands::research_commands::StartResearchInput),
            (app_state),
        ],
        call: async,
        result: fallible,
    },

    // --- The scheduler-quota arming write. Uses the `active_project_state` injection arm, and
    //     is the first registered command to do so. Declared, not detected: it writes
    //     `ExecutionState` atomics rather than an `InternalStatus`, so no detector models it.
    "set_active_project" => crate::commands::execution_commands::set_active_project {
        class: AgentControl,
        caps: [AgentControl],
        params: [
            (arg project_id: Option<String>),
            (active_project_state),
            (execution_state),
            (app_state),
        ],
        call: async,
        result: fallible,
    },

    // -----------------------------------------------------------------------------------
    // The spawn-free transcript reads (batch 4) — the PR 3.2 dependency.
    //
    // The LOCAL `get_agent_conversation` and its two page reads stay ABSENT, and their
    // absence is asserted rather than merely omitted (see
    // `the_local_transcript_reads_stay_unregistered`). Each of them opens by waking the
    // conversation's agent workspace, which reaches the `send_message` STEER sink; the wake
    // is incidental to the read (the local commands discard its error and read anyway), so
    // the answer is a seam split rather than a reclassification.
    //
    // These three delegate to the same `*_for_app_state` seams the local commands use, take
    // only `&AppState`, and therefore cannot reach the wake.
    // -----------------------------------------------------------------------------------
    "get_remote_agent_conversation"
        => crate::commands::remote_transcript_commands::get_remote_agent_conversation {
        class: Read,
        caps: [],
        params: [
            (arg conversation_id: String),
            (app_state),
        ],
        call: async,
        result: fallible,
    },
    "get_remote_agent_conversation_messages_page"
        => crate::commands::remote_transcript_commands::get_remote_agent_conversation_messages_page {
        class: Read,
        caps: [],
        params: [
            (arg conversation_id: String),
            (arg limit: Option<u32>),
            (arg offset: Option<u32>),
            (app_state),
        ],
        call: async,
        result: fallible,
    },
    "get_remote_agent_conversation_timeline_page"
        => crate::commands::remote_transcript_commands::get_remote_agent_conversation_timeline_page {
        class: Read,
        caps: [],
        params: [
            (arg conversation_id: String),
            (arg limit: Option<u32>),
            (arg before_sequence: Option<i64>),
            (app_state),
        ],
        call: async,
        result: fallible,
    },

    // -----------------------------------------------------------------------------------
    // WP3 — the un-truncated tool-call detail pair, at `ui:read`.
    //
    // The transcript reads above truncate tool payloads exactly as the local UI does, so a
    // remote client can SEE a delegate tool call but never expand it. These two are the
    // expansion, and they were refused by batch 4 only because
    // `load_delegated_tool_runtime_snapshot` swallowed five repository reads and could serve a
    // stale delegated snapshot as live. That fail-open is fixed at its source (one `AppResult`
    // seam, `Ok(None)` reserved for genuine absence), which is also what makes the transcript
    // trio's "propagates read errors" ledger reason true (follow-up A3/L2).
    //
    // Both take `&AppState` and repository reads only — no `AppHandle`, no `ExecutionState`,
    // no `ChatService` — the same three-carrier absence that licensed the transcript twins.
    // -----------------------------------------------------------------------------------
    "get_agent_message_tool_call_detail"
        => crate::commands::unified_chat_commands::get_agent_message_tool_call_detail {
        class: Read,
        caps: [],
        params: [
            (arg conversation_id: String),
            (arg message_id: String),
            (arg tool_call_id: Option<String>),
            (arg content_block_index: Option<u32>),
            (app_state),
        ],
        call: async,
        result: fallible,
    },
    "get_agent_timeline_item_tool_call_detail"
        => crate::commands::unified_chat_commands::get_agent_timeline_item_tool_call_detail {
        class: Read,
        caps: [],
        params: [
            (arg conversation_id: String),
            (arg timeline_item_id: String),
            (app_state),
        ],
        call: async,
        result: fallible,
    },

    // -----------------------------------------------------------------------------------
    // The conversation-LIST seam split (batch 5). These complete PR 3.2's read surface: the
    // transcript reads above are useless without a list to pick a conversation from.
    //
    // Unlike the transcript reads, the local list commands never fired detector (a) —
    // `probe_conversation_list_arming_paths` reports NO ARMING HITS for them. Their
    // disqualifier was that `list_agent_conversations` ACCEPTED `tauri::AppHandle` and
    // `ExecutionState` in order to build a chat service whose invoked method is a straight
    // repository delegation: authority carried, never used. (`list_agent_conversations_page`
    // never took an `AppHandle` at all; batch 4's deferral note was wrong about that.)
    //
    // Both local commands now call the same `*_for_app_state` seams these do, so the
    // extraction forks no logic (A-7) and drops two spawn-authority carriers from a read
    // command rather than merely routing around them.
    // -----------------------------------------------------------------------------------
    // Closes the read/write asymmetry: `get_execution_settings` is registered, so the pane
    // already shows the host's live values and could not persist a change to them.
    "update_remote_execution_settings"
        => crate::commands::remote_execution_settings_commands::update_remote_execution_settings {
        class: AgentControl,
        caps: [AgentControl],
        params: [
            (arg project_id: Option<String>),
            (arg input: crate::commands::execution_commands::UpdateExecutionSettingsInput),
            (execution_state),
            (app_state),
        ],
        call: async,
        result: fallible,
    },
    // Read/write-twin symmetry: the settings twin changes the host's scheduler configuration;
    // this status twin reports its actual state. It omits quota sync, stale-registry pruning,
    // and running-count caching, making the derivation process-inspection-free and write-free.
    "get_remote_execution_status"
        => crate::commands::remote_execution_status_commands::get_remote_execution_status {
        class: Read,
        caps: [],
        params: [
            (arg project_id: Option<String>),
            (execution_state),
            (app_state),
            (active_project_state),
        ],
        call: async,
        result: fallible,
    },

    // The workspace shell's two boot reads. Without them a connected client has no project
    // list and no provider answer, so it renders first-run onboarding over a populated host.
    "list_remote_projects"
        => crate::commands::remote_workspace_commands::list_remote_projects {
        class: Read,
        caps: [],
        params: [
            (app_state),
        ],
        call: async,
        result: fallible,
    },

    // The single-project read behind every project-scoped route. Without it a paired client
    // that lands on a project URL has a list but cannot load the project itself.
    "get_remote_project"
        => crate::commands::remote_workspace_commands::get_remote_project {
        class: Read,
        caps: [],
        params: [
            (arg id: String),
            (app_state),
        ],
        call: async,
        result: fallible,
    },

    "get_remote_provider_readiness"
        => crate::commands::remote_workspace_commands::get_remote_provider_readiness {
        class: Read,
        caps: [],
        params: [
            (app_state),
        ],
        call: async,
        result: fallible,
    },

    // The composer's provider feed: identity + stored selection (enabled/default/model/effort
    // names), never the `Denied` provider-settings surface (paths, probes, credentials,
    // process-config). Same spawn-free module and the same Read class as the two boot reads
    // above; a hand-written projection, never `AgentProviderSettings::into()`.
    "list_remote_agent_providers"
        => crate::commands::remote_workspace_commands::list_remote_agent_providers {
        class: Read,
        caps: [],
        params: [
            (app_state),
        ],
        call: async,
        result: fallible,
    },

    "list_remote_agent_conversations"
        => crate::commands::remote_transcript_commands::list_remote_agent_conversations {
        class: Read,
        caps: [],
        params: [
            (arg context_type: String),
            (arg context_id: String),
            (arg include_archived: Option<bool>),
            (app_state),
        ],
        call: async,
        result: fallible,
    },
    "list_remote_agent_conversations_page"
        => crate::commands::remote_transcript_commands::list_remote_agent_conversations_page {
        class: Read,
        caps: [],
        params: [
            (arg context_type: String),
            (arg context_id: String),
            (arg include_archived: Option<bool>),
            (arg archived_only: Option<bool>),
            (arg offset: Option<u32>),
            (arg limit: Option<u32>),
            (arg search: Option<String>),
            (app_state),
        ],
        call: async,
        result: fallible,
    },
    // The Agents-sidebar inbox read. Registered through the recovery-free `_for_app_state` seam
    // and the `worktree_path`-blanking facade twin, NOT the local `list_agent_sidebar_conversations`
    // (which schedules PR-supervision recovery and reaches the git CLI resolver — detector (c)).
    "list_remote_agent_sidebar_conversations"
        => crate::commands::remote_transcript_commands::list_remote_agent_sidebar_conversations {
        class: Read,
        caps: [],
        params: [
            (arg input: crate::commands::agent_sidebar_commands::AgentSidebarConversationsInput),
            (app_state),
        ],
        call: async,
        result: fallible,
    },

    // -----------------------------------------------------------------------------------
    // The B2 detector-silent getters (batch 4). Five of seventeen candidates; the other
    // twelve were refused or deferred — see `the_b2_getter_refusals_are_pinned`.
    // -----------------------------------------------------------------------------------
    "get_agent_conversation_summary"
        => crate::commands::unified_chat_commands::get_agent_conversation_summary {
        class: Read,
        caps: [],
        params: [
            (arg conversation_id: String),
            (app_state),
        ],
        call: async,
        result: fallible,
    },
    "get_agent_conversation_runtime_index"
        => crate::commands::unified_chat_commands::get_agent_conversation_runtime_index {
        class: Read,
        caps: [],
        params: [
            (arg conversation_id: String),
            (app_state),
            (execution_state),
        ],
        call: async,
        result: fallible,
    },
    // Turn-attribution reads (main #939): bounded repo lookups of persisted AgentRun rows.
    // Without them a remote transcript loses per-turn attribution the moment main's client
    // code renders it. Pure `agent_run_repo` reads — no spawn, no writes, no filesystem.
    "get_agent_run_attribution"
        => crate::commands::unified_chat_commands::get_agent_run_attribution {
        class: Read,
        caps: [],
        params: [
            (arg run_id: String),
            (app_state),
        ],
        call: async,
        result: fallible,
    },
    "get_agent_run_attributions"
        => crate::commands::unified_chat_commands::get_agent_run_attributions {
        class: Read,
        caps: [],
        params: [
            (arg run_ids: Vec<String>),
            (app_state),
        ],
        call: async,
        result: fallible,
    },
    "list_agent_conversation_workspace_publication_events"
        => crate::commands::unified_chat_commands::list_agent_conversation_workspace_publication_events {
        class: Read,
        caps: [],
        params: [
            (arg conversation_id: String),
            (app_state),
        ],
        call: async,
        result: fallible,
    },
    "get_bulk_workspace_publication_states"
        => crate::commands::agent_sidebar_commands::get_bulk_workspace_publication_states {
        class: Read,
        caps: [],
        params: [
            (arg conversation_ids: Vec<String>),
            (app_state),
        ],
        call: async,
        result: fallible,
    },
    "list_agent_models" => crate::commands::agent_model_commands::list_agent_models {
        class: Read,
        caps: [],
        params: [
            (app_state),
        ],
        call: async,
        result: fallible,
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
    // -----------------------------------------------------------------------------------
    // WP4 (a) — the eight rows batch 8/14 deferred on "AppError is not Serialize".
    //
    // It always was. `ralphx_domain::error` carries a hand-written `impl Serialize for
    // AppError`, and it has to: Tauri requires `Serialize` on a command's error type, which is
    // why the two `AppResult`-returning `task_step_commands` rows directly above this block
    // have been dispatching through the same `fallible` arm since they were registered. The
    // deferral was a false finding, not a shipped limitation, so these register at the class
    // their bodies earn with no transport change.
    //
    // The four status writes keep the detector-d capability pair; `reorder_task_steps` moves
    // sort_order only and does not.
    // -----------------------------------------------------------------------------------
    "start_step" => crate::commands::task_step_commands::start_step {
        class: AgentControl,
        caps: [AgentControl, MutatesAgentConsumedContent],
        params: [(arg step_id: String), (app_state)],
        call: async,
        result: fallible,
    },
    "complete_step" => crate::commands::task_step_commands::complete_step {
        class: AgentControl,
        caps: [AgentControl, MutatesAgentConsumedContent],
        params: [(arg step_id: String), (arg note: Option<String>), (app_state)],
        call: async,
        result: fallible,
    },
    "skip_step" => crate::commands::task_step_commands::skip_step {
        class: AgentControl,
        caps: [AgentControl, MutatesAgentConsumedContent],
        params: [(arg step_id: String), (arg reason: String), (app_state)],
        call: async,
        result: fallible,
    },
    "fail_step" => crate::commands::task_step_commands::fail_step {
        class: AgentControl,
        caps: [AgentControl, MutatesAgentConsumedContent],
        params: [(arg step_id: String), (arg error: String), (app_state)],
        call: async,
        result: fallible,
    },
    "reorder_task_steps" => crate::commands::task_step_commands::reorder_task_steps {
        class: AgentControl,
        caps: [AgentControl],
        params: [(arg task_id: String), (arg step_ids: Vec<String>), (app_state)],
        call: async,
        result: fallible,
    },
    "list_conversation_folder_references"
        => crate::commands::conversation_folder_reference_commands::list_conversation_folder_references {
        class: Read,
        caps: [],
        params: [(arg conversation_id: String), (app_state)],
        call: async,
        result: fallible,
    },
    // The authority-REDUCING half of the folder-reference pair. `add_…` stays deferred: its
    // stored path becomes an MCP filesystem root for every later spawn and has no project-root
    // allowlist, so adding widens a future agent's reach while removing only narrows it.
    "remove_conversation_folder_reference"
        => crate::commands::conversation_folder_reference_commands::remove_conversation_folder_reference {
        class: AgentControl,
        caps: [AgentControl],
        params: [
            (arg input: crate::commands::conversation_folder_reference_commands::RemoveConversationFolderReferenceInput),
            (app_state),
        ],
        call: async,
        result: fallible,
    },
    "abort_seeded_agent_conversation"
        => crate::commands::unified_chat_commands::abort_seeded_agent_conversation {
        class: AgentControl,
        caps: [AgentControl],
        params: [(arg conversation_id: String), (app_state)],
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

    // -----------------------------------------------------------------------------------
    // PR 3.1-b batch 7 — census `B3`, the review / QA / merge-pipeline read cluster, at
    // `ui:read`.
    //
    // Reclassifications, not newly-permissive rows: each sat at `AgentControl` only because
    // `review_commands` / `qa_commands` / `merge_pipeline_commands` default there, and those
    // defaults are conservative because the same modules hold the human approval actions,
    // `retry_qa`, and the review-settings write. The per-command audit — detectors (a), (b)
    // and (c) all silent, bodies hand-traced to repository or in-memory-store reads whose
    // errors propagate — is recorded in `capability_ledger` and pinned by the detector
    // calibration lists.
    //
    // The audit that authorized these ran AFTER this batch's `resolve_dispatch` fix. Before
    // it, any command delegating to an identically-named service had an empty closure and
    // read "detector-clean" vacuously; `get_task_validation_summary` is the member that
    // failed on the corrected graph and is NOT here.
    // -----------------------------------------------------------------------------------
    "get_pending_reviews" => crate::commands::review_commands::get_pending_reviews {
        class: Read,
        caps: [],
        params: [(arg project_id: String), (app_state)],
        call: async,
        result: fallible,
    },
    "get_review_by_id" => crate::commands::review_commands::get_review_by_id {
        class: Read,
        caps: [],
        params: [(arg review_id: String), (app_state)],
        call: async,
        result: fallible,
    },
    "get_reviews_by_task_id" => crate::commands::review_commands::get_reviews_by_task_id {
        class: Read,
        caps: [],
        params: [(arg task_id: String), (app_state)],
        call: async,
        result: fallible,
    },
    "get_task_state_history" => crate::commands::review_commands::get_task_state_history {
        class: Read,
        caps: [],
        params: [(arg task_id: String), (app_state)],
        call: async,
        result: fallible,
    },
    "get_fix_task_attempts" => crate::commands::review_commands::get_fix_task_attempts {
        class: Read,
        caps: [],
        params: [(arg task_id: String), (app_state)],
        call: async,
        result: fallible,
    },
    "get_task_issues" => crate::commands::review_commands::get_task_issues {
        class: Read,
        caps: [],
        params: [
            (arg task_id: String),
            (arg status_filter: Option<String>),
            (app_state),
        ],
        call: async,
        result: fallible,
    },
    "get_issue_progress" => crate::commands::review_commands::get_issue_progress {
        class: Read,
        caps: [],
        params: [(arg task_id: String), (app_state)],
        call: async,
        result: fallible,
    },
    "get_review_settings" => crate::commands::review_commands::get_review_settings {
        class: Read,
        caps: [],
        params: [(app_state)],
        call: async,
        result: fallible,
    },
    "get_qa_settings" => crate::commands::qa_commands::get_qa_settings {
        class: Read,
        caps: [],
        params: [(app_state)],
        call: async,
        result: fallible,
    },
    "get_task_qa" => crate::commands::qa_commands::get_task_qa {
        class: Read,
        caps: [],
        params: [(arg task_id: String), (app_state)],
        call: async,
        result: fallible,
    },
    "get_qa_results" => crate::commands::qa_commands::get_qa_results {
        class: Read,
        caps: [],
        params: [(arg task_id: String), (app_state)],
        call: async,
        result: fallible,
    },
    "get_merge_pipeline" => crate::commands::merge_pipeline_commands::get_merge_pipeline {
        class: Read,
        caps: [],
        params: [
            (arg project_id: Option<String>),
            (active_project_state),
            (app_state),
        ],
        call: async,
        result: fallible,
    },
    "get_merge_progress" => crate::commands::merge_pipeline_commands::get_merge_progress {
        class: Read,
        caps: [],
        params: [(arg task_id: String)],
        call: async,
        result: fallible,
    },
    "get_merge_phase_list" => crate::commands::merge_pipeline_commands::get_merge_phase_list {
        class: Read,
        caps: [],
        params: [(arg task_id: String)],
        call: async,
        result: fallible,
    },

    // -----------------------------------------------------------------------------------
    // PR 3.1-b batch 7 — census `B4`, the plan / methodology / workflow read cluster, at
    // `ui:read`.
    //
    // Same reclassification shape as the `B3` cluster above: `AgentControl` by module default
    // only. `plan_commands` defaults conservatively because it also holds `set_active_plan`
    // and `clear_active_plan`, which steer which plan the Kanban/Graph surfaces and the
    // scheduler read; `workflow_commands` because it holds the workflow writers.
    //
    // `set_active_plan` is NOT here and is not merely "the write half": its body swallows two
    // errors (`if let Ok(Some(ep))` on the execution-plan lookup, `let _ =` on the
    // `set_execution_plan_id` write), so a partial application reports success. A fail-open
    // shape gets fixed or refused, never registered — see `b4_members_that_audit_dirty...`.
    // -----------------------------------------------------------------------------------
    "get_active_plan" => crate::commands::plan_commands::get_active_plan {
        class: Read,
        caps: [],
        params: [(arg project_id: String), (app_state)],
        call: async,
        result: fallible,
    },
    "get_active_execution_plan" => crate::commands::plan_commands::get_active_execution_plan {
        class: Read,
        caps: [],
        params: [(arg project_id: String), (app_state)],
        call: async,
        result: fallible,
    },
    "list_plan_selector_candidates"
        => crate::commands::plan_commands::list_plan_selector_candidates {
        class: Read,
        caps: [],
        params: [
            (arg project_id: String),
            (arg query: Option<String>),
            (app_state),
        ],
        call: async,
        result: fallible,
    },
    "get_methodologies" => crate::commands::methodology_commands::get_methodologies {
        class: Read,
        caps: [],
        params: [(app_state)],
        call: async,
        result: fallible,
    },
    "get_active_methodology" => crate::commands::methodology_commands::get_active_methodology {
        class: Read,
        caps: [],
        params: [(app_state)],
        call: async,
        result: fallible,
    },
    "get_workflows" => crate::commands::workflow_commands::get_workflows {
        class: Read,
        caps: [],
        params: [(app_state)],
        call: async,
        result: fallible,
    },
    "get_workflow" => crate::commands::workflow_commands::get_workflow {
        class: Read,
        caps: [],
        params: [(arg id: String), (app_state)],
        call: async,
        result: fallible,
    },
    "get_builtin_workflows" => crate::commands::workflow_commands::get_builtin_workflows {
        class: Read,
        caps: [],
        params: [],
        call: async,
        result: fallible,
    },
    "get_active_workflow_columns"
        => crate::commands::workflow_commands::get_active_workflow_columns {
        class: Read,
        caps: [],
        params: [(app_state)],
        call: async,
        result: fallible,
    },

    // -----------------------------------------------------------------------------------
    // PR 3.1-b batch 8 — census `B2`, ONE row, at `ui:read`.
    //
    // `B2` is the census's highest-risk batch: it also holds the detector-(a) steer sink
    // (`send_agent_message`), the workspace `git push` surface, and the conversation
    // lifecycle writes. The module default stays `AgentControl` precisely because those
    // neighbours live in it, and this row is an exception to the default, not a relaxation
    // of it.
    //
    // `search_agent_composer_plan_references` was refused by an earlier batch under the
    // fail-open group. That fail-open is FIXED (the resolver call now propagates instead of
    // falling back to the pre-resolution id and silently dropping sessions from a list whose
    // `truncated` flag still read "complete"), and the earlier pin said explicitly that a
    // repaired error path is not a registration decision — clearing it needs the per-command
    // audit. That audit is this batch's: pure read, every repository error propagated via
    // `map_err(...)?`, no `AppHandle`/`ExecutionState`/chat service, and no route through
    // `agent_workspace_response_for_state`.
    //
    // Deliberately NOT here, each on its own finding: `list_agent_composer_skills` is
    // fail-open; `get_agent_run_status_unified` and `get_queued_agent_messages` build a
    // spawn-capable chat service to serve a read; `list_conversation_folder_references`
    // returns `AppError`, which is not `Serialize`; and `list_agent_conversations` /
    // `list_agent_conversations_page` stay refused because batch 5 already answered them
    // with the registered `list_remote_agent_conversations*` twins in
    // `remote_transcript_commands`. See `capability_ledger_tests`.
    // -----------------------------------------------------------------------------------
    "search_agent_composer_plan_references" => crate::commands::agent_composer_commands::search_agent_composer_plan_references {
        class: Read,
        caps: [],
        params: [
            (arg input: crate::commands::agent_composer_commands::SearchAgentComposerPlanReferencesInput),
            (app_state),
        ],
        call: async,
        result: fallible,
    },

    // -----------------------------------------------------------------------------------
    // PR 3.1-b batch 11 — census B4 remainder (ideation, workflow, methodology).
    //
    // The reads first. Each is ledgered `Read` on a body audit that found NO repository write,
    // not on detector silence — the whole B4 module sits at the conservative `AgentControl`
    // default and every drop below it is an exception carrying its own reason.
    //
    // Deliberately NOT here, each on its own finding: `analyze_dependencies` is a read in name
    // only and swallows the acknowledged-flag write; `export_ideation_session`,
    // `create_task_proposal`, `create_cross_project_session` and `import_ideation_session` are
    // fail-open; `get_agent_harness_availability` and `get_ideation_harness_availability` report
    // an unavailable harness as available when a settings read fails; and twelve members reach a
    // CLI resolver in their own closure and are `host-denied-spawns-process`. See
    // `capability_ledger_tests`.
    // -----------------------------------------------------------------------------------
    "get_ideation_session" => crate::commands::ideation_commands::get_ideation_session {
        class: Read,
        caps: [],
        params: [(arg id: String), (app_state)],
        call: async,
        result: fallible,
    },
    "get_ideation_session_with_data" => crate::commands::ideation_commands::get_ideation_session_with_data {
        class: Read,
        caps: [],
        params: [(arg id: String), (app_state)],
        call: async,
        result: fallible,
    },
    // The `*_for_app_state` seam here resolves the linked workspace from three repository reads.
    // It is NOT the `agent_workspace_response_for_state` hydrator that forecloses the
    // agent-conversation workspace reads, and there is no registered remote twin of this seam,
    // so it registers on its own audit rather than as a twin.
    "get_ideation_agent_workspace" => crate::commands::ideation_commands::get_ideation_agent_workspace {
        class: Read,
        caps: [],
        params: [(arg session_id: String), (app_state)],
        call: async,
        result: fallible,
    },
    "list_ideation_sessions" => crate::commands::ideation_commands::list_ideation_sessions {
        class: Read,
        caps: [],
        params: [(arg project_id: String), (arg purpose: Option<String>), (app_state)],
        call: async,
        result: fallible,
    },
    "get_session_group_counts" => crate::commands::ideation_commands::get_session_group_counts {
        class: Read,
        caps: [],
        params: [(arg project_id: String), (arg search: Option<String>), (app_state)],
        call: async,
        result: fallible,
    },
    "list_sessions_by_group" => crate::commands::ideation_commands::list_sessions_by_group {
        class: Read,
        caps: [],
        params: [
            (arg project_id: String),
            (arg group: String),
            (arg offset: Option<u32>),
            (arg limit: Option<u32>),
            (arg search: Option<String>),
            (app_state),
        ],
        call: async,
        result: fallible,
    },
    "get_child_sessions" => crate::commands::ideation_commands::get_child_sessions {
        class: Read,
        caps: [],
        params: [(arg session_id: String), (arg purpose: Option<String>), (app_state)],
        call: async,
        result: fallible,
    },
    "get_latest_child_session_id" => crate::commands::ideation_commands::get_latest_child_session_id {
        class: Read,
        caps: [],
        params: [
            (arg session_id: String),
            (arg purpose: Option<String>),
            (arg include_archived: Option<bool>),
            (app_state),
        ],
        call: async,
        result: fallible,
    },
    "get_task_proposal" => crate::commands::ideation_commands::get_task_proposal {
        class: Read,
        caps: [],
        params: [(arg id: String), (app_state)],
        call: async,
        result: fallible,
    },
    "list_session_proposals" => crate::commands::ideation_commands::list_session_proposals {
        class: Read,
        caps: [],
        params: [(arg session_id: String), (app_state)],
        call: async,
        result: fallible,
    },
    "get_proposal_dependencies" => crate::commands::ideation_commands::get_proposal_dependencies {
        class: Read,
        caps: [],
        params: [(arg proposal_id: String), (app_state)],
        call: async,
        result: fallible,
    },
    "get_proposal_dependents" => crate::commands::ideation_commands::get_proposal_dependents {
        class: Read,
        caps: [],
        params: [(arg proposal_id: String), (app_state)],
        call: async,
        result: fallible,
    },
    "get_task_blockers" => crate::commands::ideation_commands::get_task_blockers {
        class: Read,
        caps: [],
        params: [(arg task_id: String), (app_state)],
        call: async,
        result: fallible,
    },
    "get_blocked_tasks" => crate::commands::ideation_commands::get_blocked_tasks {
        class: Read,
        caps: [],
        params: [(arg task_id: String), (app_state)],
        call: async,
        result: fallible,
    },
    // The read half of the tasks-feature toggle pair. It aggregates counts and never emits or
    // persists; its writing sibling `set_tasks_feature_enabled` reaches a CLI resolver and is
    // `host-denied-spawns-process`.
    "get_tasks_disable_impact" => crate::commands::ideation_commands::get_tasks_disable_impact {
        class: Read,
        caps: [],
        params: [(app_state), (execution_state), (host_app_handle)],
        call: async,
        result: fallible,
    },
    "get_ideation_settings" => crate::commands::ideation_commands::get_ideation_settings {
        class: Read,
        caps: [],
        params: [(app_state)],
        call: async,
        result: fallible,
    },
    "get_ideation_effort_settings" => crate::commands::ideation_commands::get_ideation_effort_settings {
        class: Read,
        caps: [],
        params: [(arg project_id: Option<String>), (app_state)],
        call: async,
        result: fallible,
    },
    "get_ideation_model_settings" => crate::commands::ideation_commands::get_ideation_model_settings {
        class: Read,
        caps: [],
        params: [(arg project_id: Option<String>), (app_state)],
        call: async,
        result: fallible,
    },
    "get_agent_lane_settings" => crate::commands::ideation_commands::get_agent_lane_settings {
        class: Read,
        caps: [],
        params: [(arg project_id: Option<String>), (app_state)],
        call: async,
        result: fallible,
    },

    // The B4 writers, at `ui:agent`. A silent detector never licensed dropping these to Read.
    "update_ideation_session_title" => crate::commands::ideation_commands::update_ideation_session_title {
        class: AgentControl,
        caps: [AgentControl],
        params: [
            (arg id: String),
            (arg title: Option<String>),
            (app_state),
            (host_app_handle),
        ],
        call: async,
        result: fallible,
    },
    "reorder_proposals" => crate::commands::ideation_commands::reorder_proposals {
        class: AgentControl,
        caps: [AgentControl],
        params: [(arg session_id: String), (arg proposal_ids: Vec<String>), (app_state)],
        call: async,
        result: fallible,
    },
    "assess_proposal_priority" => crate::commands::ideation_commands::assess_proposal_priority {
        class: AgentControl,
        caps: [AgentControl],
        params: [(arg id: String), (app_state)],
        call: async,
        result: fallible,
    },
    "assess_all_priorities" => crate::commands::ideation_commands::assess_all_priorities {
        class: AgentControl,
        caps: [AgentControl],
        params: [(arg session_id: String), (app_state)],
        call: async,
        result: fallible,
    },
    "remove_proposal_dependency" => crate::commands::ideation_commands::remove_proposal_dependency {
        class: AgentControl,
        caps: [AgentControl],
        params: [(arg proposal_id: String), (arg depends_on_id: String), (app_state)],
        call: async,
        result: fallible,
    },
    // Declared `arms-auto-plan-verification`: no detector models the gate this writes.
    "update_ideation_settings" => crate::commands::ideation_commands::update_ideation_settings {
        class: AgentControl,
        caps: [AgentControl],
        params: [
            (arg settings: crate::domain::ideation::IdeationSettings),
            (app_state),
        ],
        call: async,
        result: fallible,
    },
    "update_ideation_effort_settings" => crate::commands::ideation_commands::update_ideation_effort_settings {
        class: AgentControl,
        caps: [AgentControl],
        params: [
            (arg input: crate::commands::ideation_commands::UpdateIdeationEffortInput),
            (app_state),
        ],
        call: async,
        result: fallible,
    },
    "update_ideation_model_settings" => crate::commands::ideation_commands::update_ideation_model_settings {
        class: AgentControl,
        caps: [AgentControl],
        params: [
            (arg input: crate::commands::ideation_commands::UpdateIdeationModelInput),
            (app_state),
        ],
        call: async,
        result: fallible,
    },
    // Declared `arms-agent-spawn-harness`: this row picks the harness a live agent launches with.
    "update_agent_lane_settings" => crate::commands::ideation_commands::update_agent_lane_settings {
        class: AgentControl,
        caps: [AgentControl],
        params: [
            (arg input: crate::commands::ideation_commands::UpdateAgentLaneSettingsInput),
            (app_state),
        ],
        call: async,
        result: fallible,
    },
    "create_workflow" => crate::commands::workflow_commands::create_workflow {
        class: AgentControl,
        caps: [AgentControl],
        params: [
            (arg input: crate::commands::workflow_commands::CreateWorkflowInput),
            (app_state),
        ],
        call: async,
        result: fallible,
    },
    "update_workflow" => crate::commands::workflow_commands::update_workflow {
        class: AgentControl,
        caps: [AgentControl],
        params: [
            (arg id: String),
            (arg input: crate::commands::workflow_commands::UpdateWorkflowInput),
            (app_state),
        ],
        call: async,
        result: fallible,
    },
    "set_default_workflow" => crate::commands::workflow_commands::set_default_workflow {
        class: AgentControl,
        caps: [AgentControl],
        params: [(arg id: String), (app_state)],
        call: async,
        result: fallible,
    },
    "activate_methodology" => crate::commands::methodology_commands::activate_methodology {
        class: AgentControl,
        caps: [AgentControl],
        params: [(arg id: String), (app_state)],
        call: async,
        result: fallible,
    },
    "deactivate_methodology" => crate::commands::methodology_commands::deactivate_methodology {
        class: AgentControl,
        caps: [AgentControl],
        params: [(arg id: String), (app_state)],
        call: async,
        result: fallible,
    },
    // -----------------------------------------------------------------------------------
    // PR 3.1-b batch 12 — census B5 (activity, automation, metrics, research).
    //
    // The reads first. Each is ledgered `Read` on a body audit that found NO repository write.
    // Detector silence did not buy any of them, and this block is the reason that rule is worth
    // keeping: detector (a) fires on `save_metrics_config`, a one-statement settings upsert,
    // purely because the bare name `execute` collides with `AgentWorkflowRunner::execute`.
    //
    // Deliberately NOT here: `create_automation_draft`, `trigger_automation_run_now` and
    // `retry_automation_judge` reach a real launch and are `host-denied-spawns-process`.
    // -----------------------------------------------------------------------------------
    "list_task_activity_events" => crate::commands::activity_commands::list_task_activity_events {
        class: Read,
        caps: [],
        params: [
            (arg task_id: String),
            (arg cursor: Option<String>),
            (arg limit: Option<u32>),
            (arg filter: Option<crate::commands::activity_commands::ActivityEventFilterInput>),
            (app_state),
        ],
        call: async,
        result: fallible,
    },
    "list_session_activity_events" => crate::commands::activity_commands::list_session_activity_events {
        class: Read,
        caps: [],
        params: [
            (arg session_id: String),
            (arg cursor: Option<String>),
            (arg limit: Option<u32>),
            (arg filter: Option<crate::commands::activity_commands::ActivityEventFilterInput>),
            (app_state),
        ],
        call: async,
        result: fallible,
    },
    "list_all_activity_events" => crate::commands::activity_commands::list_all_activity_events {
        class: Read,
        caps: [],
        params: [
            (arg cursor: Option<String>),
            (arg limit: Option<u32>),
            (arg filter: Option<crate::commands::activity_commands::ActivityEventFilterInput>),
            (app_state),
        ],
        call: async,
        result: fallible,
    },
    "count_task_activity_events" => crate::commands::activity_commands::count_task_activity_events {
        class: Read,
        caps: [],
        params: [
            (arg task_id: String),
            (arg filter: Option<crate::commands::activity_commands::ActivityEventFilterInput>),
            (app_state),
        ],
        call: async,
        result: fallible,
    },
    "count_session_activity_events" => crate::commands::activity_commands::count_session_activity_events {
        class: Read,
        caps: [],
        params: [
            (arg session_id: String),
            (arg filter: Option<crate::commands::activity_commands::ActivityEventFilterInput>),
            (app_state),
        ],
        call: async,
        result: fallible,
    },
    "get_project_stats" => crate::commands::metrics_commands::get_project_stats {
        class: Read,
        caps: [],
        params: [
            (arg project_id: String),
            (arg week_start_day: Option<u8>),
            (arg tz_offset_minutes: Option<i32>),
            (app_state),
        ],
        call: async,
        result: fallible,
    },
    "get_insights_stats" => crate::commands::metrics_commands::get_insights_stats {
        class: Read,
        caps: [],
        params: [
            (arg project_id: Option<String>),
            (arg week_start_day: Option<u8>),
            (arg tz_offset_minutes: Option<i32>),
            (app_state),
        ],
        call: async,
        result: fallible,
    },
    "get_project_trends" => crate::commands::metrics_commands::get_project_trends {
        class: Read,
        caps: [],
        params: [
            (arg project_id: String),
            (arg week_start_day: Option<u8>),
            (arg tz_offset_minutes: Option<i32>),
            (app_state),
        ],
        call: async,
        result: fallible,
    },
    "get_insights_trends" => crate::commands::metrics_commands::get_insights_trends {
        class: Read,
        caps: [],
        params: [
            (arg project_id: Option<String>),
            (arg week_start_day: Option<u8>),
            (arg tz_offset_minutes: Option<i32>),
            (app_state),
        ],
        call: async,
        result: fallible,
    },
    "get_project_pr_insights" => crate::commands::metrics_commands::get_project_pr_insights {
        class: Read,
        caps: [],
        params: [
            (arg project_id: String),
            (arg week_start_day: Option<u8>),
            (arg tz_offset_minutes: Option<i32>),
            (app_state),
        ],
        call: async,
        result: fallible,
    },
    "get_insights_pr_insights" => crate::commands::metrics_commands::get_insights_pr_insights {
        class: Read,
        caps: [],
        params: [
            (arg project_id: Option<String>),
            (arg week_start_day: Option<u8>),
            (arg tz_offset_minutes: Option<i32>),
            (app_state),
        ],
        call: async,
        result: fallible,
    },
    "get_metrics_config" => crate::commands::metrics_commands::get_metrics_config {
        class: Read,
        caps: [],
        params: [(arg project_id: String), (app_state)],
        call: async,
        result: fallible,
    },
    "get_task_metrics" => crate::commands::metrics_commands::get_task_metrics {
        class: Read,
        caps: [],
        params: [(arg task_id: String), (app_state)],
        call: async,
        result: fallible,
    },
    // Takes no AppState at all — a pure function over the preset table.
    "get_research_presets" => crate::commands::research_commands::get_research_presets {
        class: Read,
        caps: [],
        params: [],
        call: async,
        result: fallible,
    },
    "get_research_process" => crate::commands::research_commands::get_research_process {
        class: Read,
        caps: [],
        params: [(arg id: String), (app_state)],
        call: async,
        result: fallible,
    },
    "get_research_processes" => crate::commands::research_commands::get_research_processes {
        class: Read,
        caps: [],
        params: [(arg status: Option<String>), (app_state)],
        call: async,
        result: fallible,
    },
    "list_automations" => crate::commands::automation_commands::list_automations {
        class: Read,
        caps: [],
        params: [
            (arg input: Option<crate::commands::automation_commands::ListAutomationsInput>),
            (app_state),
        ],
        call: async,
        result: fallible,
    },
    "get_automation" => crate::commands::automation_commands::get_automation {
        class: Read,
        caps: [],
        params: [
            (arg input: crate::commands::automation_commands::AutomationIdInput),
            (app_state),
        ],
        call: async,
        result: fallible,
    },
    // The writers. Registered at `ui:agent` on a body audit, never dropped to Read.
    "save_metrics_config" => crate::commands::metrics_commands::save_metrics_config {
        class: AgentControl,
        caps: [AgentControl],
        params: [
            (arg project_id: String),
            (arg config: crate::commands::metrics_commands::MetricsConfig),
            (app_state),
        ],
        call: async,
        result: fallible,
    },
    // The research trio writes ResearchProcessStatus, which no production loop scans; the
    // already-registered `start_research` reaches the same Running value on the same basis.
    "pause_research" => crate::commands::research_commands::pause_research {
        class: AgentControl,
        caps: [AgentControl],
        params: [(arg id: String), (app_state)],
        call: async,
        result: fallible,
    },
    "resume_research" => crate::commands::research_commands::resume_research {
        class: AgentControl,
        caps: [AgentControl],
        params: [(arg id: String), (app_state)],
        call: async,
        result: fallible,
    },
    "stop_research" => crate::commands::research_commands::stop_research {
        class: AgentControl,
        caps: [AgentControl],
        params: [(arg id: String), (app_state)],
        call: async,
        result: fallible,
    },
    "pause_automation" => crate::commands::automation_commands::pause_automation {
        class: AgentControl,
        caps: [AgentControl],
        params: [
            (arg input: crate::commands::automation_commands::PauseAutomationInput),
            (app_state),
        ],
        call: async,
        result: fallible,
    },
    "stop_automation" => crate::commands::automation_commands::stop_automation {
        class: AgentControl,
        caps: [AgentControl],
        params: [
            (arg input: crate::commands::automation_commands::AutomationIdInput),
            (app_state),
        ],
        call: async,
        result: fallible,
    },
    "cancel_automation_run" => crate::commands::automation_commands::cancel_automation_run {
        class: AgentControl,
        caps: [AgentControl],
        params: [
            (arg input: crate::commands::automation_commands::AutomationRunScopedInput),
            (app_state),
        ],
        call: async,
        result: fallible,
    },
    "update_automation_settings" => crate::commands::automation_commands::update_automation_settings {
        class: AgentControl,
        caps: [AgentControl],
        params: [
            (arg input: crate::commands::automation_commands::UpdateAutomationSettingsInput),
            (app_state),
        ],
        call: async,
        result: fallible,
    },
    // The four arming writes. Each flips `automations.status` to Active — the armed value
    // `spawn_automation_scheduler` scans. Only `resume_automation_run` carries
    // `SeedsSpawnTriggeringState`: that capability is defined as detector-(b) EVIDENCE by
    // `seeds_spawn_triggering_state_tags_track_detector_b_evidence`, and it is the only one the
    // detector flags. The other three arm just as really but invisibly, so they take AGENT plus a
    // `DECLARED_MEMBERSHIPS` row — the mechanism batches 10 and 11 used for exactly this case.
    "restart_automation" => crate::commands::automation_commands::restart_automation {
        class: AgentControl,
        caps: [AgentControl],
        params: [
            (arg input: crate::commands::automation_commands::AutomationIdInput),
            (app_state),
        ],
        call: async,
        result: fallible,
    },
    "resume_automation_run" => crate::commands::automation_commands::resume_automation_run {
        class: AgentControl,
        caps: [SeedsSpawnTriggeringState],
        params: [
            (arg input: crate::commands::automation_commands::AutomationRunScopedInput),
            (app_state),
        ],
        call: async,
        result: fallible,
    },
    "retry_automation_plan_judge" => crate::commands::automation_commands::retry_automation_plan_judge {
        class: AgentControl,
        caps: [AgentControl],
        params: [
            (arg input: crate::commands::automation_commands::AutomationIdInput),
            (app_state),
        ],
        call: async,
        result: fallible,
    },
    "skip_automation_judge" => crate::commands::automation_commands::skip_automation_judge {
        class: AgentControl,
        caps: [AgentControl],
        params: [
            (arg input: crate::commands::automation_commands::AutomationRunScopedInput),
            (app_state),
        ],
        call: async,
        result: fallible,
    },

    // -----------------------------------------------------------------------------------
    // PR 3.1-b batch 13 — census B7 (artifact, notification, release-notes, task-context, ui,
    // update-channel) plus two B6 modules (persona, MCP policy).
    //
    // The reads first, each `Read` on a body audit rather than on detector silence. Batch 12
    // measured this whole block detector-silent; this batch re-measured it and then read every
    // body anyway, which is how it found that detector (a) over-reports seven persona writes,
    // detector (b) over-reports `update_notification_settings`, and — the one that matters —
    // detector (c) UNDER-reports `retry_legacy_mcp_registration_repair`, which really does spawn
    // the Claude CLI.
    //
    // Deliberately NOT here: `get_mcp_catalog`, `refresh_mcp_catalog` and
    // `retry_legacy_mcp_registration_repair` are host-denied-spawns-process, and
    // `set_update_channel` is Elevated/HostManagement.
    // -----------------------------------------------------------------------------------
    "get_artifacts" => crate::commands::artifact_commands::get_artifacts {
        class: Read,
        caps: [],
        params: [(arg artifact_type: Option<String>), (app_state)],
        call: async,
        result: fallible,
    },
    "get_artifact" => crate::commands::artifact_commands::get_artifact {
        class: Read,
        caps: [],
        params: [(arg id: String), (app_state)],
        call: async,
        result: fallible,
    },
    "get_artifact_at_version" => crate::commands::artifact_commands::get_artifact_at_version {
        class: Read,
        caps: [],
        params: [(arg id: String), (arg version: u32), (app_state)],
        call: async,
        result: fallible,
    },
    "get_artifacts_by_bucket" => crate::commands::artifact_commands::get_artifacts_by_bucket {
        class: Read,
        caps: [],
        params: [(arg bucket_id: String), (app_state)],
        call: async,
        result: fallible,
    },
    "get_artifacts_by_task" => crate::commands::artifact_commands::get_artifacts_by_task {
        class: Read,
        caps: [],
        params: [(arg task_id: String), (app_state)],
        call: async,
        result: fallible,
    },
    "get_artifact_version_history" => crate::commands::artifact_commands::get_artifact_version_history {
        class: Read,
        caps: [],
        params: [(arg id: String), (app_state)],
        call: async,
        result: fallible,
    },
    "get_buckets" => crate::commands::artifact_commands::get_buckets {
        class: Read,
        caps: [],
        params: [(app_state)],
        call: async,
        result: fallible,
    },
    // Takes no AppState at all — the `get_research_presets` shape batch 12 registered.
    "get_system_buckets" => crate::commands::artifact_commands::get_system_buckets {
        class: Read,
        caps: [],
        params: [],
        call: async,
        result: fallible,
    },
    "get_artifact_relations" => crate::commands::artifact_commands::get_artifact_relations {
        class: Read,
        caps: [],
        params: [(arg artifact_id: String), (app_state)],
        call: async,
        result: fallible,
    },
    // Registered through the facade shim, not the raw command: the remote wire must carry only
    // the `WorkerTaskView` allowlist, while the same command serves the FULL `Task` locally.
    // See `remote_server::task_projection`.
    "get_task_context" => crate::remote_server::task_projection::get_task_context {
        class: Read,
        caps: [],
        params: [(arg task_id: String), (app_state)],
        call: async,
        result: fallible,
    },
    "get_artifact_full" => crate::commands::task_context_commands::get_artifact_full {
        class: Read,
        caps: [],
        params: [(arg artifact_id: String), (app_state)],
        call: async,
        result: fallible,
    },
    "get_artifact_version" => crate::commands::task_context_commands::get_artifact_version {
        class: Read,
        caps: [],
        params: [(arg artifact_id: String), (arg version: u32), (app_state)],
        call: async,
        result: fallible,
    },
    "get_related_artifacts" => crate::commands::task_context_commands::get_related_artifacts {
        class: Read,
        caps: [],
        params: [(arg artifact_id: String), (app_state)],
        call: async,
        result: fallible,
    },
    "search_artifacts" => crate::commands::task_context_commands::search_artifacts {
        class: Read,
        caps: [],
        params: [
            (arg input: crate::commands::task_context_commands::SearchArtifactsInput),
            (app_state),
        ],
        call: async,
        result: fallible,
    },
    "get_notification_settings" => crate::commands::notification_commands::get_notification_settings {
        class: Read,
        caps: [],
        params: [(app_state)],
        call: async,
        result: fallible,
    },
    "get_unread_notification_count" => crate::commands::notification_commands::get_unread_notification_count {
        class: Read,
        caps: [],
        params: [(arg project_id: Option<String>), (app_state)],
        call: async,
        result: fallible,
    },
    "list_attention_items" => crate::commands::notification_commands::list_attention_items {
        class: Read,
        caps: [],
        params: [(arg project_id: Option<String>), (app_state)],
        call: async,
        result: fallible,
    },
    "list_notifications" => crate::commands::notification_commands::list_notifications {
        class: Read,
        caps: [],
        params: [
            (arg project_id: Option<String>),
            (arg cursor: Option<String>),
            (arg limit: Option<u32>),
            (app_state),
        ],
        call: async,
        result: fallible,
    },
    "get_current_release_notes" => crate::commands::release_notes_commands::get_current_release_notes {
        class: Read,
        caps: [],
        params: [(host_app_handle)],
        call: async,
        result: fallible,
    },
    "get_release_notes_for_version" => crate::commands::release_notes_commands::get_release_notes_for_version {
        class: Read,
        caps: [],
        params: [(host_app_handle), (arg version: String)],
        call: async,
        result: fallible,
    },
    "get_last_seen_release_notes_version" => crate::commands::release_notes_commands::get_last_seen_release_notes_version {
        class: Read,
        caps: [],
        params: [(app_state)],
        call: async,
        result: fallible,
    },
    // Fixed before registration: the directory reader was `.ok()`-swallowed, so an unreadable
    // release-notes root reported an empty version list. It now propagates every non-NotFound error.
    "list_release_notes_versions" => crate::commands::release_notes_commands::list_release_notes_versions {
        class: Read,
        caps: [],
        params: [(host_app_handle)],
        call: async,
        result: fallible,
    },
    "list_personas" => crate::commands::persona_commands::list_personas {
        class: Read,
        caps: [],
        params: [
            (arg input: crate::commands::persona_commands::ListPersonasInput),
            (app_state),
        ],
        call: async,
        result: fallible,
    },
    "get_persona" => crate::commands::persona_commands::get_persona {
        class: Read,
        caps: [],
        params: [
            (arg input: crate::commands::persona_commands::PersonaIdInput),
            (app_state),
        ],
        call: async,
        result: fallible,
    },
    "list_persona_usage" => crate::commands::persona_commands::list_persona_usage {
        class: Read,
        caps: [],
        params: [(app_state)],
        call: async,
        result: fallible,
    },
    "preview_persona_overlay" => crate::commands::persona_commands::preview_persona_overlay {
        class: Read,
        caps: [],
        params: [
            (arg input: crate::commands::persona_commands::PreviewPersonaOverlayInput),
            (app_state),
        ],
        call: async,
        result: fallible,
    },
    "get_ui_feature_flags" => crate::commands::ui_commands::get_ui_feature_flags {
        class: Read,
        caps: [],
        params: [(app_state)],
        call: sync,
        result: infallible,
    },
    "get_update_channel" => crate::commands::update_channel_commands::get_update_channel {
        class: Read,
        caps: [],
        params: [(app_state)],
        call: async,
        result: fallible,
    },
    // The writers, at `ui:agent`. Never dropped to Read on a silent detector.
    "archive_artifact" => crate::commands::artifact_commands::archive_artifact {
        class: AgentControl,
        caps: [MutatesAgentConsumedContent],
        params: [(arg artifact_id: String), (app_state), (host_app_handle)],
        call: async,
        result: fallible,
    },
    "create_bucket" => crate::commands::artifact_commands::create_bucket {
        class: AgentControl,
        caps: [AgentControl],
        params: [
            (arg input: crate::commands::artifact_commands::CreateBucketInput),
            (app_state),
        ],
        call: async,
        result: fallible,
    },
    "mark_notification_read" => crate::commands::notification_commands::mark_notification_read {
        class: AgentControl,
        caps: [AgentControl],
        params: [(arg id: String), (app_state)],
        call: async,
        result: fallible,
    },
    "mark_all_notifications_read" => crate::commands::notification_commands::mark_all_notifications_read {
        class: AgentControl,
        caps: [AgentControl],
        params: [(arg project_id: Option<String>), (app_state)],
        call: async,
        result: fallible,
    },
    "set_dock_badge_count" => crate::commands::notification_commands::set_dock_badge_count {
        class: AgentControl,
        caps: [AgentControl],
        params: [(arg count: u32), (host_app_handle)],
        call: sync,
        result: fallible,
    },
    // Detector (b) fires on this row; the ledger records WHY it does not claim the evidence
    // capability (a bare-name marker collision on `update_settings`).
    "update_notification_settings" => crate::commands::notification_commands::update_notification_settings {
        class: AgentControl,
        caps: [AgentControl],
        params: [
            (arg input: crate::commands::notification_commands::UpdateNotificationSettingsInput),
            (app_state),
        ],
        call: async,
        result: fallible,
    },
    "mark_release_notes_seen" => crate::commands::release_notes_commands::mark_release_notes_seen {
        class: AgentControl,
        caps: [AgentControl],
        params: [(arg version: String), (app_state)],
        call: async,
        result: fallible,
    },
    // The eight persona writes. Persona bodies are injected into agent prompts, so each carries
    // MutatesAgentConsumedContent rather than a bare AgentControl.
    "create_persona_draft" => crate::commands::persona_commands::create_persona_draft {
        class: AgentControl,
        caps: [MutatesAgentConsumedContent],
        params: [
            (arg input: crate::commands::persona_commands::CreatePersonaDraftInput),
            (app_state),
        ],
        call: async,
        result: fallible,
    },
    "update_persona_draft" => crate::commands::persona_commands::update_persona_draft {
        class: AgentControl,
        caps: [MutatesAgentConsumedContent],
        params: [
            (arg input: crate::commands::persona_commands::UpdatePersonaDraftInput),
            (app_state),
        ],
        call: async,
        result: fallible,
    },
    "update_persona" => crate::commands::persona_commands::update_persona {
        class: AgentControl,
        caps: [MutatesAgentConsumedContent],
        params: [
            (arg input: crate::commands::persona_commands::UpdatePersonaInput),
            (app_state),
        ],
        call: async,
        result: fallible,
    },
    "approve_persona" => crate::commands::persona_commands::approve_persona {
        class: AgentControl,
        caps: [MutatesAgentConsumedContent],
        params: [
            (arg input: crate::commands::persona_commands::PersonaIdInput),
            (app_state),
        ],
        call: async,
        result: fallible,
    },
    "approve_persona_as_new" => crate::commands::persona_commands::approve_persona_as_new {
        class: AgentControl,
        caps: [MutatesAgentConsumedContent],
        params: [
            (arg input: crate::commands::persona_commands::ApprovePersonaAsNewInput),
            (app_state),
        ],
        call: async,
        result: fallible,
    },
    "reseed_persona_draft" => crate::commands::persona_commands::reseed_persona_draft {
        class: AgentControl,
        caps: [MutatesAgentConsumedContent],
        params: [
            (arg input: crate::commands::persona_commands::PersonaIdInput),
            (app_state),
        ],
        call: async,
        result: fallible,
    },
    "archive_persona" => crate::commands::persona_commands::archive_persona {
        class: AgentControl,
        caps: [MutatesAgentConsumedContent],
        params: [
            (arg input: crate::commands::persona_commands::PersonaIdInput),
            (app_state),
        ],
        call: async,
        result: fallible,
    },
    "unarchive_persona" => crate::commands::persona_commands::unarchive_persona {
        class: AgentControl,
        caps: [MutatesAgentConsumedContent],
        params: [
            (arg input: crate::commands::persona_commands::PersonaIdInput),
            (app_state),
        ],
        call: async,
        result: fallible,
    },
    // The four MCP override writes. Each runs three fail-closed guards before the write.
    "update_mcp_server_override" => crate::commands::mcp_policy_commands::update_mcp_server_override {
        class: AgentControl,
        caps: [AgentControl],
        params: [
            (arg input: crate::commands::mcp_policy_commands::McpServerOverrideInput),
            (app_state),
        ],
        call: async,
        result: fallible,
    },
    "clear_mcp_server_override" => crate::commands::mcp_policy_commands::clear_mcp_server_override {
        class: AgentControl,
        caps: [AgentControl],
        params: [
            (arg input: crate::commands::mcp_policy_commands::ClearMcpServerOverrideInput),
            (app_state),
        ],
        call: async,
        result: fallible,
    },
    "update_mcp_tool_override" => crate::commands::mcp_policy_commands::update_mcp_tool_override {
        class: AgentControl,
        caps: [AgentControl],
        params: [
            (arg input: crate::commands::mcp_policy_commands::McpToolOverrideInput),
            (app_state),
        ],
        call: async,
        result: fallible,
    },
    "clear_mcp_tool_override" => crate::commands::mcp_policy_commands::clear_mcp_tool_override {
        class: AgentControl,
        caps: [AgentControl],
        params: [
            (arg input: crate::commands::mcp_policy_commands::ClearMcpToolOverrideInput),
            (app_state),
        ],
        call: async,
        result: fallible,
    },
    "update_ui_feature_flags" => crate::commands::ui_commands::update_ui_feature_flags {
        class: AgentControl,
        caps: [AgentControl],
        params: [
            (arg input: crate::commands::ui_commands::UpdateUiFeatureFlagsInput),
            (app_state),
        ],
        call: async,
        result: fallible,
    },

    // -----------------------------------------------------------------------------------
    // PR 3.1-b batch 14 — THE FINAL BATCH. These eleven take the P-11 ratchet to ZERO.
    //
    // The other 37 members are manifest-classified: 25 at the `host-denied-spawns-process`
    // floor (thirteen of them hand-traced past a SILENT detector (c) — see the ledger's M1/M2/M3
    // note), 3 as `v1-deferred` on `ConfiguresFutureProcessAuthority`, and 9 as
    // `v1-audit-refused` (seven on one shared non-Serialize error contract, one fail-open, and
    // `reject_fix_task` on the batch's newly minted `reaches-corrective-transition` reason).
    //
    // Nothing here is registered on detector silence. Detector (c) was silent on 36 of the 48
    // and WRONG about 13, so silence carried no weight in this batch at all; every row below
    // was hand-traced to its repository call and to its distance from the four workspace
    // helper families that make its module siblings spawn.
    // -----------------------------------------------------------------------------------

    // The reads, at `ui:read`.
    "get_start_composer_role_default"
        => crate::commands::manual_role_default_commands::get_start_composer_role_default {
        class: Read,
        caps: [],
        params: [
            (arg input: crate::commands::manual_role_default_commands::StartComposerRoleDefaultInput),
            (app_state),
        ],
        call: async,
        result: fallible,
    },
    "get_agent_conversation_role_default"
        => crate::commands::manual_role_default_commands::get_agent_conversation_role_default {
        class: Read,
        caps: [],
        params: [
            (arg input: crate::commands::manual_role_default_commands::AgentConversationRoleDefaultInput),
            (app_state),
        ],
        call: async,
        result: fallible,
    },
    // Registered where its own module sibling `get_manual_role_defaults` is REFUSED, and the
    // split is this batch's sharpest read finding: that one turns a resolution error into a
    // fabricated Claude provider default and computes the UI's control availability against it.
    // These two never touch `catalog_entry`.
    "get_workspace_review_runtime_settings"
        => crate::commands::workspace_review_settings_commands::get_workspace_review_runtime_settings {
        class: Read,
        caps: [],
        params: [(arg project_id: Option<String>), (app_state)],
        call: async,
        result: fallible,
    },

    // The writers, at `ui:agent`.
    "archive_task" => crate::commands::task_commands::mutation::archive_task {
        class: AgentControl,
        caps: [AgentControl],
        params: [(arg task_id: String), (app_state), (host_app_handle)],
        call: async,
        result: fallible,
    },
    "restore_task" => crate::commands::task_commands::mutation::restore_task {
        class: AgentControl,
        caps: [AgentControl],
        params: [(arg task_id: String), (app_state), (host_app_handle)],
        call: async,
        result: fallible,
    },
    "create_agent_conversation"
        => crate::commands::unified_chat_commands::create_agent_conversation {
        class: AgentControl,
        caps: [AgentControl],
        params: [
            (arg input: crate::commands::unified_chat_commands::CreateAgentConversationInput),
            (app_state),
        ],
        call: async,
        result: fallible,
    },
    "restore_agent_conversation"
        => crate::commands::unified_chat_commands::restore_agent_conversation {
        class: AgentControl,
        caps: [AgentControl],
        params: [(arg conversation_id: String), (app_state)],
        call: async,
        result: fallible,
    },
    "update_agent_conversation_title"
        => crate::commands::unified_chat_commands::update_agent_conversation_title {
        class: AgentControl,
        caps: [AgentControl],
        params: [
            (arg input: crate::commands::unified_chat_commands::UpdateAgentConversationTitleInput),
            (app_state),
        ],
        call: async,
        result: fallible,
    },
    // Two BOUNDED deferred-authority writes, each carrying a `DECLARED_MEMBERSHIPS` row
    // (`configures-future-agent-runtime`) because no detector watches the surface they arm.
    // They pick which MODEL a later agent runs; this batch's three Elevated rows configure the
    // containment boundary itself, which is the whole of the difference.
    "update_workspace_review_runtime_settings"
        => crate::commands::workspace_review_settings_commands::update_workspace_review_runtime_settings {
        class: AgentControl,
        caps: [AgentControl],
        params: [
            (arg input: crate::commands::workspace_review_settings_commands::UpdateWorkspaceReviewRuntimeSettingsInput),
            (app_state),
        ],
        call: async,
        result: fallible,
    },
    // Registered only after this batch fixed the fabricated-timestamp fail-open in its return
    // path (`sqlite_agent_model_registry_repo.rs`); the pre-fix body would have been an
    // `AUDIT_REFUSALS` row.
    "upsert_custom_agent_model"
        => crate::commands::agent_model_commands::upsert_custom_agent_model {
        class: AgentControl,
        caps: [AgentControl],
        params: [
            (arg input: crate::commands::agent_model_commands::UpsertCustomAgentModelInput),
            (app_state),
        ],
        call: async,
        result: fallible,
    },
    // THE STANDING HELD COMMAND, released after four batches. Its partner `reject_fix_task` is
    // refused on the new `reaches-corrective-transition` reason; the pair argument that held
    // THIS half does not survive the current registry, because `block_task` and `stop_task`
    // are both registered, so the remote brake exists. See the ledger row.
    "approve_fix_task" => crate::commands::review_commands::approve_fix_task {
        class: AgentControl,
        caps: [AgentControl],
        params: [
            (arg input: crate::commands::review_commands_types::ApproveFixTaskInput),
            (app_state),
            (execution_state),
            (host_app_handle),
        ],
        call: async,
        result: fallible,
    },
}
