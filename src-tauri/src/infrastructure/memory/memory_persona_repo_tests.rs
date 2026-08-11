use crate::domain::entities::{Persona, PersonaId, PersonaScopeFilter, PersonaStatus};
use crate::domain::repositories::PersonaRepository;
use crate::infrastructure::memory::MemoryPersonaRepository;
use chrono::{Duration, Utc};

fn persona(slug: &str, status: PersonaStatus) -> Persona {
    let now = Utc::now();
    Persona {
        id: PersonaId::new(),
        artifact_id: None,

        project_id: None,
        slug: slug.to_string(),
        name: slug.to_string(),
        description: String::new(),
        content: "content".to_string(),
        status,
        version: 1,
        content_hash: format!("hash-{slug}"),
        source_session_id: None,
        source_persona_id: None,
        source_content_hash: None,
        source_json: "{}".to_string(),
        created_at: now,
        updated_at: now,
    }
}

#[tokio::test]
async fn memory_persona_repo_round_trips_update_provenance() {
    let repo = MemoryPersonaRepository::new();
    let mut expected = persona("reviewer", PersonaStatus::Draft);
    expected.source_persona_id = Some(PersonaId::from("source-persona"));
    expected.source_content_hash = Some("source-hash".to_string());

    repo.create(expected.clone()).await.unwrap();
    let actual = repo.get_by_id(&expected.id).await.unwrap().unwrap();

    assert_eq!(actual.source_persona_id, expected.source_persona_id);
    assert_eq!(actual.source_content_hash, expected.source_content_hash);
}

#[tokio::test]
async fn memory_persona_repo_finds_active_slug_only_and_newest_seeded_draft() {
    let repo = MemoryPersonaRepository::new();
    let active = persona("reviewer", PersonaStatus::Active);
    let draft_same_slug = persona("reviewer", PersonaStatus::Draft);
    let mut older_seeded = persona("seeded", PersonaStatus::Draft);
    older_seeded.created_at -= Duration::seconds(2);
    older_seeded.source_persona_id = Some(active.id.clone());
    let mut newer_seeded = persona("seeded", PersonaStatus::Draft);
    newer_seeded.created_at -= Duration::seconds(1);
    newer_seeded.source_persona_id = Some(active.id.clone());

    repo.create(active.clone()).await.unwrap();
    repo.create(draft_same_slug).await.unwrap();
    repo.create(older_seeded).await.unwrap();
    repo.create(newer_seeded.clone()).await.unwrap();

    assert_eq!(
        repo.get_active_by_slug("reviewer", None)
            .await
            .unwrap()
            .unwrap()
            .id,
        active.id
    );
    assert!(repo
        .get_active_by_slug("seeded", None)
        .await
        .unwrap()
        .is_none());
    assert_eq!(
        repo.get_draft_by_source_persona_id(&active.id)
            .await
            .unwrap()
            .unwrap()
            .id,
        newer_seeded.id
    );
    assert!(repo
        .get_draft_by_source_persona_id(&PersonaId::from("missing-source"))
        .await
        .unwrap()
        .is_none());
}

#[tokio::test]
async fn memory_persona_repo_allows_drafts_but_rejects_duplicate_active_slugs() {
    let repo = MemoryPersonaRepository::new();
    let first_draft = persona("reviewer", PersonaStatus::Draft);
    let second_draft = persona("reviewer", PersonaStatus::Draft);
    repo.create(first_draft.clone()).await.unwrap();
    repo.create(second_draft).await.unwrap();
    repo.set_status(&first_draft.id, PersonaStatus::Active)
        .await
        .unwrap();

    let third_draft = persona("reviewer", PersonaStatus::Draft);
    repo.create(third_draft.clone()).await.unwrap();
    assert!(repo
        .set_status(&third_draft.id, PersonaStatus::Active)
        .await
        .is_err());
}

#[tokio::test]
async fn memory_persona_repo_trait_parity_create_and_get_by_id() {
    let repo = MemoryPersonaRepository::new();
    let expected = persona("reviewer", PersonaStatus::Draft);
    repo.create(expected.clone()).await.unwrap();
    assert_eq!(
        repo.get_by_id(&expected.id).await.unwrap().unwrap().id,
        expected.id
    );
}

#[tokio::test]
async fn memory_persona_repo_trait_parity_get_by_slug() {
    let repo = MemoryPersonaRepository::new();
    let expected = persona("reviewer", PersonaStatus::Draft);
    repo.create(expected.clone()).await.unwrap();
    assert_eq!(
        repo.get_by_slug("reviewer").await.unwrap().unwrap().id,
        expected.id
    );
}

#[tokio::test]
async fn memory_persona_repo_trait_parity_list() {
    let repo = MemoryPersonaRepository::new();
    let mut older = persona("one", PersonaStatus::Draft);
    older.created_at -= Duration::seconds(1);
    let newer = persona("two", PersonaStatus::Active);
    repo.create(older.clone()).await.unwrap();
    repo.create(newer.clone()).await.unwrap();

    let listed = repo.list(PersonaScopeFilter::All).await.unwrap();
    assert_eq!(listed.len(), 2);
    assert_eq!(listed[0].id, newer.id, "newest persona is listed first");
    assert_eq!(listed[1].id, older.id);
}

#[tokio::test]
async fn memory_persona_repo_trait_parity_list_by_status() {
    let repo = MemoryPersonaRepository::new();
    repo.create(persona("one", PersonaStatus::Draft))
        .await
        .unwrap();
    let active = persona("two", PersonaStatus::Active);
    repo.create(active.clone()).await.unwrap();
    assert_eq!(
        repo.list_by_status(PersonaStatus::Active).await.unwrap()[0].id,
        active.id
    );
}

#[tokio::test]
async fn memory_persona_repo_trait_parity_set_status() {
    let repo = MemoryPersonaRepository::new();
    let original = persona("reviewer", PersonaStatus::Draft);
    repo.create(original.clone()).await.unwrap();
    repo.set_status(&original.id, PersonaStatus::Archived)
        .await
        .unwrap();
    assert_eq!(
        repo.get_by_id(&original.id).await.unwrap().unwrap().status,
        PersonaStatus::Archived
    );
}

#[tokio::test]
async fn memory_persona_repo_trait_parity_delete() {
    let repo = MemoryPersonaRepository::new();
    let original = persona("reviewer", PersonaStatus::Draft);
    repo.create(original.clone()).await.unwrap();
    repo.delete(&original.id).await.unwrap();
    assert!(repo.get_by_id(&original.id).await.unwrap().is_none());
}

#[tokio::test]
async fn memory_persona_repo_trait_parity_get_by_slug_returns_none_when_missing() {
    let repo = MemoryPersonaRepository::new();
    assert!(repo.get_by_slug("missing").await.unwrap().is_none());
}

#[tokio::test]
async fn memory_persona_repo_reuses_archived_slug_and_selects_newest_match() {
    let repo = MemoryPersonaRepository::new();
    let mut archived = persona("reviewer", PersonaStatus::Archived);
    archived.created_at -= Duration::seconds(1);
    let replacement = persona("reviewer", PersonaStatus::Draft);
    repo.create(archived).await.unwrap();
    repo.create(replacement.clone()).await.unwrap();

    assert_eq!(
        repo.get_by_slug("reviewer").await.unwrap().unwrap().id,
        replacement.id
    );
}
