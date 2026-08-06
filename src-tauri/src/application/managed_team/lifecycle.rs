//! Construction defaults for durable managed-Team state.

use chrono::Utc;

use crate::domain::entities::{
    AgentRunId, ChatConversationId, DelegatedSessionId, ProjectId, TeamMember, TeamRunBinding,
    TeamRunBindingId, TeamRunBindingStatus, TeamRunTriggerKind, TeamSession, TeamSessionId,
    TeamSessionStatus, TeamWorkClassification,
};

/// Default number of members allowed to run concurrently in a new Team.
pub const DEFAULT_TEAM_CONCURRENCY: u32 = 2;
/// Default cap on concurrent in-flight automatic coordinator wake batches
/// (`Planned`/`Launching`/`Running` `WakeBatch`-triggered run bindings) for a
/// Team; settled (terminal/failed/cancelled) wake bindings do not count
/// against this budget.
pub const DEFAULT_AUTOMATIC_WAKE_LIMIT: u32 = 5;

/// Builds the durable session row for a newly ensured Team.
pub fn new_team_session(
    project_id: ProjectId,
    coordinator_conversation_id: ChatConversationId,
) -> TeamSession {
    let now = Utc::now();
    TeamSession {
        id: TeamSessionId::new(),
        project_id,
        coordinator_conversation_id,
        status: TeamSessionStatus::Active,
        strategy: None,
        configured_concurrency: DEFAULT_TEAM_CONCURRENCY,
        effective_concurrency: DEFAULT_TEAM_CONCURRENCY,
        automatic_wake_limit: DEFAULT_AUTOMATIC_WAKE_LIMIT,
        budget_policy: None,
        pending_coordination_mode: None,
        pending_exit_action: None,
        version: 0,
        last_error: None,
        created_at: now,
        updated_at: now,
        closed_at: None,
    }
}

/// Builds the member-null, coordination-only run binding recorded before a
/// managed coordinator turn launches.
pub fn new_coordinator_run_binding(
    team_id: TeamSessionId,
    conversation_id: ChatConversationId,
    agent_run_id: AgentRunId,
) -> TeamRunBinding {
    TeamRunBinding {
        id: TeamRunBindingId::new(),
        team_id,
        team_member_id: None,
        team_member_generation: None,
        agent_run_id,
        conversation_id,
        delegated_session_id: None,
        trigger_kind: TeamRunTriggerKind::UserCoordinatorTurn,
        work_classification: TeamWorkClassification::CoordinationOnly,
        assignment_id: None,
        first_message_sequence: None,
        last_message_sequence: None,
        status: TeamRunBindingStatus::Planned,
        version: 0,
        last_error: None,
        created_at: Utc::now(),
        launched_at: None,
        terminal_at: None,
    }
}

/// Builds the pre-launch binding for a claimed coordinator wake batch.
pub fn new_coordinator_wake_run_binding(
    team_id: TeamSessionId,
    conversation_id: ChatConversationId,
    agent_run_id: AgentRunId,
    first_message_sequence: i64,
    last_message_sequence: i64,
) -> TeamRunBinding {
    let mut binding = new_coordinator_run_binding(team_id, conversation_id, agent_run_id);
    binding.trigger_kind = TeamRunTriggerKind::WakeBatch;
    binding.first_message_sequence = Some(first_message_sequence);
    binding.last_message_sequence = Some(last_message_sequence);
    binding
}

/// Builds the durable pre-launch binding for an exact member assignment.
pub fn new_member_assignment_run_binding(
    member: &TeamMember,
    delegated_session_id: DelegatedSessionId,
    conversation_id: ChatConversationId,
    agent_run_id: AgentRunId,
    assignment_id: crate::domain::entities::AgentTaskAssignmentId,
    work_classification: TeamWorkClassification,
) -> TeamRunBinding {
    TeamRunBinding {
        id: TeamRunBindingId::new(),
        team_id: member.team_id.clone(),
        team_member_id: Some(member.id.clone()),
        team_member_generation: Some(member.generation),
        agent_run_id,
        conversation_id,
        delegated_session_id: Some(delegated_session_id),
        trigger_kind: TeamRunTriggerKind::Assignment,
        work_classification,
        assignment_id: Some(assignment_id),
        first_message_sequence: None,
        last_message_sequence: None,
        status: TeamRunBindingStatus::Planned,
        version: 0,
        last_error: None,
        created_at: Utc::now(),
        launched_at: None,
        terminal_at: None,
    }
}

/// Steps `binding` to `target` through the legal intermediate statuses.
///
/// `TeamRunBindingStatus` only permits single-step moves along
/// `Planned -> Launching -> Running -> Terminal` (with `Cancelled`/`Failed`
/// reachable directly from any live status), so callers that observe a
/// coordinator run *after the fact* — a successful wake send, or a run that
/// already ended — cannot express the outcome in one `transition_to`.
///
/// Mutates `binding` in memory only; the caller owns version bumping and
/// persistence. Returns `false` when no legal path exists, leaving `binding`
/// partially advanced, so callers must treat `false` as "do not persist".
pub fn advance_binding_status(
    binding: &mut TeamRunBinding,
    target: TeamRunBindingStatus,
    now: chrono::DateTime<Utc>,
) -> bool {
    // `Planned -> Launching -> Running -> Terminal` is the longest path, so
    // four iterations always terminate.
    for _ in 0..4 {
        if binding.status == target {
            return true;
        }
        let step = if binding.status.can_transition_to(target) {
            target
        } else {
            match binding.status {
                TeamRunBindingStatus::Planned => TeamRunBindingStatus::Launching,
                TeamRunBindingStatus::Launching => TeamRunBindingStatus::Running,
                _ => return false,
            }
        };
        if binding.transition_to(step, now).is_err() {
            return false;
        }
    }
    false
}
