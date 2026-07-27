use chrono::Utc;

use crate::entities::{
    AgentWorkspaceRepairAttempt, AgentWorkspaceRepairContinuation, AgentWorkspaceRepairPhase,
    AgentWorkspaceRepairSource, ChatConversationId,
};

use super::{AgentWorkspaceRepairAttemptTransition, AgentWorkspaceRepairCompatibilityProjection};

fn attempt() -> AgentWorkspaceRepairAttempt {
    let mut attempt = AgentWorkspaceRepairAttempt::new(
        ChatConversationId::from_string("5b46a460-1699-47e6-a687-71305f4e5674"),
        AgentWorkspaceRepairSource::BaseUpdate,
        AgentWorkspaceRepairContinuation::UpdateOnly,
        "origin/main",
        false,
        false,
        false,
        None,
        Utc::now(),
    );
    attempt.generation = 1;
    attempt
}

#[test]
fn transition_cas_requires_the_exact_attempt_generation_and_phase() {
    let attempt = attempt();
    let mut updated = attempt.clone();
    updated.phase = AgentWorkspaceRepairPhase::Dispatching;

    let transition = AgentWorkspaceRepairAttemptTransition {
        attempt: updated,
        expected_phase: AgentWorkspaceRepairPhase::Requested,
        expected_updated_at: attempt.updated_at,
        next_phase: AgentWorkspaceRepairPhase::Dispatching,
        compatibility_projection: None,
        events: Vec::new(),
    };

    assert!(transition.matches_attempt(&attempt));

    let mut wrong_generation = attempt.clone();
    wrong_generation.generation += 1;
    assert!(!transition.matches_attempt(&wrong_generation));

    let mut wrong_phase = attempt;
    wrong_phase.phase = AgentWorkspaceRepairPhase::Repairing;
    assert!(!transition.matches_attempt(&wrong_phase));
}

#[test]
fn compatibility_projection_keeps_workspace_projection_and_events_explicit() {
    let projection = AgentWorkspaceRepairCompatibilityProjection {
        publication_push_status: Some("needs_agent".to_string()),
        pr_supervision_status: Some("fixing".to_string()),
        pr_supervision_summary: Some("Repairing base update".to_string()),
        pr_supervision_updated_at: Some(Utc::now()),
        pr_auto_merge_current: None,
        base_commit: None,
    };

    assert_eq!(
        projection.publication_push_status.as_deref(),
        Some("needs_agent")
    );
    assert_eq!(projection.pr_supervision_status.as_deref(), Some("fixing"));
}
