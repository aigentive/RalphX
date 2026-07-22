use super::{
    clear_manual_role_persona_defaults_sync, ensure_enabled, PersonaService, SavePersonaDraftInput,
};
use ralphx_domain::personas::validation::ParsedPersona;
use serde_json::{json, Map, Value};

use crate::domain::entities::{
    Artifact, ArtifactBucketId, ArtifactType, ChatConversationId, Persona, PersonaId, PersonaStatus,
};
use crate::error::{AppError, AppResult};
use crate::infrastructure::sqlite::sqlite_artifact_repo::SqliteArtifactRepository;
use crate::infrastructure::sqlite::sqlite_chat_conversation_repo::{
    claim_builder_draft_binding_sync, clear_persona_bindings_sync, finish_builder_binding_sync,
};
use crate::infrastructure::sqlite::sqlite_persona_repo::{
    map_live_slug_unique_error, persona_create_sync, persona_from_row, persona_set_status_sync,
    PERSONA_COLUMNS,
};

pub(super) const PERSONA_ARTIFACT_BUCKET: &str = "persona-library";
pub(super) const CREATED_BY_AGENT: &str = "agent";
pub(super) const CREATED_BY_SYSTEM: &str = "system";
pub(super) const CREATED_BY_USER: &str = "user";

pub(super) fn persona_by_id_sync(
    conn: &rusqlite::Connection,
    id: &str,
) -> AppResult<Option<Persona>> {
    use rusqlite::OptionalExtension;

    conn.query_row(
        &format!("SELECT {PERSONA_COLUMNS} FROM personas WHERE id = ?1"),
        [id],
        persona_from_row,
    )
    .optional()
    .map_err(AppError::from)
}

pub(super) fn append_persona_artifact_sync(
    conn: &rusqlite::Connection,
    persona: &mut Persona,
    created_by: &str,
    extra_metadata: Option<Map<String, Value>>,
) -> AppResult<()> {
    let previous = persona.artifact_id.clone();
    let chain_version = previous
        .as_ref()
        .map(|id| {
            SqliteArtifactRepository::get_by_id_sync(conn, id.as_str())?
                .map(|artifact| artifact.metadata.version + 1)
                .ok_or_else(|| {
                    AppError::Conflict(format!(
                        "Persona {} artifact tip {} is missing",
                        persona.id, id
                    ))
                })
        })
        .transpose()?
        .unwrap_or(1);
    let mut metadata = extra_metadata.unwrap_or_default();
    metadata.insert("persona_version".to_string(), json!(persona.version));
    metadata.insert("created_by".to_string(), json!(created_by));
    let mut artifact = Artifact::new_inline(
        persona.name.clone(),
        ArtifactType::Persona,
        persona.content.clone(),
        created_by,
    )
    .with_bucket(ArtifactBucketId::from_string(PERSONA_ARTIFACT_BUCKET));
    artifact.metadata = artifact
        .metadata
        .with_version(chain_version)
        .with_custom_metadata(Value::Object(metadata));
    let artifact = if let Some(previous) = previous {
        SqliteArtifactRepository::create_with_previous_version_sync(
            conn,
            artifact,
            previous.as_str(),
        )?
    } else {
        SqliteArtifactRepository::create_sync(conn, artifact)?
    };
    let changed = conn.execute(
        "UPDATE personas SET artifact_id = ?1 WHERE id = ?2",
        rusqlite::params![artifact.id.as_str(), persona.id.as_str()],
    )?;
    if changed != 1 {
        return Err(AppError::Conflict(format!(
            "Persona {} disappeared while appending artifact history",
            persona.id
        )));
    }
    persona.artifact_id = Some(artifact.id);
    Ok(())
}

pub(super) fn create_persona_with_artifact_sync(
    conn: &rusqlite::Connection,
    persona: Persona,
    created_by: &str,
) -> AppResult<Persona> {
    let mut persona = persona_create_sync(conn, persona)?;
    append_persona_artifact_sync(conn, &mut persona, created_by, None)?;
    Ok(persona)
}

pub(super) fn delete_persona_artifact_chain_sync(
    conn: &rusqlite::Connection,
    persona_id: &str,
) -> AppResult<()> {
    conn.execute(
        "WITH RECURSIVE persona_chain(id) AS (
             SELECT artifact_id FROM personas WHERE id = ?1 AND artifact_id IS NOT NULL
             UNION ALL
             SELECT artifacts.previous_version_id
             FROM artifacts JOIN persona_chain ON artifacts.id = persona_chain.id
             WHERE artifacts.previous_version_id IS NOT NULL
         )
         DELETE FROM artifacts WHERE id IN (SELECT id FROM persona_chain)",
        [persona_id],
    )?;
    Ok(())
}

impl PersonaService {
    pub(super) async fn persist_draft(
        &self,
        persona: Persona,
        created_by: &'static str,
    ) -> AppResult<Persona> {
        let collision_slug = persona.slug.clone();
        self.db
            .run_transaction(move |conn| {
                create_persona_with_artifact_sync(conn, persona, created_by)
            })
            .await
            .map_err(|error| map_live_slug_unique_error(error, &collision_slug))
    }

    pub(super) async fn update_content_with_artifact(
        &self,
        id: &PersonaId,
        content: &str,
        parsed: ParsedPersona,
        expected_content_hash: Option<&str>,
        required_status: PersonaStatus,
        created_by: &'static str,
    ) -> AppResult<Persona> {
        let id = id.as_str().to_string();
        let content = content.to_string();
        let expected = expected_content_hash.map(str::to_string);
        self.db
            .run_transaction(move |conn| {
                let current = persona_by_id_sync(conn, &id)?
                    .ok_or_else(|| AppError::NotFound(format!("Persona not found: {id}")))?;
                if current.status != required_status {
                    return Err(AppError::Validation(format!(
                        "Persona {id} must be {required_status}"
                    )));
                }
                if let Some(expected) = expected.as_deref() {
                    if expected != current.content_hash {
                        return Err(AppError::PersonaDraftConflict {
                            expected: expected.to_string(),
                            actual: current.content_hash,
                        });
                    }
                }
                let changed = conn.execute(
                    "UPDATE personas
                     SET name = ?1, description = ?2, content = ?3, content_hash = ?4,
                         version = version + 1, updated_at = ?5
                     WHERE id = ?6 AND status = ?7",
                    rusqlite::params![
                        parsed.frontmatter.name,
                        parsed.frontmatter.description,
                        content,
                        parsed.content_hash,
                        chrono::Utc::now().to_rfc3339(),
                        id,
                        required_status.to_string(),
                    ],
                )?;
                if changed != 1 {
                    return Err(AppError::Conflict(format!(
                        "Persona {id} changed during content update"
                    )));
                }
                let mut updated = persona_by_id_sync(conn, &id)?
                    .ok_or_else(|| AppError::NotFound(format!("Persona not found: {id}")))?;
                append_persona_artifact_sync(conn, &mut updated, created_by, None)?;
                Ok(updated)
            })
            .await
    }

    pub(super) async fn approve_plain_draft(&self, id: &PersonaId) -> AppResult<Persona> {
        let id = id.as_str().to_string();
        self.db
            .run_transaction(move |conn| {
                let current = persona_by_id_sync(conn, &id)?
                    .ok_or_else(|| AppError::NotFound(format!("Persona not found: {id}")))?;
                if current.status != PersonaStatus::Draft {
                    return Err(AppError::Validation(format!("Persona {id} must be draft")));
                }
                let parsed = ralphx_domain::personas::validation::validate_persona_content(
                    &current.slug,
                    &current.content,
                )?;
                let changed = conn.execute(
                    "UPDATE personas
                     SET content_hash = ?1, version = version + 1, updated_at = ?2
                     WHERE id = ?3 AND status = 'draft'",
                    rusqlite::params![parsed.content_hash, chrono::Utc::now().to_rfc3339(), id],
                )?;
                if changed != 1 {
                    return Err(AppError::Conflict(format!(
                        "Persona draft {id} changed during approval"
                    )));
                }
                let mut approved = persona_by_id_sync(conn, &id)?
                    .ok_or_else(|| AppError::NotFound(format!("Persona not found: {id}")))?;
                append_persona_artifact_sync(conn, &mut approved, CREATED_BY_USER, None)?;
                persona_set_status_sync(conn, &id, PersonaStatus::Active)?;
                finish_builder_binding_sync(conn, &id, &id)?;
                persona_by_id_sync(conn, &id)?
                    .ok_or_else(|| AppError::NotFound(format!("Persona not found: {id}")))
            })
            .await
    }

    pub(super) async fn delete_unbound_draft(&self, id: &PersonaId) -> AppResult<()> {
        let id = id.as_str().to_string();
        self.db
            .run_transaction(move |conn| {
                let current = persona_by_id_sync(conn, &id)?
                    .ok_or_else(|| AppError::NotFound(format!("Persona not found: {id}")))?;
                if current.status != PersonaStatus::Draft {
                    return Err(AppError::Validation(format!("Persona {id} must be draft")));
                }
                delete_persona_artifact_chain_sync(conn, &id)?;
                let deleted = conn.execute("DELETE FROM personas WHERE id = ?1", [&id])?;
                if deleted != 1 {
                    return Err(AppError::Conflict(format!(
                        "Persona draft {id} changed during deletion"
                    )));
                }
                Ok(())
            })
            .await
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
                let persona = create_persona_with_artifact_sync(conn, persona, CREATED_BY_AGENT)?;
                claim_builder_draft_binding_sync(conn, &conversation_id, &draft_id)?;
                Ok(persona)
            })
            .await
            .map_err(|error| map_live_slug_unique_error(error, &collision_slug))
    }

    pub async fn unarchive_persona(
        &self,
        feature_enabled: bool,
        id: &PersonaId,
    ) -> AppResult<Persona> {
        ensure_enabled(feature_enabled)?;
        let persona = self.require_status(id, PersonaStatus::Archived).await?;
        if let Some(conflict) = self
            .persona_repo
            .get_active_by_slug(&persona.slug, persona.project_id.as_ref())
            .await?
        {
            if conflict.id != *id {
                return Err(AppError::Validation(format!(
                    "Cannot restore persona: active persona `{}` already uses slug `{}` in this scope",
                    conflict.name, persona.slug
                )));
            }
        }
        // Bindings cleared at archive time stay cleared; restore never rewrites
        // chat_conversations.persona_id. The repository re-enforces active-slug
        // uniqueness (SQLite partial index / memory check) against races.
        self.persona_repo
            .set_status(id, PersonaStatus::Active)
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
                clear_manual_role_persona_defaults_sync(conn, &id_value)?;
                Ok(())
            })
            .await?;
        self.get_persona(feature_enabled, id).await
    }
}
