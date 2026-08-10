use ralphx_remote_protocol::ResetReason;

use super::session_registry::{
    RemoteSessionAdmission, RemoteSessionRegistry, MAX_SESSIONS_PER_DEVICE,
};
use crate::domain::entities::{RemoteDeviceId, RemoteSessionId};

fn admit(
    registry: &RemoteSessionRegistry,
    device_id: &RemoteDeviceId,
    session_id: &RemoteSessionId,
) -> super::session_registry::RemoteSessionKillChannel {
    match registry.register(device_id, session_id) {
        RemoteSessionAdmission::Admitted(channel) => channel,
        RemoteSessionAdmission::CapExceeded { limit } => {
            panic!("session should be admitted below the cap of {limit}")
        }
    }
}

#[test]
fn revoking_a_device_signals_every_one_of_its_sessions_and_forgets_them() {
    let registry = RemoteSessionRegistry::new();
    let device = RemoteDeviceId::from_string("device-1");
    let first_id = RemoteSessionId::from_string("session-1");
    let second_id = RemoteSessionId::from_string("session-2");
    let mut first = admit(&registry, &device, &first_id);
    let mut second = admit(&registry, &device, &second_id);

    let signalled = registry.kill_device(&device, ResetReason::Revoked);

    assert_eq!(signalled, 2);
    assert_eq!(first.try_recv(), Some(ResetReason::Revoked));
    assert_eq!(second.try_recv(), Some(ResetReason::Revoked));
    assert_eq!(registry.device_session_count(&device), 0);
    assert_eq!(registry.live_session_count(), 0);
}

#[test]
fn teardown_never_reaches_another_devices_sessions() {
    let registry = RemoteSessionRegistry::new();
    let revoked = RemoteDeviceId::from_string("device-revoked");
    let bystander = RemoteDeviceId::from_string("device-bystander");
    let mut revoked_channel = admit(
        &registry,
        &revoked,
        &RemoteSessionId::from_string("session-a"),
    );
    let mut bystander_channel = admit(
        &registry,
        &bystander,
        &RemoteSessionId::from_string("session-b"),
    );

    registry.kill_device(&revoked, ResetReason::Revoked);

    assert_eq!(revoked_channel.try_recv(), Some(ResetReason::Revoked));
    assert_eq!(
        bystander_channel.try_recv(),
        None,
        "an unrelated device must keep its live session"
    );
    assert_eq!(registry.device_session_count(&bystander), 1);
}

#[test]
fn disabling_the_listener_tears_down_every_device() {
    let registry = RemoteSessionRegistry::new();
    let first = RemoteDeviceId::from_string("device-1");
    let second = RemoteDeviceId::from_string("device-2");
    let mut first_channel = admit(&registry, &first, &RemoteSessionId::from_string("s-1"));
    let mut second_channel = admit(&registry, &second, &RemoteSessionId::from_string("s-2"));

    let signalled = registry.kill_all(ResetReason::HostDisabled);

    assert_eq!(signalled, 2);
    assert_eq!(first_channel.try_recv(), Some(ResetReason::HostDisabled));
    assert_eq!(second_channel.try_recv(), Some(ResetReason::HostDisabled));
    assert_eq!(registry.live_session_count(), 0);
}

#[test]
fn killing_one_session_is_precise_idempotent_and_cleans_the_device_entry() {
    let registry = RemoteSessionRegistry::new();
    let device = RemoteDeviceId::from_string("device-1");
    let unknown_device = RemoteDeviceId::from_string("device-unknown");
    let first_id = RemoteSessionId::from_string("session-1");
    let second_id = RemoteSessionId::from_string("session-2");
    let unknown_session = RemoteSessionId::from_string("session-unknown");
    let mut first = admit(&registry, &device, &first_id);
    let mut second = admit(&registry, &device, &second_id);

    assert!(!registry.kill_session(&unknown_device, &unknown_session, ResetReason::Revoked));
    assert!(!registry.kill_session(&device, &unknown_session, ResetReason::Revoked));
    assert!(registry.kill_session(&device, &first_id, ResetReason::Revoked));
    assert_eq!(first.try_recv(), Some(ResetReason::Revoked));
    assert_eq!(second.try_recv(), None);
    assert_eq!(registry.live_sessions(&device), vec![second_id.clone()]);

    assert!(registry.kill_session(&device, &second_id, ResetReason::HostDisabled));
    assert_eq!(second.try_recv(), Some(ResetReason::HostDisabled));
    assert_eq!(registry.device_session_count(&device), 0);
    assert_eq!(registry.live_session_count(), 0);
    assert!(!registry.kill_session(&device, &second_id, ResetReason::HostDisabled));
}

#[test]
fn a_second_teardown_of_the_same_device_signals_nothing() {
    let registry = RemoteSessionRegistry::new();
    let device = RemoteDeviceId::from_string("device-1");
    let _channel = admit(&registry, &device, &RemoteSessionId::from_string("s-1"));

    let first = registry.kill_device(&device, ResetReason::Revoked);
    let second = registry.kill_device(&device, ResetReason::Revoked);

    assert_eq!(first, 1);
    assert_eq!(second, 0, "teardown must be idempotent, not double-counted");
}

#[test]
fn a_session_that_closes_on_its_own_leaves_no_stale_entry() {
    let registry = RemoteSessionRegistry::new();
    let device = RemoteDeviceId::from_string("device-1");
    let session = RemoteSessionId::from_string("s-1");
    let _channel = admit(&registry, &device, &session);

    registry.unregister(&device, &session);
    registry.unregister(&device, &session);

    assert_eq!(registry.live_sessions(&device), Vec::new());
    assert_eq!(registry.live_session_count(), 0);
    assert_eq!(registry.kill_device(&device, ResetReason::Revoked), 0);
}

#[test]
fn a_device_cannot_exceed_the_session_cap() {
    let registry = RemoteSessionRegistry::new();
    let device = RemoteDeviceId::from_string("device-1");
    let mut channels = Vec::new();
    for index in 0..MAX_SESSIONS_PER_DEVICE {
        channels.push(admit(
            &registry,
            &device,
            &RemoteSessionId::from_string(format!("session-{index}")),
        ));
    }

    let over_cap = registry.register(&device, &RemoteSessionId::from_string("session-extra"));

    assert!(matches!(
        over_cap,
        RemoteSessionAdmission::CapExceeded {
            limit: MAX_SESSIONS_PER_DEVICE
        }
    ));
    assert_eq!(
        registry.device_session_count(&device),
        MAX_SESSIONS_PER_DEVICE
    );
}

#[test]
fn re_registering_a_session_id_replaces_its_channel_without_consuming_cap() {
    let registry = RemoteSessionRegistry::new();
    let device = RemoteDeviceId::from_string("device-1");
    let session = RemoteSessionId::from_string("session-1");
    let mut stale = admit(&registry, &device, &session);
    let mut fresh = admit(&registry, &device, &session);

    registry.kill_device(&device, ResetReason::Revoked);

    assert_eq!(registry.device_session_count(&device), 0);
    assert_eq!(fresh.try_recv(), Some(ResetReason::Revoked));
    assert_eq!(
        stale.try_recv(),
        None,
        "the replaced channel must not be double-signalled"
    );
}

#[tokio::test]
async fn a_live_session_awaits_its_kill_signal() {
    let registry = RemoteSessionRegistry::new();
    let device = RemoteDeviceId::from_string("device-1");
    let mut channel = admit(&registry, &device, &RemoteSessionId::from_string("s-1"));
    let waiter = tokio::spawn(async move { channel.recv().await });

    registry.kill_device(&device, ResetReason::Revoked);

    assert_eq!(
        waiter.await.expect("waiter should finish"),
        Some(ResetReason::Revoked)
    );
}
