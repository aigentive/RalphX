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
    let mut device =
        coverage_sample_device(crate::domain::entities::RemoteScopeSet::from_scopes([
            ralphx_remote_protocol::Scope::UiOperate,
        ]));
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

// ---------------------------------------------------------------------------------------
// §5.5: an authority change whose decision left no audit trail must not report success.
// ---------------------------------------------------------------------------------------

mod audit_is_fatal {
    use std::sync::{Arc, Mutex};

    use async_trait::async_trait;
    use ralphx_remote_protocol::Scope;

    use crate::commands::remote_device_commands::{
        revoke_device_with_context, set_agent_control_with_context, RemoteDeviceIdInput,
        SetRemoteDeviceAgentControlInput,
    };
    use crate::domain::entities::{
        RemoteAuditAction, RemoteAuditEntry, RemoteDevice, RemoteDeviceId, RemoteScopeSet,
    };
    use crate::domain::repositories::{
        RemoteAuditLogRepository, RemoteDeviceLookup, RemoteDeviceRepository,
    };
    use crate::error::{AppError, AppResult};
    use crate::infrastructure::sqlite::{run_migrations, DbConnection};
    use crate::remote_server::auth::RemoteAuthContext;

    /// Records every scope write so a compensating revert is observable.
    struct RecordingDeviceRepository {
        device: Mutex<RemoteDevice>,
        scope_writes: Mutex<Vec<RemoteScopeSet>>,
        revocations: Mutex<usize>,
    }

    impl RecordingDeviceRepository {
        fn with_scopes(scopes: RemoteScopeSet) -> Self {
            Self {
                device: Mutex::new(RemoteDevice {
                    id: RemoteDeviceId::from_string("device-1".to_string()),
                    name: "iPhone".to_string(),
                    token_hash: "hash".to_string(),
                    token_prefix: "rxd_live_abcd".to_string(),
                    scopes,
                    created_at: "2026-07-27T00:00:00+00:00".to_string(),
                    last_seen_at: None,
                    revoked_at: None,
                }),
                scope_writes: Mutex::new(Vec::new()),
                revocations: Mutex::new(0),
            }
        }

        fn scope_writes(&self) -> Vec<RemoteScopeSet> {
            self.scope_writes.lock().expect("scope writes").clone()
        }
    }

    #[async_trait]
    impl RemoteDeviceRepository for RecordingDeviceRepository {
        async fn lookup_by_token_hash(&self, _token_hash: &str) -> AppResult<RemoteDeviceLookup> {
            unreachable!("host-local commands do not resolve tokens")
        }

        async fn get(&self, _id: &RemoteDeviceId) -> AppResult<Option<RemoteDevice>> {
            Ok(Some(self.device.lock().expect("device").clone()))
        }

        async fn list(&self) -> AppResult<Vec<RemoteDevice>> {
            Ok(vec![self.device.lock().expect("device").clone()])
        }

        async fn revoke(&self, _id: &RemoteDeviceId, now: &str) -> AppResult<Option<RemoteDevice>> {
            *self.revocations.lock().expect("revocations") += 1;
            let mut device = self.device.lock().expect("device");
            device.revoked_at = Some(now.to_string());
            Ok(Some(device.clone()))
        }

        async fn set_scopes(
            &self,
            _id: &RemoteDeviceId,
            scopes: &RemoteScopeSet,
        ) -> AppResult<Option<RemoteDevice>> {
            self.scope_writes
                .lock()
                .expect("scope writes")
                .push(scopes.clone());
            let mut device = self.device.lock().expect("device");
            device.scopes = scopes.clone();
            Ok(Some(device.clone()))
        }

        async fn touch_last_seen(&self, _id: &RemoteDeviceId, _now: &str) -> AppResult<()> {
            Ok(())
        }
    }

    struct FailingAuditRepository;

    #[async_trait]
    impl RemoteAuditLogRepository for FailingAuditRepository {
        async fn record(
            &self,
            _device_id: Option<&RemoteDeviceId>,
            _action: RemoteAuditAction,
            _detail: Option<&str>,
            _now: &str,
        ) -> AppResult<()> {
            Err(AppError::Database("database is locked".to_string()))
        }

        async fn list_recent(&self, _limit: Option<i64>) -> AppResult<Vec<RemoteAuditEntry>> {
            Err(AppError::Database("database is locked".to_string()))
        }

        async fn prune_before(&self, _cutoff: &str) -> AppResult<usize> {
            Err(AppError::Database("database is locked".to_string()))
        }
    }

    fn context_with(devices: Arc<RecordingDeviceRepository>) -> RemoteAuthContext {
        let conn = rusqlite::Connection::open_in_memory().expect("in-memory database should open");
        run_migrations(&conn).expect("migrations should apply");
        let mut context = RemoteAuthContext::host_local(
            DbConnection::new(conn),
            crate::remote_server::session_registry::RemoteSessionRegistry::new(),
        );
        context.devices = devices;
        context.audit = Arc::new(FailingAuditRepository);
        context
    }

    #[tokio::test]
    async fn agent_control_grant_is_reverted_and_refused_when_the_audit_write_fails() {
        let devices = Arc::new(RecordingDeviceRepository::with_scopes(
            RemoteScopeSet::default_pairing_grant(),
        ));
        let context = context_with(devices.clone());

        let error = set_agent_control_with_context(
            &context,
            SetRemoteDeviceAgentControlInput {
                device_id: "device-1".to_string(),
                enabled: true,
            },
        )
        .await
        .expect_err("an unaudited grant must not report success");

        assert!(error.contains("audit log"), "unexpected message: {error}");
        let writes = devices.scope_writes();
        assert_eq!(writes.len(), 2, "the grant must be compensated: {writes:?}");
        assert!(
            writes[0].contains(Scope::UiAgent),
            "the grant is written first"
        );
        assert!(
            !writes[1].contains(Scope::UiAgent),
            "the unaudited grant must be reverted"
        );
        assert!(!devices
            .device
            .lock()
            .expect("device")
            .scopes
            .contains(Scope::UiAgent));
    }

    #[tokio::test]
    async fn agent_control_withdrawal_keeps_the_narrowed_grant_but_still_refuses() {
        let devices = Arc::new(RecordingDeviceRepository::with_scopes(
            RemoteScopeSet::default_pairing_grant().with(Scope::UiAgent),
        ));
        let context = context_with(devices.clone());

        let error = set_agent_control_with_context(
            &context,
            SetRemoteDeviceAgentControlInput {
                device_id: "device-1".to_string(),
                enabled: false,
            },
        )
        .await
        .expect_err("an unaudited withdrawal must not report success");

        assert!(error.contains("audit log"), "unexpected message: {error}");
        let writes = devices.scope_writes();
        assert_eq!(
            writes.len(),
            1,
            "withdrawal is fail-safe and must not be reverted: {writes:?}"
        );
        assert!(!devices
            .device
            .lock()
            .expect("device")
            .scopes
            .contains(Scope::UiAgent));
    }

    #[tokio::test]
    async fn device_revocation_stands_but_is_refused_when_the_audit_write_fails() {
        let devices = Arc::new(RecordingDeviceRepository::with_scopes(
            RemoteScopeSet::default_pairing_grant(),
        ));
        let context = context_with(devices.clone());

        let error = revoke_device_with_context(
            &context,
            RemoteDeviceIdInput {
                device_id: "device-1".to_string(),
            },
        )
        .await
        .expect_err("an unaudited revocation must not report success");

        assert!(error.contains("audit log"), "unexpected message: {error}");
        assert_eq!(*devices.revocations.lock().expect("revocations"), 1);
        assert!(
            devices.device.lock().expect("device").revoked_at.is_some(),
            "revocation is the fail-safe direction and must stand"
        );
    }
}
