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
//! # Runtime parameterisation (deviation, recorded)
//!
//! [`dispatch`] is generic over `R: tauri::Runtime` so the P-4 parity suite can drive the
//! *production* dispatch path under `tauri::test::MockRuntime` rather than a test-only fork
//! of it (A-7: no forked command fns). The consequence is that the `app_handle` injection form
//! yields `AppHandle<R>`, so a command whose signature demands the Wry-monomorphic
//! `tauri::AppHandle` is not registrable in this PR. No `Read`-class command needs one; PR 3.1
//! must either monomorphise the facade on `Wry` or widen those signatures.

use ralphx_remote_protocol::{Capability, ErrorCode, RiskClass, Scope};
use serde_json::Value;

/// An argument-sensitive authorization predicate, evaluated on parsed args BEFORE dispatch.
///
/// Returns the scope the *specific request* requires, which may exceed the command's class
/// scope (the `update_task` field-level predicate is the canonical user, §3.3).
pub type AuthzPredicate = fn(&Value) -> Scope;

/// A validation predicate, evaluated after authorization and before dispatch.
pub type ValidatePredicate = fn(&Value) -> Result<(), String>;

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

    fn bad_args(message: impl Into<String>) -> Self {
        Self {
            code: ErrorCode::RemoteCommandUnavailable,
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
    serde_json::from_value(raw).map_err(|error| {
        RemoteInvokeError::bad_args(format!("Invalid argument `{name}`: {error}"))
    })
}

/// Serialises a command's success value exactly as the Tauri IPC layer does.
pub fn serialize_ok<T: serde::Serialize>(value: T) -> Result<Value, RemoteInvokeError> {
    serde_json::to_value(value).map_err(|error| RemoteInvokeError {
        code: ErrorCode::RemoteCommandUnavailable,
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
                            @invoke app, args, $target, $call, $result, [ $( $param ),* ]
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
    (@reject_forbidden_param (app_state)) => {};
    (@reject_forbidden_param (execution_state)) => {};
    (@reject_forbidden_param (active_project_state)) => {};
    (@reject_forbidden_param (app_handle)) => {};

    // --- (b) the FIXED injection table -------------------------------------------------
    // These four arms ARE the table. An extractor form absent here has no matching arm, so
    // registering a command that needs it is a compile error (C-12, X-11).
    (@bind $app:ident, $args:ident, (app_state)) => {
        $app.state::<$crate::application::AppState>()
    };
    (@bind $app:ident, $args:ident, (execution_state)) => {
        $app.state::<::std::sync::Arc<$crate::commands::execution_commands::ExecutionState>>()
    };
    (@bind $app:ident, $args:ident, (active_project_state)) => {
        $app.state::<::std::sync::Arc<$crate::commands::execution_commands::ActiveProjectState>>()
    };
    (@bind $app:ident, $args:ident, (app_handle)) => {
        $app.clone()
    };
    (@bind $app:ident, $args:ident, (arg $n:ident : $t:ty)) => {
        match $crate::remote_server::registry::extract_arg::<$t>($args, stringify!($n)) {
            Ok(value) => value,
            Err(error) => return Err(error),
        }
    };

    // --- call + result shaping ---------------------------------------------------------
    (@invoke $app:ident, $args:ident, $target:path, async, fallible, [ $( $param:tt ),* ]) => {{
        match $target( $( $crate::remote_commands!(@bind $app, $args, $param) ),* ).await {
            Ok(value) => $crate::remote_server::registry::serialize_ok(value)
                .map($crate::remote_server::registry::DispatchOutcome::Ok),
            Err(error) => Ok($crate::remote_server::registry::DispatchOutcome::Err(
                $crate::remote_server::registry::serialize_ok(error)?,
            )),
        }
    }};
    (@invoke $app:ident, $args:ident, $target:path, async, infallible, [ $( $param:tt ),* ]) => {{
        let value = $target( $( $crate::remote_commands!(@bind $app, $args, $param) ),* ).await;
        $crate::remote_server::registry::serialize_ok(value)
            .map($crate::remote_server::registry::DispatchOutcome::Ok)
    }};
    (@invoke $app:ident, $args:ident, $target:path, sync, fallible, [ $( $param:tt ),* ]) => {{
        match $target( $( $crate::remote_commands!(@bind $app, $args, $param) ),* ) {
            Ok(value) => $crate::remote_server::registry::serialize_ok(value)
                .map($crate::remote_server::registry::DispatchOutcome::Ok),
            Err(error) => Ok($crate::remote_server::registry::DispatchOutcome::Err(
                $crate::remote_server::registry::serialize_ok(error)?,
            )),
        }
    }};
    (@invoke $app:ident, $args:ident, $target:path, sync, infallible, [ $( $param:tt ),* ]) => {{
        let value = $target( $( $crate::remote_commands!(@bind $app, $args, $param) ),* );
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
/// authority, so those requests demand `ui:agent`. `category`/`priority` are structurally inert
/// — the `WorkerTaskView` projection excludes them from every worker payload — and stay
/// `ui:operate`. `internal_status` never reaches here: `validate_update_task_input` rejects it.
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
// commands (`get_git_branches`, `get_task_file_changes`/`get_file_diff`,
// `get_codex_cli_diagnostics`, `build_agent_issue_report`) which are NOT `Read`. Mutating
// classes land in PR 1.5 (`ui:agent` suite) and PR 3.1 (full coverage).
crate::remote_commands! {
    "health_check" => crate::commands::health::health_check {
        class: Read,
        caps: [],
        params: [],
        call: sync,
        result: infallible,
    },
    "list_projects" => crate::commands::project_commands::list_projects {
        class: Read,
        caps: [],
        params: [(app_state)],
        call: async,
        result: fallible,
    },
    "get_project" => crate::commands::project_commands::get_project {
        class: Read,
        caps: [],
        params: [(arg id: String), (app_state)],
        call: async,
        result: fallible,
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
}
