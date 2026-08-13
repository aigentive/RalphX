//! TTL reaper for delivered macOS notifications.
//!
//! `mac_notification_sys` exposes no timeout and no cancellation for a parked click-wait. The one
//! lever it does give us is auto-dismiss detection: when a notification disappears from Notification
//! Center, the crate's poll resolves the waiter, its main-run-loop timer is invalidated, and the
//! waiter thread exits — releasing its click-wait permit. So removing stale entries is how
//! click-wait slots are reclaimed, and it clears accumulated notification clutter as a bonus.
//!
//! Every `NSUserNotificationCenter` call must happen on the main thread, matching the crate's own
//! constraint. One `deliveredNotifications` call per reap interval is negligible next to the ~110
//! calls/sec that unbounded click-waits produced.
#![allow(deprecated)]

use std::sync::OnceLock;
use std::time::{Duration, SystemTime};

use objc2_foundation::NSUserNotificationCenter;
use tauri::{AppHandle, Runtime};

use crate::application::desktop_notification_budget::click_wait_budget;
use crate::infrastructure::agents::claude::stream_timeouts;

/// Minimum spacing between reap ticks, so a misconfigured interval cannot spin.
const MIN_REAP_INTERVAL: Duration = Duration::from_secs(1);

/// A notification currently sitting in Notification Center, reduced to what expiry needs.
pub(crate) struct DeliveredEntry {
    pub(crate) identifier: Option<String>,
    pub(crate) delivered_at: Option<SystemTime>,
}

/// Positions of entries safe to remove: ours (the crate always sets a UUID identifier) and strictly
/// older than `ttl`.
///
/// Anything we cannot positively identify or cannot age is kept. That protects genuine third-party
/// notifications, which matters in dev runs where RalphX borrows Terminal's bundle identifier.
pub(crate) fn select_expired(
    entries: &[DeliveredEntry],
    now: SystemTime,
    ttl: Duration,
) -> Vec<usize> {
    entries
        .iter()
        .enumerate()
        .filter_map(|(index, entry)| {
            let identifier = entry.identifier.as_deref()?;
            uuid::Uuid::parse_str(identifier).ok()?;
            let age = now.duration_since(entry.delivered_at?).ok()?;
            (age > ttl).then_some(index)
        })
        .collect()
}

static REAPER_STARTED: OnceLock<()> = OnceLock::new();

/// Starts the reaper thread once per process. Called from the actionable-notification path, so it
/// stays free until RalphX actually sends one.
pub(crate) fn ensure_started<R: Runtime>(app_handle: AppHandle<R>) {
    if REAPER_STARTED.set(()).is_err() {
        return;
    }

    let timeouts = stream_timeouts();
    let interval = Duration::from_secs(timeouts.desktop_notification_reap_interval_secs)
        .max(MIN_REAP_INTERVAL);
    let ttl = Duration::from_secs(timeouts.desktop_notification_click_wait_ttl_secs);

    if let Err(error) = std::thread::Builder::new()
        .name("ralphx-notification-reaper".to_string())
        .spawn(move || run_reaper(app_handle, interval, ttl))
    {
        tracing::warn!(
            error = %error,
            "Failed to start desktop notification reaper; stale notifications will not be cleared"
        );
    }
}

fn run_reaper<R: Runtime>(app_handle: AppHandle<R>, interval: Duration, ttl: Duration) {
    // The first tick always runs so leftovers from a previous app run get cleared even if nothing
    // is waiting; later ticks only matter while click-waits are outstanding.
    let mut hygiene_tick_pending = true;

    loop {
        std::thread::sleep(interval);

        if !hygiene_tick_pending && click_wait_budget().active_count() == 0 {
            continue;
        }
        hygiene_tick_pending = false;

        if let Err(error) = app_handle.run_on_main_thread(move || reap(ttl)) {
            tracing::debug!(error = %error, "Skipped desktop notification reap tick");
        }
    }
}

/// Removes expired RalphX notifications. Main thread only.
fn reap(ttl: Duration) {
    let center = NSUserNotificationCenter::defaultUserNotificationCenter();
    let delivered = center.deliveredNotifications().to_vec();

    let entries: Vec<DeliveredEntry> = delivered
        .iter()
        .map(|notification| DeliveredEntry {
            identifier: notification
                .identifier()
                .map(|identifier| identifier.to_string()),
            delivered_at: notification
                .actualDeliveryDate()
                .and_then(|date| system_time_from_epoch_secs(date.timeIntervalSince1970())),
        })
        .collect();

    let expired = select_expired(&entries, SystemTime::now(), ttl);
    if expired.is_empty() {
        return;
    }

    for index in &expired {
        if let Some(notification) = delivered.get(*index) {
            center.removeDeliveredNotification(notification);
        }
    }

    tracing::debug!(
        removed = expired.len(),
        "Removed stale RalphX notifications from Notification Center"
    );
}

fn system_time_from_epoch_secs(seconds: f64) -> Option<SystemTime> {
    let offset = Duration::try_from_secs_f64(seconds).ok()?;
    SystemTime::UNIX_EPOCH.checked_add(offset)
}
