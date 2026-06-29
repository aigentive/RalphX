/// Verification gate — checks whether a session's plan is eligible for acceptance
/// or proposal mutation (create/update/delete).
///
/// Called from all 3 acceptance paths: Tauri IPC, internal MCP HTTP, external MCP.
use crate::domain::entities::ideation::{VerificationError, VerificationStatus};
use crate::domain::entities::ideation::SessionOrigin;
use crate::domain::entities::IdeationSession;
use crate::domain::ideation::config::IdeationSettings;

/// Resolved gating policy for a specific (settings, origin) pair.
///
/// Computed once per request via `resolve_effective_gate_policy` and passed to all
/// gate callsites. This is a pure value type — no DB or async operations.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EffectiveGatePolicy {
    pub require_verification_for_accept: bool,
    pub require_verification_for_proposals: bool,
    pub require_accept_for_finalize: bool,
}

/// Resolve the effective gating policy for a session.
///
/// For `SessionOrigin::External`, each field is overridden by the corresponding
/// `external_overrides` value if `Some`, otherwise falls back to the base field.
/// For all other origins, the base fields are used directly (overrides ignored).
///
/// This function is pure and synchronous — call it once per request and cache the result.
pub fn resolve_effective_gate_policy(
    settings: &IdeationSettings,
    origin: SessionOrigin,
) -> EffectiveGatePolicy {
    match origin {
        SessionOrigin::External => EffectiveGatePolicy {
            require_verification_for_accept: settings
                .external_overrides
                .require_verification_for_accept
                .unwrap_or(settings.require_verification_for_accept),
            require_verification_for_proposals: settings
                .external_overrides
                .require_verification_for_proposals
                .unwrap_or(settings.require_verification_for_proposals),
            require_accept_for_finalize: settings
                .external_overrides
                .require_accept_for_finalize
                .unwrap_or(settings.require_accept_for_finalize),
        },
        _ => EffectiveGatePolicy {
            require_verification_for_accept: settings.require_verification_for_accept,
            require_verification_for_proposals: settings.require_verification_for_proposals,
            require_accept_for_finalize: settings.require_accept_for_finalize,
        },
    }
}

/// Identifies the proposal mutation operation being gated.
///
/// Used by `check_proposal_verification_gate()` to determine which statuses to block.
/// Distinct from `UpdateSource` which tracks the caller origin (API vs. IPC).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProposalOperation {
    Create,
    Update,
    Delete,
}

impl std::fmt::Display for ProposalOperation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ProposalOperation::Create => write!(f, "create"),
            ProposalOperation::Update => write!(f, "update"),
            ProposalOperation::Delete => write!(f, "delete"),
        }
    }
}

/// Check if the session's plan is eligible for acceptance.
///
/// # Errors
///
/// Returns a `VerificationError` when the gate blocks acceptance.
pub fn check_verification_gate(
    session: &IdeationSession,
    policy: &EffectiveGatePolicy,
) -> Result<(), VerificationError> {
    // Check active states first: optional verification can bypass unverified/completed
    // outcomes, but it must not finalize while the current verifier owns the plan.
    if session.verification_in_progress {
        let (round, max_rounds) = (
            session.verification_current_round.unwrap_or(0),
            session.verification_max_rounds.unwrap_or(0),
        );
        return Err(VerificationError::InProgress { round, max_rounds });
    }
    if session.verification_status == VerificationStatus::Reviewing {
        let (round, max_rounds) = (
            session.verification_current_round.unwrap_or(0),
            session.verification_max_rounds.unwrap_or(0),
        );
        return Err(VerificationError::InProgress { round, max_rounds });
    }
    if !policy.require_verification_for_accept {
        return Ok(());
    }
    // External sessions cannot accept with Skipped status — they must run verification to completion
    if session.verification_status == VerificationStatus::Skipped
        && session.origin == SessionOrigin::External
    {
        return Err(VerificationError::ExternalCannotSkip);
    }

    match session.verification_status {
        VerificationStatus::Verified | VerificationStatus::Skipped | VerificationStatus::ImportedVerified => Ok(()),
        // Defense-in-depth: Reviewing arm still present in case in_progress flag is inconsistent
        VerificationStatus::Reviewing => {
            let (round, max_rounds) =
                (session.verification_current_round.unwrap_or(0), session.verification_max_rounds.unwrap_or(0));
            Err(VerificationError::InProgress { round, max_rounds })
        }
        VerificationStatus::NeedsRevision => {
            let count = session.verification_gap_count;
            Err(VerificationError::HasUnresolvedGaps { count })
        }
        VerificationStatus::Unverified => Err(VerificationError::NotVerified),
    }
}

/// Check if a proposal mutation is allowed given the session's plan verification state.
///
/// # Arguments
///
/// - `session` — the ideation session the proposal belongs to
/// - `settings` — project-level ideation settings
/// - `parent_verification_status` — verification status of the parent session (only relevant when
///   `session.inherited_plan_artifact_id` is set and `session.plan_artifact_id` is None).
///   Pass `None` when the parent session cannot be found — this triggers graceful degradation
///   (proposals are allowed to avoid orphaned child sessions).
/// - `operation` — the mutation being attempted (`Create`, `Update`, or `Delete`)
///
/// # Gate Logic
///
/// - Config bypass: `!settings.require_verification_for_proposals` → `Ok(())`
/// - No plan, no inherited plan → `Ok(())` (passthrough — nothing to verify)
/// - Own plan (`plan_artifact_id.is_some()`) → use `session.verification_status`
/// - Inherited plan only → use `parent_verification_status`; `None` (parent deleted) → `Ok(())`
/// - `Create`: blocks `Unverified`, `Reviewing`, `NeedsRevision`
/// - `Update | Delete`: blocks `Reviewing`, `NeedsRevision` only (allowing edits before verification starts)
///
/// # Errors
///
/// Returns a `VerificationError::Proposal*` variant when the gate blocks the operation.
pub fn check_proposal_verification_gate(
    session: &IdeationSession,
    policy: &EffectiveGatePolicy,
    parent_verification_status: Option<VerificationStatus>,
    operation: ProposalOperation,
) -> Result<(), VerificationError> {
    // Config bypass — gate is opt-in
    if !policy.require_verification_for_proposals {
        return Ok(());
    }

    // Determine which verification status to check
    let effective_status = if session.plan_artifact_id.is_some() {
        // Child with own plan → use own status
        session.verification_status
    } else if session.inherited_plan_artifact_id.is_some() {
        // Child with inherited plan → use parent status
        match parent_verification_status {
            // Parent deleted (FK ON DELETE SET NULL) → graceful degradation
            None => return Ok(()),
            Some(status) => status,
        }
    } else {
        // No plan at all → passthrough (nothing to verify)
        return Ok(());
    };

    let op_str = operation.to_string();

    match (operation, effective_status) {
        // Verified or ImportedVerified always allow
        (_, VerificationStatus::Verified | VerificationStatus::ImportedVerified) => Ok(()),

        // Skipped blocks Create — users must re-run verification before creating new proposals
        (ProposalOperation::Create, VerificationStatus::Skipped) => {
            Err(VerificationError::ProposalSkippedNotAllowed)
        }
        // Skipped allows Update/Delete — existing proposals can still be modified
        (ProposalOperation::Update | ProposalOperation::Delete, VerificationStatus::Skipped) => Ok(()),

        // Create blocks Unverified
        (ProposalOperation::Create, VerificationStatus::Unverified) => {
            Err(VerificationError::ProposalNotVerified)
        }
        // Update/Delete allow Unverified (editing before review starts is fine)
        (ProposalOperation::Update | ProposalOperation::Delete, VerificationStatus::Unverified) => {
            Ok(())
        }

        // All operations block Reviewing
        (_, VerificationStatus::Reviewing) => {
            let (round, max_rounds) =
                (session.verification_current_round.unwrap_or(0), session.verification_max_rounds.unwrap_or(0));
            Err(VerificationError::ProposalReviewInProgress {
                operation: op_str,
                round,
                max_rounds,
            })
        }

        // All operations block NeedsRevision
        (_, VerificationStatus::NeedsRevision) => {
            let gap_count = session.verification_gap_count as usize;
            Err(VerificationError::ProposalHasUnresolvedGaps {
                operation: op_str,
                gap_count,
            })
        }
    }
}

#[cfg(test)]
#[path = "verification_gate_tests.rs"]
mod tests;
