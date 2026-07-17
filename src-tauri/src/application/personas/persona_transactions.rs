use super::{ensure_enabled, PersonaService, SavePersonaDraftInput};
use crate::domain::entities::{ChatConversationId, Persona, PersonaId, PersonaStatus};
use crate::error::AppResult;
use crate::infrastructure::sqlite::sqlite_chat_conversation_repo::{
    clear_persona_bindings_sync, update_builder_draft_binding_sync,
};
use crate::infrastructure::sqlite::sqlite_persona_repo::{
    map_live_slug_unique_error, persona_create_sync, persona_set_status_sync,
};

impl PersonaService {
    pub async fn create_bound_draft(
        &self,
        feature_enabled: bool,
        conversation_id: &ChatConversationId,
        input: SavePersonaDraftInput,
    ) -> AppResult<Persona> {
        let persona = self.build_draft(feature_enabled, input).await?;
        let collision_slug = persona.slug.clone();
        let conversation_id = conversation_id.as_str();
        let draft_id = persona.id.to_string();
        self.db
            .run_transaction(move |conn| {
                let persona = persona_create_sync(conn, persona)?;
                update_builder_draft_binding_sync(conn, &conversation_id, Some(&draft_id))?;
                Ok(persona)
            })
            .await
            .map_err(|error| map_live_slug_unique_error(error, &collision_slug))
    }

    pub async fn archive_persona(
        &self,
        feature_enabled: bool,
        id: &PersonaId,
    ) -> AppResult<Persona> {
        ensure_enabled(feature_enabled)?;
        self.require_status(id, PersonaStatus::Active).await?;
        // Repository ownership stays explicit even though this SQLite-only operation
        // uses the sync helper to keep both writes under one transaction lock.
        let _ = &self.chat_conversation_repo;
        let id_value = id.as_str().to_string();
        // This is intentionally one transaction; do not call async repositories here.
        self.db
            .run_transaction(move |conn| {
                persona_set_status_sync(conn, &id_value, PersonaStatus::Archived)?;
                clear_persona_bindings_sync(conn, &id_value)?;
                Ok(())
            })
            .await?;
        self.get_persona(feature_enabled, id).await
    }
}
