use ralphx_lib::application::chat_service::record_persona_run_attribution;
use ralphx_lib::application::persona_prompt::ResolvedPersona;
use ralphx_lib::application::AppState;
use ralphx_lib::domain::agents::AgentHarnessKind;
use ralphx_lib::domain::entities::{AgentRun, ChatConversationId, PersonaId};

fn persona_attribution_fixture() -> ResolvedPersona {
    ResolvedPersona {
        id: PersonaId::from("persona-truthful-write"),
        slug: "truthful-write".to_string(),
        version: 4,
        content_hash: "truthful-write-hash".to_string(),
        block: "SECRET_PERSONA_BODY_MUST_NOT_PERSIST".to_string(),
        skipped_reason: None,
    }
}

#[tokio::test]
async fn project_scope_mismatch_forces_negative_attribution_even_if_caller_marks_injected() {
    let state = AppState::new_test();
    let conversation_id = ChatConversationId::new();
    let run = state
        .agent_run_repo
        .create(AgentRun::new(conversation_id))
        .await
        .expect("persona run should persist");
    let mut persona = persona_attribution_fixture();
    persona.skipped_reason = Some("project_scope_mismatch");
    persona.block.clear();

    record_persona_run_attribution(
        &state.agent_run_repo,
        state.events.as_ref(),
        &conversation_id,
        &run.id.as_str(),
        AgentHarnessKind::Codex,
        Some(&persona),
        true,
        None,
    )
    .await;

    let attributed = state
        .agent_run_repo
        .get_by_id(&run.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(attributed.persona_injected, Some(false));
    assert_eq!(
        attributed.persona_skipped_reason.as_deref(),
        Some("project_scope_mismatch")
    );
}

#[tokio::test]
async fn persona_recording_leaves_terminal_pre_spawn_runs_unattributed() {
    for failed in [false, true] {
        let state = AppState::new_test();
        let conversation_id = ChatConversationId::new();
        let run = state
            .agent_run_repo
            .create(AgentRun::new(conversation_id))
            .await
            .expect("persona run should persist");
        if failed {
            state
                .agent_run_repo
                .fail(&run.id, "Agent stopped by user")
                .await
                .expect("pre-spawn stop failure should persist");
        } else {
            state
                .agent_run_repo
                .cancel(&run.id)
                .await
                .expect("pre-spawn cancellation should persist");
        }

        record_persona_run_attribution(
            &state.agent_run_repo,
            state.events.as_ref(),
            &conversation_id,
            &run.id.as_str(),
            AgentHarnessKind::Codex,
            Some(&persona_attribution_fixture()),
            false,
            None,
        )
        .await;

        let terminal = state
            .agent_run_repo
            .get_by_id(&run.id)
            .await
            .expect("persona run lookup should succeed")
            .expect("persona run should exist");
        assert_eq!(terminal.persona_id, None);
        assert_eq!(terminal.persona_injected, None);
        assert_eq!(terminal.persona_skipped_reason, None);
    }
}

#[tokio::test]
async fn persona_recording_replaces_missing_or_empty_negative_reason_with_unknown() {
    for (index, reason) in [None, Some(""), Some("   ")].into_iter().enumerate() {
        let state = AppState::new_test();
        let conversation_id = ChatConversationId::new();
        let run = state
            .agent_run_repo
            .create(AgentRun::new(conversation_id))
            .await
            .expect("persona run should persist");

        record_persona_run_attribution(
            &state.agent_run_repo,
            state.events.as_ref(),
            &conversation_id,
            &run.id.as_str(),
            AgentHarnessKind::Codex,
            Some(&persona_attribution_fixture()),
            false,
            reason,
        )
        .await;

        let attributed = state
            .agent_run_repo
            .get_by_id(&run.id)
            .await
            .expect("persona run lookup should succeed")
            .expect("persona run should exist");
        assert_eq!(
            attributed.persona_skipped_reason.as_deref(),
            Some("unknown"),
            "case {index} must persist evidence for a negative attribution"
        );
        assert_eq!(attributed.persona_injected, Some(false));
        assert!(!serde_json::to_string(&attributed)
            .expect("persona run should serialize")
            .contains("SECRET_PERSONA_BODY_MUST_NOT_PERSIST"));
    }
}
