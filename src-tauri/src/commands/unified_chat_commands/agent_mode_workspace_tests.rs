use super::*;

#[test]
fn only_write_capable_agent_conversation_modes_require_workspace() {
    assert!(!agent_mode_requires_workspace(
        AgentConversationWorkspaceMode::Chat
    ));
    assert!(agent_mode_requires_workspace(
        AgentConversationWorkspaceMode::Edit
    ));
    assert!(agent_mode_requires_workspace(
        AgentConversationWorkspaceMode::Plan
    ));
    assert!(agent_mode_requires_workspace(
        AgentConversationWorkspaceMode::Ideation
    ));
}

#[test]
fn source_pr_backed_chat_mode_creates_workspace() {
    let source_pull_request = AgentWorkspaceSourcePullRequest {
        number: 123,
        url: None,
        title: None,
        head_ref_name: "feature/source-pr".to_string(),
        base_ref_name: Some("main".to_string()),
        head_ref_oid: None,
    };

    assert!(agent_mode_should_create_workspace(
        AgentConversationWorkspaceMode::Chat,
        Some(&source_pull_request),
    ));
    assert!(!agent_mode_should_create_workspace(
        AgentConversationWorkspaceMode::Chat,
        None,
    ));
    assert!(agent_mode_should_create_workspace(
        AgentConversationWorkspaceMode::Edit,
        None,
    ));
}

#[test]
fn plan_agent_conversation_mode_round_trips_through_api_string() {
    let mode = "plan"
        .parse::<AgentConversationWorkspaceMode>()
        .expect("plan mode should parse");

    assert_eq!(mode, AgentConversationWorkspaceMode::Plan);
    assert_eq!(mode.to_string(), "plan");
}

#[test]
fn review_pr_agent_conversation_mode_round_trips_through_api_string() {
    let mode = "review_pr"
        .parse::<AgentConversationWorkspaceMode>()
        .expect("review_pr mode should parse");

    assert_eq!(mode, AgentConversationWorkspaceMode::ReviewPr);
    assert_eq!(mode.to_string(), "review_pr");
}

#[test]
fn active_agent_conversations_support_expected_valid_mode_transition_matrix() {
    let modes = [
        AgentConversationWorkspaceMode::Chat,
        AgentConversationWorkspaceMode::Edit,
        AgentConversationWorkspaceMode::Plan,
        AgentConversationWorkspaceMode::Ideation,
        AgentConversationWorkspaceMode::ReviewPr,
    ];

    for current_mode in modes {
        for target_mode in modes {
            assert!(
                validate_agent_conversation_mode_transition(
                    current_mode,
                    target_mode,
                    &AgentConversationWorkspaceModeLock::unlocked()
                )
                .is_ok(),
                "{current_mode} -> {target_mode} should be allowed"
            )
        }
    }
}

#[test]
fn active_state_owned_conversations_cannot_leave_ideation_mode() {
    for target_mode in [
        AgentConversationWorkspaceMode::Chat,
        AgentConversationWorkspaceMode::Edit,
        AgentConversationWorkspaceMode::Plan,
        AgentConversationWorkspaceMode::ReviewPr,
    ] {
        let error = validate_agent_conversation_mode_transition(
            AgentConversationWorkspaceMode::Ideation,
            target_mode,
            &AgentConversationWorkspaceModeLock::locked("Plan execution is still active"),
        )
        .expect_err("state-owned conversations should not leave ideation mode");

        assert!(error.contains("Plan execution is still active"));
    }
}

#[test]
fn state_owned_workspaces_remain_in_their_current_mode() {
    let modes = [
        AgentConversationWorkspaceMode::Chat,
        AgentConversationWorkspaceMode::Edit,
        AgentConversationWorkspaceMode::Plan,
        AgentConversationWorkspaceMode::Tasks,
        AgentConversationWorkspaceMode::Autopilot,
        AgentConversationWorkspaceMode::Ideation,
        AgentConversationWorkspaceMode::ReviewPr,
    ];

    for current_mode in modes {
        assert!(validate_agent_conversation_mode_transition(
            current_mode,
            current_mode,
            &AgentConversationWorkspaceModeLock::locked("Workspace state is still active"),
        )
        .is_ok());

        for target_mode in modes {
            if target_mode == current_mode {
                continue;
            }

            let error = validate_agent_conversation_mode_transition(
                current_mode,
                target_mode,
                &AgentConversationWorkspaceModeLock::locked("Workspace state is still active"),
            )
            .expect_err("state-owned workspaces should remain in their current mode");

            assert!(error.contains("Workspace state is still active"));
        }
    }
}
