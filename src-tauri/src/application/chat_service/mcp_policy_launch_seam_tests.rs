use super::resolve_mcp_launch_policy_with_service;
use crate::domain::agents::AgentHarnessKind;

#[tokio::test]
async fn missing_launch_policy_service_blocks_spawn_resolution() {
    let error = resolve_mcp_launch_policy_with_service(
        None,
        AgentHarnessKind::Codex,
        Some("project-1"),
        std::path::Path::new("/tmp/project"),
    )
    .await
    .expect_err("launch policy service is required");

    assert!(error.to_string().contains("policy service is unavailable"));
}

#[test]
fn queue_recovery_and_retry_have_no_conditional_app_state_policy_bypass() {
    for source in [
        include_str!("chat_service_queue.rs"),
        include_str!("chat_service_recovery.rs"),
        include_str!("chat_service_handlers.rs"),
    ] {
        for policy_call in source.match_indices(".mcp_policy_service()") {
            let start = policy_call.0.saturating_sub(240);
            assert!(
                !source[start..policy_call.0].contains("try_state::<AppState>()"),
                "MCP policy application must not be conditional on try_state"
            );
        }
    }
}
