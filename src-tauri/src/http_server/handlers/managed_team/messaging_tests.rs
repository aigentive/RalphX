//! Idempotency-key derivation tests for the managed-Team message handler.

use super::messaging::team_tool_idempotency_key;
use crate::application::managed_team::ManagedTeamMessageTarget;
use crate::domain::entities::{AgentRunId, TeamMessageKind};

// AgentRunId::from_string collapses non-UUID input to Uuid::nil(), so tests
// must use real UUID literals to represent distinct runs.
const RUN_A: &str = "11111111-1111-4111-8111-111111111111";
const RUN_B: &str = "22222222-2222-4222-8222-222222222222";

fn run_id() -> AgentRunId {
    AgentRunId::from_string(RUN_A)
}

#[test]
fn distinct_content_from_same_run_produces_distinct_keys() {
    let first = team_tool_idempotency_key(
        &run_id(),
        &ManagedTeamMessageTarget::Coordinator,
        TeamMessageKind::Status,
        "first update",
    );
    let second = team_tool_idempotency_key(
        &run_id(),
        &ManagedTeamMessageTarget::Coordinator,
        TeamMessageKind::Status,
        "second update",
    );
    assert_ne!(first, second);
}

#[test]
fn identical_logical_message_repeats_the_same_key() {
    let build = || {
        team_tool_idempotency_key(
            &run_id(),
            &ManagedTeamMessageTarget::MemberName("scout".to_string()),
            TeamMessageKind::Status,
            "same payload",
        )
    };
    assert_eq!(build(), build());
}

#[test]
fn distinct_targets_and_kinds_produce_distinct_keys() {
    let coordinator = team_tool_idempotency_key(
        &run_id(),
        &ManagedTeamMessageTarget::Coordinator,
        TeamMessageKind::Status,
        "payload",
    );
    let broadcast = team_tool_idempotency_key(
        &run_id(),
        &ManagedTeamMessageTarget::Broadcast,
        TeamMessageKind::Status,
        "payload",
    );
    let member = team_tool_idempotency_key(
        &run_id(),
        &ManagedTeamMessageTarget::MemberName("scout".to_string()),
        TeamMessageKind::Status,
        "payload",
    );
    let question = team_tool_idempotency_key(
        &run_id(),
        &ManagedTeamMessageTarget::Coordinator,
        TeamMessageKind::Question,
        "payload",
    );
    assert_ne!(coordinator, broadcast);
    assert_ne!(coordinator, member);
    assert_ne!(broadcast, member);
    assert_ne!(coordinator, question);
}

#[test]
fn key_is_scoped_to_the_source_run() {
    let from_first_run = team_tool_idempotency_key(
        &AgentRunId::from_string(RUN_A),
        &ManagedTeamMessageTarget::Coordinator,
        TeamMessageKind::Status,
        "payload",
    );
    let from_second_run = team_tool_idempotency_key(
        &AgentRunId::from_string(RUN_B),
        &ManagedTeamMessageTarget::Coordinator,
        TeamMessageKind::Status,
        "payload",
    );
    assert_ne!(from_first_run, from_second_run);
    assert!(from_first_run.starts_with(&format!("team-tool:{RUN_A}:")));
}
