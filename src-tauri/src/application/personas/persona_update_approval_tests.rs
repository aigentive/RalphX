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
                project_id: None,
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

async fn create_active_in_project(
    service: &PersonaService,
    slug: &str,
    project_id: &ProjectId,
) -> Persona {
    let draft = service
        .create_draft(
            true,
            SavePersonaDraftInput {
                project_id: Some(project_id.clone()),
                slug: slug.to_string(),
                content: persona_content(slug, "Project source body"),
                source_session_id: None,
                source_persona_id: None,
                source_content_hash: None,
            },
        )
        .await
        .unwrap();
    service.approve_persona(true, &draft.id).await.unwrap()
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
                project_id: source.project_id.clone(),
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
async fn approve_as_new_allows_global_slug_but_rejects_same_project_scope() {
    let db = SqliteTestDb::new("approve_as_new_project_scope");
    let service = sqlite_service(&db);
    let project_id = ProjectId::from_string("project-a".to_string());
    create_active(&service, "shared-approval").await;
    let source = create_active_in_project(&service, "shared-approval", &project_id).await;
    let draft = service
        .create_draft(
            true,
            SavePersonaDraftInput {
                project_id: Some(project_id.clone()),
                slug: source.slug.clone(),
                content: persona_content(&source.slug, "First replacement"),
                source_session_id: None,
                source_persona_id: Some(source.id.clone()),
                source_content_hash: Some(source.content_hash.clone()),
            },
        )
        .await
        .unwrap();
    service
        .persona_repo
        .set_status(&source.id, PersonaStatus::Archived)
        .await
        .unwrap();
    let approved = service
        .approve_persona_as_new(true, &draft.id, None)
        .await
        .expect("global same slug must not conflict with project approval");
    assert_eq!(approved.project_id.as_ref(), Some(&project_id));

    let conflicting = service
        .create_draft(
            true,
            SavePersonaDraftInput {
                project_id: Some(project_id),
                slug: "shared-approval".to_string(),
                content: persona_content("shared-approval", "Second replacement"),
                source_session_id: None,
                source_persona_id: Some(source.id),
                source_content_hash: Some(source.content_hash),
            },
        )
        .await
        .unwrap();
    let error = service
        .approve_persona_as_new(true, &conflicting.id, None)
        .await
        .expect_err("same project active slug must conflict");
    assert!(matches!(error, AppError::Conflict(message) if message.contains("already in use")));
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
async fn seeded_approval_conflicts_when_source_update_matches_no_rows() {
    let db = SqliteTestDb::new("seeded_approval_source_update_zero_rows");
    let (service, source, draft, conversations) = seeded_fixture(&db, "source-update-zero").await;
    let trigger = format!(
        "CREATE TRIGGER ignore_seeded_source_update BEFORE UPDATE ON personas
         WHEN OLD.id = '{}' BEGIN SELECT RAISE(IGNORE); END;",
        source.id.as_str()
    );
    db.shared_conn()
        .lock()
        .await
        .execute_batch(&trigger)
        .expect("source update trigger should install");

    let error = service
        .approve_persona(true, &draft.id)
        .await
        .expect_err("ignored source update must be reported as a conflict");

    assert!(
        matches!(error, AppError::Conflict(message) if message.contains("changed during approval"))
    );
    assert_eq!(service.get_persona(true, &source.id).await.unwrap(), source);
    assert_eq!(service.get_draft(true, &draft.id).await.unwrap(), draft);
    assert_bindings(&service, &conversations, Some(draft.id.as_str())).await;
}

#[tokio::test]
async fn seeded_approval_conflicts_when_draft_delete_matches_no_rows() {
    let db = SqliteTestDb::new("seeded_approval_draft_delete_zero_rows");
    let (service, source, draft, conversations) = seeded_fixture(&db, "draft-delete-zero").await;
    let trigger = format!(
        "CREATE TRIGGER ignore_seeded_draft_delete BEFORE DELETE ON personas
         WHEN OLD.id = '{}' BEGIN SELECT RAISE(IGNORE); END;",
        draft.id.as_str()
    );
    db.shared_conn()
        .lock()
        .await
        .execute_batch(&trigger)
        .expect("draft delete trigger should install");

    let error = service
        .approve_persona(true, &draft.id)
        .await
        .expect_err("ignored draft delete must be reported as a conflict");

    assert!(
        matches!(error, AppError::Conflict(message) if message.contains("disappeared during approval"))
    );
    assert_eq!(service.get_persona(true, &source.id).await.unwrap(), source);
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

    assert!(
        matches!(error, AppError::Conflict(message) if message.starts_with("SourceChangedSinceSeed:"))
    );
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
    assert!(service
        .persona_repo
        .get_by_id(&draft.id)
        .await
        .unwrap()
        .is_none());
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
    assert!(service
        .persona_repo
        .get_by_id(&draft.id)
        .await
        .unwrap()
        .is_none());
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
async fn deleting_a_seeded_draft_clears_every_builder_binding_atomically() {
    let db = SqliteTestDb::new("seeded_draft_delete_bindings");
    let (service, source, draft, conversations) = seeded_fixture(&db, "delete-seeded").await;

    service
        .hard_delete_draft(true, &draft.id)
        .await
        .expect("seeded draft deletion should commit with binding cleanup");

    assert!(service
        .persona_repo
        .get_by_id(&draft.id)
        .await
        .unwrap()
        .is_none());
    assert_eq!(service.get_persona(true, &source.id).await.unwrap(), source);
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
    assert!(
        matches!(error, AppError::Conflict(message) if message.starts_with("SourceNoLongerActive:"))
    );
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
async fn approve_as_new_rejects_drafts_that_are_not_seeded_updates() {
    let db = SqliteTestDb::new("approve_as_new_unseeded");
    let service = sqlite_service(&db);
    let draft = service
        .create_draft(
            true,
            SavePersonaDraftInput {
                project_id: None,
                slug: "unseeded-draft".to_string(),
                content: persona_content("unseeded-draft", "Standalone draft"),
                source_session_id: None,
                source_persona_id: None,
                source_content_hash: None,
            },
        )
        .await
        .expect("standalone draft should create");

    let error = service
        .approve_persona_as_new(true, &draft.id, Some("replacement-slug"))
        .await
        .expect_err("approve-as-new only applies to seeded update drafts");

    assert!(
        matches!(error, AppError::Validation(message) if message.contains("not a seeded update draft"))
    );
    assert_eq!(service.get_draft(true, &draft.id).await.unwrap(), draft);
}

#[tokio::test]
async fn approve_as_new_rejects_when_the_source_is_still_active() {
    let db = SqliteTestDb::new("approve_as_new_source_active");
    let (service, _source, draft, conversations) = seeded_fixture(&db, "still-active-source").await;

    let error = service
        .approve_persona_as_new(true, &draft.id, Some("should-not-activate"))
        .await
        .expect_err("active sources must be updated in place");

    assert!(
        matches!(error, AppError::Conflict(message) if message.starts_with("SourceStillActive:"))
    );
    assert_eq!(service.get_draft(true, &draft.id).await.unwrap(), draft);
    assert_bindings(&service, &conversations, Some(draft.id.as_str())).await;
}

#[tokio::test]
async fn approve_as_new_rejects_explicit_slug_used_by_another_open_draft() {
    let db = SqliteTestDb::new("approve_as_new_draft_slug_collision");
    let (service, source, draft, conversations) = seeded_fixture(&db, "draft-collision").await;
    service.archive_persona(true, &source.id).await.unwrap();
    let other_draft = service
        .create_draft(
            true,
            SavePersonaDraftInput {
                project_id: None,
                slug: "occupied-draft-slug".to_string(),
                content: persona_content("occupied-draft-slug", "Other draft"),
                source_session_id: None,
                source_persona_id: None,
                source_content_hash: None,
            },
        )
        .await
        .expect("other draft should reserve its slug");

    let error = service
        .approve_persona_as_new(true, &draft.id, Some("occupied-draft-slug"))
        .await
        .expect_err("explicit replacement slug must not collide with an open draft");

    assert!(
        matches!(error, AppError::Conflict(message) if message.contains("occupied-draft-slug"))
    );
    assert_eq!(service.get_draft(true, &draft.id).await.unwrap(), draft);
    assert_eq!(
        service.get_draft(true, &other_draft.id).await.unwrap(),
        other_draft
    );
    assert_bindings(&service, &conversations, Some(draft.id.as_str())).await;
}

#[tokio::test]
async fn approve_as_new_atomically_renames_when_the_inherited_slug_is_taken() {
    let db = SqliteTestDb::new("seeded_approval_as_new_rename");
    let (service, source, draft, conversations) = seeded_fixture(&db, "taken-source").await;
    service.archive_persona(true, &source.id).await.unwrap();
    let mut replacement = source.clone();
    replacement.id = crate::domain::entities::PersonaId::new();
    replacement.status = PersonaStatus::Active;
    replacement.created_at = chrono::Utc::now();
    replacement.updated_at = replacement.created_at;
    let replacement = service
        .persona_repo
        .create(replacement)
        .await
        .expect("database fixture may occupy the active slug beside a seeded draft");

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
