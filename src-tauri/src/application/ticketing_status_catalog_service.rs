use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use chrono::Utc;

use crate::domain::integrations::{
    ObservedTicketingStatus, TicketingStatusCatalogEntry, TicketingStatusCatalogRepository,
    TicketingStatusCatalogUpsert, TicketingStatusPresentationPatch,
};
use crate::error::AppResult;

pub struct TicketingStatusCatalogService {
    repo: Arc<dyn TicketingStatusCatalogRepository>,
}

impl TicketingStatusCatalogService {
    pub fn new(repo: Arc<dyn TicketingStatusCatalogRepository>) -> Self {
        Self { repo }
    }

    pub async fn list_status_catalog(
        &self,
        provider: &str,
        scope_kind: &str,
        scope_id: &str,
    ) -> AppResult<Vec<TicketingStatusCatalogEntry>> {
        self.repo
            .list_status_catalog(provider, scope_kind, scope_id)
            .await
    }

    pub async fn update_status_presentation(
        &self,
        provider: &str,
        scope_kind: &str,
        scope_id: &str,
        patches: Vec<TicketingStatusPresentationPatch>,
    ) -> AppResult<Vec<TicketingStatusCatalogEntry>> {
        self.repo
            .update_status_presentation(provider, scope_kind, scope_id, patches)
            .await
    }

    pub async fn sync_observed_statuses(
        &self,
        provider: &str,
        scope_kind: &str,
        scope_id: &str,
        observed: Vec<ObservedTicketingStatus>,
    ) -> AppResult<Vec<TicketingStatusCatalogEntry>> {
        let existing = self
            .repo
            .list_status_catalog(provider, scope_kind, scope_id)
            .await?;
        let existing_display_orders: HashMap<_, _> = existing
            .iter()
            .map(|entry| (entry.provider_status_id.as_str(), entry.display_order))
            .collect();
        let has_existing_catalog = !existing.is_empty();
        let mut next_display_order = existing
            .iter()
            .map(|entry| entry.display_order)
            .max()
            .unwrap_or(-1)
            + 1;

        let mut seen = HashSet::new();
        let mut observed = observed
            .into_iter()
            .filter(|status| seen.insert(status.provider_status_id.clone()))
            .collect::<Vec<_>>();
        observed.sort_by(|left, right| {
            left.provider_order
                .cmp(&right.provider_order)
                .then_with(|| {
                    left.provider_status_name
                        .to_lowercase()
                        .cmp(&right.provider_status_name.to_lowercase())
                })
                .then_with(|| left.provider_status_id.cmp(&right.provider_status_id))
        });

        let now = Utc::now();
        for (index, status) in observed.iter().enumerate() {
            let display_order = existing_display_orders
                .get(status.provider_status_id.as_str())
                .copied()
                .unwrap_or_else(|| {
                    if has_existing_catalog {
                        let order = next_display_order;
                        next_display_order += 1;
                        order
                    } else {
                        status.provider_order.unwrap_or(index as i64)
                    }
                });
            self.repo
                .upsert_status_catalog_entry(TicketingStatusCatalogUpsert {
                    provider: provider.to_string(),
                    scope_kind: scope_kind.to_string(),
                    scope_id: scope_id.to_string(),
                    provider_status_id: status.provider_status_id.clone(),
                    provider_status_name: status.provider_status_name.clone(),
                    provider_category: status.provider_category.clone(),
                    provider_color: status.provider_color.clone(),
                    provider_order: status.provider_order,
                    display_order,
                    is_terminal: status.is_terminal,
                    last_seen_at: now,
                    metadata_json: status.metadata_json.clone(),
                })
                .await?;
        }

        let observed_ids = observed
            .into_iter()
            .map(|status| status.provider_status_id)
            .collect::<Vec<_>>();
        self.repo
            .mark_missing_statuses_stale(provider, scope_kind, scope_id, &observed_ids, now)
            .await
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use crate::domain::integrations::{ObservedTicketingStatus, TicketingStatusPresentationPatch};
    use crate::infrastructure::memory::MemoryTicketingStatusCatalogRepository;

    use super::TicketingStatusCatalogService;

    fn observed(id: &str, name: &str, provider_order: i64) -> ObservedTicketingStatus {
        ObservedTicketingStatus {
            provider_status_id: id.to_string(),
            provider_status_name: name.to_string(),
            provider_category: "todo".to_string(),
            provider_color: Some(format!("#{provider_order:06}")),
            provider_order: Some(provider_order),
            is_terminal: false,
            metadata_json: None,
        }
    }

    #[tokio::test]
    async fn sync_seeds_provider_order_then_preserves_user_display_order() {
        let service = TicketingStatusCatalogService::new(Arc::new(
            MemoryTicketingStatusCatalogRepository::new(),
        ));

        let entries = service
            .sync_observed_statuses(
                "linear",
                "linear_team",
                "team-1",
                vec![
                    observed("started", "Started", 1),
                    observed("backlog", "Backlog", 0),
                ],
            )
            .await
            .unwrap();
        assert_eq!(
            entries
                .iter()
                .map(|entry| entry.provider_status_id.as_str())
                .collect::<Vec<_>>(),
            vec!["backlog", "started"]
        );

        service
            .update_status_presentation(
                "linear",
                "linear_team",
                "team-1",
                vec![TicketingStatusPresentationPatch {
                    provider_status_id: "started".to_string(),
                    display_order: Some(-1),
                    color_override: Some(Some("#ff8800".to_string())),
                    is_visible: Some(false),
                }],
            )
            .await
            .unwrap();

        let entries = service
            .sync_observed_statuses(
                "linear",
                "linear_team",
                "team-1",
                vec![
                    observed("backlog", "Backlog", 0),
                    observed("started", "In Progress", 1),
                ],
            )
            .await
            .unwrap();
        let started = entries
            .iter()
            .find(|entry| entry.provider_status_id == "started")
            .unwrap();
        assert_eq!(started.display_order, -1);
        assert_eq!(started.color_override.as_deref(), Some("#ff8800"));
        assert!(!started.is_visible);
        assert_eq!(started.provider_status_name, "In Progress");
    }

    #[tokio::test]
    async fn sync_marks_removed_provider_statuses_stale_and_appends_new_statuses() {
        let service = TicketingStatusCatalogService::new(Arc::new(
            MemoryTicketingStatusCatalogRepository::new(),
        ));
        service
            .sync_observed_statuses(
                "clickup",
                "clickup_space",
                "space-1",
                vec![observed("open", "Open", 0), observed("done", "Done", 1)],
            )
            .await
            .unwrap();

        let entries = service
            .sync_observed_statuses(
                "clickup",
                "clickup_space",
                "space-1",
                vec![observed("open", "Open", 0), observed("review", "Review", 1)],
            )
            .await
            .unwrap();

        assert_eq!(entries.len(), 3);
        assert!(entries
            .iter()
            .find(|entry| entry.provider_status_id == "done")
            .unwrap()
            .stale_since
            .is_some());
        assert_eq!(
            entries
                .iter()
                .find(|entry| entry.provider_status_id == "review")
                .unwrap()
                .display_order,
            2
        );
    }
}
