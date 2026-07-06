// Recovery policy types and pure decision logic for reconciliation.
//
// Contains all enums, structs, and the RecoveryPolicy decision methods.
// No I/O, no async — pure logic only.

use serde::Serialize;

use crate::domain::entities::{AgentRunStatus, InternalStatus};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecoveryContext {
    Execution,
    Review,
    Merge,
    PendingMerge,
    QaRefining,
    QaTesting,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RecoveryActionKind {
    None,
    ExecuteEntryActions,
    Transition(InternalStatus),
    AttemptMergeAutoComplete,
    Prompt,
}

#[derive(Debug, Clone)]
pub struct RecoveryDecision {
    pub action: RecoveryActionKind,
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Copy)]
pub struct RecoveryEvidence {
    pub run_status: Option<AgentRunStatus>,
    pub registry_running: bool,
    pub can_start: bool,
    pub is_stale: bool,
    pub is_deferred: bool,
}

impl RecoveryEvidence {
    pub fn has_conflict(&self) -> bool {
        match self.run_status {
            Some(AgentRunStatus::Running) => !self.registry_running,
            Some(_) => self.registry_running,
            None => self.registry_running,
        }
    }
}

#[derive(Default)]
pub struct RecoveryPolicy;

impl RecoveryPolicy {
    pub fn decide_reconciliation(
        &self,
        context: RecoveryContext,
        evidence: RecoveryEvidence,
    ) -> RecoveryDecision {
        match context {
            RecoveryContext::Execution => {
                if evidence.has_conflict() {
                    return RecoveryDecision {
                        action: RecoveryActionKind::Prompt,
                        reason: Some(
                            "Execution run state conflicts with running process tracking."
                                .to_string(),
                        ),
                    };
                }
                if evidence.run_status == Some(AgentRunStatus::Completed) {
                    return RecoveryDecision {
                        action: RecoveryActionKind::Transition(InternalStatus::PendingReview),
                        reason: None,
                    };
                }
                // Cancelled/Failed agent runs: the agent died without completing.
                // Re-execute entry actions to respawn (within retry budget enforced
                // by the caller in reconcile_completed_execution).
                if matches!(
                    evidence.run_status,
                    Some(AgentRunStatus::Cancelled) | Some(AgentRunStatus::Failed)
                ) {
                    if evidence.can_start {
                        return RecoveryDecision {
                            action: RecoveryActionKind::ExecuteEntryActions,
                            reason: Some(
                                "Agent run cancelled/failed — re-executing.".to_string(),
                            ),
                        };
                    }
                    return RecoveryDecision {
                        action: RecoveryActionKind::Prompt,
                        reason: Some(
                            "Agent run cancelled/failed but max concurrency reached.".to_string(),
                        ),
                    };
                }
                if evidence.run_status.is_none() {
                    if evidence.can_start {
                        return RecoveryDecision {
                            action: RecoveryActionKind::ExecuteEntryActions,
                            reason: None,
                        };
                    }
                    return RecoveryDecision {
                        action: RecoveryActionKind::Prompt,
                        reason: Some(
                            "Execution run missing but max concurrency is reached.".to_string(),
                        ),
                    };
                }
                RecoveryDecision {
                    action: RecoveryActionKind::None,
                    reason: None,
                }
            }
            RecoveryContext::Review => {
                if evidence.has_conflict() {
                    return RecoveryDecision {
                        action: RecoveryActionKind::Prompt,
                        reason: Some(
                            "Review run state conflicts with running process tracking.".to_string(),
                        ),
                    };
                }
                if evidence.run_status == Some(AgentRunStatus::Completed) {
                    return RecoveryDecision {
                        action: RecoveryActionKind::ExecuteEntryActions,
                        reason: None,
                    };
                }
                if matches!(
                    evidence.run_status,
                    Some(AgentRunStatus::Cancelled) | Some(AgentRunStatus::Failed)
                ) {
                    if evidence.can_start {
                        return RecoveryDecision {
                            action: RecoveryActionKind::ExecuteEntryActions,
                            reason: Some(
                                "Review agent cancelled/failed — re-executing.".to_string(),
                            ),
                        };
                    }
                    return RecoveryDecision {
                        action: RecoveryActionKind::Prompt,
                        reason: Some(
                            "Review agent cancelled/failed but max concurrency reached."
                                .to_string(),
                        ),
                    };
                }
                if evidence.run_status.is_none() {
                    if evidence.can_start {
                        return RecoveryDecision {
                            action: RecoveryActionKind::ExecuteEntryActions,
                            reason: None,
                        };
                    }
                    return RecoveryDecision {
                        action: RecoveryActionKind::Prompt,
                        reason: Some(
                            "Review run missing but max concurrency is reached.".to_string(),
                        ),
                    };
                }
                RecoveryDecision {
                    action: RecoveryActionKind::None,
                    reason: None,
                }
            }
            RecoveryContext::Merge => {
                if evidence.has_conflict() {
                    return RecoveryDecision {
                        action: RecoveryActionKind::Prompt,
                        reason: Some(
                            "Merge run state conflicts with running process tracking.".to_string(),
                        ),
                    };
                }
                if evidence.run_status == Some(AgentRunStatus::Completed) {
                    return RecoveryDecision {
                        action: RecoveryActionKind::AttemptMergeAutoComplete,
                        reason: None,
                    };
                }
                if evidence.is_stale {
                    return RecoveryDecision {
                        action: RecoveryActionKind::AttemptMergeAutoComplete,
                        reason: Some(
                            "Merge timed out — attempting auto-complete before escalating."
                                .to_string(),
                        ),
                    };
                }
                if matches!(
                    evidence.run_status,
                    Some(AgentRunStatus::Cancelled) | Some(AgentRunStatus::Failed)
                ) {
                    if evidence.can_start {
                        return RecoveryDecision {
                            action: RecoveryActionKind::ExecuteEntryActions,
                            reason: Some(
                                "Merger agent cancelled/failed — re-executing.".to_string(),
                            ),
                        };
                    }
                    return RecoveryDecision {
                        action: RecoveryActionKind::Prompt,
                        reason: Some(
                            "Merger agent cancelled/failed but max concurrency reached."
                                .to_string(),
                        ),
                    };
                }
                if evidence.run_status.is_none() {
                    if evidence.can_start {
                        return RecoveryDecision {
                            action: RecoveryActionKind::ExecuteEntryActions,
                            reason: None,
                        };
                    }
                    return RecoveryDecision {
                        action: RecoveryActionKind::Prompt,
                        reason: Some(
                            "Merge run missing but max concurrency is reached.".to_string(),
                        ),
                    };
                }
                RecoveryDecision {
                    action: RecoveryActionKind::None,
                    reason: None,
                }
            }
            RecoveryContext::PendingMerge => {
                if !evidence.is_stale {
                    return RecoveryDecision {
                        action: RecoveryActionKind::None,
                        reason: None,
                    };
                }
                if evidence.is_deferred {
                    return RecoveryDecision {
                        action: RecoveryActionKind::ExecuteEntryActions,
                        reason: Some(
                            "Stale deferred merge — re-triggering entry actions.".to_string(),
                        ),
                    };
                }
                RecoveryDecision {
                    action: RecoveryActionKind::Transition(InternalStatus::MergeIncomplete),
                    reason: Some(
                        "Stale pending merge with no deferred flag — surfacing to user."
                            .to_string(),
                    ),
                }
            }
            RecoveryContext::QaRefining | RecoveryContext::QaTesting => {
                if !evidence.is_stale {
                    return RecoveryDecision {
                        action: RecoveryActionKind::None,
                        reason: None,
                    };
                }
                if !evidence.can_start {
                    return RecoveryDecision {
                        action: RecoveryActionKind::Prompt,
                        reason: Some(
                            "QA task is stale but max concurrency is reached.".to_string(),
                        ),
                    };
                }
                RecoveryDecision {
                    action: RecoveryActionKind::ExecuteEntryActions,
                    reason: None,
                }
            }
        }
    }

    pub fn decide_execution_stop(&self, evidence: RecoveryEvidence) -> RecoveryDecision {
        if evidence.has_conflict() {
            return RecoveryDecision {
                action: RecoveryActionKind::Prompt,
                reason: Some(
                    "Execution run state conflicts with running process tracking.".to_string(),
                ),
            };
        }
        if evidence.run_status == Some(AgentRunStatus::Completed) {
            return RecoveryDecision {
                action: RecoveryActionKind::Transition(InternalStatus::PendingReview),
                reason: None,
            };
        }
        RecoveryDecision {
            action: RecoveryActionKind::Transition(InternalStatus::Ready),
            reason: None,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RecoveryPromptAction {
    pub(crate) id: String,
    pub(crate) label: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RecoveryPromptEvent {
    pub(crate) task_id: String,
    pub(crate) status: InternalStatus,
    pub(crate) context_type: String,
    pub(crate) reason: String,
    pub(crate) primary_action: RecoveryPromptAction,
    pub(crate) secondary_action: RecoveryPromptAction,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UserRecoveryAction {
    Restart,
    Cancel,
}

/// Result of comparing source branch SHA at failure time vs current SHA.
#[derive(Debug)]
pub(crate) enum ShaComparisonResult {
    /// SHA unchanged — retrying would produce the same conflict
    Unchanged(String),
    /// SHA changed — new commits pushed, retry may succeed
    Changed { old_sha: String, new_sha: String },
    /// No SHA was stored at failure time — cannot compare, allow retry
    NoStoredSha,
    /// Git error while reading current SHA — allow retry conservatively
    GitError,
}
