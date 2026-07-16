#![cfg(test)]

use std::sync::Arc;

use ralphx_domain::personas::validation::compute_content_hash;

use super::{PersonaService, SavePersonaDraftInput, PERSONA_UNAVAILABLE_PREFIX};
use crate::application::AppState;
use crate::domain::entities::{
    ChatConversation, ChatConversationId, IdeationSessionId, PersonaId, PersonaScopeFilter,
    PersonaStatus, ProjectId,
};
use crate::error::AppError;
use crate::infrastructure::sqlite::{
    sqlite_chat_conversation_repo::clear_persona_bindings_sync,
    sqlite_persona_repo::persona_set_status_sync,
};
use crate::infrastructure::sqlite::{
    DbConnection, SqliteChatConversationRepository, SqlitePersonaRepository,
};
use crate::testing::SqliteTestDb;

fn persona_content(slug: &str, body: &str) -> String {
    format!("---\nname: {slug}\nkind: persona\ndescription: Test persona\n---\n{body}")
}

/// Expected hash derived through the shared parser, so tests never
/// hand-replicate `split_frontmatter`'s exact frontmatter/body boundaries.
fn expected_hash(content: &str) -> String {
    let (frontmatter, body) = ralphx_domain::personas::skill_markdown::split_frontmatter(content)
        .expect("test persona content should carry frontmatter");
    compute_content_hash(frontmatter, body)
}

fn memory_service() -> PersonaService {
    let state = AppState::new_test();
    PersonaService::new(state.db, state.persona_repo, state.chat_conversation_repo)
}

fn sqlite_service(db: &SqliteTestDb) -> PersonaService {
    let shared = db.shared_conn();
    PersonaService::new(
        DbConnection::from_shared(Arc::clone(&shared)),
        Arc::new(SqlitePersonaRepository::from_shared(Arc::clone(&shared))),
        Arc::new(SqliteChatConversationRepository::from_shared(shared)),
    )
}

fn draft_input(slug: &str, body: &str) -> SavePersonaDraftInput {
    SavePersonaDraftInput {
        project_id: None,
        slug: slug.to_string(),
        content: persona_content(slug, body),
        source_session_id: Some("source-session".to_string()),
        source_persona_id: None,
        source_content_hash: None,
    }
}

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

async fn create_active(service: &PersonaService, slug: &str) -> PersonaId {
    let draft = service
        .create_draft(true, draft_input(slug, "Initial body"))
        .await
        .expect("draft should be created");
    service
        .approve_persona(true, &draft.id)
        .await
        .expect("draft should be approved");
    draft.id
}

fn assert_disabled(result: Result<impl std::fmt::Debug, AppError>) {
    assert!(matches!(result, Err(AppError::FeatureDisabled(_))));
}

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
async fn save_persona_draft_updates_own_draft_and_bumps_version() {
    let service = memory_service();
    let draft = service
        .create_draft(true, draft_input("draft-update", "First body"))
        .await
        .expect("draft should be created");
    let content = persona_content("draft-update", "Updated body");

    let updated = service
        .update_draft(true, &draft.id, &content)
        .await
        .expect("draft should update");

    assert_eq!(updated.version, 2);
    assert_eq!(updated.content, content);
}

#[tokio::test]
async fn save_persona_draft_cannot_touch_active_or_archived_rows() {
    let service = memory_service();
    let active_id = create_active(&service, "draft-guard").await;
    let content = persona_content("draft-guard", "Attempted change");

    assert!(service
        .update_draft(true, &active_id, &content)
        .await
        .is_err());
    service
        .persona_repo
        .set_status(&active_id, PersonaStatus::Archived)
        .await
        .expect("test fixture should archive the row");
    assert!(service
        .update_draft(true, &active_id, &content)
        .await
        .is_err());
}

#[tokio::test]
async fn approve_transitions_draft_to_active_and_recomputes_hash() {
    let service = memory_service();
    let draft = service
        .create_draft(true, draft_input("approve-persona", "Approved body"))
        .await
        .expect("draft should be created");
    service
        .persona_repo
        .update_content(&draft.id, &draft.content, "tampered-hash")
        .await
        .expect("stored hash should be tampered for regression coverage");

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
    assert_disabled(service.update_draft(false, &id, &content).await);
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
