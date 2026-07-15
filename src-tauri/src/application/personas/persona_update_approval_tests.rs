#![cfg(test)]

use std::sync::Arc;

use super::{PersonaService, SavePersonaDraftInput};
use crate::domain::entities::{
    AgentConversationWorkspaceMode, ChatConversation, Persona, PersonaStatus, ProjectId,
};
use crate::error::AppError;
use crate::infrastructure::sqlite::{
    DbConnection, SqliteChatConversationRepository, SqlitePersonaRepository,
};
use crate::testing::SqliteTestDb;

fn persona_content(slug: &str, body: &str) -> String {
    format!("---\nname: {slug}\nkind: persona\ndescription: Update approval test\n---\n{body}")
}

fn sqlite_service(db: &SqliteTestDb) -> PersonaService {
    let shared = db.shared_conn();
    PersonaService::new(
        DbConnection::from_shared(Arc::clone(&shared)),
        Arc::new(SqlitePersonaRepository::from_shared(Arc::clone(&shared))),
        Arc::new(SqliteChatConversationRepository::from_shared(shared)),
    )
}

async fn create_active(service: &PersonaService, slug: &str) -> Persona {
    let draft = service
        .create_draft(
            true,
            SavePersonaDraftInput {
                slug: slug.to_string(),
                content: persona_content(slug, "Initial source body"),
                source_session_id: None,
                source_persona_id: None,
                source_content_hash: None,
            },
        )
        .await
        .expect("source draft should create");
    service
        .approve_persona(true, &draft.id)
        .await
        .expect("source draft should approve")
}

async fn create_builder_conversation(service: &PersonaService) -> ChatConversation {
    let mut conversation = ChatConversation::new_project(ProjectId::new());
    conversation.agent_mode = Some(AgentConversationWorkspaceMode::PersonaBuilder);
    service
        .chat_conversation_repo
        .create(conversation)
        .await
        .expect("builder conversation should persist")
}

async fn seeded_fixture(
    db: &SqliteTestDb,
    slug: &str,
) -> (PersonaService, Persona, Persona, Vec<ChatConversation>) {
    let service = sqlite_service(db);
    let source = create_active(&service, slug).await;
    let first = create_builder_conversation(&service).await;
    let second = create_builder_conversation(&service).await;
    let draft = service
        .create_bound_draft(
            true,
            &first.id,
            SavePersonaDraftInput {
                slug: source.slug.clone(),
                content: persona_content(slug, "Builder revision"),
                source_session_id: Some(first.id.as_str().to_string()),
                source_persona_id: Some(source.id.clone()),
                source_content_hash: Some(source.content_hash.clone()),
            },
        )
        .await
        .expect("seeded draft should create and bind");
    service
        .chat_conversation_repo
        .update_builder_draft_binding(&second.id, Some(draft.id.as_str()))
        .await
        .expect("second restored conversation should bind to the same draft");
    (service, source, draft, vec![first, second])
}

async fn assert_bindings(
    service: &PersonaService,
    conversations: &[ChatConversation],
    expected: Option<&str>,
) {
    for conversation in conversations {
        let loaded = service
            .chat_conversation_repo
            .get_by_id(&conversation.id)
            .await
            .expect("conversation lookup should succeed")
            .expect("conversation should exist");
        assert_eq!(loaded.builder_draft_id.as_deref(), expected);
    }
}

#[tokio::test]
async fn seeded_approval_rolls_back_source_write_when_draft_delete_fails() {
    let db = SqliteTestDb::new("seeded_approval_rollback");
    let (service, source, draft, conversations) = seeded_fixture(&db, "rollback-source").await;
    let trigger = format!(
        "CREATE TRIGGER fail_seeded_draft_delete BEFORE DELETE ON personas
         WHEN OLD.id = '{}' BEGIN SELECT RAISE(ABORT, 'forced draft delete failure'); END;",
        draft.id.as_str()
    );
    db.shared_conn()
        .lock()
        .await
        .execute_batch(&trigger)
        .expect("rollback trigger should install");

    let error = service
        .approve_persona(true, &draft.id)
        .await
        .expect_err("draft delete failure must roll back the source write");

    assert!(matches!(error, AppError::Database(_)));
    assert_eq!(
        service.get_persona(true, &source.id).await.unwrap(),
        source,
        "source must be byte-for-byte unchanged after rollback"
    );
    assert_eq!(service.get_draft(true, &draft.id).await.unwrap(), draft);
    assert_bindings(&service, &conversations, Some(draft.id.as_str())).await;
}

#[tokio::test]
async fn stale_source_blocks_apply_until_the_draft_is_explicitly_reseeded() {
    let db = SqliteTestDb::new("seeded_approval_stale_source");
    let (service, source, draft, conversations) = seeded_fixture(&db, "stale-source").await;
    let manual_content = persona_content(&source.slug, "Manual edit while builder was open");
    let manually_updated = service
        .update_persona(true, &source.id, &manual_content)
        .await
        .expect("manual source update should succeed");

    let error = service
        .approve_persona(true, &draft.id)
        .await
        .expect_err("stale seeded draft must not overwrite the source");

    assert!(matches!(error, AppError::Conflict(message) if message.starts_with("SourceChangedSinceSeed:")));
    assert_eq!(
        service.get_persona(true, &source.id).await.unwrap(),
        manually_updated
    );
    assert_eq!(service.get_draft(true, &draft.id).await.unwrap(), draft);
    assert_bindings(&service, &conversations, Some(draft.id.as_str())).await;

    let reseeded = service
        .reseed_persona_draft(true, &draft.id)
        .await
        .expect("explicit reseed should accept the new source baseline");
    assert_eq!(
        reseeded.source_content_hash.as_deref(),
        Some(manually_updated.content_hash.as_str())
    );
    assert_eq!(reseeded.content, draft.content, "reseed keeps builder work");

    let applied = service
        .approve_persona(true, &draft.id)
        .await
        .expect("reseeded draft should apply");
    assert_eq!(applied.id, source.id);
    assert_eq!(applied.content, draft.content);
    assert_eq!(applied.version, manually_updated.version + 1);
    assert!(service.persona_repo.get_by_id(&draft.id).await.unwrap().is_none());
    assert_bindings(&service, &conversations, None).await;
}

#[tokio::test]
async fn double_approval_writes_the_source_once_and_clears_every_binding() {
    let db = SqliteTestDb::new("seeded_approval_reentry");
    let (service, source, draft, conversations) = seeded_fixture(&db, "reentry-source").await;

    let applied = service
        .approve_persona(true, &draft.id)
        .await
        .expect("first approval should apply the seeded draft");
    assert_eq!(applied.id, source.id);
    assert_eq!(applied.version, source.version + 1);
    assert!(service.persona_repo.get_by_id(&draft.id).await.unwrap().is_none());
    assert_bindings(&service, &conversations, None).await;

    let error = service
        .approve_persona(true, &draft.id)
        .await
        .expect_err("deleted seeded draft must not apply twice");
    assert!(matches!(error, AppError::NotFound(_)));
    assert_eq!(
        service.get_persona(true, &source.id).await.unwrap(),
        applied,
        "second approval must not mutate the source"
    );
    assert_bindings(&service, &conversations, None).await;
}

#[tokio::test]
async fn archived_source_requires_explicit_approve_as_new_recovery() {
    let db = SqliteTestDb::new("seeded_approval_as_new");
    let (service, source, draft, conversations) = seeded_fixture(&db, "archived-source").await;
    service
        .archive_persona(true, &source.id)
        .await
        .expect("source should archive");

    let error = service
        .approve_persona(true, &draft.id)
        .await
        .expect_err("ordinary approval must not silently become create-new");
    assert!(matches!(error, AppError::Conflict(message) if message.starts_with("SourceNoLongerActive:")));
    assert_bindings(&service, &conversations, Some(draft.id.as_str())).await;

    let approved = service
        .approve_persona_as_new(true, &draft.id, None)
        .await
        .expect("explicit recovery should approve the draft as a new persona");
    assert_eq!(approved.id, draft.id);
    assert_eq!(approved.status, PersonaStatus::Active);
    assert!(approved.source_persona_id.is_none());
    assert!(approved.source_content_hash.is_none());
    assert_bindings(&service, &conversations, None).await;
}

#[tokio::test]
async fn approve_as_new_atomically_renames_when_the_inherited_slug_is_taken() {
    let db = SqliteTestDb::new("seeded_approval_as_new_rename");
    let (service, source, draft, conversations) = seeded_fixture(&db, "taken-source").await;
    service.archive_persona(true, &source.id).await.unwrap();
    let replacement = create_active(&service, &source.slug).await;

    let collision = service
        .approve_persona_as_new(true, &draft.id, None)
        .await
        .expect_err("active inherited slug requires an explicit replacement slug");
    assert!(matches!(collision, AppError::Conflict(message) if message.contains("taken-source")));
    assert_eq!(service.get_draft(true, &draft.id).await.unwrap(), draft);
    assert_bindings(&service, &conversations, Some(draft.id.as_str())).await;

    let approved = service
        .approve_persona_as_new(true, &draft.id, Some("recovered-persona"))
        .await
        .expect("replacement slug should be rewritten and approved atomically");
    assert_eq!(approved.id, draft.id);
    assert_eq!(approved.slug, "recovered-persona");
    assert!(approved.content.contains("name: recovered-persona"));
    assert_eq!(
        service.get_persona(true, &replacement.id).await.unwrap(),
        replacement
    );
    assert_bindings(&service, &conversations, None).await;
}
