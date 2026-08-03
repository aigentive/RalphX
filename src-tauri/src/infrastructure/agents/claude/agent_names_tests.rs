use super::*;

#[test]
fn launch_role_maps_feedback_loop_agents_for_both_name_forms() {
    assert_eq!(
        launch_role_for_agent_name(AGENT_WORKSPACE_REVIEWER),
        Some("workspace_reviewer")
    );
    assert_eq!(
        launch_role_for_agent_name(SHORT_WORKSPACE_REVIEWER),
        Some("workspace_reviewer")
    );
    assert_eq!(
        launch_role_for_agent_name(AGENT_WORKSPACE_REPAIR),
        Some("workspace_repair")
    );
    assert_eq!(
        launch_role_for_agent_name(SHORT_AGENT_WORKSPACE_REPAIR),
        Some("workspace_repair")
    );
    assert_eq!(
        launch_role_for_agent_name(AGENT_WORKSPACE_PR_FIXER),
        Some("pr_fixer")
    );
    assert_eq!(
        launch_role_for_agent_name(SHORT_AGENT_WORKSPACE_PR_FIXER),
        Some("pr_fixer")
    );
}

#[test]
fn launch_role_is_none_for_ordinary_conversation_agents() {
    assert_eq!(launch_role_for_agent_name(AGENT_GENERAL_WORKER), None);
    assert_eq!(launch_role_for_agent_name(SHORT_GENERAL_WORKER), None);
    assert_eq!(launch_role_for_agent_name(AGENT_CHAT_PROJECT), None);
    assert_eq!(launch_role_for_agent_name("unknown-agent"), None);
}
