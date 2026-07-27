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

// Exact names only. PR 0.1 mechanically audits this table against live emit and subscribe sites.
// PR 0.2 decision: render deltas (tool_call/message/hook) are transient; queue and recovery
// lifecycle invalidations are durable because clients must refetch authoritative state after replay.
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
    webview("my:event"),
    webview("window:focus"),
    webview("dock:updated"),
    webview("updater:status"),
];
