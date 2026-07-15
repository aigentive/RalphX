use chrono::Utc;
use ralphx_domain::personas::validation::{compose_persona_content, validate_persona_content};
use rusqlite::OptionalExtension;
use serde_json::{json, Value};

use super::{ensure_enabled, PersonaService};
use crate::domain::entities::{Persona, PersonaId, PersonaStatus};
use crate::error::{AppError, AppResult};
use crate::infrastructure::sqlite::sqlite_chat_conversation_repo::clear_builder_draft_bindings_sync;
use crate::infrastructure::sqlite::sqlite_persona_repo::{persona_from_row, PERSONA_COLUMNS};

const SOURCE_CHANGED_SINCE_SEED: &str = "SourceChangedSinceSeed:";
const SOURCE_NO_LONGER_ACTIVE: &str = "SourceNoLongerActive:";

pub fn draft_applied_payload(draft_id: &PersonaId, source: &Persona) -> Value {
    json!({
        "draft_id": draft_id.as_str(),
        "source_persona_id": source.id.as_str(),
    })
}

fn persona_by_id(conn: &rusqlite::Connection, id: &str) -> AppResult<Option<Persona>> {
    conn.query_row(
        &format!("SELECT {PERSONA_COLUMNS} FROM personas WHERE id = ?1"),
        [id],
        persona_from_row,
    )
    .optional()
    .map_err(AppError::from)
}

fn require_draft(conn: &rusqlite::Connection, id: &str) -> AppResult<Persona> {
    let persona = persona_by_id(conn, id)?
        .ok_or_else(|| AppError::NotFound(format!("Persona not found: {id}")))?;
    if persona.status != PersonaStatus::Draft {
        return Err(AppError::Validation(format!("Persona {id} must be draft")));
    }
    Ok(persona)
}

fn require_active_source(conn: &rusqlite::Connection, draft: &Persona) -> AppResult<Persona> {
    let source_id = draft.source_persona_id.as_ref().ok_or_else(|| {
        AppError::Validation(format!("Persona {} is not a seeded update draft", draft.id))
    })?;
    let source = persona_by_id(conn, source_id.as_str())?;
    match source {
        Some(source) if source.status == PersonaStatus::Active => Ok(source),
        _ => Err(AppError::Conflict(format!(
            "{SOURCE_NO_LONGER_ACTIVE} source persona {source_id} is not active"
        ))),
    }
}

fn ensure_source_is_current(draft: &Persona, source: &Persona) -> AppResult<()> {
    if draft.source_content_hash.as_deref() != Some(source.content_hash.as_str()) {
        return Err(AppError::Conflict(format!(
            "{SOURCE_CHANGED_SINCE_SEED} source persona {} changed after draft {} was seeded",
            source.id, draft.id
        )));
    }
    Ok(())
}

fn active_slug_owner(conn: &rusqlite::Connection, slug: &str) -> AppResult<Option<String>> {
    conn.query_row(
        "SELECT id FROM personas WHERE slug = ?1 AND status = 'active' LIMIT 1",
        [slug],
        |row| row.get(0),
    )
    .optional()
    .map_err(AppError::from)
}

fn ensure_new_slug_available(
    conn: &rusqlite::Connection,
    slug: &str,
    draft_id: &str,
) -> AppResult<()> {
    let occupied = conn
        .query_row(
            "SELECT 1 FROM personas
             WHERE slug = ?1 AND status != 'archived' AND id != ?2 LIMIT 1",
            rusqlite::params![slug, draft_id],
            |_| Ok(()),
        )
        .optional()?
        .is_some();
    if occupied {
        return Err(AppError::Conflict(format!(
            "Persona slug `{slug}` is already in use"
        )));
    }
    Ok(())
}

impl PersonaService {
    pub(super) async fn delete_bound_draft(&self, id: &PersonaId) -> AppResult<()> {
        let draft_id = id.as_str().to_string();
        self.db
            .run_transaction(move |conn| {
                require_draft(conn, &draft_id)?;
                clear_builder_draft_bindings_sync(conn, &draft_id)?;
                let deleted = conn.execute("DELETE FROM personas WHERE id = ?1", [&draft_id])?;
                if deleted != 1 {
                    return Err(AppError::Conflict(format!(
                        "Persona draft {draft_id} changed during deletion"
                    )));
                }
                Ok(())
            })
            .await
    }

    pub(super) async fn apply_seeded_draft(&self, id: &PersonaId) -> AppResult<Persona> {
        let draft_id = id.as_str().to_string();
        self.db
            .run_transaction(move |conn| {
                let draft = require_draft(conn, &draft_id)?;
                let source = require_active_source(conn, &draft)?;
                ensure_source_is_current(&draft, &source)?;
                let parsed = validate_persona_content(&source.slug, &draft.content)?;
                let now = Utc::now().to_rfc3339();
                let changed = conn.execute(
                    "UPDATE personas
                     SET name = ?1, description = ?2, content = ?3, content_hash = ?4,
                         version = version + 1, updated_at = ?5
                     WHERE id = ?6 AND status = 'active' AND content_hash = ?7",
                    rusqlite::params![
                        parsed.frontmatter.name,
                        parsed.frontmatter.description,
                        draft.content,
                        parsed.content_hash,
                        now,
                        source.id.as_str(),
                        source.content_hash,
                    ],
                )?;
                if changed != 1 {
                    return Err(AppError::Conflict(format!(
                        "{SOURCE_CHANGED_SINCE_SEED} source persona {} changed during approval",
                        source.id
                    )));
                }
                clear_builder_draft_bindings_sync(conn, &draft_id)?;
                let deleted = conn.execute("DELETE FROM personas WHERE id = ?1", [&draft_id])?;
                if deleted != 1 {
                    return Err(AppError::Conflict(format!(
                        "Seeded persona draft {draft_id} disappeared during approval"
                    )));
                }
                persona_by_id(conn, source.id.as_str())?
                    .ok_or_else(|| AppError::NotFound(format!("Persona not found: {}", source.id)))
            })
            .await
    }

    pub async fn reseed_persona_draft(
        &self,
        feature_enabled: bool,
        id: &PersonaId,
    ) -> AppResult<Persona> {
        ensure_enabled(feature_enabled)?;
        let draft_id = id.as_str().to_string();
        self.db
            .run_transaction(move |conn| {
                let draft = require_draft(conn, &draft_id)?;
                let source = require_active_source(conn, &draft)?;
                let changed = conn.execute(
                    "UPDATE personas SET source_content_hash = ?1, updated_at = ?2 WHERE id = ?3",
                    rusqlite::params![source.content_hash, Utc::now().to_rfc3339(), draft_id,],
                )?;
                if changed != 1 {
                    return Err(AppError::Conflict(format!(
                        "Persona draft {draft_id} changed during reseed"
                    )));
                }
                persona_by_id(conn, &draft_id)?
                    .ok_or_else(|| AppError::NotFound(format!("Persona not found: {draft_id}")))
            })
            .await
    }

    pub async fn approve_persona_as_new(
        &self,
        feature_enabled: bool,
        id: &PersonaId,
        new_slug: Option<&str>,
    ) -> AppResult<Persona> {
        ensure_enabled(feature_enabled)?;
        let draft_id = id.as_str().to_string();
        let new_slug = new_slug.map(str::to_string);
        self.db
            .run_transaction(move |conn| {
                let draft = require_draft(conn, &draft_id)?;
                let source_id = draft.source_persona_id.as_ref().ok_or_else(|| {
                    AppError::Validation(format!("Persona {draft_id} is not a seeded update draft"))
                })?;
                if persona_by_id(conn, source_id.as_str())?
                    .is_some_and(|source| source.status == PersonaStatus::Active)
                {
                    return Err(AppError::Conflict(format!(
                        "SourceStillActive: source persona {source_id} must be updated in place"
                    )));
                }

                let target_slug = new_slug.as_deref().unwrap_or(&draft.slug);
                if new_slug.is_none() && active_slug_owner(conn, target_slug)?.is_some() {
                    return Err(AppError::Conflict(format!(
                        "Persona slug `{target_slug}` is already in use; provide a new slug"
                    )));
                }
                ensure_new_slug_available(conn, target_slug, &draft_id)?;

                let old = validate_persona_content(&draft.slug, &draft.content)?;
                let content = if target_slug == draft.slug {
                    draft.content.clone()
                } else {
                    compose_persona_content(target_slug, &old.frontmatter.description, &old.body)
                };
                let parsed = validate_persona_content(target_slug, &content)?;
                let changed = conn.execute(
                    "UPDATE personas
                     SET slug = ?1, name = ?2, description = ?3, content = ?4,
                         content_hash = ?5, source_persona_id = NULL,
                         source_content_hash = NULL, status = 'active',
                         version = version + 1, updated_at = ?6
                     WHERE id = ?7 AND status = 'draft'",
                    rusqlite::params![
                        target_slug,
                        parsed.frontmatter.name,
                        parsed.frontmatter.description,
                        content,
                        parsed.content_hash,
                        Utc::now().to_rfc3339(),
                        draft_id,
                    ],
                )?;
                if changed != 1 {
                    return Err(AppError::Conflict(format!(
                        "Persona draft {draft_id} changed during approval"
                    )));
                }
                clear_builder_draft_bindings_sync(conn, &draft_id)?;
                persona_by_id(conn, &draft_id)?
                    .ok_or_else(|| AppError::NotFound(format!("Persona not found: {draft_id}")))
            })
            .await
    }
}
