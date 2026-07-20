#![cfg(test)]

use std::sync::Arc;

use super::persona_service_test_support::*;
use super::{PersonaService, PERSONA_UNAVAILABLE_PREFIX};
use crate::application::AppState;
use crate::domain::entities::{PersonaId, PersonaScopeFilter, PersonaStatus, ProjectId};
use crate::error::AppError;
use crate::infrastructure::sqlite::{
    DbConnection, SqliteChatConversationRepository, SqlitePersonaRepository,
};
use crate::testing::SqliteTestDb;

#[tokio::test]
async fn hard_delete_only_for_draft_status() {
    let service = memory_service();
    let draft = service
        .create_draft(true, draft_input("delete-draft", "Draft body"))
        .await
        .expect("draft should be created");
    service
        .hard_delete_draft(true, &draft.id)
        .await
        .expect("draft should delete");
    assert!(service.get_persona(true, &draft.id).await.is_err());

    let active_id = create_active(&service, "delete-active").await;
    assert!(service.hard_delete_draft(true, &active_id).await.is_err());
}

#[tokio::test]
async fn binding_validation_rejects_draft_and_archived_personas() {
    let service = memory_service();
    let draft = service
        .create_draft(true, draft_input("bindable", "Draft body"))
        .await
        .expect("draft should be created");
    let draft_error = service
        .ensure_bindable(
            true,
            &draft.id,
            &ProjectId::from_string("project-a".to_string()),
        )
        .await
        .expect_err("draft should not be bindable");
    assert!(draft_error
        .to_string()
        .starts_with(PERSONA_UNAVAILABLE_PREFIX));
    service
        .approve_persona(true, &draft.id)
        .await
        .expect("draft should approve");
    assert!(service
        .ensure_bindable(
            true,
            &draft.id,
            &ProjectId::from_string("project-a".to_string()),
        )
        .await
        .is_ok());
    service
        .persona_repo
        .set_status(&draft.id, PersonaStatus::Archived)
        .await
        .expect("test fixture should archive the row");
    assert!(matches!(
        service
            .ensure_bindable(
                true,
                &draft.id,
                &ProjectId::from_string("project-a".to_string()),
            )
            .await,
        Err(AppError::PersonaUnavailable(message)) if message.starts_with(PERSONA_UNAVAILABLE_PREFIX)
    ));
}

#[tokio::test]
async fn binding_validation_rejects_cross_project_but_accepts_global_and_same_project() {
    let service = memory_service();
    let global = create_active(&service, "global-bindable").await;
    assert!(service
        .ensure_bindable(
            true,
            &global,
            &ProjectId::from_string("project-a".to_string()),
        )
        .await
        .is_ok());

    let mut input = draft_input("scoped-bindable", "Scoped body");
    input.project_id = Some(ProjectId::from_string("project-a".to_string()));
    let scoped = service.create_draft(true, input).await.unwrap();
    service.approve_persona(true, &scoped.id).await.unwrap();
    assert!(service
        .ensure_bindable(
            true,
            &scoped.id,
            &ProjectId::from_string("project-a".to_string()),
        )
        .await
        .is_ok());
    let mismatch = service
        .ensure_bindable(
            true,
            &scoped.id,
            &ProjectId::from_string("project-b".to_string()),
        )
        .await
        .expect_err("cross-project persona must not bind");
    assert!(matches!(mismatch, AppError::PersonaUnavailable(_)));
}

#[tokio::test]
async fn all_lifecycle_entry_points_fail_closed_when_flag_off() {
    let service = memory_service();
    let id = PersonaId::new();
    let content = persona_content("disabled-persona", "Disabled body");

    assert_disabled(
        service
            .create_draft(false, draft_input("disabled-persona", "Body"))
            .await,
    );
    assert_disabled(service.update_draft(false, &id, &content, None).await);
    assert_disabled(service.get_draft(false, &id).await);
    assert_disabled(service.approve_persona(false, &id).await);
    assert_disabled(service.reseed_persona_draft(false, &id).await);
    assert_disabled(service.approve_persona_as_new(false, &id, None).await);
    assert_disabled(service.update_persona(false, &id, &content).await);
    assert_disabled(service.archive_persona(false, &id).await);
    assert_disabled(service.hard_delete_draft(false, &id).await);
    assert_disabled(service.list_personas(false, PersonaScopeFilter::All).await);
    assert_disabled(service.get_persona(false, &id).await);
    assert_disabled(
        service
            .ensure_bindable(false, &id, &ProjectId::from_string("project-a".to_string()))
            .await,
    );
}

#[tokio::test]
async fn personas_visible_across_dual_app_states() {
    let db = SqliteTestDb::new("persona-dual-app-state");
    let first_shared = db.shared_conn();
    let mut first_state = AppState::new_test();
    first_state.db = DbConnection::from_shared(Arc::clone(&first_shared));
    first_state.persona_repo = Arc::new(SqlitePersonaRepository::from_shared(Arc::clone(
        &first_shared,
    )));
    first_state.chat_conversation_repo =
        Arc::new(SqliteChatConversationRepository::from_shared(first_shared));
    let first = PersonaService::new(
        first_state.db.clone(),
        first_state.persona_repo.clone(),
        first_state.chat_conversation_repo.clone(),
    );
    let second_db = Arc::new(tokio::sync::Mutex::new(db.new_connection()));
    let mut second_state = AppState::new_test();
    second_state.db = DbConnection::from_shared(Arc::clone(&second_db));
    second_state.persona_repo =
        Arc::new(SqlitePersonaRepository::from_shared(Arc::clone(&second_db)));
    second_state.chat_conversation_repo =
        Arc::new(SqliteChatConversationRepository::from_shared(second_db));
    let second = PersonaService::new(
        second_state.db.clone(),
        second_state.persona_repo.clone(),
        second_state.chat_conversation_repo.clone(),
    );

    let created = first
        .create_draft(
            true,
            draft_input("shared-state", "Visible to both app states"),
        )
        .await
        .expect("first state should create draft");
    let observed = second
        .get_draft(true, &created.id)
        .await
        .expect("second state should observe the draft");

    assert_eq!(observed.id, created.id);
}
