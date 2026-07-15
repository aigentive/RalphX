use async_trait::async_trait;

use crate::entities::{Persona, PersonaId, PersonaStatus};
use crate::error::AppResult;

#[async_trait]
pub trait PersonaRepository: Send + Sync {
    async fn create(&self, persona: Persona) -> AppResult<Persona>;

    async fn get_by_id(&self, id: &PersonaId) -> AppResult<Option<Persona>>;

    async fn get_by_slug(&self, slug: &str) -> AppResult<Option<Persona>>;

    async fn get_active_by_slug(&self, slug: &str) -> AppResult<Option<Persona>>;

    async fn list(&self) -> AppResult<Vec<Persona>>;

    async fn list_by_status(&self, status: PersonaStatus) -> AppResult<Vec<Persona>>;

    async fn update_content(
        &self,
        id: &PersonaId,
        content: &str,
        content_hash: &str,
    ) -> AppResult<()>;

    async fn set_status(&self, id: &PersonaId, status: PersonaStatus) -> AppResult<()>;

    /// Delete a persona. The caller enforces the draft-only policy.
    async fn delete(&self, id: &PersonaId) -> AppResult<()>;
}
