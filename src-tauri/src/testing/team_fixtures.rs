//! Shared builders for managed Team repository tests (SQLite + memory parity).

use chrono::{DateTime, Utc};

use crate::domain::entities::{
    ChatConversationId, ProjectId, TeamMember, TeamMemberId, TeamMemberStatus, TeamMessage,
    TeamMessageActorKind, TeamMessageDelivery, TeamMessageDeliveryId, TeamMessageDeliveryStatus,
    TeamMessageEnvelopeTarget, TeamMessageId, TeamMessageKind, TeamRunBinding, TeamRunBindingId,
    TeamRunBindingStatus, TeamRunTriggerKind, TeamSession, TeamSessionId, TeamSessionStatus,
    TeamWakeBatch, TeamWakeBatchId, TeamWakeBatchStatus, TeamWakeRecipientKind,
    TeamWorkClassification, TeamWorkspaceReservation, TeamWorkspaceReservationId,
};

pub fn fixed_time() -> DateTime<Utc> {
    DateTime::parse_from_rfc3339("2026-07-28T12:00:00+00:00")
        .expect("valid fixture timestamp")
        .with_timezone(&Utc)
}

/// Deterministic, distinct conversation IDs. `ChatConversationId` wraps a UUID
/// (arbitrary strings collapse to the nil UUID), so fixtures must mint real ones.
pub fn team_conversation_id(index: u64) -> ChatConversationId {
    ChatConversationId::from_string(format!("00000000-0000-0000-0000-{index:012}"))
}

/// Deterministic, distinct agent run IDs (`AgentRunId` is UUID-backed too).
pub fn team_agent_run_id(index: u64) -> crate::domain::entities::AgentRunId {
    crate::domain::entities::AgentRunId::from_string(format!("00000000-0000-0000-0001-{index:012}"))
}

/// Seeds a minimal `managed_team_sessions` row so FK-checked child tables can insert.
pub fn seed_team_session_row(conn: &rusqlite::Connection, team_id: &str, conversation_index: u64) {
    conn.execute(
        "INSERT INTO managed_team_sessions (
            id, project_id, coordinator_conversation_id, status,
            configured_concurrency, effective_concurrency, automatic_wake_limit,
            created_at, updated_at
        ) VALUES (?1, 'project-1', ?2, 'active', 2, 2, 5, ?3, ?3)",
        rusqlite::params![
            team_id,
            team_conversation_id(conversation_index).as_str(),
            fixed_time().to_rfc3339(),
        ],
    )
    .expect("seed managed_team_sessions row");
}

/// Seeds a minimal `managed_team_members` row for FK-checked reservations/bindings.
pub fn seed_team_member_row(conn: &rusqlite::Connection, member_id: &str, team_id: &str) {
    conn.execute(
        "INSERT INTO managed_team_members (
            id, team_id, normalized_name, name, canonical_agent_name,
            role_summary, status, created_at, updated_at
        ) VALUES (?1, ?2, ?1, ?1, 'ralphx-general-worker', 'test member', 'idle', ?3, ?3)",
        rusqlite::params![member_id, team_id, fixed_time().to_rfc3339()],
    )
    .expect("seed managed_team_members row");
}

pub fn team_session(id: &str, conversation_id: &ChatConversationId) -> TeamSession {
    TeamSession {
        id: TeamSessionId::from_string(id),
        project_id: ProjectId::from_string("project-1".to_string()),
        coordinator_conversation_id: *conversation_id,
        status: TeamSessionStatus::Active,
        strategy: None,
        configured_concurrency: 2,
        effective_concurrency: 2,
        automatic_wake_limit: 5,
        budget_policy: None,
        pending_coordination_mode: None,
        pending_exit_action: None,
        version: 0,
        last_error: None,
        created_at: fixed_time(),
        updated_at: fixed_time(),
        closed_at: None,
    }
}

pub fn team_member(id: &str, team_id: &str, name: &str) -> TeamMember {
    TeamMember {
        id: TeamMemberId::from_string(id),
        team_id: TeamSessionId::from_string(team_id),
        normalized_name: name.to_lowercase(),
        name: name.to_string(),
        canonical_agent_name: "ralphx-general-worker".to_string(),
        role_summary: "test member".to_string(),
        harness: None,
        logical_model: None,
        logical_effort: None,
        delegated_session_id: None,
        generation: 0,
        current_run_id: None,
        current_assignment_id: None,
        status: TeamMemberStatus::Provisioning,
        last_activity_at: None,
        last_error: None,
        created_at: fixed_time(),
        updated_at: fixed_time(),
        stopped_at: None,
    }
}

pub fn team_message(id: &str, team_id: &str, sequence: i64) -> TeamMessage {
    TeamMessage {
        id: TeamMessageId::from_string(id),
        team_id: TeamSessionId::from_string(team_id),
        sequence,
        sender_kind: TeamMessageActorKind::Coordinator,
        sender_member_id: None,
        target_kind: TeamMessageEnvelopeTarget::Broadcast,
        target_member_id: None,
        kind: TeamMessageKind::Instruction,
        content: format!("message {sequence}"),
        source_run_id: None,
        assignment_id: None,
        idempotency_key: format!("idem-{id}"),
        created_at: fixed_time(),
    }
}

pub fn team_delivery(id: &str, message_id: &str, member_id: Option<&str>) -> TeamMessageDelivery {
    TeamMessageDelivery {
        id: TeamMessageDeliveryId::from_string(id),
        message_id: TeamMessageId::from_string(message_id),
        recipient_kind: if member_id.is_some() {
            TeamMessageActorKind::Member
        } else {
            TeamMessageActorKind::Coordinator
        },
        recipient_member_id: member_id.map(TeamMemberId::from_string),
        recipient_generation: member_id.map(|_| 0),
        status: TeamMessageDeliveryStatus::Pending,
        queued_message_id: None,
        attempt_count: 0,
        next_retry_at: None,
        last_error: None,
        created_at: fixed_time(),
        updated_at: fixed_time(),
    }
}

pub fn team_run_binding(id: &str, team_id: &str, run_index: u64) -> TeamRunBinding {
    TeamRunBinding {
        id: TeamRunBindingId::from_string(id),
        team_id: TeamSessionId::from_string(team_id),
        team_member_id: None,
        team_member_generation: None,
        agent_run_id: team_agent_run_id(run_index),
        conversation_id: team_conversation_id(0),
        delegated_session_id: None,
        trigger_kind: TeamRunTriggerKind::UserCoordinatorTurn,
        work_classification: TeamWorkClassification::CoordinationOnly,
        assignment_id: None,
        first_message_sequence: None,
        last_message_sequence: None,
        status: TeamRunBindingStatus::Planned,
        version: 0,
        last_error: None,
        created_at: fixed_time(),
        launched_at: None,
        terminal_at: None,
    }
}

pub fn member_run_binding(
    id: &str,
    team_id: &str,
    run_index: u64,
    member_id: &str,
    generation: i64,
) -> TeamRunBinding {
    let mut binding = team_run_binding(id, team_id, run_index);
    binding.team_member_id = Some(TeamMemberId::from_string(member_id));
    binding.team_member_generation = Some(generation);
    binding.work_classification = TeamWorkClassification::Write;
    binding.trigger_kind = TeamRunTriggerKind::Assignment;
    binding
}

pub fn team_wake_batch(id: &str, team_id: &str, member_id: &str) -> TeamWakeBatch {
    TeamWakeBatch {
        id: TeamWakeBatchId::from_string(id),
        team_id: TeamSessionId::from_string(team_id),
        recipient_kind: TeamWakeRecipientKind::Member,
        recipient_member_id: Some(TeamMemberId::from_string(member_id)),
        recipient_generation: Some(0),
        first_message_sequence: 1,
        last_message_sequence: 1,
        delivery_ids: vec![TeamMessageDeliveryId::from_string("delivery-1")],
        status: TeamWakeBatchStatus::Queued,
        planned_agent_run_id: None,
        bound_agent_run_id: None,
        attempt_count: 0,
        budget_count: 0,
        lease_token: None,
        version: 0,
        last_error: None,
        created_at: fixed_time(),
        updated_at: fixed_time(),
        settled_at: None,
    }
}

pub fn team_reservation(id: &str, team_id: &str, member_id: &str) -> TeamWorkspaceReservation {
    TeamWorkspaceReservation {
        id: TeamWorkspaceReservationId::from_string(id),
        team_id: TeamSessionId::from_string(team_id),
        team_member_id: TeamMemberId::from_string(member_id),
        assignment_id: None,
        team_member_generation: 0,
        writable_paths: vec!["src/module_a".to_string()],
        generated_outputs: Vec::new(),
        resource_locks: Vec::new(),
        work_classification: TeamWorkClassification::Write,
        attempt_number: 1,
        acquired_at: fixed_time(),
        released_at: None,
    }
}
