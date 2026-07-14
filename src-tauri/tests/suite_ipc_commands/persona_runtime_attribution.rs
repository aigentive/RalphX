use chrono::{TimeDelta, Utc};
use ralphx_lib::application::AppState;
use ralphx_lib::commands::unified_chat_commands::get_agent_conversation_summary_for_app_state;
use ralphx_lib::domain::entities::{
    AgentRun, AgentRunStatus, ChatConversation, ChatConversationId, ProjectId,
};

fn persona_run(
    conversation_id: ChatConversationId,
    started_offset_seconds: i64,
    status: AgentRunStatus,
    injected: Option<bool>,
    skipped_reason: Option<&str>,
) -> AgentRun {
    let mut run = AgentRun::new(conversation_id);
    run.started_at = Utc::now() - TimeDelta::seconds(100 - started_offset_seconds);
    match status {
        AgentRunStatus::Running => {}
        AgentRunStatus::Completed => run.complete(),
        AgentRunStatus::Failed => run.fail("persona fixture failure"),
        AgentRunStatus::Cancelled => run.cancel(),
    }
    run.persona_id = Some("persona-design-voice".to_string());
    run.persona_slug = Some("design-voice".to_string());
    run.persona_version = Some(3);
    run.persona_content_hash = Some("design-voice-hash".to_string());
    run.persona_injected = injected;
    run.persona_skipped_reason = skipped_reason.map(str::to_string);
    run
}

async fn persona_summary_fixture() -> (AppState, ChatConversation) {
    let state = AppState::new_test();
    let conversation = state
        .chat_conversation_repo
        .create(ChatConversation::new_project(ProjectId::from_string(
            "project-persona-runtime-attribution".to_string(),
        )))
        .await
        .expect("persona conversation should persist");
    (state, conversation)
}

#[tokio::test]
async fn persona_completed_applied_run_wins_over_newer_cancelled_run_and_history_is_preserved() {
    let (state, conversation) = persona_summary_fixture().await;
    let completed = persona_run(
        conversation.id,
        0,
        AgentRunStatus::Completed,
        Some(true),
        None,
    );
    let completed_id = completed.id.as_str();
    state
        .agent_run_repo
        .create(completed)
        .await
        .expect("completed persona run should persist");
    let cancelled = persona_run(
        conversation.id,
        10,
        AgentRunStatus::Cancelled,
        Some(false),
        None,
    );
    let cancelled_id = cancelled.id.as_str();
    state
        .agent_run_repo
        .create(cancelled)
        .await
        .expect("cancelled persona run should persist");

    let response = get_agent_conversation_summary_for_app_state(&state, conversation.id.as_str())
        .await
        .expect("persona summary should load")
        .expect("persona conversation should exist");

    assert_eq!(
        response.last_run_persona_run_id.as_deref(),
        Some(completed_id.as_str())
    );
    assert_eq!(response.last_run_persona_injected, Some(true));
    assert_eq!(response.persona_runs.len(), 2);
    let cancelled_history = response
        .persona_runs
        .iter()
        .find(|run| run.run_id == cancelled_id)
        .expect("cancelled run should remain in transcript attribution history");
    assert!(!cancelled_history.injected);
    assert_eq!(cancelled_history.skipped_reason, None);
}

#[tokio::test]
async fn persona_newer_completed_reasoned_negative_wins_over_older_applied_run() {
    let (state, conversation) = persona_summary_fixture().await;
    state
        .agent_run_repo
        .create(persona_run(
            conversation.id,
            0,
            AgentRunStatus::Completed,
            Some(true),
            None,
        ))
        .await
        .expect("older applied persona run should persist");
    let skipped = persona_run(
        conversation.id,
        10,
        AgentRunStatus::Completed,
        Some(false),
        Some("native_agent_flag"),
    );
    let skipped_id = skipped.id.as_str();
    state
        .agent_run_repo
        .create(skipped)
        .await
        .expect("newer skipped persona run should persist");

    let response = get_agent_conversation_summary_for_app_state(&state, conversation.id.as_str())
        .await
        .expect("persona summary should load")
        .expect("persona conversation should exist");

    assert_eq!(
        response.last_run_persona_run_id.as_deref(),
        Some(skipped_id.as_str())
    );
    assert_eq!(response.last_run_persona_injected, Some(false));
    assert_eq!(
        response.last_run_persona_skipped_reason.as_deref(),
        Some("native_agent_flag")
    );
}

#[tokio::test]
async fn persona_reasonless_false_is_unknown_in_conversation_dto() {
    let (state, conversation) = persona_summary_fixture().await;
    state
        .agent_run_repo
        .create(persona_run(
            conversation.id,
            0,
            AgentRunStatus::Completed,
            Some(false),
            None,
        ))
        .await
        .expect("reason-less persona run should persist");

    let response = get_agent_conversation_summary_for_app_state(&state, conversation.id.as_str())
        .await
        .expect("persona summary should load")
        .expect("persona conversation should exist");

    assert_eq!(
        response.last_run_persona_slug.as_deref(),
        Some("design-voice")
    );
    assert_eq!(response.last_run_persona_injected, None);
    assert_eq!(response.last_run_persona_skipped_reason, None);
    assert_eq!(response.persona_runs.len(), 1);
    assert!(!response.persona_runs[0].injected);
}

#[tokio::test]
async fn persona_attribution_falls_back_when_newest_executed_run_has_none() {
    let (state, conversation) = persona_summary_fixture().await;
    let attributed = persona_run(
        conversation.id,
        0,
        AgentRunStatus::Completed,
        Some(true),
        None,
    );
    let attributed_id = attributed.id.as_str();
    state
        .agent_run_repo
        .create(attributed)
        .await
        .expect("attributed persona run should persist");
    let mut unattributed = AgentRun::new(conversation.id);
    unattributed.started_at = Utc::now() - TimeDelta::seconds(90);
    unattributed.complete();
    unattributed.logical_model = Some("model-from-newest-run".to_string());
    state
        .agent_run_repo
        .create(unattributed)
        .await
        .expect("unattributed run should persist");

    let response = get_agent_conversation_summary_for_app_state(&state, conversation.id.as_str())
        .await
        .expect("persona summary should load")
        .expect("persona conversation should exist");

    assert_eq!(
        response.logical_model.as_deref(),
        Some("model-from-newest-run")
    );
    assert_eq!(
        response.last_run_persona_run_id.as_deref(),
        Some(attributed_id.as_str())
    );
    assert_eq!(response.last_run_persona_injected, Some(true));
}
