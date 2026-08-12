use std::str::FromStr;

use crate::agents::{
    default_atlassian_access, AtlassianMcpAccess, RoutingRole, ATLASSIAN_READ_TOOLS,
    ATLASSIAN_WRITE_TOOLS, ROUTING_ROLES,
};

fn is_bare_snake_case(name: &str) -> bool {
    let mut chars = name.chars();
    match chars.next() {
        Some(first) if first.is_ascii_lowercase() => {}
        _ => return false,
    }
    chars.all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit() || ch == '_')
}

#[test]
fn every_routing_role_maps_to_a_tier() {
    for role in ROUTING_ROLES {
        let access = default_atlassian_access(role);
        assert_ne!(
            access,
            AtlassianMcpAccess::None,
            "role {role} must have an explicit built-in tier"
        );
    }
}

#[test]
fn write_tier_defaults_match_the_approved_role_set() {
    let write_roles: Vec<RoutingRole> = ROUTING_ROLES
        .into_iter()
        .filter(|role| default_atlassian_access(*role) == AtlassianMcpAccess::ReadWrite)
        .collect();

    assert_eq!(
        write_roles,
        vec![
            RoutingRole::WorkspaceEdit,
            RoutingRole::WorkspacePrFixer,
            RoutingRole::ExecutionWorker,
            RoutingRole::ExecutionReexecutor,
        ]
    );
}

#[test]
fn all_other_roles_default_to_read() {
    for role in ROUTING_ROLES {
        if matches!(
            role,
            RoutingRole::WorkspaceEdit
                | RoutingRole::WorkspacePrFixer
                | RoutingRole::ExecutionWorker
                | RoutingRole::ExecutionReexecutor
        ) {
            continue;
        }
        assert_eq!(
            default_atlassian_access(role),
            AtlassianMcpAccess::Read,
            "role {role} should default to read"
        );
    }
}

#[test]
fn tool_names_are_bare_snake_case() {
    for name in ATLASSIAN_READ_TOOLS.iter().chain(ATLASSIAN_WRITE_TOOLS) {
        assert!(
            is_bare_snake_case(name),
            "tool name '{name}' must match ^[a-z][a-z0-9_]*$"
        );
    }
}

#[test]
fn read_and_write_tool_lists_are_disjoint_and_unique() {
    let mut all: Vec<&str> = ATLASSIAN_READ_TOOLS
        .iter()
        .chain(ATLASSIAN_WRITE_TOOLS)
        .copied()
        .collect();
    let total = all.len();
    all.sort_unstable();
    all.dedup();
    assert_eq!(all.len(), total, "atlassian tool names must be unique");
}

#[test]
fn granted_tools_follow_the_tier() {
    assert!(AtlassianMcpAccess::None.granted_tools().is_empty());
    assert_eq!(
        AtlassianMcpAccess::Read.granted_tools(),
        ATLASSIAN_READ_TOOLS.to_vec()
    );

    let read_write = AtlassianMcpAccess::ReadWrite.granted_tools();
    assert_eq!(
        read_write.len(),
        ATLASSIAN_READ_TOOLS.len() + ATLASSIAN_WRITE_TOOLS.len()
    );
    for name in ATLASSIAN_WRITE_TOOLS {
        assert!(read_write.contains(name), "{name} missing from write tier");
    }
}

#[test]
fn tier_predicates_gate_read_and_write() {
    assert!(!AtlassianMcpAccess::None.allows_read());
    assert!(!AtlassianMcpAccess::None.allows_write());
    assert!(AtlassianMcpAccess::Read.allows_read());
    assert!(!AtlassianMcpAccess::Read.allows_write());
    assert!(AtlassianMcpAccess::ReadWrite.allows_read());
    assert!(AtlassianMcpAccess::ReadWrite.allows_write());
}

#[test]
fn tier_serializes_as_snake_case() {
    assert_eq!(
        serde_json::to_string(&AtlassianMcpAccess::ReadWrite).unwrap(),
        "\"read_write\""
    );
    assert_eq!(
        serde_json::from_str::<AtlassianMcpAccess>("\"read\"").unwrap(),
        AtlassianMcpAccess::Read
    );
    assert_eq!(
        AtlassianMcpAccess::from_str("read_write").unwrap(),
        AtlassianMcpAccess::ReadWrite
    );
    assert!(AtlassianMcpAccess::from_str("write").is_err());
}
