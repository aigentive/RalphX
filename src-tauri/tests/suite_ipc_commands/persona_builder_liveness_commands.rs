use std::fs;

use chrono::Utc;
use ralphx_lib::application::persona_ingest::{
    persona_ingest_conversation_path, persona_ingest_storage_path,
};
use ralphx_lib::application::AppState;
use ralphx_lib::commands::persona_builder_commands::{
    get_persona_builder_ingest_status_for_state, PersonaBuilderIngestStatusInput,
};
use ralphx_lib::domain::entities::{
    AgentConversationWorkspaceMode, ChatConversation, Persona, PersonaId, PersonaStatus, ProjectId,
};

fn draft(id: &PersonaId, status: PersonaStatus) -> Persona {
    let now = Utc::now();
    Persona {
        id: id.clone(),
        artifact_id: None,

        project_id: None,
        slug: "bound-liveness".to_string(),
        name: "bound-liveness".to_string(),
        description: "Bound draft liveness fixture".to_string(),
        content: "---\nname: bound-liveness\nkind: persona\ndescription: Bound draft liveness fixture\n---\nBody".to_string(),
        status,
        version: 1,
        content_hash: "fixture-hash".to_string(),
        source_session_id: None,
        source_persona_id: None,
        source_content_hash: None,
        source_json: "{}".to_string(),
        created_at: now,
        updated_at: now,
    }
}

async fn status(state: &AppState, conversation_id: &str) -> bool {
    let temp = tempfile::tempdir().expect("bound draft liveness temp directory");
    get_persona_builder_ingest_status_for_state(
        PersonaBuilderIngestStatusInput {
            conversation_id: conversation_id.to_string(),
        },
        state,
        true,
        temp.path(),
    )
    .await
    .expect("bound draft status lookup should succeed")
    .live
}

#[tokio::test]
async fn mount_gate_accepts_only_an_existing_bound_draft_without_ingest_files() {
    let state = AppState::new_test();
    let draft_id = PersonaId::new();
    let mut conversation = ChatConversation::new_project(ProjectId::new());
    conversation.agent_mode = Some(AgentConversationWorkspaceMode::PersonaBuilder);
    conversation.builder_draft_id = Some(draft_id.as_str().to_string());
    let conversation = state
        .chat_conversation_repo
        .create(conversation)
        .await
        .expect("bound PersonaBuilder conversation should persist");

    assert!(
        !status(&state, &conversation.id.as_str()).await,
        "dangling builder_draft_id must not satisfy the mount gate"
    );

    state
        .persona_repo
        .create(draft(&draft_id, PersonaStatus::Draft))
        .await
        .expect("bound draft should persist");
    assert!(
        status(&state, &conversation.id.as_str()).await,
        "an existing bound Draft is live without filesystem context"
    );

    state
        .persona_repo
        .set_status(&draft_id, PersonaStatus::Active)
        .await
        .expect("fixture should make the binding point at a non-Draft");
    assert!(
        !status(&state, &conversation.id.as_str()).await,
        "a bound Active persona must not satisfy the mount gate"
    );

    state
        .persona_repo
        .delete(&draft_id)
        .await
        .expect("fixture should restore a dangling binding");
    assert!(!status(&state, &conversation.id.as_str()).await);

    let temp = tempfile::tempdir().expect("masked dangling binding temp directory");
    let ingest_root = persona_ingest_conversation_path(
        &persona_ingest_storage_path(temp.path()),
        &conversation.id.as_str(),
    );
    fs::create_dir_all(&ingest_root).expect("create ingest root beside dangling binding");
    fs::write(
        ingest_root.join("context.md"),
        "Context must not mask binding\n",
    )
    .expect("write ingest context beside dangling binding");
    let masked = get_persona_builder_ingest_status_for_state(
        PersonaBuilderIngestStatusInput {
            conversation_id: conversation.id.as_str(),
        },
        &state,
        true,
        temp.path(),
    )
    .await
    .expect("dangling binding status should remain a normal fail-closed response");
    assert!(
        !masked.live,
        "ingested files must not mask an explicit dangling draft binding"
    );
}
