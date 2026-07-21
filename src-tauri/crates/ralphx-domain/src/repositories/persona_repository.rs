use async_trait::async_trait;

use crate::entities::{Persona, PersonaId, PersonaScopeFilter, PersonaStatus};
use crate::error::AppResult;

#[async_trait]
pub trait PersonaRepository: Send + Sync {
    async fn create(&self, persona: Persona) -> AppResult<Persona>;

    async fn get_by_id(&self, id: &PersonaId) -> AppResult<Option<Persona>>;

    async fn get_by_slug(&self, slug: &str) -> AppResult<Option<Persona>>;

    async fn get_active_by_slug(
        &self,
        slug: &str,
        project_id: Option<&crate::entities::ProjectId>,
    ) -> AppResult<Option<Persona>>;

    async fn get_draft_by_source_persona_id(
        &self,
        source_persona_id: &PersonaId,
    ) -> AppResult<Option<Persona>> {
        let _ = source_persona_id;
        Ok(None)
    }

    async fn list(&self, scope: PersonaScopeFilter) -> AppResult<Vec<Persona>>;

    async fn list_by_status(&self, status: PersonaStatus) -> AppResult<Vec<Persona>>;

    async fn set_status(&self, id: &PersonaId, status: PersonaStatus) -> AppResult<()>;

    /// Delete a persona. The caller enforces the draft-only policy.
    async fn delete(&self, id: &PersonaId) -> AppResult<()>;
}
