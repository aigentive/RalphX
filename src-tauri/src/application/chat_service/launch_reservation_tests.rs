use std::sync::Arc;
use std::time::Duration;

use super::launch_reservation::LaunchReservationGuard;
use crate::domain::services::{MemoryRunningAgentRegistry, RunningAgentKey, RunningAgentRegistry};

#[tokio::test]
async fn guard_renews_only_the_owned_launch_reservation() {
    let registry = Arc::new(MemoryRunningAgentRegistry::new());
    let key = RunningAgentKey::new("project", "slow-launch");
    registry
        .try_register(key.clone(), "conversation".into(), "run-owned".into())
        .await
        .unwrap();
    let before = registry.get(&key).await.unwrap().last_active_at.unwrap();

    let guard = LaunchReservationGuard::new(
        registry.clone(),
        key.clone(),
        "run-owned".into(),
        Duration::from_millis(30),
    );
    tokio::time::sleep(Duration::from_millis(25)).await;
    guard.stop();

    let after = registry.get(&key).await.unwrap().last_active_at.unwrap();
    assert!(after > before);
    assert!(!registry
        .renew_reservation(&key, "run-stale", chrono::Utc::now())
        .await
        .unwrap());
}

#[tokio::test]
async fn coverage_regression_guard_stops_after_losing_reservation_ownership() {
    let registry = Arc::new(MemoryRunningAgentRegistry::new());
    let key = RunningAgentKey::new("project", "replaced-launch");
    registry
        .try_register(key.clone(), "conversation".into(), "run-current".into())
        .await
        .unwrap();
    let before = registry.get(&key).await.unwrap().last_active_at.unwrap();

    let guard = LaunchReservationGuard::new(
        registry.clone(),
        key.clone(),
        "run-stale".into(),
        Duration::from_millis(30),
    );
    tokio::time::sleep(Duration::from_millis(25)).await;

    let current = registry.get(&key).await.unwrap();
    assert_eq!(current.agent_run_id, "run-current");
    assert_eq!(current.last_active_at, Some(before));
    drop(guard);
}
