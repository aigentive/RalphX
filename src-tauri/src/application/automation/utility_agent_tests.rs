use std::sync::Arc;
use std::time::Duration;

use chrono::Utc;

use super::utility_agent::{invoke_automation_utility_agent, AutomationUtilityModelPolicy};
use crate::application::AppState;
use crate::domain::entities::{
    Automation, AutomationId, AutomationPlanApprovalMode, AutomationPrMergeMode, AutomationStatus,
    Project, ProjectId,
};
use crate::infrastructure::agents::claude::agent_names;
use crate::infrastructure::{MockAgenticClient, MockCallType};

fn automation(project_id: ProjectId) -> Automation {
    let now = Utc::now();
    Automation {
        id: AutomationId::from_string("automation-1"),
        project_id,
        name: "Automation 1".to_string(),
        status: AutomationStatus::Active,
        paused_reason_code: None,
        paused_reason_detail: None,
        goal_prompt: "Ship the automation helper.".to_string(),
        setup_conversation_id: None,
        provider_harness: "claude".to_string(),
        model_id: "sonnet".to_string(),
        logical_effort: Some("medium".to_string()),
        run_mode: "edit".to_string(),
        base_ref_kind: "project_default".to_string(),
        base_ref: "main".to_string(),
        base_display_name: Some("main".to_string()),
        base_source_pull_request_json: None,
        goal_items_json: None,
        chain_mode: "merged_base".to_string(),
        completion_signal: "pr_merged".to_string(),
        plan_approval_mode: AutomationPlanApprovalMode::Manual,
        pr_merge_mode: AutomationPrMergeMode::Manual,
        plan_deep_verification: false,
        max_runs: 25,
        max_consecutive_failures: 3,
        first_run_prompt: Some("Implement the first slice.".to_string()),
        setup_analysis_summary: None,
        spec_artifact_id: None,
        authoring_state_json: None,
        created_at: now,
        updated_at: now,
    }
}

#[tokio::test]
async fn utility_agent_invokes_mock_runtime_from_project_checkout() {
    let temp = tempfile::tempdir().expect("temp checkout");
    let mut project = Project::new(
        "Automation Project".to_string(),
        temp.path().to_string_lossy().into_owned(),
    );
    project.id = ProjectId::from_string("project-1".to_string());
    let client = Arc::new(MockAgenticClient::new());
    let state = AppState::new_test().with_agent_client(client.clone());
    state.project_repo.create(project.clone()).await.unwrap();

    let output = invoke_automation_utility_agent(
        &state,
        &automation(project.id),
        agent_names::AGENT_AUTOMATION_JUDGE,
        "automation utility test",
        "Judge the latest automation run.".to_string(),
        Duration::from_millis(1),
        AutomationUtilityModelPolicy::LockedDefault,
    )
    .await
    .unwrap();

    assert_eq!(output.raw_output, "MOCK_COMPLETION");
    assert!(output.model_id.is_some());
    let calls = client.get_calls().await;
    assert!(calls.iter().any(|call| {
        matches!(
            &call.call_type,
            MockCallType::Spawn { prompt, .. }
                if prompt.contains("Judge the latest automation run.")
        )
    }));
    assert!(calls
        .iter()
        .any(|call| { matches!(&call.call_type, MockCallType::WaitForCompletion { .. }) }));
}

#[tokio::test]
async fn utility_agent_fails_closed_for_invalid_harness_before_spawn() {
    let client = Arc::new(MockAgenticClient::new());
    let state = AppState::new_test().with_agent_client(client.clone());
    let mut automation = automation(ProjectId::from_string("project-1".to_string()));
    automation.provider_harness = "unknown".to_string();

    let error = invoke_automation_utility_agent(
        &state,
        &automation,
        agent_names::AGENT_AUTOMATION_JUDGE,
        "automation utility test",
        "prompt".to_string(),
        Duration::from_secs(1),
        AutomationUtilityModelPolicy::Override(None),
    )
    .await
    .unwrap_err();

    assert!(error.to_string().contains("unknown"));
    assert!(client.get_calls().await.is_empty());
}

#[tokio::test]
async fn utility_agent_requires_existing_project_checkout_before_spawn() {
    let client = Arc::new(MockAgenticClient::new());
    let state = AppState::new_test().with_agent_client(client.clone());
    let missing_project = ProjectId::from_string("missing-project".to_string());

    let error = invoke_automation_utility_agent(
        &state,
        &automation(missing_project),
        agent_names::AGENT_AUTOMATION_JUDGE,
        "automation utility test",
        "prompt".to_string(),
        Duration::from_secs(1),
        AutomationUtilityModelPolicy::LockedDefault,
    )
    .await
    .unwrap_err();

    assert!(error
        .to_string()
        .contains("project missing-project not found"));
    assert!(client.get_calls().await.is_empty());
}
