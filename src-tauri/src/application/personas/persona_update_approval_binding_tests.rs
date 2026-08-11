#![cfg(test)]

use super::persona_update_approval_test_support::*;
use super::SavePersonaDraftInput;
use crate::error::AppError;
use crate::testing::SqliteTestDb;

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
async fn draft_hard_delete_removes_its_entire_artifact_chain() {
    let db = SqliteTestDb::new("seeded_draft_delete_artifacts");
    let (service, _source, draft, _conversations) = seeded_fixture(&db, "delete-chain").await;
    let updated = service
        .update_draft_as_agent(
            true,
            &draft.id,
            &persona_content("delete-chain", "Second draft version"),
        )
        .await
        .unwrap();
    let ids = chain_ids(&db, updated.artifact_id.as_ref().unwrap());
    assert_eq!(ids.len(), 2);

    service.hard_delete_draft(true, &draft.id).await.unwrap();

    db.with_connection(|conn| {
        for id in ids {
            let count: i64 = conn
                .query_row(
                    "SELECT COUNT(*) FROM artifacts WHERE id = ?1",
                    [id],
                    |row| row.get(0),
                )
                .unwrap();
            assert_eq!(count, 0, "deleted draft chain must leave no artifact row");
        }
    });
}

#[tokio::test]
async fn approvals_finalize_bindings_and_preserve_archived_result_pointer() {
    let db = SqliteTestDb::new("uniform_persona_approval_bindings");
    let service = sqlite_service(&db);
    let plain_conversation = create_builder_conversation(&service).await;
    let plain = service
        .create_bound_draft(
            true,
            &plain_conversation.id,
            SavePersonaDraftInput {
                project_id: None,
                slug: "plain-result".to_string(),
                content: persona_content("plain-result", "Plain approval"),
                source_session_id: Some(plain_conversation.id.as_str().to_string()),
                source_persona_id: None,
                source_content_hash: None,
            },
        )
        .await
        .unwrap();
    let before_count = chain_ids(&db, plain.artifact_id.as_ref().unwrap()).len();
    let approved_plain = service.approve_persona(true, &plain.id).await.unwrap();
    assert_eq!(
        chain_ids(&db, approved_plain.artifact_id.as_ref().unwrap()).len(),
        before_count + 1,
        "plain approval must append exactly once"
    );
    assert_eq!(
        artifact_row(&db, approved_plain.artifact_id.as_ref().unwrap()).1,
        "user"
    );
    assert_finished_bindings(
        &service,
        std::slice::from_ref(&plain_conversation),
        plain.id.as_str(),
    )
    .await;

    service.archive_persona(true, &plain.id).await.unwrap();
    assert_finished_bindings(&service, &[plain_conversation], plain.id.as_str()).await;

    let (seeded_service, source, seeded, seeded_conversations) =
        seeded_fixture(&db, "seeded-result").await;
    let applied = seeded_service
        .approve_persona(true, &seeded.id)
        .await
        .unwrap();
    assert_finished_bindings(&seeded_service, &seeded_conversations, source.id.as_str()).await;
    assert_eq!(applied.id, source.id);

    let (as_new_service, source, as_new, as_new_conversations) =
        seeded_fixture(&db, "as-new-result").await;
    as_new_service
        .archive_persona(true, &source.id)
        .await
        .unwrap();
    let approved_new = as_new_service
        .approve_persona_as_new(true, &as_new.id, Some("as-new-renamed"))
        .await
        .unwrap();
    assert_finished_bindings(
        &as_new_service,
        &as_new_conversations,
        approved_new.id.as_str(),
    )
    .await;
    assert_eq!(
        artifact_row(&db, approved_new.artifact_id.as_ref().unwrap()).1,
        "system"
    );
}

#[tokio::test]
async fn seeded_apply_grafts_source_history_and_leaves_draft_history_orphaned() {
    let db = SqliteTestDb::new("seeded_graft_shape");
    let (service, source, draft, conversations) = seeded_fixture(&db, "graft-source").await;
    let source_tip = source.artifact_id.clone().unwrap();
    let draft = service
        .update_draft_as_agent(
            true,
            &draft.id,
            &persona_content("graft-source", "Final graft content"),
        )
        .await
        .unwrap();
    let draft_tip = draft.artifact_id.clone().unwrap();
    let draft_chain = chain_ids(&db, &draft_tip);

    let applied = service.approve_persona(true, &draft.id).await.unwrap();

    let applied_tip = applied.artifact_id.as_ref().unwrap();
    let (parent, created_by, metadata) = artifact_row(&db, applied_tip);
    assert_eq!(parent.as_deref(), Some(source_tip.as_str()));
    assert_eq!(created_by, "agent");
    assert_eq!(metadata["source_draft_id"], draft.id.as_str());
    assert_eq!(metadata["draft_tip_artifact_id"], draft_tip.as_str());
    let source_history = chain_ids(&db, applied_tip);
    assert!(source_history.contains(&source_tip.to_string()));
    assert!(draft_chain.iter().all(|id| !source_history.contains(id)));
    db.with_connection(|conn| {
        for id in &draft_chain {
            let count: i64 = conn
                .query_row(
                    "SELECT COUNT(*) FROM artifacts WHERE id = ?1",
                    [id],
                    |row| row.get(0),
                )
                .unwrap();
            assert_eq!(
                count, 1,
                "graft metadata must point to recoverable orphan rows"
            );
        }
    });
    assert_finished_bindings(&service, &conversations, source.id.as_str()).await;
}
