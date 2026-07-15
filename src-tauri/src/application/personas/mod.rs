use std::sync::Arc;

use chrono::Utc;
use ralphx_domain::personas::validation::validate_persona_content;
use serde_json::{json, Value};

use crate::domain::entities::{ChatConversationId, Persona, PersonaId, PersonaStatus};
use crate::domain::repositories::{ChatConversationRepository, PersonaRepository};
use crate::error::{AppError, AppResult};
use crate::infrastructure::sqlite::sqlite_chat_conversation_repo::{
    clear_persona_bindings_sync, update_builder_draft_binding_sync,
};
use crate::infrastructure::sqlite::sqlite_persona_repo::{
    map_live_slug_unique_error, persona_create_sync, persona_set_status_sync,
};
use crate::infrastructure::sqlite::DbConnection;

mod persona_update_approval;

pub use persona_update_approval::draft_applied_payload;

/// Prefix used by every caller that reports a persona that cannot be bound.
pub const PERSONA_UNAVAILABLE_PREFIX: &str = "[Persona unavailable:";
/// Prefix used by every persona surface when the feature is disabled.
pub const PERSONA_FEATURE_DISABLED_PREFIX: &str = "[Personas disabled:";

/// The draft-update event contract intentionally excludes persona content/body.
pub fn draft_updated_payload(persona: &Persona) -> Value {
    json!({
        "draft_id": persona.id.as_str(),
        "version": persona.version,
        "content_hash": persona.content_hash,
    })
}

#[derive(Debug, Clone)]
pub struct SavePersonaDraftInput {
    pub slug: String,
    pub content: String,
    pub source_session_id: Option<String>,
    pub source_persona_id: Option<PersonaId>,
    pub source_content_hash: Option<String>,
}

/// Application authority for persona lifecycle changes.
///
/// Feature state is deliberately passed to each entry point rather than cached here:
/// Tauri and HTTP own separate AppState instances and must enforce their current boundary.
#[derive(Clone)]
pub struct PersonaService {
    db: DbConnection,
    persona_repo: Arc<dyn PersonaRepository>,
    chat_conversation_repo: Arc<dyn ChatConversationRepository>,
}

impl PersonaService {
    pub fn new(
        db: DbConnection,
        persona_repo: Arc<dyn PersonaRepository>,
        chat_conversation_repo: Arc<dyn ChatConversationRepository>,
    ) -> Self {
        Self {
            db,
            persona_repo,
            chat_conversation_repo,
        }
    }

    pub async fn create_draft(
        &self,
        feature_enabled: bool,
        input: SavePersonaDraftInput,
    ) -> AppResult<Persona> {
        let persona = self.build_draft(feature_enabled, input).await?;
        self.persona_repo.create(persona).await
    }

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

    async fn build_draft(
        &self,
        feature_enabled: bool,
        input: SavePersonaDraftInput,
    ) -> AppResult<Persona> {
        ensure_enabled(feature_enabled)?;
        let parsed = validate_persona_content(&input.slug, &input.content)?;
        if input.source_persona_id.is_none() {
            self.ensure_live_slug_available(&input.slug, None).await?;
        }
        let now = Utc::now();
        Ok(Persona {
                id: PersonaId::new(),
                slug: input.slug,
                name: parsed.frontmatter.name,
                description: parsed.frontmatter.description,
                content: input.content,
                status: PersonaStatus::Draft,
                version: 1,
                content_hash: parsed.content_hash,
                source_session_id: input.source_session_id,
                source_persona_id: input.source_persona_id,
                source_content_hash: input.source_content_hash,
                source_json: "{}".to_string(),
                created_at: now,
                updated_at: now,
            })
    }

    pub async fn update_draft(
        &self,
        feature_enabled: bool,
        id: &PersonaId,
        content: &str,
    ) -> AppResult<Persona> {
        ensure_enabled(feature_enabled)?;
        let persona = self.require_status(id, PersonaStatus::Draft).await?;
        let parsed = validate_persona_content(&persona.slug, content)?;
        self.persona_repo
            .update_content(id, content, &parsed.content_hash)
            .await?;
        self.get_draft(feature_enabled, id).await
    }

    pub async fn get_draft(&self, feature_enabled: bool, id: &PersonaId) -> AppResult<Persona> {
        ensure_enabled(feature_enabled)?;
        self.require_status(id, PersonaStatus::Draft).await
    }

    pub async fn approve_persona(
        &self,
        feature_enabled: bool,
        id: &PersonaId,
    ) -> AppResult<Persona> {
        ensure_enabled(feature_enabled)?;
        let persona = self.require_status(id, PersonaStatus::Draft).await?;
        if persona.source_persona_id.is_some() {
            return self.apply_seeded_draft(id).await;
        }
        let parsed = validate_persona_content(&persona.slug, &persona.content)?;
        self.ensure_active_slug_available(&persona.slug, Some(id))
            .await?;
        self.persona_repo
            .update_content(id, &persona.content, &parsed.content_hash)
            .await?;
        self.persona_repo
            .set_status(id, PersonaStatus::Active)
            .await?;
        self.get_persona(feature_enabled, id).await
    }

    pub async fn update_persona(
        &self,
        feature_enabled: bool,
        id: &PersonaId,
        content: &str,
    ) -> AppResult<Persona> {
        ensure_enabled(feature_enabled)?;
        let persona = self.require_status(id, PersonaStatus::Active).await?;
        let parsed = validate_persona_content(&persona.slug, content)?;
        self.persona_repo
            .update_content(id, content, &parsed.content_hash)
            .await?;
        self.get_persona(feature_enabled, id).await
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

    pub async fn hard_delete_draft(&self, feature_enabled: bool, id: &PersonaId) -> AppResult<()> {
        ensure_enabled(feature_enabled)?;
        let draft = self.require_status(id, PersonaStatus::Draft).await?;
        if draft.source_persona_id.is_some()
            || self
                .chat_conversation_repo
                .get_by_builder_draft_id(id.as_str())
                .await?
                .is_some()
        {
            return self.delete_bound_draft(id).await;
        }
        self.persona_repo.delete(id).await
    }

    pub async fn list_personas(&self, feature_enabled: bool) -> AppResult<Vec<Persona>> {
        ensure_enabled(feature_enabled)?;
        self.persona_repo.list().await
    }

    pub async fn get_persona(&self, feature_enabled: bool, id: &PersonaId) -> AppResult<Persona> {
        ensure_enabled(feature_enabled)?;
        self.persona_repo
            .get_by_id(id)
            .await?
            .ok_or_else(|| AppError::NotFound(format!("Persona not found: {id}")))
    }

    pub async fn ensure_bindable(
        &self,
        feature_enabled: bool,
        id: &PersonaId,
    ) -> AppResult<Persona> {
        ensure_enabled(feature_enabled)?;
        let persona = self.persona_repo.get_by_id(id).await?;
        match persona.filter(Persona::is_bindable) {
            Some(persona) => Ok(persona),
            None => Err(AppError::PersonaUnavailable(format!(
                "{PERSONA_UNAVAILABLE_PREFIX} persona {id} is not active]"
            ))),
        }
    }

    async fn ensure_live_slug_available(
        &self,
        slug: &str,
        excluding: Option<&PersonaId>,
    ) -> AppResult<()> {
        let occupied = self.persona_repo.list().await?.into_iter().any(|persona| {
            persona.slug == slug
                && persona.status != PersonaStatus::Archived
                && excluding.is_none_or(|id| id != &persona.id)
        });
        if occupied {
            return Err(AppError::Validation(format!(
                "Persona slug `{slug}` is already in use"
            )));
        }
        Ok(())
    }

    async fn ensure_active_slug_available(
        &self,
        slug: &str,
        excluding: Option<&PersonaId>,
    ) -> AppResult<()> {
        let occupied = self
            .persona_repo
            .get_active_by_slug(slug)
            .await?
            .is_some_and(|persona| excluding.is_none_or(|id| id != &persona.id));
        if occupied {
            return Err(AppError::Validation(format!(
                "Persona slug `{slug}` is already in use"
            )));
        }
        Ok(())
    }

    async fn require_status(&self, id: &PersonaId, status: PersonaStatus) -> AppResult<Persona> {
        let persona = self
            .persona_repo
            .get_by_id(id)
            .await?
            .ok_or_else(|| AppError::NotFound(format!("Persona not found: {id}")))?;
        if persona.status != status {
            return Err(AppError::Validation(format!(
                "Persona {id} must be {status}"
            )));
        }
        Ok(persona)
    }
}

fn ensure_enabled(feature_enabled: bool) -> AppResult<()> {
    if feature_enabled {
        Ok(())
    } else {
        Err(AppError::FeatureDisabled(format!(
            "{PERSONA_FEATURE_DISABLED_PREFIX} agent personas feature is disabled]"
        )))
    }
}

#[path = "persona_service_tests.rs"]
mod persona_service_tests;

#[path = "persona_update_approval_tests.rs"]
mod persona_update_approval_tests;
