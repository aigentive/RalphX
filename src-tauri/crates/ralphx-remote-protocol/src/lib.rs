use serde::{Deserialize, Serialize};
use serde_json::Value;

pub const PROTOCOL_VERSION: u32 = 1;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EnvironmentDescriptor {
    pub environment_id: String,
    pub app_version: String,
    pub protocol_version: u32,
    pub min_client_protocol: u32,
    pub platform: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum Scope {
    #[serde(rename = "ui:read")]
    UiRead,
    #[serde(rename = "ui:operate")]
    UiOperate,
    #[serde(rename = "ui:agent")]
    UiAgent,
    #[serde(rename = "ui:elevated")]
    UiElevated,
}

pub const SCOPES: &[Scope] = &[
    Scope::UiRead,
    Scope::UiOperate,
    Scope::UiAgent,
    Scope::UiElevated,
];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum RiskClass {
    Read,
    Operate,
    PathScoped,
    AgentControl,
    Elevated,
    Denied,
}

pub const RISK_CLASSES: &[RiskClass] = &[
    RiskClass::Read,
    RiskClass::Operate,
    RiskClass::PathScoped,
    RiskClass::AgentControl,
    RiskClass::Elevated,
    RiskClass::Denied,
];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Capability {
    SpawnsProcess,
    WritesArbitraryPath,
    MutatesWorkingDirectory,
    ConfiguresFutureProcessAuthority,
    TouchesCredentials,
    PtyControl,
    AgentControl,
    SeedsSpawnTriggeringState,
    MutatesAgentConsumedContent,
    HostManagement,
    DeletesEntity,
}

pub const CAPABILITIES: &[Capability] = &[
    Capability::SpawnsProcess,
    Capability::WritesArbitraryPath,
    Capability::MutatesWorkingDirectory,
    Capability::ConfiguresFutureProcessAuthority,
    Capability::TouchesCredentials,
    Capability::PtyControl,
    Capability::AgentControl,
    Capability::SeedsSpawnTriggeringState,
    Capability::MutatesAgentConsumedContent,
    Capability::HostManagement,
    Capability::DeletesEntity,
];

/// Compile-time class/capability consistency gate used by the remote registry.
///
/// Proof shape for "every forbidden capability under `Read`/`Operate` is compile-rejected"
/// (phase-0 PR 0.2 acceptance #3 / P-17(b)): the doctest below proves the `const`-assert
/// MECHANISM rejects a forbidden pair at compile time, and
/// `class_permits_rejects_every_capability_for_read_and_operate` in `tests/protocol_contract.rs`
/// exhaustively proves the `const fn` returns `false` for all 11 capabilities under both
/// classes. Because the assert is `const`, `false` for a pair is exactly a compile rejection
/// for that pair — the two together are equivalent to 22 individual `compile_fail` fixtures
/// without paying 22 rustc invocations.
///
/// ```compile_fail
/// use ralphx_remote_protocol::{class_permits, Capability, RiskClass};
/// const _: () = assert!(class_permits(
///     RiskClass::Operate,
///     &[Capability::SeedsSpawnTriggeringState],
/// ));
/// ```
pub const fn class_permits(class: RiskClass, capabilities: &[Capability]) -> bool {
    let mut index = 0;
    while index < capabilities.len() {
        let permitted = match class {
            RiskClass::Read | RiskClass::Operate | RiskClass::Denied => false,
            RiskClass::PathScoped => matches!(capabilities[index], Capability::WritesArbitraryPath),
            RiskClass::AgentControl => matches!(
                capabilities[index],
                Capability::AgentControl
                    | Capability::SeedsSpawnTriggeringState
                    | Capability::MutatesAgentConsumedContent
            ),
            RiskClass::Elevated => true,
        };
        if !permitted {
            return false;
        }
        index += 1;
    }
    !matches!(class, RiskClass::Denied)
}

/// The scopes a **v1** remote facade may grant. `ui:elevated` is a v1 non-goal, so a ledgered
/// command whose class maps to it is deferred rather than registerable. Kept next to
/// [`class_permits`] because the two together are the whole v1 exposability question.
pub const V1_FACADE_SCOPES: &[Scope] = &[Scope::UiRead, Scope::UiOperate, Scope::UiAgent];

/// How a ledgered command resolves against the v1 remote facade (P-11 third disposition).
///
/// The P-11 ratchet used to admit exactly two answers — remote-registered, or client-local with
/// a reason. A large block of host commands is neither and never will be: the facade denies or
/// defers them. This enum is that third answer, derived from the ledger row itself so the
/// authority stays in `capability_ledger.rs` and the drift scan only reads the rendered value.
///
/// Every variant other than [`V1Resolution::Registerable`] is *manifest-classified*: it needs no
/// `remote_commands!` entry and no client-local reason, and invoking it remotely stays a
/// `REMOTE_COMMAND_UNAVAILABLE` `find_spec` miss.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum V1Resolution {
    /// The class maps to a v1 scope and no capability blocks it — eligible for a hand-audited
    /// `remote_commands!` entry. Eligible, not approved: the per-command audit still owns the
    /// final call.
    Registerable,
    /// `RiskClass::Denied` — `scope_for_class` yields `None`, so no grant at any capability set
    /// can reach it and registering it fails the `class_permits` const-assert.
    HostDenied,
    /// Carries `Capability::SpawnsProcess`, which `class_permits` admits only under `Elevated`;
    /// with `ui:elevated` excluded from v1 this is unexposable at every v1 scope.
    HostDeniedSpawnsProcess,
    /// Ledgered `Elevated` without `SpawnsProcess` — reachable only under `ui:elevated`, which
    /// v1 excludes. Deferred, not denied.
    V1Deferred,
    /// The class/capability arithmetic admits a v1 scope, but a recorded per-command audit found
    /// a property of the command AS IT STANDS that no v1 scope can accommodate.
    ///
    /// This variant is NOT derivable from `(class, capabilities)` — that pair is exactly what
    /// [`v1_resolution`] partitions, and widening it would break that partition. It is reachable
    /// only through an explicit ledger audit-refusal row, so the classification cannot be bought
    /// by adjusting a class; it has to be bought by recording a finding and pinning it.
    ///
    /// PR 3.1-b batch 9 added it after establishing what it must NOT be used for. The facade
    /// already serves 16 `agentControl` ops, three of which carry `SeedsSpawnTriggeringState`,
    /// so "detector (a)/(b) fires" is a reason a batch declined to register a command — never a
    /// reason the host denies it. Refusals of that shape stay `Registerable` and stay on the
    /// ratchet. See [`AuditRefusalReason`] for the four findings that do qualify.
    V1AuditRefused,
}

pub const V1_RESOLUTIONS: &[V1Resolution] = &[
    V1Resolution::Registerable,
    V1Resolution::HostDenied,
    V1Resolution::HostDeniedSpawnsProcess,
    V1Resolution::V1Deferred,
    V1Resolution::V1AuditRefused,
];

/// Why a per-command audit refused a command that the class/capability arithmetic would admit.
///
/// The bar every variant must clear: **the finding must disqualify the command at EVERY v1
/// scope, independent of any future audit.** "It writes", "it arms", "it steers" do not clear
/// that bar — `ui:operate` and `ui:agent` are live v1 scopes with registered ops carrying
/// exactly those capabilities — and there is deliberately no variant for them, so a later batch
/// cannot reach for one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum AuditRefusalReason {
    /// Renders a host failure as absence or success, so no scope can serve it honestly: a
    /// remote caller cannot distinguish "nothing here" from "the host could not tell". The
    /// unlocking fix is to propagate the error.
    FailOpenUntilFixed,
    /// Constructs spawn-capable machinery to serve a read, handing any caller a constructed
    /// steer surface. The unlocking fix is a read-only seam, as already done for the
    /// `list_remote_*` reads.
    ConstructsSpawnCapableService,
    /// The success or error payload cannot cross the facade at all — e.g. an error type that is
    /// not `Serialize`, so the `fallible` dispatch arm cannot render it. An error-contract
    /// change unlocks it; it is not an authority finding.
    TransportShapeDeferred,
    /// A registered remote twin already answers this query through a deliberately split seam.
    /// Registering the local name would put two facade paths on one query for no new
    /// capability, so the refusal is architectural rather than pending anything.
    SeamResolvedViaRemoteTwin,
    /// The command reaches `transition_task_corrective` / `apply_corrective_transition`, the
    /// repair-path-only state-machine jump.
    ///
    /// Added by PR 3.1-b batch 14 to close the ratchet honestly rather than mis-file
    /// `reject_fix_task` under one of the four reasons above, none of which is true of it: it
    /// does not fail open, it constructs no spawn-capable service to serve a read, its payload
    /// crosses the facade fine, and no registered twin answers it.
    ///
    /// The bar this clears, and why it is not the generic "it arms / it steers" code the
    /// vocabulary deliberately withholds: this names ONE specific mechanism that is already a
    /// hard, separately CI-enforced invariant
    /// (`no_registered_facade_target_reaches_a_corrective_transition`), so the refusal is
    /// mechanically falsifiable in both directions — a pin asserts the closure DOES reach a
    /// corrective sink, and the refusal fails loudly if the body is ever fixed. A corrective
    /// jump lets a caller pick a repair destination the ordinary state machine forbids, which
    /// no v1 scope can accommodate.
    ReachesCorrectiveTransition,
}

pub const AUDIT_REFUSAL_REASONS: &[AuditRefusalReason] = &[
    AuditRefusalReason::FailOpenUntilFixed,
    AuditRefusalReason::ConstructsSpawnCapableService,
    AuditRefusalReason::TransportShapeDeferred,
    AuditRefusalReason::SeamResolvedViaRemoteTwin,
    AuditRefusalReason::ReachesCorrectiveTransition,
];

/// Resolve a ledger row that also carries an audit refusal.
///
/// The mechanical refusals win FIRST and unconditionally. A command that both spawns a process
/// and has an audit finding is `HostDeniedSpawnsProcess`, because that is the stronger and
/// cheaper-to-verify statement; letting the audit row mask it would let a hand-written table
/// downgrade a mechanically proven denial.
pub const fn v1_resolution_with_audit(
    class: RiskClass,
    capabilities: &[Capability],
    audit_refused: bool,
) -> V1Resolution {
    match v1_resolution(class, capabilities) {
        V1Resolution::Registerable if audit_refused => V1Resolution::V1AuditRefused,
        mechanical => mechanical,
    }
}

/// Derive a ledger row's [`V1Resolution`] from its declared class and capabilities.
///
/// Ordering is deliberate and matches the census's disposition split: `Denied` first (no scope
/// exists), then `SpawnsProcess` (the capability, not the class, is what forecloses v1), then
/// the remaining non-v1 classes as deferrals.
pub const fn v1_resolution(class: RiskClass, capabilities: &[Capability]) -> V1Resolution {
    if matches!(class, RiskClass::Denied) {
        return V1Resolution::HostDenied;
    }
    let mut index = 0;
    while index < capabilities.len() {
        if matches!(capabilities[index], Capability::SpawnsProcess) {
            return V1Resolution::HostDeniedSpawnsProcess;
        }
        index += 1;
    }
    match scope_for_v1_class(class) {
        Some(_) => V1Resolution::Registerable,
        None => V1Resolution::V1Deferred,
    }
}

/// The v1 scope a class maps to, or `None` when v1 grants no scope for it.
///
/// Mirrors `remote_server::registry::scope_for_class` restricted to [`V1_FACADE_SCOPES`]; it
/// lives here because the protocol crate is what the ledger derivation may depend on.
const fn scope_for_v1_class(class: RiskClass) -> Option<Scope> {
    match class {
        RiskClass::Read => Some(Scope::UiRead),
        RiskClass::Operate | RiskClass::PathScoped => Some(Scope::UiOperate),
        RiskClass::AgentControl => Some(Scope::UiAgent),
        RiskClass::Elevated | RiskClass::Denied => None,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ResetReason {
    #[serde(rename = "cursor_pruned")]
    CursorPruned,
    #[serde(rename = "epoch_changed")]
    EpochChanged,
    #[serde(rename = "after_seq_gt_max")]
    AfterSeqGtMax,
    #[serde(rename = "read_error")]
    ReadError,
    #[serde(rename = "revoked")]
    Revoked,
    #[serde(rename = "host_disabled")]
    HostDisabled,
}
pub const RESET_REASONS: &[ResetReason] = &[
    ResetReason::CursorPruned,
    ResetReason::EpochChanged,
    ResetReason::AfterSeqGtMax,
    ResetReason::ReadError,
    ResetReason::Revoked,
    ResetReason::HostDisabled,
];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ErrorCode {
    #[serde(rename = "REMOTE_COMMAND_UNAVAILABLE")]
    RemoteCommandUnavailable,
    #[serde(rename = "REMOTE_FORBIDDEN")]
    RemoteForbidden,
    #[serde(rename = "REMOTE_UNAUTHORIZED")]
    RemoteUnauthorized,
    #[serde(rename = "REMOTE_UNREACHABLE")]
    RemoteUnreachable,
    #[serde(rename = "REMOTE_VERSION_MISMATCH")]
    RemoteVersionMismatch,
    #[serde(rename = "REMOTE_TIMEOUT_UNKNOWN")]
    RemoteTimeoutUnknown,
    #[serde(rename = "REMOTE_REQUEST_IN_PROGRESS")]
    RemoteRequestInProgress,
    #[serde(rename = "REMOTE_REQUEST_ID_REUSED")]
    RemoteRequestIdReused,
    /// The request reached a registered command but its arguments could not be deserialized.
    /// Distinct from `RemoteCommandUnavailable`, which is reserved for "no such command".
    #[serde(rename = "REMOTE_INVALID_ARGUMENTS")]
    RemoteInvalidArguments,
    /// The host failed while producing the answer (e.g. a response that will not serialize).
    /// A host-side fault, never a statement about the client's request.
    #[serde(rename = "REMOTE_INTERNAL_ERROR")]
    RemoteInternalError,
}
pub const ERROR_CODES: &[ErrorCode] = &[
    ErrorCode::RemoteCommandUnavailable,
    ErrorCode::RemoteForbidden,
    ErrorCode::RemoteUnauthorized,
    ErrorCode::RemoteUnreachable,
    ErrorCode::RemoteVersionMismatch,
    ErrorCode::RemoteTimeoutUnknown,
    ErrorCode::RemoteRequestInProgress,
    ErrorCode::RemoteRequestIdReused,
    ErrorCode::RemoteInvalidArguments,
    ErrorCode::RemoteInternalError,
];

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum ServerFrame {
    Hello {
        protocol_version: u32,
        environment_id: String,
        stream_epoch: String,
        server_version: String,
        max_seq: u64,
        heartbeat_secs: u32,
    },
    Event {
        seq: Option<u64>,
        name: String,
        payload: Value,
    },
    ReplayDone {
        through_seq: u64,
    },
    Reset {
        reason: ResetReason,
    },
    Heartbeat {
        t: u64,
    },
    Error {
        code: ErrorCode,
        message: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum ClientFrame {
    Subscribe {
        after_seq: u64,
        stream_epoch: String,
    },
    CursorAck {
        seq: u64,
    },
    HeartbeatAck {
        t: u64,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum EventDelivery {
    Durable,
    Transient,
    LocalOnly,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum EventOrigin {
    Backend,
    Webview,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EventClassification {
    pub name: &'static str,
    pub delivery: EventDelivery,
    pub origin: EventOrigin,
    pub excluded_from_v1: bool,
}

impl EventClassification {
    pub fn find(name: &str) -> Option<&'static Self> {
        EVENT_CLASSIFICATIONS
            .iter()
            .find(|entry| entry.name == name)
    }
}

const fn backend(name: &'static str, delivery: EventDelivery) -> EventClassification {
    EventClassification {
        name,
        delivery,
        origin: EventOrigin::Backend,
        excluded_from_v1: false,
    }
}
const fn webview(name: &'static str) -> EventClassification {
    EventClassification {
        name,
        delivery: EventDelivery::LocalOnly,
        origin: EventOrigin::Webview,
        excluded_from_v1: false,
    }
}
/// Backend-emitted host chrome that must never fan out to remote clients (§3.4 Local-only:
/// "window/dock/updater chrome"). `origin` stays truthful — these really are Rust emits — and
/// the capture bank drops every `LocalOnly` row structurally, so a truthful origin costs nothing.
const fn local_backend(name: &'static str) -> EventClassification {
    EventClassification {
        name,
        delivery: EventDelivery::LocalOnly,
        origin: EventOrigin::Backend,
        excluded_from_v1: false,
    }
}

// Exact names only. PR 0.1 mechanically audits this table against live emit and subscribe sites.
// PR 0.2 decision: render deltas (tool_call/message/hook) are transient; queue and recovery
// lifecycle invalidations are durable because clients must refetch authoritative state after replay.
// PR 0.2 decision (gates): `permission:request`/`permission:resolved`/`permission:expired` and
// `agent:ask_user_question` are wire-Transient, which is NOT a contradiction of §3.4's
// "permission / question gates are NOT transient" line. That line rejects treating gates as
// fire-and-forget: §3.4's resolution hydrates pending-gate truth via
// `list_pending_permission_gates` / `list_pending_question_gates` on every connect/reset,
// "without a durable event log for it". So the gate lifecycle is durable-gate-hydrated while the
// live nudge stays a transient frame — these names must never enter `remote_event_log`.
// PR 1.8 decision (local-only chrome): the three backend-emitted names below are host-owned UI
// chrome with no remote meaning. `ralphx://check-for-updates` / `ralphx://show-release-notes`
// come from the native menu (`application/native_menu.rs`) and act on the host binary.
// `gh-auth:login_prompt` (`commands/project_commands.rs`) surfaces the host owner's `gh` device
// -code prompt; relaying it would be worse than useless because its command surface
// (`login_gh_with_browser`, the whole `project_commands` git/gh module) is Denied remotely
// (§3.3 module-deny list) — a remote client could see the prompt but never complete it.
pub const EVENT_CLASSIFICATIONS: &[EventClassification] = &[
    backend("task:created", EventDelivery::Durable),
    backend("task:status_changed", EventDelivery::Durable),
    backend("task:merge_progress", EventDelivery::Durable),
    backend("task:merge_phases", EventDelivery::Durable),
    backend("task:archived", EventDelivery::Durable),
    backend("task:restored", EventDelivery::Durable),
    backend("notification:created", EventDelivery::Durable),
    backend("notification:updated", EventDelivery::Durable),
    backend("agent:run_started", EventDelivery::Durable),
    backend("agent:run_completed", EventDelivery::Durable),
    backend("agent:turn_completed", EventDelivery::Durable),
    backend("agent:message_created", EventDelivery::Durable),
    backend("agent:task_started", EventDelivery::Durable),
    backend("agent:task_completed", EventDelivery::Durable),
    backend("agent:error", EventDelivery::Durable),
    backend("agent:queue_sent", EventDelivery::Durable),
    backend("agent:message_queued", EventDelivery::Durable),
    backend("agent:session_recovered", EventDelivery::Durable),
    backend("automation:updated", EventDelivery::Durable),
    backend("automation:deleted", EventDelivery::Durable),
    backend("ticketing:cache_invalidated", EventDelivery::Durable),
    backend("task_validation:event", EventDelivery::Durable),
    backend("proposal:created", EventDelivery::Durable),
    backend("step:created", EventDelivery::Durable),
    backend("plan_artifact:created", EventDelivery::Durable),
    backend("plan_artifact:approved", EventDelivery::Durable),
    backend("execution:status_changed", EventDelivery::Durable),
    backend("agent:conversation_created", EventDelivery::Durable),
    backend("agent:conversation_forked", EventDelivery::Durable),
    backend("agent:conversation_title_updated", EventDelivery::Durable),
    backend("agent:question_resolved", EventDelivery::Durable),
    backend("agent:stopped", EventDelivery::Durable),
    backend("agent:workspace_changed", EventDelivery::Durable),
    backend("automation:run:updated", EventDelivery::Durable),
    backend("dependency:added", EventDelivery::Durable),
    backend("dependency:removed", EventDelivery::Durable),
    backend("execution:error", EventDelivery::Durable),
    backend("execution:queue_changed", EventDelivery::Durable),
    backend("file:change", EventDelivery::Durable),
    backend("ideation:child_session_created", EventDelivery::Durable),
    backend(
        "ideation:finalize_pending_confirmation",
        EventDelivery::Durable,
    ),
    backend("ideation:session_accepted", EventDelivery::Durable),
    backend("ideation:session_created", EventDelivery::Durable),
    backend("ideation:session_title_updated", EventDelivery::Durable),
    backend("merge:validation_start", EventDelivery::Durable),
    backend("merge:validation_step", EventDelivery::Durable),
    backend("notification:desktop_activated", EventDelivery::Durable),
    backend("persona:applied", EventDelivery::Durable),
    backend("persona:draft_updated", EventDelivery::Durable),
    backend("persona:injection_skipped", EventDelivery::Durable),
    backend("plan:merge_complete", EventDelivery::Durable),
    backend("plan_artifact:updated", EventDelivery::Durable),
    backend("plan_verification:status_changed", EventDelivery::Durable),
    backend("pr_review_artifact:created", EventDelivery::Durable),
    backend("pr_review_artifact:updated", EventDelivery::Durable),
    backend("project:analysis_complete", EventDelivery::Durable),
    backend("project:analysis_failed", EventDelivery::Durable),
    backend("proposal:deleted", EventDelivery::Durable),
    backend("proposal:priority_assessed", EventDelivery::Durable),
    backend("proposal:updated", EventDelivery::Durable),
    backend("proposals:reordered", EventDelivery::Durable),
    backend("qa:prep", EventDelivery::Durable),
    backend("qa:test", EventDelivery::Durable),
    backend("recovery:prompt", EventDelivery::Durable),
    backend("review:update", EventDelivery::Durable),
    backend("session:priorities_assessed", EventDelivery::Durable),
    backend("step:deleted", EventDelivery::Durable),
    backend("step:status_changed", EventDelivery::Durable),
    backend("step:updated", EventDelivery::Durable),
    backend("steps:reordered", EventDelivery::Durable),
    backend("supervisor:alert", EventDelivery::Durable),
    backend("supervisor:event", EventDelivery::Durable),
    backend("task:event", EventDelivery::Durable),
    backend("task:provider_error_paused", EventDelivery::Durable),
    backend("workspace_review_artifact:created", EventDelivery::Durable),
    backend("workspace_review_artifact:updated", EventDelivery::Durable),
    backend("agent:chunk", EventDelivery::Transient),
    backend("agent:usage_updated", EventDelivery::Transient),
    backend("agent:tool_call", EventDelivery::Transient),
    backend("agent:message", EventDelivery::Transient),
    backend("agent:hook", EventDelivery::Transient),
    backend("agent:ask_user_question", EventDelivery::Transient),
    backend("agent:heartbeat", EventDelivery::Transient),
    backend("agent:startup_progress", EventDelivery::Transient),
    backend("agent:workflow_progress", EventDelivery::Transient),
    backend("execution:stderr", EventDelivery::Transient),
    backend("permission:expired", EventDelivery::Transient),
    backend("permission:request", EventDelivery::Transient),
    backend("permission:resolved", EventDelivery::Transient),
    EventClassification {
        name: "agent_terminal:event",
        delivery: EventDelivery::Transient,
        origin: EventOrigin::Backend,
        excluded_from_v1: true,
    },
    webview("task:updated"),
    local_backend("ralphx://check-for-updates"),
    local_backend("ralphx://show-release-notes"),
    local_backend("gh-auth:login_prompt"),
    // PR 1.4 (§5.5): WS session lifecycle, emitted by the host when a remote client's socket
    // opens and closes. **Local-only is a security property, not housekeeping** — forwarding
    // these would let every paired device watch every other device connect and disconnect. The
    // host's own Remote Access pane consumes them (PR 1.7); the capture bank drops Local-only
    // rows structurally, so they can never reach the sequencer or `remote_event_log`.
    local_backend("remote:session_connected"),
    local_backend("remote:session_closed"),
    // PR 1.7 (§5.5): a pairing code was redeemed on `/remote/v1/auth/pair`. Local-only for the
    // same reason as the session pair — forwarding it would tell every paired device when a new
    // device joins the host. The host's own Remote Access pane consumes it to re-read its device,
    // session, outstanding-code, and audit lists from the backend.
    local_backend("remote:device_paired"),
    // PR 2.3: the CLIENT proxy's inbound relay channel — the outbound-WS relay re-emits every
    // host frame (`remote:stream_frame`) and its own teardown (`remote:stream_closed`) onto the
    // local bus for the TS NetworkEventBus. Local-only so a client's own relay can never be
    // captured and fanned back out to a host it is itself paired with. The environment id
    // travels in the PAYLOAD, not the name: a dynamic emit name is unresolvable to the P-6
    // emit scanner.
    local_backend("remote:stream_frame"),
    local_backend("remote:stream_closed"),
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn class_permits_enforces_representative_capability_boundaries() {
        assert!(!class_permits(
            RiskClass::Read,
            &[Capability::SpawnsProcess]
        ));
        assert!(class_permits(
            RiskClass::PathScoped,
            &[Capability::WritesArbitraryPath]
        ));
        assert!(!class_permits(
            RiskClass::PathScoped,
            &[Capability::SpawnsProcess]
        ));
        assert!(class_permits(
            RiskClass::AgentControl,
            &[Capability::AgentControl]
        ));
        assert!(!class_permits(
            RiskClass::AgentControl,
            &[Capability::SpawnsProcess]
        ));
        assert!(class_permits(
            RiskClass::Elevated,
            &[Capability::SpawnsProcess]
        ));
        assert!(!class_permits(RiskClass::Denied, &[]));
    }

    #[test]
    fn event_classification_find_returns_known_events_only() {
        let classification =
            EventClassification::find("task:created").expect("known event should resolve");
        assert_eq!(classification.delivery, EventDelivery::Durable);
        assert!(EventClassification::find("nonexistent:name").is_none());
    }
}
