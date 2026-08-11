#![cfg(test)]

use super::persona_service_test_support::*;
use crate::domain::entities::{ChatConversation, PersonaStatus, ProjectId};
use crate::error::AppError;
use crate::testing::SqliteTestDb;

fn bind_conversation(db: &SqliteTestDb, persona_id: &str) -> String {
    let project = db.seed_project("persona-usage-project");
    let conversation = db.insert_conversation(ChatConversation::new_project(
        ProjectId::from_string(project.id.as_str().to_string()),
    ));
    db.with_connection(|conn| {
        conn.execute(
            "UPDATE chat_conversations SET persona_id = ?1 WHERE id = ?2",
            rusqlite::params![persona_id, conversation.id.as_str()],
        )
        .expect("bind conversation to persona");
    });
    conversation.id.as_str().to_string()
}

fn seed_agent_run(db: &SqliteTestDb, conversation_id: &str, persona_id: &str, started_at: &str) {
    db.with_connection(|conn| {
        conn.execute(
            "INSERT INTO agent_runs (id, conversation_id, status, started_at, persona_id)
             VALUES (?1, ?2, 'completed', ?3, ?4)",
            rusqlite::params![
                uuid::Uuid::new_v4().to_string(),
                conversation_id,
                started_at,
                persona_id
            ],
        )
        .expect("seed persona-attributed agent run");
    });
}

#[tokio::test]
async fn unarchive_restores_archived_persona_without_rebinding_conversations() {
    let db = SqliteTestDb::new("persona-unarchive-restore");
    let service = sqlite_service(&db);
    let id = create_active(&service, "restore-me").await;
    let conversation_id = bind_conversation(&db, id.as_str());

    service
        .archive_persona(true, &id)
        .await
        .expect("archive should succeed");
    let restored = service
        .unarchive_persona(true, &id)
        .await
        .expect("restore should succeed");

    assert_eq!(restored.status, PersonaStatus::Active);
    let rebound: Option<String> = db.with_connection(|conn| {
        conn.query_row(
            "SELECT persona_id FROM chat_conversations WHERE id = ?1",
            [conversation_id.as_str()],
            |row| row.get(0),
        )
        .expect("conversation row should exist")
    });
    assert_eq!(
        rebound, None,
        "restore must never resurrect cleared conversation bindings"
    );
}

#[tokio::test]
async fn unarchive_rejects_non_archived_personas() {
    let service = memory_service();
    let active_id = create_active(&service, "still-active").await;
    let draft = service
        .create_draft(true, draft_input("still-draft", "Body"))
        .await
        .expect("draft should be created");

    for id in [&active_id, &draft.id] {
        let error = service
            .unarchive_persona(true, id)
            .await
            .expect_err("only archived personas can be restored");
        assert!(matches!(error, AppError::Validation(_)));
    }
}

#[tokio::test]
async fn unarchive_rejects_active_slug_collision_in_same_scope() {
    let service = memory_service();
    let first = create_active(&service, "shared-slug").await;
    service
        .archive_persona(true, &first)
        .await
        .expect("archive should succeed");
    let second = create_active(&service, "shared-slug").await;

    let error = service
        .unarchive_persona(true, &first)
        .await
        .expect_err("restore must not create a second active persona per slug/scope");
    assert!(
        matches!(&error, AppError::Validation(message) if message.contains("shared-slug")),
        "collision error should name the slug: {error}"
    );
    // The already-active persona and the archived one are both untouched.
    assert_eq!(
        service.get_persona(true, &second).await.unwrap().status,
        PersonaStatus::Active
    );
    assert_eq!(
        service.get_persona(true, &first).await.unwrap().status,
        PersonaStatus::Archived
    );
}

#[tokio::test]
async fn unarchive_requires_feature_flag() {
    let service = memory_service();
    let id = create_active(&service, "flag-gated").await;
    service
        .archive_persona(true, &id)
        .await
        .expect("archive should succeed");

    assert_disabled(service.unarchive_persona(false, &id).await);
}

#[tokio::test]
async fn list_persona_usage_derives_bound_count_and_last_run() {
    let db = SqliteTestDb::new("persona-usage-derived");
    let service = sqlite_service(&db);
    let used = create_active(&service, "used-persona").await;
    let unused = create_active(&service, "unused-persona").await;
    let first_conversation = bind_conversation(&db, used.as_str());
    let second_conversation = bind_conversation(&db, used.as_str());
    seed_agent_run(
        &db,
        &first_conversation,
        used.as_str(),
        "2026-07-20T10:00:00+00:00",
    );
    seed_agent_run(
        &db,
        &second_conversation,
        used.as_str(),
        "2026-07-21T09:30:00+00:00",
    );

    let usage = service
        .list_persona_usage(true)
        .await
        .expect("usage query should succeed");

    let used_row = usage
        .iter()
        .find(|row| row.persona_id == used.as_str())
        .expect("used persona should be reported");
    assert_eq!(used_row.bound_conversation_count, 2);
    assert_eq!(
        used_row.last_run_at.as_deref(),
        Some("2026-07-21T09:30:00+00:00")
    );

    let unused_row = usage
        .iter()
        .find(|row| row.persona_id == unused.as_str())
        .expect("unused persona should still be reported");
    assert_eq!(unused_row.bound_conversation_count, 0);
    assert_eq!(unused_row.last_run_at, None);
}

#[tokio::test]
async fn list_persona_usage_requires_feature_flag() {
    let service = memory_service();
    assert_disabled(service.list_persona_usage(false).await);
}
