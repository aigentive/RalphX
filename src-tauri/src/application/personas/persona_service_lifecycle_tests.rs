#![cfg(test)]

use super::persona_service_test_support::*;
use super::PERSONA_DRAFT_CONFLICT_CODE;
use crate::domain::entities::{ChatConversation, IdeationSessionId, PersonaStatus};
use crate::error::AppError;
use crate::infrastructure::sqlite::{
    sqlite_chat_conversation_repo::clear_persona_bindings_sync,
    sqlite_persona_repo::persona_set_status_sync,
};
use crate::testing::SqliteTestDb;

#[tokio::test]
async fn save_persona_draft_creates_draft_with_fresh_slug() {
    let service = memory_service();

    let draft = service
        .create_draft(true, draft_input("fresh-persona", "Draft body"))
        .await
        .expect("draft should be created");

    assert_eq!(draft.status, PersonaStatus::Draft);
    assert_eq!(draft.version, 1);
    assert_eq!(draft.source_session_id.as_deref(), Some("source-session"));
}

#[tokio::test]
async fn save_persona_draft_rejects_slug_collision_with_live_rows() {
    let service = memory_service();
    let draft = service
        .create_draft(true, draft_input("shared-persona", "Draft body"))
        .await
        .expect("draft should be created");
    let active_id = create_active(&service, "active-persona").await;
    service
        .persona_repo
        .set_status(&active_id, PersonaStatus::Archived)
        .await
        .expect("test fixture should archive the row");

    assert!(service
        .create_draft(true, draft_input("shared-persona", "Second body"))
        .await
        .is_err());
    service
        .approve_persona(true, &draft.id)
        .await
        .expect("draft should become active");
    assert!(service
        .create_draft(true, draft_input("shared-persona", "Third body"))
        .await
        .is_err());
    let reused = service
        .create_draft(
            true,
            draft_input("active-persona", "Archived slug is reusable"),
        )
        .await
        .expect("archived slug should be reusable");
    assert_eq!(reused.slug, "active-persona");
}

#[tokio::test]
async fn update_draft_with_matching_hash_updates_content_and_bumps_version() {
    let service = memory_service();
    let draft = service
        .create_draft(true, draft_input("draft-update", "First body"))
        .await
        .expect("draft should be created");
    let content = persona_content("draft-update", "Updated body");

    let updated = service
        .update_draft(true, &draft.id, &content, Some(&draft.content_hash))
        .await
        .expect("draft should update");

    assert_eq!(updated.version, 2);
    assert_eq!(updated.content, content);
}

#[tokio::test]
async fn update_draft_with_stale_hash_conflicts_without_changing_content() {
    let service = memory_service();
    let draft = service
        .create_draft(true, draft_input("draft-conflict", "First body"))
        .await
        .expect("draft should be created");
    let content = persona_content("draft-conflict", "Rejected body");

    let error = service
        .update_draft(true, &draft.id, &content, Some("stale-content-hash"))
        .await
        .expect_err("a stale editor must not overwrite the current draft");

    assert!(
        matches!(&error, AppError::PersonaDraftConflict { expected, actual }
        if expected == "stale-content-hash" && actual.as_str() == draft.content_hash)
    );
    assert!(error.to_string().starts_with(PERSONA_DRAFT_CONFLICT_CODE));
    assert_eq!(
        service.get_draft(true, &draft.id).await.unwrap(),
        draft,
        "CAS rejection must leave every persisted draft field unchanged"
    );
}

#[tokio::test]
async fn update_draft_without_expected_hash_preserves_agent_write_behavior() {
    let service = memory_service();
    let draft = service
        .create_draft(true, draft_input("agent-draft-update", "First body"))
        .await
        .expect("draft should be created");
    let content = persona_content("agent-draft-update", "Agent update");

    let updated = service
        .update_draft(true, &draft.id, &content, None)
        .await
        .expect("the existing agent path should update without a CAS hash");

    assert_eq!(updated.version, draft.version + 1);
    assert_eq!(updated.content, content);
}

#[tokio::test]
async fn save_persona_draft_cannot_touch_active_or_archived_rows() {
    let service = memory_service();
    let active_id = create_active(&service, "draft-guard").await;
    let content = persona_content("draft-guard", "Attempted change");

    assert!(service
        .update_draft(true, &active_id, &content, None)
        .await
        .is_err());
    service
        .persona_repo
        .set_status(&active_id, PersonaStatus::Archived)
        .await
        .expect("test fixture should archive the row");
    assert!(service
        .update_draft(true, &active_id, &content, None)
        .await
        .is_err());
}

#[tokio::test]
async fn approve_transitions_draft_to_active_and_recomputes_hash() {
    let db = SqliteTestDb::new("approve_recomputes_persona_hash");
    let service = sqlite_service(&db);
    let draft = service
        .create_draft(true, draft_input("approve-persona", "Approved body"))
        .await
        .expect("draft should be created");
    db.with_connection(|conn| {
        conn.execute(
            "UPDATE personas SET content_hash = ?1 WHERE id = ?2",
            rusqlite::params!["tampered-hash", draft.id.as_str()],
        )
        .expect("stored hash should be tampered for regression coverage");
    });

    let active = service
        .approve_persona(true, &draft.id)
        .await
        .expect("draft should approve");

    assert_eq!(active.status, PersonaStatus::Active);
    assert_ne!(active.content_hash, "tampered-hash");
    assert_eq!(
        active.content_hash,
        expected_hash(&persona_content("approve-persona", "Approved body"))
    );
}

#[tokio::test]
async fn update_persona_rejects_draft_and_archived() {
    let service = memory_service();
    let draft = service
        .create_draft(true, draft_input("active-only", "Draft body"))
        .await
        .expect("draft should be created");
    let content = persona_content("active-only", "Changed body");

    assert!(service
        .update_persona(true, &draft.id, &content)
        .await
        .is_err());
    service
        .approve_persona(true, &draft.id)
        .await
        .expect("draft should approve");
    service
        .persona_repo
        .set_status(&draft.id, PersonaStatus::Archived)
        .await
        .expect("test fixture should archive the row");
    assert!(service
        .update_persona(true, &draft.id, &content)
        .await
        .is_err());
}

#[tokio::test]
async fn update_persona_recomputes_hash_and_bumps_version_on_active() {
    let service = memory_service();
    let id = create_active(&service, "active-update").await;
    let content = persona_content("active-update", "Changed body");

    let updated = service
        .update_persona(true, &id, &content)
        .await
        .expect("active persona should update");

    assert_eq!(updated.version, 3);
    assert_ne!(updated.content_hash, "tampered-hash");
    assert_eq!(updated.content_hash, expected_hash(&content));
}

#[tokio::test]
async fn archive_clears_bindings_and_archives_in_one_transaction() {
    let db = SqliteTestDb::new("persona-service-archive");
    let service = sqlite_service(&db);
    let id = create_active(&service, "archive-persona").await;
    let bound_one = ChatConversation::new_ideation(IdeationSessionId::new());
    let bound_two = ChatConversation::new_ideation(IdeationSessionId::new());
    let unbound = ChatConversation::new_ideation(IdeationSessionId::new());
    for conversation in [&bound_one, &bound_two, &unbound] {
        service
            .chat_conversation_repo
            .create(conversation.clone())
            .await
            .expect("conversation should persist");
    }
    for conversation in [&bound_one, &bound_two] {
        service
            .chat_conversation_repo
            .update_persona_binding(&conversation.id, Some(id.as_str()))
            .await
            .expect("binding should persist");
    }
    db.with_connection(|conn| {
        conn.execute(
            "INSERT INTO manual_role_defaults (scope_type, scope_id, role, value_json)
             VALUES ('global', '', 'workspace_chat', json_object(
                 'harness', 'codex',
                 'serviceTier', 'provider_default',
                 'personaId', ?1
             ))",
            [id.as_str()],
        )
        .expect("manual role persona default should persist");
    });

    let archived = service
        .archive_persona(true, &id)
        .await
        .expect("archive should succeed");

    assert_eq!(archived.status, PersonaStatus::Archived);
    for conversation in [&bound_one, &bound_two, &unbound] {
        let loaded = service
            .chat_conversation_repo
            .get_by_id(&conversation.id)
            .await
            .expect("conversation lookup should succeed")
            .expect("conversation should exist");
        assert!(loaded.persona_id.is_none());
    }
    db.with_connection(|conn| {
        let remaining = conn
            .query_row("SELECT COUNT(*) FROM manual_role_defaults", [], |row| {
                row.get::<_, i64>(0)
            })
            .expect("manual role default count");
        assert_eq!(remaining, 0);
    });

    let rollback_id = create_active(&service, "archive-rollback").await;
    let rollback_conversation = ChatConversation::new_ideation(IdeationSessionId::new());
    service
        .chat_conversation_repo
        .create(rollback_conversation.clone())
        .await
        .expect("rollback conversation should persist");
    service
        .chat_conversation_repo
        .update_persona_binding(&rollback_conversation.id, Some(rollback_id.as_str()))
        .await
        .expect("rollback binding should persist");
    let rollback_id_value = rollback_id.as_str().to_string();
    let rollback = service
        .db
        .run_transaction(move |conn| {
            persona_set_status_sync(conn, &rollback_id_value, PersonaStatus::Archived)?;
            clear_persona_bindings_sync(conn, &rollback_id_value)?;
            Err::<(), AppError>(AppError::Validation("forced rollback".to_string()))
        })
        .await;
    assert!(rollback.is_err());
    assert_eq!(
        service
            .persona_repo
            .get_by_id(&rollback_id)
            .await
            .expect("persona lookup should succeed")
            .expect("persona should exist")
            .status,
        PersonaStatus::Active
    );
    assert_eq!(
        service
            .chat_conversation_repo
            .get_by_id(&rollback_conversation.id)
            .await
            .expect("conversation lookup should succeed")
            .expect("conversation should exist")
            .persona_id
            .as_deref(),
        Some(rollback_id.as_str())
    );
}
