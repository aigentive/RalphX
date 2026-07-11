use crate::domain::entities::{Persona, PersonaId, PersonaStatus};
use crate::domain::repositories::PersonaRepository;
use crate::error::AppError;
use crate::infrastructure::sqlite::SqlitePersonaRepository;
use crate::testing::SqliteTestDb;
use chrono::Utc;

fn setup_repo() -> (SqliteTestDb, SqlitePersonaRepository) {
    let db = SqliteTestDb::new("sqlite_persona_repo_tests");
    let repo = SqlitePersonaRepository::from_shared(db.shared_conn());
    (db, repo)
}

fn persona(slug: &str, status: PersonaStatus) -> Persona {
    let now = Utc::now();
    Persona {
        id: PersonaId::new(),
        slug: slug.to_string(),
        name: format!("{slug} persona"),
        description: "A focused test persona".to_string(),
        content: format!("---\nname: {slug}\nkind: persona\ndescription: Test\n---\nBody"),
        status,
        version: 1,
        content_hash: format!("hash-{slug}"),
        source_session_id: Some("session-1".to_string()),
        source_json: "{\"source\":\"test\"}".to_string(),
        created_at: now,
        updated_at: now,
    }
}

#[tokio::test]
async fn create_and_get_round_trips_all_columns() {
    let (_db, repo) = setup_repo();
    let expected = persona("reviewer", PersonaStatus::Draft);

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
    assert_eq!(actual.source_json, expected.source_json);
}

#[tokio::test]
async fn slug_unique_violation_maps_to_validation_error() {
    let (_db, repo) = setup_repo();
    repo.create(persona("reviewer", PersonaStatus::Active))
        .await
        .unwrap();

    let error = repo
        .create(persona("reviewer", PersonaStatus::Draft))
        .await
        .expect_err("live slug collision should fail");

    assert!(matches!(error, AppError::Validation(message) if message.contains("reviewer")));
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
async fn live_slug_collision_still_rejected() {
    let (_db, repo) = setup_repo();
    repo.create(persona("reviewer", PersonaStatus::Draft))
        .await
        .unwrap();

    assert!(matches!(
        repo.create(persona("reviewer", PersonaStatus::Active))
            .await,
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
async fn update_content_bumps_version_and_hash() {
    let (_db, repo) = setup_repo();
    let original = persona("reviewer", PersonaStatus::Draft);
    repo.create(original.clone()).await.unwrap();

    repo.update_content(&original.id, "new content", "new hash")
        .await
        .unwrap();
    let updated = repo.get_by_id(&original.id).await.unwrap().unwrap();

    assert_eq!(updated.content, "new content");
    assert_eq!(updated.content_hash, "new hash");
    assert_eq!(updated.version, 2);
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
