use std::collections::{HashMap, HashSet};
use std::str::FromStr;

use crate::agents::{
    AgentHarnessKind, AgentLane, LogicalEffort, ManualRoleDefault, ManualServiceTier, RoutingRole,
    RoutingRoleFamily, ROUTING_ROLES, ROUTING_ROLE_COUNT, ROUTING_ROLE_FAMILIES,
};
use crate::entities::{CoordinationMode, PersonaId};

#[test]
fn catalog_contains_all_documented_roles_once_in_seven_families() {
    assert_eq!(ROUTING_ROLE_COUNT, 29);
    assert_eq!(ROUTING_ROLES.len(), ROUTING_ROLE_COUNT);
    assert_eq!(ROUTING_ROLE_FAMILIES.len(), 7);

    let unique_roles = ROUTING_ROLES.into_iter().collect::<HashSet<_>>();
    assert_eq!(unique_roles.len(), ROUTING_ROLE_COUNT);

    let family_counts = ROUTING_ROLES
        .into_iter()
        .fold(HashMap::new(), |mut counts, role| {
            *counts.entry(role.metadata().family).or_insert(0usize) += 1;
            counts
        });
    assert_eq!(family_counts.get(&RoutingRoleFamily::Workspace), Some(&6));
    assert_eq!(family_counts.get(&RoutingRoleFamily::Automation), Some(&2));
    assert_eq!(
        family_counts.get(&RoutingRoleFamily::FeedbackLoops),
        Some(&4)
    );
    assert_eq!(family_counts.get(&RoutingRoleFamily::Ideation), Some(&4));
    assert_eq!(family_counts.get(&RoutingRoleFamily::Delegation), Some(&1));
    assert_eq!(family_counts.get(&RoutingRoleFamily::Execution), Some(&7));
    assert_eq!(family_counts.get(&RoutingRoleFamily::Utility), Some(&5));
}

#[test]
fn routing_role_family_keys_and_labels_are_stable() {
    let expected = [
        (RoutingRoleFamily::Workspace, "workspace", "Workspace"),
        (RoutingRoleFamily::Automation, "automation", "Automation"),
        (
            RoutingRoleFamily::FeedbackLoops,
            "feedback_loops",
            "Feedback Loops",
        ),
        (RoutingRoleFamily::Ideation, "ideation", "Ideation"),
        (RoutingRoleFamily::Delegation, "delegation", "Delegation"),
        (RoutingRoleFamily::Execution, "execution", "Execution"),
        (RoutingRoleFamily::Utility, "utility", "Utility"),
    ];

    for (family, key, display_name) in expected {
        assert_eq!(family.key(), key);
        assert_eq!(family.display_name(), display_name);
        assert_eq!(
            serde_json::from_str::<RoutingRoleFamily>(&format!("\"{key}\"")).unwrap(),
            family
        );
    }
}

#[test]
fn only_execution_roles_require_tasks() {
    for role in ROUTING_ROLES {
        assert_eq!(
            role.metadata().requires_tasks,
            role.metadata().family == RoutingRoleFamily::Execution,
            "{} has incorrect Tasks applicability",
            role.metadata().key,
        );
    }
}

#[test]
fn every_role_round_trips_through_display_parse_and_serde() {
    for role in ROUTING_ROLES {
        let key = role.to_string();
        assert_eq!(RoutingRole::from_str(&key).unwrap(), role);

        let json = serde_json::to_string(&role).unwrap();
        assert_eq!(serde_json::from_str::<RoutingRole>(&json).unwrap(), role);
        assert_eq!(role.metadata().key, key);
        assert!(!role.metadata().display_name.is_empty());
        assert!(
            !role.metadata().description.trim().is_empty(),
            "{} must expose a task-oriented description",
            role.metadata().key
        );
    }
    assert!(RoutingRole::from_str("execution_branch_updater").is_err());
}

#[test]
fn legacy_lane_mapping_is_explicit_and_branch_updater_maps_to_workspace_repair() {
    let expected = [
        (AgentLane::IdeationPrimary, RoutingRole::IdeationPrimary),
        (AgentLane::IdeationVerifier, RoutingRole::IdeationVerifier),
        (AgentLane::IdeationSubagent, RoutingRole::IdeationSubagent),
        (
            AgentLane::IdeationVerifierSubagent,
            RoutingRole::IdeationVerifierSubagent,
        ),
        (AgentLane::ExecutionWorker, RoutingRole::ExecutionWorker),
        (AgentLane::ExecutionReviewer, RoutingRole::ExecutionReviewer),
        (
            AgentLane::ExecutionReexecutor,
            RoutingRole::ExecutionReexecutor,
        ),
        (AgentLane::ExecutionMerger, RoutingRole::ExecutionMerger),
        (
            AgentLane::ExecutionBranchUpdater,
            RoutingRole::WorkspaceRepair,
        ),
    ];

    for (lane, role) in expected {
        assert_eq!(RoutingRole::from_legacy_lane(lane), role);
        assert_eq!(role.legacy_lane(), Some(lane));
    }
    assert_eq!(RoutingRole::WorkspaceChat.legacy_lane(), None);
}

#[test]
fn manual_default_preserves_exact_optional_controls() {
    let value = ManualRoleDefault {
        harness: AgentHarnessKind::Codex,
        model: Some("gpt-5.6".to_string()),
        effort: Some(LogicalEffort::XHigh),
        service_tier: ManualServiceTier::Standard,
        coordination_mode: Some(CoordinationMode::Solo),
        persona_id: Some(PersonaId::from("persona-1")),
        approval_policy: Some("never".to_string()),
        sandbox_mode: Some("danger-full-access".to_string()),
    };

    let json = serde_json::to_value(&value).unwrap();
    assert_eq!(json["serviceTier"], "standard");
    assert_eq!(json["coordinationMode"], "solo");
    assert_eq!(json["personaId"], "persona-1");
    assert_eq!(
        serde_json::from_value::<ManualRoleDefault>(json).unwrap(),
        value
    );
}

#[test]
fn manual_service_tier_distinguishes_provider_default_standard_and_fast() {
    assert_ne!(
        ManualServiceTier::ProviderDefault,
        ManualServiceTier::Standard
    );
    assert_ne!(ManualServiceTier::Standard, ManualServiceTier::Fast);
    assert_eq!(
        ManualServiceTier::default(),
        ManualServiceTier::ProviderDefault
    );
}

#[test]
fn manual_service_tier_rejects_unknown_strings() {
    assert_eq!(
        ManualServiceTier::from_str("provider_default")
            .unwrap()
            .to_string(),
        "provider_default"
    );
    assert!(ManualServiceTier::from_str("turbo").is_err());
}
