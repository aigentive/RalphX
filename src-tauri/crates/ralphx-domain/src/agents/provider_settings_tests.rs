use super::*;

#[test]
fn disabled_codex_defaults_are_mcp_ready_but_not_enabled() {
    let settings = AgentProviderSettings::disabled_defaults(AgentHarnessKind::Codex);

    assert_eq!(settings.provider, AgentHarnessKind::Codex);
    assert!(!settings.enabled);
    assert!(!settings.is_default);
    assert_eq!(settings.model.as_deref(), Some("gpt-5.5"));
    assert_eq!(settings.effort, Some(LogicalEffort::XHigh));
    assert_eq!(settings.approval_policy.as_deref(), Some("never"));
    assert_eq!(settings.sandbox_mode.as_deref(), Some("danger-full-access"));
}

#[test]
fn disabled_claude_defaults_are_most_permissive_but_not_enabled() {
    let settings = AgentProviderSettings::disabled_defaults(AgentHarnessKind::Claude);

    assert_eq!(settings.provider, AgentHarnessKind::Claude);
    assert!(!settings.enabled);
    assert!(!settings.is_default);
    assert_eq!(settings.model.as_deref(), Some("sonnet"));
    assert_eq!(settings.effort, Some(LogicalEffort::Medium));
    assert_eq!(
        settings.claude_permission_mode.as_deref(),
        Some("bypassPermissions")
    );
    assert!(settings.claude_dangerously_skip_permissions);
    assert!(!settings.claude_allow_dangerously_skip_permissions);
}
