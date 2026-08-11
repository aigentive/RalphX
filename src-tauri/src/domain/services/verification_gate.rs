use crate::domain::entities::ideation::SessionOrigin;
/// Verification gate — checks whether a session's plan is eligible for acceptance.
///
/// Called from all 3 acceptance paths: Tauri IPC, internal MCP HTTP, external MCP.
use crate::domain::entities::ideation::VerificationError;
use crate::domain::entities::IdeationSession;
use crate::domain::ideation::config::IdeationSettings;

/// Resolved gating policy for a specific (settings, origin) pair.
///
/// Computed once per request via `resolve_effective_gate_policy` and passed to all
/// gate callsites. This is a pure value type — no DB or async operations.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EffectiveGatePolicy {
    pub auto_verify_plans: bool,
    pub require_verification_for_accept: bool,
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
            auto_verify_plans: settings
                .external_overrides
                .auto_verify_plans
                .unwrap_or(settings.auto_verify_plans),
            require_verification_for_accept: settings
                .external_overrides
                .require_verification_for_accept
                .unwrap_or(settings.require_verification_for_accept),
            require_accept_for_finalize: settings
                .external_overrides
                .require_accept_for_finalize
                .unwrap_or(settings.require_accept_for_finalize),
        },
        _ => EffectiveGatePolicy {
            auto_verify_plans: settings.auto_verify_plans,
            require_verification_for_accept: settings.require_verification_for_accept,
            require_accept_for_finalize: settings.require_accept_for_finalize,
        },
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
    if !policy.require_verification_for_accept {
        return Ok(());
    }

    if session.has_exact_plan_verification() {
        Ok(())
    } else {
        Err(VerificationError::NotVerified)
    }
}

#[cfg(test)]
#[path = "verification_gate_tests.rs"]
mod tests;
