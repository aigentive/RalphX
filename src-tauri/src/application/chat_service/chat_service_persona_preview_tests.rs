//! Proof obligation: the injected-prompt preview is byte-identical to the
//! spawn-time overlay because both flow through `resolve_persona_for_send`.

use std::sync::Arc;

use chrono::Utc;

use crate::application::chat_service::AppChatService;
use crate::application::persona_prompt::render_persona_block;
use crate::application::AppState;
use crate::domain::entities::{
    ChatConversation, IdeationSessionId, Persona, PersonaId, PersonaStatus, ProjectId,
};
use crate::domain::repositories::PersonaRepository;
use crate::infrastructure::memory::MemoryPersonaRepository;

fn preview_persona(slug: &str) -> Persona {
    Persona {
        id: PersonaId::from(format!("{slug}-id")),
        artifact_id: None,
        project_id: None,
        slug: slug.to_string(),
        name: format!("{slug} persona"),
        description: "preview fixture".to_string(),
        content: format!(
            "---\nname: {slug}\nkind: persona\ndescription: Preview fixture\n---\nSpeak tersely."
        ),
        status: PersonaStatus::Active,
        version: 3,
        content_hash: format!("{slug}-hash"),
        source_session_id: None,
        source_persona_id: None,
        source_content_hash: None,
        source_json: "{}".to_string(),
        created_at: Utc::now(),
        updated_at: Utc::now(),
    }
}

async fn preview_fixture(
    persona: &Persona,
    bind: bool,
) -> (AppChatService, crate::domain::entities::ChatConversationId) {
    let mut state = AppState::new_test();
    let persona_repo = Arc::new(MemoryPersonaRepository::new());
    persona_repo
        .create(persona.clone())
        .await
        .expect("persona fixture should persist");
    state.persona_repo = persona_repo;

    let mut conversation =
        ChatConversation::new_project(ProjectId::from_string("preview-project".to_string()));
    if bind {
        conversation.persona_id = Some(persona.id.as_str().to_string());
    }
    let conversation_id = conversation.id.clone();
    state
        .chat_conversation_repo
        .create(conversation)
        .await
        .expect("conversation fixture should persist");

    let service = state
        .build_chat_service()
        .with_persona_feature_enabled(true);
    (service, conversation_id)
}

#[tokio::test]
async fn preview_block_is_byte_identical_to_spawn_time_render() {
    let persona = preview_persona("preview-parity");
    let (service, conversation_id) = preview_fixture(&persona, true).await;

    let preview = service
        .preview_persona_overlay(&conversation_id)
        .await
        .expect("preview should resolve")
        .expect("bound active persona should produce an overlay");

    let spawn_time = render_persona_block(&persona).expect("fixture renders");
    assert_eq!(preview.block, spawn_time.block);
    assert_eq!(preview.slug, persona.slug);
    assert_eq!(preview.version, persona.version);
    assert_eq!(preview.skipped_reason, None);
}

#[tokio::test]
async fn preview_returns_none_without_binding_or_with_flag_off() {
    let persona = preview_persona("preview-none");
    let (service, conversation_id) = preview_fixture(&persona, false).await;
    assert_eq!(
        service
            .preview_persona_overlay(&conversation_id)
            .await
            .expect("preview should resolve"),
        None
    );

    let (flag_off_service, bound_conversation_id) = {
        let (service, conversation_id) = preview_fixture(&persona, true).await;
        (service.with_persona_feature_enabled(false), conversation_id)
    };
    assert_eq!(
        flag_off_service
            .preview_persona_overlay(&bound_conversation_id)
            .await
            .expect("flag-off preview should not error"),
        None
    );
}

#[tokio::test]
async fn preview_is_project_context_only() {
    let persona = preview_persona("preview-ideation");
    let mut state = AppState::new_test();
    let persona_repo = Arc::new(MemoryPersonaRepository::new());
    persona_repo
        .create(persona.clone())
        .await
        .expect("persona fixture should persist");
    state.persona_repo = persona_repo;

    let mut conversation = ChatConversation::new_ideation(IdeationSessionId::new());
    conversation.persona_id = Some(persona.id.as_str().to_string());
    let conversation_id = conversation.id.clone();
    state
        .chat_conversation_repo
        .create(conversation)
        .await
        .expect("conversation fixture should persist");

    let service = state
        .build_chat_service()
        .with_persona_feature_enabled(true);
    assert_eq!(
        service
            .preview_persona_overlay(&conversation_id)
            .await
            .expect("preview should resolve"),
        None,
        "persona eligibility stays Project-context-only"
    );
}
