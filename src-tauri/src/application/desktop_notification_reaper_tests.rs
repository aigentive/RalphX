use std::time::{Duration, SystemTime};

use crate::application::desktop_notification_reaper::{select_expired, DeliveredEntry};

const TTL: Duration = Duration::from_secs(900);

fn now() -> SystemTime {
    SystemTime::UNIX_EPOCH + Duration::from_secs(1_800_000_000)
}

fn ralphx_entry(age: Duration) -> DeliveredEntry {
    DeliveredEntry {
        identifier: Some(uuid::Uuid::new_v4().to_string()),
        delivered_at: Some(now() - age),
    }
}

#[test]
fn selects_ralphx_entries_older_than_the_ttl() {
    let entries = vec![ralphx_entry(TTL + Duration::from_secs(1))];

    assert_eq!(select_expired(&entries, now(), TTL), vec![0]);
}

#[test]
fn keeps_entries_at_or_inside_the_ttl_boundary() {
    let entries = vec![ralphx_entry(TTL), ralphx_entry(Duration::from_secs(1))];

    assert!(select_expired(&entries, now(), TTL).is_empty());
}

#[test]
fn keeps_notifications_that_are_not_ours() {
    let entries = vec![DeliveredEntry {
        identifier: Some("com.example.some-other-app".to_string()),
        delivered_at: Some(now() - (TTL * 4)),
    }];

    assert!(select_expired(&entries, now(), TTL).is_empty());
}

#[test]
fn keeps_notifications_without_an_identifier() {
    let entries = vec![DeliveredEntry {
        identifier: None,
        delivered_at: Some(now() - (TTL * 4)),
    }];

    assert!(select_expired(&entries, now(), TTL).is_empty());
}

#[test]
fn keeps_notifications_that_cannot_be_aged() {
    let entries = vec![DeliveredEntry {
        identifier: Some(uuid::Uuid::new_v4().to_string()),
        delivered_at: None,
    }];

    assert!(select_expired(&entries, now(), TTL).is_empty());
}

#[test]
fn returns_positional_indices_for_a_mixed_list() {
    let entries = vec![
        ralphx_entry(Duration::from_secs(5)),
        ralphx_entry(TTL * 2),
        DeliveredEntry {
            identifier: Some("not-a-uuid".to_string()),
            delivered_at: Some(now() - (TTL * 2)),
        },
        ralphx_entry(TTL + Duration::from_secs(1)),
    ];

    assert_eq!(select_expired(&entries, now(), TTL), vec![1, 3]);
}

#[test]
fn empty_input_selects_nothing() {
    assert!(select_expired(&[], now(), TTL).is_empty());
}

#[test]
fn future_delivery_dates_are_never_selected() {
    let entries = vec![DeliveredEntry {
        identifier: Some(uuid::Uuid::new_v4().to_string()),
        delivered_at: Some(now() + Duration::from_secs(60)),
    }];

    assert!(select_expired(&entries, now(), TTL).is_empty());
}
