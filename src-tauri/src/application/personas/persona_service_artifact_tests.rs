#![cfg(test)]

use super::persona_service_test_support::*;
use super::SavePersonaDraftInput;
use crate::domain::entities::{
    ChatConversation, ChatConversationId, PersonaId, PersonaScopeFilter, PersonaStatus, ProjectId,
};
use crate::error::AppError;
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
async fn create_persona_draft_rolls_back_row_tip_status_and_bindings_when_append_fails() {
    let db = SqliteTestDb::new("create_persona_draft_append_rollback");
    let service = sqlite_service(&db);
    let conversation = service
        .chat_conversation_repo
        .create(ChatConversation::new_project(ProjectId::new()))
        .await
        .expect("unbound conversation fixture should persist");
    let artifacts_before = artifact_count(&db);
    fail_persona_artifact_appends(&db);

    let error = service
        .create_draft(
            true,
            draft_input("create-append-rollback", "Must never persist"),
        )
        .await
        .expect_err("artifact append failure must roll back draft creation");

    assert!(matches!(error, AppError::Database(_)));
    assert!(
        service
            .list_personas(true, PersonaScopeFilter::All)
            .await
            .unwrap()
            .is_empty(),
        "failed creation must leave no persona content, tip, or status row"
    );
    assert_eq!(artifact_count(&db), artifacts_before);
    let conversation_after = service
        .chat_conversation_repo
        .get_by_id(&conversation.id)
        .await
        .unwrap()
        .unwrap();
    assert!(conversation_after.persona_id.is_none());
    assert!(conversation_after.builder_draft_id.is_none());
    assert!(conversation_after.builder_result_persona_id.is_none());
}

#[tokio::test]
async fn update_persona_rolls_back_content_tip_status_and_bindings_when_append_fails() {
    let db = SqliteTestDb::new("update_persona_append_rollback");
    let service = sqlite_service(&db);
    let persona_id = create_active(&service, "active-append-rollback").await;
    let persona_before = service.get_persona(true, &persona_id).await.unwrap();
    let artifacts_before = persona_artifacts(&db, &persona_id);
    let conversation = service
        .chat_conversation_repo
        .create(ChatConversation::new_project(ProjectId::new()))
        .await
        .expect("conversation fixture should persist");
    service
        .chat_conversation_repo
        .update_persona_binding(&conversation.id, Some(persona_id.as_str()))
        .await
        .expect("persona binding fixture should persist");
    fail_persona_artifact_appends(&db);

    let error = service
        .update_persona(
            true,
            &persona_id,
            &persona_content("active-append-rollback", "Must roll back"),
        )
        .await
        .expect_err("artifact append failure must roll back active persona update");

    assert!(matches!(error, AppError::Database(_)));
    assert_eq!(
        service.get_persona(true, &persona_id).await.unwrap(),
        persona_before
    );
    assert_eq!(persona_artifacts(&db, &persona_id), artifacts_before);
    let conversation_after = service
        .chat_conversation_repo
        .get_by_id(&conversation.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        conversation_after.persona_id.as_deref(),
        Some(persona_id.as_str())
    );
    assert!(conversation_after.builder_draft_id.is_none());
    assert!(conversation_after.builder_result_persona_id.is_none());
}

#[tokio::test]
async fn approve_persona_plain_rolls_back_content_tip_status_and_bindings_when_append_fails() {
    let db = SqliteTestDb::new("approve_persona_plain_append_rollback");
    let service = sqlite_service(&db);
    let project_id = ProjectId::new();
    let mut builder = ChatConversation::new_project(project_id.clone());
    builder.agent_mode =
        Some(crate::domain::entities::AgentConversationWorkspaceMode::PersonaBuilder);
    let builder = service
        .chat_conversation_repo
        .create(builder)
        .await
        .expect("builder conversation fixture should persist");
    let mut input = draft_input("plain-approval-append-rollback", "Draft body");
    input.project_id = Some(project_id);
    let draft = service
        .create_bound_draft(true, &builder.id, input)
        .await
        .expect("bound plain draft should persist");
    let artifacts_before = persona_artifacts(&db, &draft.id);
    fail_persona_artifact_appends(&db);

    let error = service
        .approve_persona(true, &draft.id)
        .await
        .expect_err("artifact append failure must roll back plain approval");

    assert!(matches!(error, AppError::Database(_)));
    assert_eq!(service.get_draft(true, &draft.id).await.unwrap(), draft);
    assert_eq!(persona_artifacts(&db, &draft.id), artifacts_before);
    let builder_after = service
        .chat_conversation_repo
        .get_by_id(&builder.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        builder_after.builder_draft_id.as_deref(),
        Some(draft.id.as_str())
    );
    assert!(builder_after.builder_result_persona_id.is_none());
    assert!(builder_after.persona_id.is_none());
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
    let content_v2 = persona_content("approval-race", "Version two from another connection");
    let hash_v2 = expected_hash(&content_v2);
    db.new_connection()
        .execute(
            "UPDATE personas SET content = ?1, content_hash = ?2, version = version + 1 WHERE id = ?3",
            rusqlite::params![content_v2, hash_v2, draft.id.as_str()],
        )
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
    db.new_connection()
        .execute(
            "UPDATE personas SET content = ?1, content_hash = ?2, version = version + 1 WHERE id = ?3",
            rusqlite::params!["invalid markdown", "stale-hash", draft.id.as_str()],
        )
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
