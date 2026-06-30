use std::sync::Arc;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use tokio::sync::RwLock;
use uuid::Uuid;

use crate::domain::integrations::{
    TicketingStatusCatalogEntry, TicketingStatusCatalogRepository, TicketingStatusCatalogUpsert,
    TicketingStatusPresentationPatch,
};
use crate::error::AppResult;

#[derive(Default)]
pub struct MemoryTicketingStatusCatalogRepository {
    entries: Arc<RwLock<Vec<TicketingStatusCatalogEntry>>>,
}

impl MemoryTicketingStatusCatalogRepository {
    pub fn new() -> Self {
        Self::default()
    }
}

fn matches_scope(
    entry: &TicketingStatusCatalogEntry,
    provider: &str,
    scope_kind: &str,
    scope_id: &str,
) -> bool {
    entry.provider == provider && entry.scope_kind == scope_kind && entry.scope_id == scope_id
}

fn sort_entries(entries: &mut [TicketingStatusCatalogEntry]) {
    entries.sort_by(|left, right| {
        left.display_order
            .cmp(&right.display_order)
            .then_with(|| left.provider_order.cmp(&right.provider_order))
            .then_with(|| {
                left.provider_status_name
                    .to_lowercase()
                    .cmp(&right.provider_status_name.to_lowercase())
            })
            .then_with(|| left.provider_status_id.cmp(&right.provider_status_id))
    });
}

fn new_entry(input: TicketingStatusCatalogUpsert) -> TicketingStatusCatalogEntry {
    let now = Utc::now();
    TicketingStatusCatalogEntry {
        id: Uuid::new_v4().to_string(),
        provider: input.provider,
        scope_kind: input.scope_kind,
        scope_id: input.scope_id,
        provider_status_id: input.provider_status_id,
        provider_status_name: input.provider_status_name,
        provider_category: input.provider_category,
        provider_color: input.provider_color,
        provider_order: input.provider_order,
        display_order: input.display_order,
        color_override: None,
        is_visible: true,
        is_terminal: input.is_terminal,
        last_seen_at: Some(input.last_seen_at),
        stale_since: None,
        metadata_json: input.metadata_json,
        created_at: now,
        updated_at: now,
    }
}

fn apply_upsert(entry: &mut TicketingStatusCatalogEntry, input: TicketingStatusCatalogUpsert) {
    entry.provider_status_name = input.provider_status_name;
    entry.provider_category = input.provider_category;
    entry.provider_color = input.provider_color;
    entry.provider_order = input.provider_order;
    entry.display_order = input.display_order;
    entry.is_terminal = input.is_terminal;
    entry.last_seen_at = Some(input.last_seen_at);
    entry.stale_since = None;
    entry.metadata_json = input.metadata_json;
    entry.updated_at = Utc::now();
}

#[async_trait]
impl TicketingStatusCatalogRepository for MemoryTicketingStatusCatalogRepository {
    async fn list_status_catalog(
        &self,
        provider: &str,
        scope_kind: &str,
        scope_id: &str,
    ) -> AppResult<Vec<TicketingStatusCatalogEntry>> {
        let mut entries: Vec<_> = self
            .entries
            .read()
            .await
            .iter()
            .filter(|entry| matches_scope(entry, provider, scope_kind, scope_id))
            .cloned()
            .collect();
        sort_entries(&mut entries);
        Ok(entries)
    }

    async fn upsert_status_catalog_entry(
        &self,
        input: TicketingStatusCatalogUpsert,
    ) -> AppResult<TicketingStatusCatalogEntry> {
        let mut entries = self.entries.write().await;
        if let Some(entry) = entries.iter_mut().find(|entry| {
            matches_scope(entry, &input.provider, &input.scope_kind, &input.scope_id)
                && entry.provider_status_id == input.provider_status_id
        }) {
            apply_upsert(entry, input);
            return Ok(entry.clone());
        }

        let entry = new_entry(input);
        entries.push(entry.clone());
        Ok(entry)
    }

    async fn update_status_presentation(
        &self,
        provider: &str,
        scope_kind: &str,
        scope_id: &str,
        patches: Vec<TicketingStatusPresentationPatch>,
    ) -> AppResult<Vec<TicketingStatusCatalogEntry>> {
        let mut entries = self.entries.write().await;
        for patch in patches {
            let Some(entry) = entries.iter_mut().find(|entry| {
                matches_scope(entry, provider, scope_kind, scope_id)
                    && entry.provider_status_id == patch.provider_status_id
            }) else {
                continue;
            };
            if let Some(display_order) = patch.display_order {
                entry.display_order = display_order;
            }
            if let Some(color_override) = patch.color_override {
                entry.color_override = color_override;
            }
            if let Some(is_visible) = patch.is_visible {
                entry.is_visible = is_visible;
            }
            entry.updated_at = Utc::now();
        }

        let mut scoped: Vec<_> = entries
            .iter()
            .filter(|entry| matches_scope(entry, provider, scope_kind, scope_id))
            .cloned()
            .collect();
        sort_entries(&mut scoped);
        Ok(scoped)
    }

    async fn mark_missing_statuses_stale(
        &self,
        provider: &str,
        scope_kind: &str,
        scope_id: &str,
        observed_provider_status_ids: &[String],
        stale_since: DateTime<Utc>,
    ) -> AppResult<Vec<TicketingStatusCatalogEntry>> {
        let observed: std::collections::HashSet<&str> = observed_provider_status_ids
            .iter()
            .map(String::as_str)
            .collect();
        let mut entries = self.entries.write().await;
        for entry in entries.iter_mut().filter(|entry| {
            matches_scope(entry, provider, scope_kind, scope_id)
                && !observed.contains(entry.provider_status_id.as_str())
        }) {
            if entry.stale_since.is_none() {
                entry.stale_since = Some(stale_since);
            }
            entry.updated_at = Utc::now();
        }

        let mut scoped: Vec<_> = entries
            .iter()
            .filter(|entry| matches_scope(entry, provider, scope_kind, scope_id))
            .cloned()
            .collect();
        sort_entries(&mut scoped);
        Ok(scoped)
    }
}
