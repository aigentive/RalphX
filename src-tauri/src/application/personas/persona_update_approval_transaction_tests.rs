#![cfg(test)]

use super::persona_service_test_support::fail_persona_artifact_appends;
use super::persona_update_approval_test_support::*;
use super::SavePersonaDraftInput;
use crate::domain::entities::{PersonaStatus, ProjectId};
use crate::error::AppError;
use crate::testing::SqliteTestDb;

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
async fn seeded_approval_rolls_back_every_writer_when_artifact_append_fails() {
    let db = SqliteTestDb::new("seeded_approval_append_rollback");
    let (service, source, draft, conversations) =
        seeded_fixture(&db, "append-rollback-source").await;
    let source_tip = source.artifact_id.clone();
    db.shared_conn()
        .lock()
        .await
        .execute_batch(
            "CREATE TRIGGER fail_seeded_source_artifact_insert
             BEFORE INSERT ON artifacts WHEN NEW.type = 'persona'
             BEGIN SELECT RAISE(ABORT, 'forced source artifact failure'); END;",
        )
        .expect("append rollback trigger should install");

    let error = service
        .approve_persona(true, &draft.id)
        .await
        .expect_err("artifact append failure must roll back the seeded apply");

    assert!(matches!(error, AppError::Database(_)));
    let source_after = service.get_persona(true, &source.id).await.unwrap();
    assert_eq!(source_after.content, source.content);
    assert_eq!(source_after.artifact_id, source_tip);
    assert_eq!(service.get_draft(true, &draft.id).await.unwrap(), draft);
    assert_bindings(&service, &conversations, Some(draft.id.as_str())).await;
}

async fn assert_approve_as_new_append_rollback(
    db_name: &str,
    source_slug: &str,
    new_slug: Option<&str>,
) {
    let db = SqliteTestDb::new(db_name);
    let (service, source, draft, conversations) = seeded_fixture(&db, source_slug).await;
    let archived_source = service
        .archive_persona(true, &source.id)
        .await
        .expect("source fixture should archive");
    let draft_chain_before = chain_ids(&db, draft.artifact_id.as_ref().unwrap());
    fail_persona_artifact_appends(&db);

    let error = service
        .approve_persona_as_new(true, &draft.id, new_slug)
        .await
        .expect_err("artifact append failure must roll back approve-as-new");

    assert!(matches!(error, AppError::Database(_)));
    assert_eq!(
        service.get_draft(true, &draft.id).await.unwrap(),
        draft,
        "content, tip, status, slug, and provenance must roll back together"
    );
    assert_eq!(
        service.get_persona(true, &source.id).await.unwrap(),
        archived_source
    );
    assert_eq!(
        chain_ids(&db, draft.artifact_id.as_ref().unwrap()),
        draft_chain_before,
        "failed approval must not leave an appended artifact"
    );
    assert_pending_bindings(&service, &conversations, draft.id.as_str()).await;
}

#[tokio::test]
async fn approve_persona_as_new_same_slug_rolls_back_every_writer_when_append_fails() {
    assert_approve_as_new_append_rollback(
        "approve_as_new_same_slug_append_rollback",
        "approve-as-new-same-slug",
        None,
    )
    .await;
}

#[tokio::test]
async fn approve_persona_as_new_recompose_rolls_back_every_writer_when_append_fails() {
    assert_approve_as_new_append_rollback(
        "approve_as_new_recompose_append_rollback",
        "approve-as-new-recompose",
        Some("approve-as-new-renamed"),
    )
    .await;
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
async fn manual_seeded_draft_edits_preserve_source_freshness_in_both_directions() {
    let current_db = SqliteTestDb::new("manual_seeded_edit_current_source");
    let (current_service, current_source, current_draft, _) =
        seeded_fixture(&current_db, "manual-current-source").await;
    let current_content = persona_content(&current_source.slug, "Manual draft refinement");
    let current_edited = current_service
        .update_draft(
            true,
            &current_draft.id,
            &current_content,
            Some(&current_draft.content_hash),
        )
        .await
        .expect("manual editing must be allowed for seeded drafts");
    assert_eq!(
        current_edited.source_content_hash, current_draft.source_content_hash,
        "manual editing must not rewrite the seed baseline"
    );
    let applied = current_service
        .approve_persona(true, &current_draft.id)
        .await
        .expect("an unchanged source must not spuriously trip seed freshness");
    assert_eq!(applied.id, current_source.id);
    assert_eq!(applied.content, current_content);

    let stale_db = SqliteTestDb::new("manual_seeded_edit_stale_source");
    let (stale_service, stale_source, stale_draft, _) =
        seeded_fixture(&stale_db, "manual-stale-source").await;
    let stale_content = persona_content(&stale_source.slug, "Manual draft before source drift");
    let stale_edited = stale_service
        .update_draft(
            true,
            &stale_draft.id,
            &stale_content,
            Some(&stale_draft.content_hash),
        )
        .await
        .expect("manual seeded draft edit should succeed before source drift");
    stale_service
        .update_persona(
            true,
            &stale_source.id,
            &persona_content(&stale_source.slug, "Source changed independently"),
        )
        .await
        .expect("source fixture should change independently");

    let error = stale_service
        .approve_persona(true, &stale_draft.id)
        .await
        .expect_err("source drift must still block a manually edited seeded draft");
    assert!(matches!(error, AppError::Conflict(message)
        if message.starts_with("SourceChangedSinceSeed:")));
    assert_eq!(
        stale_service
            .get_draft(true, &stale_draft.id)
            .await
            .unwrap(),
        stale_edited,
        "failed freshness validation must preserve the manual draft edit"
    );
}
