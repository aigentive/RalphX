use crate::domain::entities::{Persona, PersonaId, PersonaStatus};
use crate::domain::repositories::PersonaRepository;
use crate::infrastructure::memory::MemoryPersonaRepository;
use chrono::Utc;

fn persona(slug: &str, status: PersonaStatus) -> Persona {
    let now = Utc::now();
    Persona {
        id: PersonaId::new(),
        slug: slug.to_string(),
        name: slug.to_string(),
        description: String::new(),
        content: "content".to_string(),
        status,
        version: 1,
        content_hash: format!("hash-{slug}"),
        source_session_id: None,
        source_json: "{}".to_string(),
        created_at: now,
        updated_at: now,
    }
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
    repo.create(persona("one", PersonaStatus::Draft))
        .await
        .unwrap();
    repo.create(persona("two", PersonaStatus::Active))
        .await
        .unwrap();
    assert_eq!(repo.list().await.unwrap().len(), 2);
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
async fn memory_persona_repo_trait_parity_update_content() {
    let repo = MemoryPersonaRepository::new();
    let original = persona("reviewer", PersonaStatus::Draft);
    repo.create(original.clone()).await.unwrap();
    repo.update_content(&original.id, "new content", "new hash")
        .await
        .unwrap();
    let updated = repo.get_by_id(&original.id).await.unwrap().unwrap();
    assert_eq!(
        (
            updated.content.as_str(),
            updated.content_hash.as_str(),
            updated.version
        ),
        ("new content", "new hash", 2)
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
