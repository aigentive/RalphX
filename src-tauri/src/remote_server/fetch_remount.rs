//! Curated fetch-route remount on the remote listener (:3849) — PR 1.5-B.
//!
//! The frontend reaches UI-owned HTTP handlers through `backendFetch`. In a remote environment
//! that call is proxied by the `remote_fetch` Rust command, which re-sends the SAME `/api/...`
//! path to this listener. Nothing on :3849 serves those paths unless it is listed here.
//!
//! Three properties define this module:
//!
//! 1. **One allowlist, one builder.** [`REMOUNT_ALLOWLIST`] is the only description of the
//!    mounted set, and [`remount_router`] is the only thing that mounts. The builder reports
//!    back exactly what it mounted ([`RemountBuild::mounted`]), pushed inside the same loop
//!    that adds the route, so the report cannot drift from what the router serves.
//! 2. **Same scope enforcement as `/invoke`.** Each row carries a [`RiskClass`], and the gate
//!    resolves it through [`scope_for_class`] — the identical function the invoke facade uses.
//!    A class-to-scope change therefore moves both surfaces at once, by construction.
//! 3. **Read-only in v1.** `/invoke` makes mutations at-most-once by keying a durable dedup
//!    record on the client's `requestId` (PR 1.5-C). A proxied fetch carries no such key, so a
//!    remounted mutating route would be at-least-once on every retry — precisely the property
//!    1.5-C fails closed to avoid. Mutations stay on the facade, which has the idempotency
//!    substrate; fetch gets the reads. The full unmounted-with-reason census is in
//!    `docs/architecture/remote-r8-appstate.md`.

use std::sync::Arc;

use axum::{
    extract::{DefaultBodyLimit, Request},
    middleware::{self, Next},
    response::Response,
    routing::{get, post, MethodRouter},
    Router,
};
use ralphx_remote_protocol::{ErrorCode, RiskClass};

use crate::application::AppState;
use crate::commands::ExecutionState;
use crate::http_server::delegation::DelegationService;
use crate::http_server::handlers::{
    get_agent_workflow_run, get_latest_agent_workflow_run_for_script,
    get_plan_complexity_assessment, get_plan_verification, get_session_plan, list_agent_task_lists,
    list_agent_tasks, list_agent_tasks_for_list,
};
use crate::http_server::types::HttpServerState;
use crate::remote_server::auth::RemoteIdentity;
use crate::remote_server::invoke::status_for_error_code;
use crate::remote_server::registry::scope_for_class;
use crate::remote_server::remote_error_response;

/// Body cap for remounted POST routes.
///
/// Same reasoning as `REMOTE_INVOKE_BODY_LIMIT_BYTES`: every mounted POST carries a filter or an
/// id list, never payload bytes. Attachments have their own, larger endpoint.
pub(crate) const REMOTE_FETCH_BODY_LIMIT_BYTES: usize = 256 * 1024;

// ---------------------------------------------------------------------------------------
// Path constants
//
// Shared by the allowlist rows and the handler match below, so a row and its handler can never
// disagree about the path string.
// ---------------------------------------------------------------------------------------

pub(crate) const PATH_SESSION_PLAN: &str = "/api/get_session_plan/:session_id";
pub(crate) const PATH_PLAN_COMPLEXITY: &str = "/api/plan_complexity_assessment/:session_id";
pub(crate) const PATH_PLAN_VERIFICATION: &str = "/api/ideation/sessions/:id/verification";
pub(crate) const PATH_AGENT_TASKS_LIST: &str = "/api/agent_tasks/list";
pub(crate) const PATH_AGENT_TASK_LISTS: &str = "/api/agent_tasks/lists";
pub(crate) const PATH_AGENT_TASKS_FOR_LIST: &str = "/api/agent_tasks/list_for_list";
pub(crate) const PATH_WORKFLOW_RUN_GET: &str = "/api/agent_workflows/runs/get";
pub(crate) const PATH_WORKFLOW_RUN_LATEST: &str = "/api/agent_workflows/runs/latest";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RemountMethod {
    Get,
    Post,
}

/// One checked-in row of the curated remount surface.
#[derive(Debug, Clone, Copy)]
pub(crate) struct RemountRoute {
    pub path: &'static str,
    pub method: RemountMethod,
    /// Drives the scope gate through [`scope_for_class`], exactly as `/invoke` does.
    pub class: RiskClass,
    /// Why this classification — the audit finding, not a restatement of the class.
    pub reason: &'static str,
}

/// The curated fetch surface. **The only** description of what :3849 serves under `/api`.
pub(crate) const REMOUNT_ALLOWLIST: &[RemountRoute] = &[
    RemountRoute {
        path: PATH_SESSION_PLAN,
        method: RemountMethod::Get,
        class: RiskClass::Read,
        reason: "reads the session's current plan artifact; no write, no spawn sink",
    },
    RemountRoute {
        path: PATH_PLAN_COMPLEXITY,
        method: RemountMethod::Get,
        class: RiskClass::Read,
        reason: "single SELECT through the shared DbConnection",
    },
    RemountRoute {
        path: PATH_PLAN_VERIFICATION,
        method: RemountMethod::Get,
        class: RiskClass::Read,
        reason: "derives status from ideation/agent-run rows and the shared message queue; \
                 requests no verification and arms nothing",
    },
    RemountRoute {
        path: PATH_AGENT_TASKS_LIST,
        method: RemountMethod::Post,
        class: RiskClass::Read,
        reason: "AgentTaskService read; scope comes from the request body, not a trust header",
    },
    RemountRoute {
        path: PATH_AGENT_TASK_LISTS,
        method: RemountMethod::Post,
        class: RiskClass::Read,
        reason: "AgentTaskService read; scope comes from the request body, not a trust header",
    },
    RemountRoute {
        path: PATH_AGENT_TASKS_FOR_LIST,
        method: RemountMethod::Post,
        class: RiskClass::Read,
        reason: "AgentTaskService read; scope comes from the request body, not a trust header",
    },
    RemountRoute {
        path: PATH_WORKFLOW_RUN_GET,
        method: RemountMethod::Post,
        class: RiskClass::Read,
        reason: "agent_workflow_repo progress read; starts, pauses and cancels are NOT mounted",
    },
    RemountRoute {
        path: PATH_WORKFLOW_RUN_LATEST,
        method: RemountMethod::Post,
        class: RiskClass::Read,
        reason: "agent_workflow_repo latest-run read; starts, pauses and cancels are NOT mounted",
    },
];

/// Routes named as PERMANENTLY denied, with the reason recorded next to the name.
///
/// A path absent from [`REMOUNT_ALLOWLIST`] is already unreachable; these two are additionally
/// named so the denial is a checked-in, testable claim rather than an accident of omission.
/// Both are unvalidated dual-decision resolve sinks: the decision arrives raw in the client's
/// JSON body, so mounting either would let a paired device approve *or* deny on the user's
/// behalf through a surface with no per-decision authorization.
pub(crate) const REMOUNT_DENIED_SINKS: &[(&str, &str)] = &[
    (
        "POST /api/permission/resolve",
        "unvalidated-dual-decision-resolve",
    ),
    (
        "POST /api/question/resolve",
        "unvalidated-dual-decision-resolve",
    ),
];

// ---------------------------------------------------------------------------------------
// R-8: the state the remount runs against
// ---------------------------------------------------------------------------------------

/// The **same** `Arc<AppState>` the :3847 listener serves from, published as managed state so
/// the remount can reach it without constructing a third `AppState`.
///
/// R-8 in one sentence: a remounted handler and its :3847 twin read the same memory, so
/// invoke-vs-fetch divergence for authority-relevant state is structurally impossible rather
/// than merely untested. `build_http_app_state` already Arc-shares the authority-bearing fields
/// (and the SQLite connection behind every repository) with the Tauri-managed `AppState`; this
/// newtype extends that identity to the third consumer instead of forking a new graph.
///
/// Registered at the `server_boot` seam, which is the one place holding both Arcs and the
/// `AppHandle`. Absence is fail-CLOSED: [`remount_router`] is never called, so the `/api`
/// routes simply do not exist on :3849 and answer with the 404 fallback.
pub struct SharedHttpAppState {
    app_state: Arc<AppState>,
    execution_state: Arc<ExecutionState>,
    /// Constructed once, not per request, so the identity test can pin it too.
    ///
    /// Fresh rather than shared by design: `DelegationService` only backs `/api/internal/*`,
    /// which this module never mounts. `no_mounted_route_touches_delegation_service` pins that.
    delegation_service: Arc<DelegationService>,
}

impl SharedHttpAppState {
    pub fn new(app_state: Arc<AppState>, execution_state: Arc<ExecutionState>) -> Self {
        Self {
            app_state,
            execution_state,
            delegation_service: Arc::new(DelegationService::new()),
        }
    }

    /// Hands the remount the very same Arcs — never a rebuild.
    pub(crate) fn http_server_state(&self) -> HttpServerState {
        HttpServerState {
            app_state: Arc::clone(&self.app_state),
            execution_state: Arc::clone(&self.execution_state),
            delegation_service: Arc::clone(&self.delegation_service),
        }
    }

    pub(crate) fn app_state(&self) -> &Arc<AppState> {
        &self.app_state
    }

    pub(crate) fn execution_state(&self) -> &Arc<ExecutionState> {
        &self.execution_state
    }
}

// ---------------------------------------------------------------------------------------
// Builder
// ---------------------------------------------------------------------------------------

/// Why a remount build refused. Every variant means "mount nothing", never "mount a subset".
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum RemountBuildError {
    /// An allowlist row has no handler. Fail closed rather than serve a shorter surface than
    /// the checked-in artifact claims.
    UnmappedRoute(&'static str),
    /// Two rows claim the same path+method. `Router::route` would panic; refuse first.
    DuplicateRoute(&'static str),
}

impl std::fmt::Display for RemountBuildError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnmappedRoute(path) => {
                write!(formatter, "remount allowlist row `{path}` has no handler")
            }
            Self::DuplicateRoute(path) => {
                write!(formatter, "remount allowlist lists `{path}` more than once")
            }
        }
    }
}

/// The built sub-router plus the exact set it mounted.
pub(crate) struct RemountBuild {
    pub router: Router,
    /// Recorded inside the mount loop, so it is a record of mounts, not a parallel list.
    pub mounted: Vec<(&'static str, RemountMethod)>,
}

/// Mounts EXACTLY [`REMOUNT_ALLOWLIST`] against the shared :3847 state.
///
/// All-or-nothing: any row that cannot be resolved refuses the whole build, so :3849 can never
/// advertise a partially-mounted `/api` surface that diverges from the checked-in allowlist.
pub(crate) fn remount_router(
    shared: &SharedHttpAppState,
) -> Result<RemountBuild, RemountBuildError> {
    let http_state = shared.http_server_state();
    let mut router: Router<HttpServerState> = Router::new();
    let mut mounted: Vec<(&'static str, RemountMethod)> = Vec::new();

    for route in REMOUNT_ALLOWLIST {
        if mounted
            .iter()
            .any(|(path, method)| *path == route.path && *method == route.method)
        {
            return Err(RemountBuildError::DuplicateRoute(route.path));
        }

        let Some(handler) = method_router_for(route) else {
            return Err(RemountBuildError::UnmappedRoute(route.path));
        };

        // Order matters: the scope gate is the innermost layer relative to the listener's auth
        // middleware, so it always sees a `RemoteIdentity` that bearer auth already validated.
        let mut handler = handler.layer(middleware::from_fn_with_state(
            route.class,
            enforce_remount_scope,
        ));
        if route.method == RemountMethod::Post {
            handler = handler.layer(DefaultBodyLimit::max(REMOTE_FETCH_BODY_LIMIT_BYTES));
        }

        router = router.route(route.path, handler);
        mounted.push((route.path, route.method));
    }

    Ok(RemountBuild {
        router: router.with_state(http_state),
        mounted,
    })
}

/// The allowlist row → handler mapping. Exhaustive over [`REMOUNT_ALLOWLIST`]; anything else
/// returns `None` so the builder refuses instead of silently skipping.
fn method_router_for(route: &RemountRoute) -> Option<MethodRouter<HttpServerState>> {
    use RemountMethod::{Get, Post};
    Some(match (route.method, route.path) {
        (Get, PATH_SESSION_PLAN) => get(get_session_plan),
        (Get, PATH_PLAN_COMPLEXITY) => get(get_plan_complexity_assessment),
        (Get, PATH_PLAN_VERIFICATION) => get(get_plan_verification),
        (Post, PATH_AGENT_TASKS_LIST) => post(list_agent_tasks),
        (Post, PATH_AGENT_TASK_LISTS) => post(list_agent_task_lists),
        (Post, PATH_AGENT_TASKS_FOR_LIST) => post(list_agent_tasks_for_list),
        (Post, PATH_WORKFLOW_RUN_GET) => post(get_agent_workflow_run),
        (Post, PATH_WORKFLOW_RUN_LATEST) => post(get_latest_agent_workflow_run_for_script),
        _ => return None,
    })
}

// ---------------------------------------------------------------------------------------
// Scope gate
// ---------------------------------------------------------------------------------------

/// Per-route scope enforcement, resolved through the facade's own [`scope_for_class`].
///
/// Fails closed three ways:
/// - no `RemoteIdentity` in extensions (a layering regression) → `REMOTE_INTERNAL_ERROR`;
/// - a class with no scope (`RiskClass::Denied`) → `REMOTE_FORBIDDEN`;
/// - the device lacks the required scope → `REMOTE_FORBIDDEN`, before the handler runs.
async fn enforce_remount_scope(
    axum::extract::State(class): axum::extract::State<RiskClass>,
    request: Request,
    next: Next,
) -> Response {
    let Some(identity) = request.extensions().get::<RemoteIdentity>().cloned() else {
        tracing::error!(
            path = %request.uri().path(),
            "remounted fetch route ran without an authenticated identity; refusing"
        );
        return remote_error_response(
            status_for_error_code(ErrorCode::RemoteInternalError),
            ErrorCode::RemoteInternalError,
            "This route could not resolve the caller's identity; the request was not served.",
        );
    };

    let Some(required) = scope_for_class(class) else {
        return remote_error_response(
            status_for_error_code(ErrorCode::RemoteForbidden),
            ErrorCode::RemoteForbidden,
            "This route is denied on the remote facade.",
        );
    };

    if !identity.has_scope(required) {
        return remote_error_response(
            status_for_error_code(ErrorCode::RemoteForbidden),
            ErrorCode::RemoteForbidden,
            "This route requires a scope this device was not granted.",
        );
    }

    next.run(request).await
}
