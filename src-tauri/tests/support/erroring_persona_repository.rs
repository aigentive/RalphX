use async_trait::async_trait;

use ralphx_lib::domain::entities::{
    Persona, PersonaId, PersonaScopeFilter, PersonaStatus, ProjectId,
};
use ralphx_lib::domain::repositories::PersonaRepository;
use ralphx_lib::error::{AppError, AppResult};

pub struct ErroringPersonaRepository;

fn repository_error<T>() -> AppResult<T> {
    Err(AppError::Infrastructure(
        "persona repository exploded".to_string(),
    ))
}

#[async_trait]
impl PersonaRepository for ErroringPersonaRepository {
    async fn create(&self, _persona: Persona) -> AppResult<Persona> {
        repository_error()
    }

    async fn get_by_id(&self, _id: &PersonaId) -> AppResult<Option<Persona>> {
        repository_error()
    }

    async fn get_by_slug(&self, _slug: &str) -> AppResult<Option<Persona>> {
        repository_error()
    }

    async fn get_active_by_slug(
        &self,
        _slug: &str,
        _project_id: Option<&ProjectId>,
    ) -> AppResult<Option<Persona>> {
        repository_error()
    }

    async fn list(&self, _scope: PersonaScopeFilter) -> AppResult<Vec<Persona>> {
        repository_error()
    }

    async fn list_by_status(&self, _status: PersonaStatus) -> AppResult<Vec<Persona>> {
        repository_error()
    }

    async fn set_status(&self, _id: &PersonaId, _status: PersonaStatus) -> AppResult<()> {
        repository_error()
    }

    async fn delete(&self, _id: &PersonaId) -> AppResult<()> {
        repository_error()
    }
}
