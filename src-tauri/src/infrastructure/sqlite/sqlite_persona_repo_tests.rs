use crate::domain::entities::{Persona, PersonaId, PersonaScopeFilter, PersonaStatus};
use crate::domain::repositories::PersonaRepository;
use crate::error::AppError;
use crate::infrastructure::sqlite::SqlitePersonaRepository;
use crate::testing::SqliteTestDb;
use chrono::{Duration, Utc};

fn project_id(value: &str) -> crate::domain::entities::ProjectId {
    crate::domain::entities::ProjectId::from_string(value.to_string())
}

fn setup_repo() -> (SqliteTestDb, SqlitePersonaRepository) {
    let db = SqliteTestDb::new("sqlite_persona_repo_tests");
    let repo = SqlitePersonaRepository::from_shared(db.shared_conn());
    (db, repo)
}

fn persona(slug: &str, status: PersonaStatus) -> Persona {
    let now = Utc::now();
    Persona {
        id: PersonaId::new(),
        artifact_id: None,

        project_id: None,
        slug: slug.to_string(),
        name: format!("{slug} persona"),
        description: "A focused test persona".to_string(),
        content: format!("---\nname: {slug}\nkind: persona\ndescription: Test\n---\nBody"),
        status,
        version: 1,
        content_hash: format!("hash-{slug}"),
        source_session_id: Some("session-1".to_string()),
        source_persona_id: None,
        source_content_hash: None,
        source_json: "{\"source\":\"test\"}".to_string(),
        created_at: now,
        updated_at: now,
    }
}

#[tokio::test]
async fn create_and_get_round_trips_all_columns() {
    let (_db, repo) = setup_repo();
    let source = persona("source-reviewer", PersonaStatus::Active);
    repo.create(source.clone()).await.unwrap();
    let mut expected = persona("reviewer", PersonaStatus::Draft);
    expected.source_persona_id = Some(source.id);
    expected.source_content_hash = Some("source-content-hash".to_string());

    repo.create(expected.clone()).await.unwrap();
    let actual = repo.get_by_id(&expected.id).await.unwrap().unwrap();

    assert_eq!(actual.id, expected.id);
    assert_eq!(actual.slug, expected.slug);
    assert_eq!(actual.name, expected.name);
    assert_eq!(actual.description, expected.description);
    assert_eq!(actual.content, expected.content);
    assert_eq!(actual.status, expected.status);
    assert_eq!(actual.version, expected.version);
    assert_eq!(actual.content_hash, expected.content_hash);
    assert_eq!(actual.source_session_id, expected.source_session_id);
    assert_eq!(actual.source_persona_id, expected.source_persona_id);
    assert_eq!(actual.source_content_hash, expected.source_content_hash);
    assert_eq!(actual.source_json, expected.source_json);
}

#[tokio::test]
async fn active_slug_unique_violation_maps_to_validation_error() {
    let (_db, repo) = setup_repo();
    repo.create(persona("reviewer", PersonaStatus::Active))
        .await
        .unwrap();

    let error = repo
        .create(persona("reviewer", PersonaStatus::Active))
        .await
        .expect_err("active slug collision should fail");

    assert!(matches!(error, AppError::Validation(message) if message.contains("reviewer")));
}

#[tokio::test]
async fn drafts_may_share_an_active_slug_at_the_repository_boundary() {
    let (_db, repo) = setup_repo();
    repo.create(persona("reviewer", PersonaStatus::Active))
        .await
        .unwrap();

    repo.create(persona("reviewer", PersonaStatus::Draft))
        .await
        .expect("seeded draft may share an active slug");
    repo.create(persona("reviewer", PersonaStatus::Draft))
        .await
        .expect("multiple seeded drafts may coexist");
}

#[tokio::test]
async fn archived_slug_is_reusable_by_new_persona() {
    let (_db, repo) = setup_repo();
    let archived = persona("reviewer", PersonaStatus::Archived);
    repo.create(archived).await.unwrap();

    let replacement = persona("reviewer", PersonaStatus::Draft);
    repo.create(replacement.clone()).await.unwrap();

    assert_eq!(
        repo.get_by_slug("reviewer").await.unwrap().unwrap().id,
        replacement.id
    );
}

#[tokio::test]
async fn promoting_a_draft_rejects_an_existing_active_slug() {
    let (_db, repo) = setup_repo();
    repo.create(persona("reviewer", PersonaStatus::Draft))
        .await
        .unwrap();
    let draft = persona("reviewer", PersonaStatus::Draft);
    repo.create(draft.clone()).await.unwrap();
    let active = persona("reviewer", PersonaStatus::Active);
    repo.create(active).await.unwrap();

    assert!(matches!(
        repo.set_status(&draft.id, PersonaStatus::Active).await,
        Err(AppError::Validation(_))
    ));
}

#[tokio::test]
async fn list_by_status_filters() {
    let (_db, repo) = setup_repo();
    let active = persona("active", PersonaStatus::Active);
    repo.create(active.clone()).await.unwrap();
    repo.create(persona("draft", PersonaStatus::Draft))
        .await
        .unwrap();

    let listed = repo.list_by_status(PersonaStatus::Active).await.unwrap();

    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].id, active.id);
}

#[tokio::test]
async fn list_and_get_by_slug_use_newest_persona_and_missing_ids_are_none() {
    let (_db, repo) = setup_repo();
    let mut older = persona("older", PersonaStatus::Draft);
    older.created_at -= Duration::seconds(1);
    let newer = persona("newer", PersonaStatus::Draft);
    repo.create(older.clone()).await.unwrap();
    repo.create(newer.clone()).await.unwrap();

    let listed = repo.list(PersonaScopeFilter::All).await.unwrap();
    assert_eq!(
        listed.iter().map(|persona| &persona.id).collect::<Vec<_>>(),
        vec![&newer.id, &older.id]
    );
    assert!(repo
        .get_by_id(&PersonaId::from("missing-persona"))
        .await
        .unwrap()
        .is_none());
    assert!(repo.get_by_slug("missing").await.unwrap().is_none());
}

#[tokio::test]
async fn set_status_transitions_row() {
    let (_db, repo) = setup_repo();
    let original = persona("reviewer", PersonaStatus::Draft);
    repo.create(original.clone()).await.unwrap();

    repo.set_status(&original.id, PersonaStatus::Active)
        .await
        .unwrap();

    assert_eq!(
        repo.get_by_id(&original.id).await.unwrap().unwrap().status,
        PersonaStatus::Active
    );
}

#[tokio::test]
async fn delete_removes_persona_from_future_reads() {
    let (_db, repo) = setup_repo();
    let original = persona("delete-me", PersonaStatus::Draft);
    repo.create(original.clone()).await.unwrap();

    repo.delete(&original.id).await.unwrap();

    assert!(repo.get_by_id(&original.id).await.unwrap().is_none());
    assert!(repo.get_by_slug("delete-me").await.unwrap().is_none());
}

#[tokio::test]
async fn scoped_filters_return_all_global_or_global_plus_selected_project() {
    let (_db, repo) = setup_repo();
    let global = repo
        .create(persona("global", PersonaStatus::Active))
        .await
        .unwrap();
    let mut project_a = persona("project-a-persona", PersonaStatus::Active);
    project_a.project_id = Some(project_id("project-a"));
    let project_a = repo.create(project_a).await.unwrap();
    let mut project_b = persona("project-b-persona", PersonaStatus::Active);
    project_b.project_id = Some(project_id("project-b"));
    let project_b = repo.create(project_b).await.unwrap();

    assert_eq!(repo.list(PersonaScopeFilter::All).await.unwrap().len(), 3);
    let globals = repo.list(PersonaScopeFilter::GlobalOnly).await.unwrap();
    assert_eq!(globals.len(), 1);
    assert_eq!(globals[0].id, global.id);
    let scoped = repo
        .list(PersonaScopeFilter::GlobalAndProject(project_id(
            "project-a",
        )))
        .await
        .unwrap();
    assert_eq!(scoped.len(), 2);
    assert!(scoped.iter().any(|value| value.id == global.id));
    assert!(scoped.iter().any(|value| value.id == project_a.id));
    assert!(!scoped.iter().any(|value| value.id == project_b.id));
}

#[tokio::test]
async fn active_slug_uniqueness_is_scoped_and_maps_friendly_errors_in_both_scopes() {
    let (_db, repo) = setup_repo();
    repo.create(persona("shared", PersonaStatus::Active))
        .await
        .expect("global persona should create");
    let mut project = persona("shared", PersonaStatus::Active);
    project.project_id = Some(project_id("project-a"));
    repo.create(project)
        .await
        .expect("same slug in project scope should create");

    let global_error = repo
        .create(persona("shared", PersonaStatus::Active))
        .await
        .expect_err("duplicate global slug should fail");
    assert!(
        matches!(global_error, AppError::Validation(message) if message == "Persona slug `shared` is already in use")
    );

    let mut project_duplicate = persona("shared", PersonaStatus::Active);
    project_duplicate.project_id = Some(project_id("project-a"));
    let project_error = repo
        .create(project_duplicate)
        .await
        .expect_err("duplicate project slug should fail");
    assert!(
        matches!(project_error, AppError::Validation(message) if message == "Persona slug `shared` is already in use")
    );
}
