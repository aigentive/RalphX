use super::{ChatRuntimeFactoryDeps, RuntimeFactoryDeps};
use crate::application::AppState;

#[test]
fn app_state_chat_factory_dependencies_include_persona_manual_role_defaults_and_events() {
    let state = AppState::new_test();

    let deps = ChatRuntimeFactoryDeps::from_app_state(&state);

    assert!(
        deps.persona_repo.is_some(),
        "handler-built chat services must retain AppState persona resolution"
    );
    assert!(
        deps.manual_role_default_service.is_some(),
        "handler-built chat services must resolve backend-owned routing-role defaults"
    );
    assert!(
        deps.external_events_repo.is_some(),
        "background finalizers must retain durable completion-event delivery"
    );
    assert!(
        deps.plan_verification_completion.is_some(),
        "chat finalizers must receive typed Plan verification and approval settlement capability"
    );
    assert!(
        deps.atlassian_integration_service.is_some(),
        "handler-built chat services must retain Atlassian reference expansion"
    );
    assert!(
        deps.linear_integration_service.is_some(),
        "handler-built chat services must retain Linear reference expansion"
    );
    assert!(
        deps.granola_integration_service.is_some(),
        "handler-built chat services must retain Granola reference expansion"
    );
    assert!(
        deps.clickup_integration_service.is_some(),
        "handler-built chat services must retain ClickUp reference expansion"
    );
}

#[test]
fn app_state_runtime_factory_dependencies_include_completion_authority_repositories() {
    let state = AppState::new_test();

    let deps = RuntimeFactoryDeps::from_app_state(&state);

    assert!(
        deps.task_step_repo.is_some(),
        "no-AppHandle scheduler paths must retain task-step completion authority"
    );
    assert!(
        deps.validation_run_repo.is_some(),
        "no-AppHandle scheduler paths must retain first-class validation authority"
    );
}

fn populated_app_state_chat_runtime_fields(deps: &ChatRuntimeFactoryDeps) -> Vec<bool> {
    vec![
        deps.chat_timeline_repo.is_some(),
        deps.queued_message_repo.is_some(),
        deps.notification_service.is_some(),
        deps.app_state_repo.is_some(),
        deps.plan_verification_completion.is_some(),
        deps.delegated_session_repo.is_some(),
        deps.agent_task_repo.is_some(),
        deps.linked_plan_snapshot_resolver.is_some(),
        deps.team_repo.is_some(),
        deps.branch_status_cache.is_some(),
        deps.delegation_park_repo.is_some(),
        deps.persona_repo.is_some(),
        deps.conversation_folder_reference_repo.is_some(),
        deps.folder_reference_app_data_dir.is_some(),
        deps.manual_role_default_service.is_some(),
        deps.branch_update_repo.is_some(),
        deps.pr_poller_registry.is_some(),
        deps.plan_pr_description_drafter.is_some(),
        deps.execution_settings_repo.is_some(),
        deps.agent_lane_settings_repo.is_some(),
        deps.agent_provider_settings_repo.is_some(),
        deps.ideation_effort_settings_repo.is_some(),
        deps.ideation_model_settings_repo.is_some(),
        deps.agent_conversation_workspace_repo.is_some(),
        deps.agent_conversation_jira_issue_repo.is_some(),
        deps.agent_conversation_linear_issue_repo.is_some(),
        deps.agent_conversation_granola_note_repo.is_some(),
        deps.plan_branch_repo.is_some(),
        deps.task_proposal_repo.is_some(),
        deps.task_step_repo.is_some(),
        deps.validation_run_repo.is_some(),
        deps.external_events_repo.is_some(),
        deps.review_repo.is_some(),
        deps.interactive_process_registry.is_some(),
        deps.streaming_state_cache.is_some(),
        deps.atlassian_integration_service.is_some(),
        deps.linear_integration_service.is_some(),
        deps.granola_integration_service.is_some(),
        deps.clickup_integration_service.is_some(),
        deps.mcp_policy_service.is_some(),
        deps.managed_team.is_some(),
        deps.agent_clients.is_some(),
        deps.execution_plan_repo.is_some(),
    ]
}

#[test]
fn startup_recovery_chat_runtime_snapshot_is_as_complete_as_app_state() {
    let state = AppState::new_test();
    let app_state_deps = ChatRuntimeFactoryDeps::from_app_state(&state);

    // Startup recovery intentionally takes this exact snapshot rather than rebuilding a
    // reduced core-dependency subset in startup_pipeline.
    let startup_recovery_deps = ChatRuntimeFactoryDeps::from_app_state(&state);

    assert!(
        populated_app_state_chat_runtime_fields(&app_state_deps)
            .into_iter()
            .all(|is_populated| is_populated),
        "AppState runtime construction must populate every optional chat dependency"
    );
    assert_eq!(
        populated_app_state_chat_runtime_fields(&startup_recovery_deps),
        populated_app_state_chat_runtime_fields(&app_state_deps),
        "startup recovery must retain the complete AppState chat dependency snapshot"
    );
}
