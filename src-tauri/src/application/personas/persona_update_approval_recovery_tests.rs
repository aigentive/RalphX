#![cfg(test)]

use super::persona_update_approval_test_support::*;
use super::SavePersonaDraftInput;
use crate::domain::entities::PersonaStatus;
use crate::error::AppError;
use crate::testing::SqliteTestDb;

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
