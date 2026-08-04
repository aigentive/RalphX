use std::fmt;
use std::str::FromStr;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::{AgentRunId, ChatConversationId};

macro_rules! repair_string_enum {
    ($name:ident { $($variant:ident => $wire:literal),+ $(,)? }) => {
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
        #[serde(rename_all = "snake_case")]
        pub enum $name { $($variant),+ }

        impl $name {
            pub fn as_str(self) -> &'static str {
                match self { $(Self::$variant => $wire),+ }
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str(self.as_str())
            }
        }

        impl FromStr for $name {
            type Err = String;

            fn from_str(value: &str) -> Result<Self, Self::Err> {
                match value {
                    $($wire => Ok(Self::$variant),)+
                    _ => Err(format!("unknown {} value: '{value}'", stringify!($name))),
                }
            }
        }
    };
}

macro_rules! repair_id {
    ($name:ident) => {
        #[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
        #[serde(transparent)]
        pub struct $name(String);

        impl $name {
            pub fn new() -> Self {
                Self(Uuid::new_v4().to_string())
            }

            pub fn from_string(value: impl Into<String>) -> Self {
                Self(value.into())
            }

            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl Default for $name {
            fn default() -> Self {
                Self::new()
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str(&self.0)
            }
        }
    };
}

repair_id!(AgentWorkspaceRepairAttemptId);
repair_id!(AgentWorkspaceRepairEffectId);

repair_string_enum!(AgentWorkspaceRepairSource {
    BaseUpdate => "base_update",
    Publish => "publish",
    PrConflict => "pr_conflict",
    PrAutofix => "pr_autofix",
    Legacy => "legacy",
});

repair_string_enum!(AgentWorkspaceRepairPhase {
    Requested => "requested",
    Dispatching => "dispatching",
    Repairing => "repairing",
    Validating => "validating",
    AwaitingReview => "awaiting_review",
    ContinuationPending => "continuation_pending",
    Continuing => "continuing",
    Ready => "ready",
    Blocked => "blocked",
});

repair_string_enum!(AgentWorkspaceRepairContinuation {
    UpdateOnly => "update_only",
    Publish => "publish",
    ResumePrSupervision => "resume_pr_supervision",
    Manual => "manual",
});

impl AgentWorkspaceRepairContinuation {
    pub fn priority(self) -> u8 {
        match self {
            Self::Manual => 0,
            Self::UpdateOnly => 1,
            Self::Publish => 2,
            Self::ResumePrSupervision => 3,
        }
    }

    pub fn is_automatic(self) -> bool {
        !matches!(self, Self::Manual)
    }
}

repair_string_enum!(AgentWorkspaceRepairOutcome {
    Succeeded => "succeeded",
    Superseded => "superseded",
    Failed => "failed",
    Cancelled => "cancelled",
});

repair_string_enum!(AgentWorkspaceRepairEffectKind {
    PushBranch => "push_branch",
    CreatePr => "create_pr",
    UpdatePr => "update_pr",
    RestoreAutoMerge => "restore_auto_merge",
});

repair_string_enum!(AgentWorkspaceRepairEffectStatus {
    Pending => "pending",
    InFlight => "in_flight",
    Observed => "observed",
    Failed => "failed",
});

repair_string_enum!(AgentWorkspaceRepairOperationStage {
    UpdatingBase => "updating_base",
    Repairing => "repairing",
    Validating => "validating",
    Reviewing => "reviewing",
    Publishing => "publishing",
    Ready => "ready",
    Blocked => "blocked",
    Held => "held",
});

repair_string_enum!(AgentWorkspaceRepairOperationStatus {
    Active => "active",
    Ready => "ready",
    Blocked => "blocked",
    Held => "held",
});

repair_string_enum!(AgentWorkspaceRepairOperationHoldReason {
    UnchangedHealth => "pr_autofix_unchanged_health",
    PreExistingOnBase => "pr_autofix_pre_existing_on_base",
    CiRerunPending => "pr_autofix_ci_rerun_pending",
});

pub const PR_AUTOFIX_PRE_EXISTING_ON_BASE_PENDING_REASON: &str = "pr_autofix_pre_existing_on_base";
pub const PR_AUTOFIX_UNCHANGED_HEALTH_PENDING_REASON: &str = "pr_autofix_unchanged_health";
pub const PR_AUTOFIX_HEAD_REDRIVE_PENDING_REASON_PREFIX: &str = "pr_autofix_head_redrive:";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentWorkspaceRepairAttempt {
    pub id: AgentWorkspaceRepairAttemptId,
    pub conversation_id: ChatConversationId,
    pub generation: u64,
    pub source: AgentWorkspaceRepairSource,
    pub phase: AgentWorkspaceRepairPhase,
    pub continuation: AgentWorkspaceRepairContinuation,
    pub reserved_agent_run_id: Option<AgentRunId>,
    pub target_base_ref: String,
    pub target_base_commit: Option<String>,
    pub pending_reasons: Vec<String>,
    pub review_required: bool,
    pub auto_publish_enabled: bool,
    #[serde(default)]
    pub explicit_publish_requested: bool,
    pub auto_merge_desired: bool,
    pub auto_merge_method: Option<String>,
    pub dispatch_count: u32,
    /// Backend-owned retry budget for transient GitHub Actions reruns.
    pub ci_rerun_count: u32,
    /// PR-health fingerprint for the currently requested rerun; never model supplied.
    pub ci_rerun_fingerprint: Option<String>,
    /// Exact PR head observed by the poller before dispatching a PR autofix run.
    pub pr_autofix_dispatch_head_commit: Option<String>,
    /// Stable failing PR-health identity observed by the poller before dispatching a PR autofix.
    pub pr_autofix_health_fingerprint: Option<String>,
    pub next_dispatch_at: Option<DateTime<Utc>>,
    pub repair_head_commit: Option<String>,
    pub summary: Option<String>,
    pub blocker: Option<String>,
    pub git_common_dir: Option<String>,
    pub target_ref: Option<String>,
    pub target_identity_version: Option<u64>,
    pub target_lease_epoch: Option<u64>,
    pub outcome: Option<AgentWorkspaceRepairOutcome>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub settled_at: Option<DateTime<Utc>>,
}

impl AgentWorkspaceRepairAttempt {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        conversation_id: ChatConversationId,
        source: AgentWorkspaceRepairSource,
        continuation: AgentWorkspaceRepairContinuation,
        target_base_ref: impl Into<String>,
        review_required: bool,
        auto_publish_enabled: bool,
        auto_merge_desired: bool,
        auto_merge_method: Option<String>,
        now: DateTime<Utc>,
    ) -> Self {
        Self {
            id: AgentWorkspaceRepairAttemptId::new(),
            conversation_id,
            generation: 0,
            source,
            phase: AgentWorkspaceRepairPhase::Requested,
            continuation,
            reserved_agent_run_id: None,
            target_base_ref: target_base_ref.into(),
            target_base_commit: None,
            pending_reasons: Vec::new(),
            review_required,
            auto_publish_enabled,
            explicit_publish_requested: false,
            auto_merge_desired,
            auto_merge_method,
            dispatch_count: 0,
            ci_rerun_count: 0,
            ci_rerun_fingerprint: None,
            pr_autofix_dispatch_head_commit: None,
            pr_autofix_health_fingerprint: None,
            next_dispatch_at: None,
            repair_head_commit: None,
            summary: None,
            blocker: None,
            git_common_dir: None,
            target_ref: None,
            target_identity_version: None,
            target_lease_epoch: None,
            outcome: None,
            created_at: now,
            updated_at: now,
            settled_at: None,
        }
    }

    pub fn is_unsettled(&self) -> bool {
        self.settled_at.is_none()
    }

    pub fn operation_snapshot(&self) -> AgentWorkspaceRepairOperationSnapshot {
        let publish_redrive = self.phase == AgentWorkspaceRepairPhase::Ready
            && self
                .pending_reasons
                .iter()
                .any(|reason| reason.starts_with(PR_AUTOFIX_HEAD_REDRIVE_PENDING_REASON_PREFIX));
        let hold_reason = if self.phase == AgentWorkspaceRepairPhase::Ready && !publish_redrive {
            self.pending_reasons
                .iter()
                .find_map(|reason| AgentWorkspaceRepairOperationHoldReason::from_str(reason).ok())
                .or_else(|| {
                    ((self.ci_rerun_count > 0 && self.ci_rerun_fingerprint.is_some())
                        || self
                            .pending_reasons
                            .iter()
                            .any(|reason| reason == "pr_autofix_awaiting_ci"))
                    .then_some(AgentWorkspaceRepairOperationHoldReason::CiRerunPending)
                })
        } else {
            None
        };
        let stage = if publish_redrive {
            AgentWorkspaceRepairOperationStage::Publishing
        } else if hold_reason.is_some() {
            AgentWorkspaceRepairOperationStage::Held
        } else {
            match self.phase {
                AgentWorkspaceRepairPhase::Requested | AgentWorkspaceRepairPhase::Dispatching => {
                    AgentWorkspaceRepairOperationStage::UpdatingBase
                }
                AgentWorkspaceRepairPhase::Repairing => {
                    AgentWorkspaceRepairOperationStage::Repairing
                }
                AgentWorkspaceRepairPhase::Validating => {
                    AgentWorkspaceRepairOperationStage::Validating
                }
                AgentWorkspaceRepairPhase::AwaitingReview => {
                    AgentWorkspaceRepairOperationStage::Reviewing
                }
                AgentWorkspaceRepairPhase::ContinuationPending
                | AgentWorkspaceRepairPhase::Continuing => {
                    AgentWorkspaceRepairOperationStage::Publishing
                }
                AgentWorkspaceRepairPhase::Ready => AgentWorkspaceRepairOperationStage::Ready,
                AgentWorkspaceRepairPhase::Blocked => AgentWorkspaceRepairOperationStage::Blocked,
            }
        };
        let status = if publish_redrive {
            AgentWorkspaceRepairOperationStatus::Active
        } else if hold_reason.is_some() {
            AgentWorkspaceRepairOperationStatus::Held
        } else {
            match self.phase {
                AgentWorkspaceRepairPhase::Ready => AgentWorkspaceRepairOperationStatus::Ready,
                AgentWorkspaceRepairPhase::Blocked => AgentWorkspaceRepairOperationStatus::Blocked,
                _ => AgentWorkspaceRepairOperationStatus::Active,
            }
        };

        AgentWorkspaceRepairOperationSnapshot {
            operation_id: self.id.to_string(),
            generation: self.generation,
            source: self.source,
            stage,
            status,
            hold_reason,
            summary: self.summary.clone(),
            blocker: self.blocker.clone(),
            automatic_continuation: self.continuation.is_automatic()
                && matches!(status, AgentWorkspaceRepairOperationStatus::Active),
            started_at: self.created_at,
            updated_at: self.updated_at,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentWorkspaceRepairEffect {
    pub id: AgentWorkspaceRepairEffectId,
    pub attempt_id: AgentWorkspaceRepairAttemptId,
    pub kind: AgentWorkspaceRepairEffectKind,
    pub status: AgentWorkspaceRepairEffectStatus,
    pub idempotency_key: String,
    pub intended_head_oid: Option<String>,
    pub expected_remote_oid: Option<String>,
    pub expected_pr_number: Option<i64>,
    pub expected_remote_absent: bool,
    pub receipt_json: Option<String>,
    pub last_error: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub completed_at: Option<DateTime<Utc>>,
}

impl AgentWorkspaceRepairEffect {
    pub fn new(
        attempt_id: AgentWorkspaceRepairAttemptId,
        kind: AgentWorkspaceRepairEffectKind,
        idempotency_key: impl Into<String>,
        now: DateTime<Utc>,
    ) -> Self {
        Self {
            id: AgentWorkspaceRepairEffectId::new(),
            attempt_id,
            kind,
            status: AgentWorkspaceRepairEffectStatus::Pending,
            idempotency_key: idempotency_key.into(),
            intended_head_oid: None,
            expected_remote_oid: None,
            expected_pr_number: None,
            expected_remote_absent: false,
            receipt_json: None,
            last_error: None,
            created_at: now,
            updated_at: now,
            completed_at: None,
        }
    }

    pub fn can_complete_observed(
        &self,
        receipt_json: Option<&str>,
        completed_at: DateTime<Utc>,
    ) -> Result<(), String> {
        if receipt_json.is_none_or(|receipt| receipt.trim().is_empty()) {
            return Err("observed repair effects require a receipt".to_string());
        }
        if completed_at < self.created_at {
            return Err("repair effect completion cannot predate creation".to_string());
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum AgentWorkspaceRepairCompletionAuthority {
    Current(Box<AgentWorkspaceRepairAttempt>),
    Superseded,
    AlreadyCompleted,
    AlreadyBlocked,
    Invalid,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentWorkspaceRepairOperationSnapshot {
    pub operation_id: String,
    pub generation: u64,
    pub source: AgentWorkspaceRepairSource,
    pub stage: AgentWorkspaceRepairOperationStage,
    pub status: AgentWorkspaceRepairOperationStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hold_reason: Option<AgentWorkspaceRepairOperationHoldReason>,
    pub summary: Option<String>,
    pub blocker: Option<String>,
    pub automatic_continuation: bool,
    pub started_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}
