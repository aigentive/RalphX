#![cfg(test)]

use std::sync::Arc;

use super::persona_service_test_support::*;
use super::{PersonaService, SavePersonaDraftInput};
use crate::domain::entities::{
    ChatConversation, ChatConversationId, PersonaId, PersonaScopeFilter, PersonaStatus, ProjectId,
};
use crate::error::AppError;
use crate::infrastructure::sqlite::{
    DbConnection, SqliteChatConversationRepository, SqlitePersonaRepository,
};
use crate::testing::SqliteTestDb;

#[tokio::test]
async fn bound_draft_creation_rolls_back_when_the_conversation_is_missing() {
    let db = SqliteTestDb::new("bound_draft_creation_rollback");
    let service = sqlite_service(&db);

    let error = service
        .create_bound_draft(
            true,
            &ChatConversationId::from_string("missing-conversation".to_string()),
            draft_input("transactional-draft", "Must roll back"),
        )
        .await
        .expect_err("binding failure must roll back the inserted draft");

    assert!(matches!(error, AppError::NotFound(_)));
    assert!(service
        .list_personas(true, PersonaScopeFilter::All)
        .await
        .unwrap()
        .is_empty());
}

#[tokio::test]
async fn persona_writer_artifacts_record_user_and_agent_attribution() {
    let db = SqliteTestDb::new("persona_writer_artifact_attribution");
    let service = sqlite_service(&db);
    let draft = service
        .create_draft(true, draft_input("writer-history", "Created manually"))
        .await
        .unwrap();
    assert_eq!(persona_artifacts(&db, &draft.id)[0].1, "user");

    let manually_updated = service
        .update_draft(
            true,
            &draft.id,
            &persona_content("writer-history", "Manual draft edit"),
            None,
        )
        .await
        .unwrap();
    assert_eq!(persona_artifacts(&db, &draft.id).last().unwrap().1, "user");
    assert_eq!(
        manually_updated.artifact_id,
        Some(crate::domain::entities::ArtifactId::from_string(
            persona_artifacts(&db, &draft.id).last().unwrap().0.clone(),
        ))
    );

    let mut conversation = ChatConversation::new_project(ProjectId::new());
    conversation.agent_mode =
        Some(crate::domain::entities::AgentConversationWorkspaceMode::PersonaBuilder);
    let conversation = service
        .chat_conversation_repo
        .create(conversation)
        .await
        .unwrap();
    let bound = service
        .create_bound_draft(
            true,
            &conversation.id,
            draft_input("agent-history", "Agent create"),
        )
        .await
        .unwrap();
    assert_eq!(persona_artifacts(&db, &bound.id)[0].1, "agent");
    service
        .update_draft_as_agent(
            true,
            &bound.id,
            &persona_content("agent-history", "Agent update"),
        )
        .await
        .unwrap();
    assert_eq!(persona_artifacts(&db, &bound.id).last().unwrap().1, "agent");
}

#[tokio::test]
async fn artifact_insert_failure_rolls_back_persona_content_and_tip() {
    let db = SqliteTestDb::new("persona_artifact_failure_rollback");
    let service = sqlite_service(&db);
    let draft = service
        .create_draft(true, draft_input("atomic-history", "Before"))
        .await
        .unwrap();
    let artifacts_before = persona_artifacts(&db, &draft.id);
    db.with_connection(|conn| {
        conn.execute_batch(
            "CREATE TRIGGER fail_persona_artifact_insert
             BEFORE INSERT ON artifacts WHEN NEW.type = 'persona'
             BEGIN SELECT RAISE(ABORT, 'forced artifact failure'); END;",
        )
        .unwrap();
    });

    let error = service
        .update_draft(
            true,
            &draft.id,
            &persona_content("atomic-history", "Must roll back"),
            None,
        )
        .await
        .expect_err("artifact failure must abort persona mutation");

    assert!(matches!(error, AppError::Database(_)));
    assert_eq!(service.get_draft(true, &draft.id).await.unwrap(), draft);
    assert_eq!(persona_artifacts(&db, &draft.id), artifacts_before);
}

#[tokio::test]
async fn plain_approval_hashes_the_transactional_content_and_appends_that_tip() {
    let db = SqliteTestDb::new("plain_approval_transactional_content");
    let service = sqlite_service(&db);
    let draft = service
        .create_draft(true, draft_input("approval-race", "Version one"))
        .await
        .unwrap();
    let stale_read = service.get_draft(true, &draft.id).await.unwrap();
    let concurrent_shared = Arc::new(tokio::sync::Mutex::new(db.new_connection()));
    let concurrent = PersonaService::new(
        DbConnection::from_shared(Arc::clone(&concurrent_shared)),
        Arc::new(SqlitePersonaRepository::from_shared(Arc::clone(
            &concurrent_shared,
        ))),
        Arc::new(SqliteChatConversationRepository::from_shared(
            concurrent_shared,
        )),
    );
    let content_v2 = persona_content("approval-race", "Version two from another connection");
    let hash_v2 = expected_hash(&content_v2);
    concurrent
        .persona_repo
        .update_content(&draft.id, &content_v2, &hash_v2, None)
        .await
        .expect("concurrent repository write should land after the stale read");

    let approved = service
        .approve_plain_draft(&stale_read.id)
        .await
        .expect("approval should validate and hash the transactional re-read");

    assert_eq!(approved.content, content_v2);
    assert_eq!(approved.content_hash, hash_v2);
    let tip_content: String = db.with_connection(|conn| {
        conn.query_row(
            "SELECT content_text FROM artifacts WHERE id = ?1",
            [approved.artifact_id.as_ref().unwrap().as_str()],
            |row| row.get(0),
        )
        .unwrap()
    });
    assert_eq!(tip_content, approved.content);
}

#[tokio::test]
async fn plain_approval_rejects_invalid_transactional_content_without_mutation() {
    let db = SqliteTestDb::new("plain_approval_transactional_validation");
    let service = sqlite_service(&db);
    let draft = service
        .create_draft(true, draft_input("approval-invalid-race", "Valid version"))
        .await
        .unwrap();
    let stale_read = service.get_draft(true, &draft.id).await.unwrap();
    service
        .persona_repo
        .update_content(&draft.id, "invalid markdown", "stale-hash", None)
        .await
        .expect("concurrent invalid repository write should land");
    let before_approval = service.get_draft(true, &draft.id).await.unwrap();

    let error = service
        .approve_plain_draft(&stale_read.id)
        .await
        .expect_err("transactional validation must reject the changed content");

    assert!(matches!(error, AppError::Validation(_)));
    assert_eq!(
        service.get_draft(true, &draft.id).await.unwrap(),
        before_approval
    );
    assert_eq!(persona_artifacts(&db, &draft.id).len(), 1);
}

#[tokio::test]
async fn seeded_update_draft_can_share_source_slug_and_preserves_provenance() {
    let service = memory_service();
    let source_id = create_active(&service, "shared-persona").await;
    let source = service
        .get_persona(true, &source_id)
        .await
        .expect("source persona");
    let input = SavePersonaDraftInput {
        project_id: source.project_id.clone(),
        slug: source.slug.clone(),
        content: persona_content(&source.slug, "Seeded update"),
        source_session_id: Some("builder-conversation".to_string()),
        source_persona_id: Some(source.id.clone()),
        source_content_hash: Some(source.content_hash.clone()),
    };

    let draft = service
        .create_draft(true, input)
        .await
        .expect("seeded draft may share source slug");

    assert_eq!(draft.source_persona_id.as_ref(), Some(&source.id));
    assert_eq!(
        draft.source_content_hash.as_deref(),
        Some(source.content_hash.as_str())
    );
}

#[tokio::test]
async fn approve_fails_closed_when_another_active_persona_owns_the_slug() {
    let service = memory_service();
    let draft = service
        .create_draft(true, draft_input("approval-collision", "Waiting draft"))
        .await
        .expect("draft should create before the active-slug race");
    let mut active_owner = draft.clone();
    active_owner.id = PersonaId::new();
    active_owner.status = PersonaStatus::Active;
    service
        .persona_repo
        .create(active_owner)
        .await
        .expect("repository fixture should simulate another activation after draft creation");

    let error = service
        .approve_persona(true, &draft.id)
        .await
        .expect_err("active slug collision must block approval");
    assert!(
        matches!(error, AppError::Validation(message) if message.contains("approval-collision"))
    );
    assert_eq!(
        service
            .get_draft(true, &draft.id)
            .await
            .expect("draft remains authoritative")
            .status,
        PersonaStatus::Draft
    );
}
