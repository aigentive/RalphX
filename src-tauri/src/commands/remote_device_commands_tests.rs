use std::sync::Mutex;

use async_trait::async_trait;

use super::list_audit_entries_from;
use crate::domain::entities::{RemoteAuditAction, RemoteAuditEntry, RemoteDeviceId};
use crate::domain::repositories::RemoteAuditLogRepository;
use crate::error::AppResult;

struct RecordingAuditRepository {
    entries: Vec<RemoteAuditEntry>,
    limits: Mutex<Vec<Option<i64>>>,
}

impl RecordingAuditRepository {
    fn new(entries: Vec<RemoteAuditEntry>) -> Self {
        Self {
            entries,
            limits: Mutex::new(Vec::new()),
        }
    }
}

#[async_trait]
impl RemoteAuditLogRepository for RecordingAuditRepository {
    async fn record(
        &self,
        _device_id: Option<&RemoteDeviceId>,
        _action: RemoteAuditAction,
        _detail: Option<&str>,
        _now: &str,
    ) -> AppResult<()> {
        unreachable!("audit listing must not write")
    }

    async fn list_recent(&self, limit: Option<i64>) -> AppResult<Vec<RemoteAuditEntry>> {
        self.limits.lock().expect("limits lock").push(limit);
        Ok(self.entries.clone())
    }

    async fn prune_before(&self, _cutoff: &str) -> AppResult<usize> {
        unreachable!("audit listing must not prune")
    }
}

#[tokio::test]
async fn omitted_audit_limit_is_delegated_as_none() {
    let repository = RecordingAuditRepository::new(Vec::new());

    let entries = list_audit_entries_from(&repository, None)
        .await
        .expect("audit listing should succeed");

    assert!(entries.is_empty());
    assert_eq!(*repository.limits.lock().expect("limits lock"), vec![None]);
}

#[tokio::test]
async fn explicit_audit_limit_is_delegated_unchanged() {
    let repository = RecordingAuditRepository::new(Vec::new());

    list_audit_entries_from(&repository, Some(1_337))
        .await
        .expect("audit listing should succeed");

    assert_eq!(
        *repository.limits.lock().expect("limits lock"),
        vec![Some(1_337)]
    );
}

#[test]
fn audit_entry_serializes_with_frontend_field_names_and_explicit_nulls() {
    let entry = RemoteAuditEntry {
        id: 42,
        device_id: None,
        action: "auth_rejected".to_string(),
        detail: None,
        created_at: "2026-07-27T12:34:56+00:00".to_string(),
    };

    let json = serde_json::to_value(entry).expect("audit entry should serialize");

    assert_eq!(
        json,
        serde_json::json!({
            "id": 42,
            "deviceId": null,
            "action": "auth_rejected",
            "detail": null,
            "createdAt": "2026-07-27T12:34:56+00:00",
        })
    );
}
