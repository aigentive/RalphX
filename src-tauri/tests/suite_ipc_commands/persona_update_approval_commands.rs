use std::sync::Arc;

use ralphx_events::RecordingEventSink;
use ralphx_lib::application::personas::{PersonaService, SavePersonaDraftInput};
use ralphx_lib::application::AppState;
use ralphx_lib::commands::persona_commands::{
    approve_persona_as_new_for_state, approve_persona_for_state, reseed_persona_draft_for_state,
    ApprovePersonaAsNewInput, PersonaIdInput,
};
use ralphx_lib::domain::entities::{
    AgentConversationWorkspaceMode, ChatConversation, Persona, ProjectId,
};
use ralphx_lib::infrastructure::sqlite::{
    SqliteChatConversationRepository, SqlitePersonaRepository,
};

fn persona_content(slug: &str, body: &str) -> String {
    format!("---\nname: {slug}\nkind: persona\ndescription: Command event test\n---\n{body}")
}

fn command_state() -> (AppState, RecordingEventSink) {
    let mut state = AppState::new_sqlite_test();
    let shared = Arc::clone(state.db.inner());
    state.persona_repo = Arc::new(SqlitePersonaRepository::from_shared(Arc::clone(&shared)));
    state.chat_conversation_repo = Arc::new(SqliteChatConversationRepository::from_shared(shared));
    let events = RecordingEventSink::new();
    state.events = Arc::new(events.clone());
    (state, events)
}

fn service(state: &AppState) -> PersonaService {
    PersonaService::new(
        state.db.clone(),
        Arc::clone(&state.persona_repo),
        Arc::clone(&state.chat_conversation_repo),
    )
}

async fn seeded_draft(state: &AppState, slug: &str) -> (Persona, Persona) {
    let service = service(state);
    let source_draft = service
        .create_draft(
            true,
            SavePersonaDraftInput {
                project_id: None,
                slug: slug.to_string(),
                content: persona_content(slug, "Source"),
                source_session_id: None,
                source_persona_id: None,
                source_content_hash: None,
            },
        )
        .await
        .unwrap();
    let source = service
        .approve_persona(true, &source_draft.id)
        .await
        .unwrap();
    let mut conversation = ChatConversation::new_project(ProjectId::new());
    conversation.agent_mode = Some(AgentConversationWorkspaceMode::PersonaBuilder);
    let conversation = state
        .chat_conversation_repo
        .create(conversation)
        .await
        .unwrap();
    let draft = service
        .create_bound_draft(
            true,
            &conversation.id,
            SavePersonaDraftInput {
                project_id: source.project_id.clone(),
                slug: slug.to_string(),
                content: persona_content(slug, "Revision without event body"),
                source_session_id: Some(conversation.id.as_str().to_string()),
                source_persona_id: Some(source.id.clone()),
                source_content_hash: Some(source.content_hash.clone()),
            },
        )
        .await
        .unwrap();
    (source, draft)
}

#[test]
fn approve_as_new_input_uses_camel_case_optional_slug() {
    let input: ApprovePersonaAsNewInput =
        serde_json::from_str(r#"{"id":"draft-1","newSlug":"replacement-persona"}"#)
            .expect("approve-as-new input should deserialize from Tauri camelCase");
    assert_eq!(input.id, "draft-1");
    assert_eq!(input.new_slug.as_deref(), Some("replacement-persona"));
}

#[tokio::test]
async fn sourced_approval_emits_body_free_applied_event_only_after_commit() {
    let (state, events) = command_state();
    let (source, draft) = seeded_draft(&state, "event-source").await;

    let applied = approve_persona_for_state(
        PersonaIdInput {
            id: draft.id.as_str().to_string(),
        },
        &state,
        true,
    )
    .await
    .expect("seeded approval command should return the updated source");

    assert_eq!(applied.id, source.id);
    let events = events.events();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].event, "persona:draft_applied");
    assert_eq!(events[0].payload["draft_id"], draft.id.as_str());
    assert_eq!(events[0].payload["source_persona_id"], source.id.as_str());
    assert!(!events[0]
        .payload
        .to_string()
        .contains("Revision without event body"));
}

#[tokio::test]
async fn failed_stale_approval_emits_no_terminal_event_and_reseed_command_recovers() {
    let (state, events) = command_state();
    let (source, draft) = seeded_draft(&state, "command-reseed").await;
    service(&state)
        .update_persona(
            true,
            &source.id,
            &persona_content("command-reseed", "Manual source edit"),
        )
        .await
        .unwrap();

    let error = approve_persona_for_state(
        PersonaIdInput {
            id: draft.id.as_str().to_string(),
        },
        &state,
        true,
    )
    .await
    .expect_err("stale approval should fail");
    assert!(error.contains("SourceChangedSinceSeed:"));
    assert!(events.events().is_empty());

    let reseeded = reseed_persona_draft_for_state(
        PersonaIdInput {
            id: draft.id.as_str().to_string(),
        },
        &state,
        true,
    )
    .await
    .expect("explicit reseed command should update the source baseline");
    assert_ne!(reseeded.source_content_hash, draft.source_content_hash);
}

#[tokio::test]
async fn approve_as_new_command_requires_explicit_source_terminal_recovery() {
    let (state, events) = command_state();
    let (source, draft) = seeded_draft(&state, "command-as-new").await;
    service(&state)
        .archive_persona(true, &source.id)
        .await
        .unwrap();

    let approved = approve_persona_as_new_for_state(
        ApprovePersonaAsNewInput {
            id: draft.id.as_str().to_string(),
            new_slug: None,
        },
        &state,
        true,
    )
    .await
    .expect("explicit approve-as-new command should preserve builder work");

    assert_eq!(approved.id, draft.id);
    assert!(approved.source_persona_id.is_none());
    assert!(
        events.events().is_empty(),
        "apply event is source-update only"
    );
}
