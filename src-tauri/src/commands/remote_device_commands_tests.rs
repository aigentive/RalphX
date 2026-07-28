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

fn coverage_sample_device(
    scopes: crate::domain::entities::RemoteScopeSet,
) -> crate::domain::entities::RemoteDevice {
    crate::domain::entities::RemoteDevice {
        id: RemoteDeviceId::from_string("device-1"),
        name: "MacBook".to_string(),
        token_hash: "hash".to_string(),
        token_prefix: "rxd_live_a3f2".to_string(),
        scopes,
        created_at: "2026-07-27T19:15:00+00:00".to_string(),
        last_seen_at: Some("2026-07-27T20:15:00+00:00".to_string()),
        revoked_at: None,
    }
}

/// Session-scoped teardown must not speak the host-scoped word. `host_disabled` tells a client
/// remote access is OFF — a §3.2 vocabulary a supervisor is entitled to park on — so using it to
/// close ONE socket turns an owner's single disconnect into a device-wide outage while the host
/// stays enabled and the device stays paired. `Revoked` is the advisory member the sibling
/// session-scoped paths (`set_remote_device_agent_control`, `DELETE /remote/v1/session`) use.
#[test]
fn disconnecting_one_session_does_not_claim_the_host_was_disabled() {
    let source = include_str!("remote_device_commands.rs");
    let body = source
        .split("pub async fn disconnect_remote_session")
        .nth(1)
        .expect("the disconnect command should exist");
    let end = body.find("\n}\n").expect("the command body should end");
    let body = &body[..end];
    assert!(
        body.contains("ResetReason::Revoked"),
        "a single-session disconnect must use the advisory session-scoped reason"
    );
    assert!(
        !body.contains("ResetReason::HostDisabled"),
        "the host is still enabled; host_disabled would park the whole device"
    );
}

#[test]
fn device_view_maps_active_agent_control_device_fields() {
    let device = coverage_sample_device(crate::domain::entities::RemoteScopeSet::from_scopes([
        ralphx_remote_protocol::Scope::UiRead,
        ralphx_remote_protocol::Scope::UiAgent,
    ]));

    let view = super::device_view(&device, 2);

    assert_eq!(view.id, "device-1");
    assert_eq!(view.name, "MacBook");
    assert_eq!(view.token_prefix, "rxd_live_a3f2");
    assert_eq!(
        view.scopes,
        vec![
            ralphx_remote_protocol::Scope::UiRead,
            ralphx_remote_protocol::Scope::UiAgent
        ]
    );
    assert!(view.agent_control_granted);
    assert_eq!(view.created_at, "2026-07-27T19:15:00+00:00");
    assert_eq!(
        view.last_seen_at.as_deref(),
        Some("2026-07-27T20:15:00+00:00")
    );
    assert!(view.revoked_at.is_none());
    assert_eq!(view.live_session_count, 2);
}

#[test]
fn device_view_maps_revoked_device_without_agent_control_or_last_seen() {
    let mut device = coverage_sample_device(crate::domain::entities::RemoteScopeSet::from_scopes(
        [ralphx_remote_protocol::Scope::UiOperate],
    ));
    device.last_seen_at = None;
    device.revoked_at = Some("2026-07-27T21:15:00+00:00".to_string());

    let view = super::device_view(&device, 0);

    assert!(!view.agent_control_granted);
    assert!(view.last_seen_at.is_none());
    assert_eq!(
        view.revoked_at.as_deref(),
        Some("2026-07-27T21:15:00+00:00")
    );
    assert_eq!(view.live_session_count, 0);
}
