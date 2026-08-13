use std::sync::Arc;

use crate::application::desktop_notification::{build_send_spec, NativeSendSpec};
use crate::application::desktop_notification_budget::{ClickWaitBudget, SendMode};
use crate::domain::entities::{
    NewNotification, NotificationCategory, NotificationSeverity, NotificationTarget,
};

fn notification(title: &str, body: Option<&str>) -> crate::domain::entities::Notification {
    NewNotification {
        project_id: Some("project-1".into()),
        category: NotificationCategory::ReviewNeeded,
        severity: NotificationSeverity::ActionRequired,
        title: title.into(),
        body: body.map(str::to_owned),
        target: NotificationTarget::none(),
        dedupe_key: None,
    }
    .into_notification(chrono::Utc::now())
}

fn granted_permit() -> SendMode {
    Arc::new(ClickWaitBudget::new(1)).plan_send()
}

#[test]
fn a_granted_permit_produces_a_click_waiting_send() {
    let notification = notification("Review needed", Some("Task 42 is ready"));

    let spec = build_send_spec(&notification, &granted_permit());

    assert_eq!(
        spec,
        NativeSendSpec {
            title: "Review needed".to_string(),
            message: "Task 42 is ready".to_string(),
            wait_for_click: true,
        }
    );
}

#[test]
fn an_over_budget_send_never_waits_for_a_click() {
    let notification = notification("Review needed", Some("Task 42 is ready"));

    let spec = build_send_spec(&notification, &SendMode::FireAndForget);

    assert!(
        !spec.wait_for_click,
        "over-budget notifications must not register a main-thread click-wait timer"
    );
    assert_eq!(spec.title, "Review needed");
    assert_eq!(spec.message, "Task 42 is ready");
}

#[test]
fn a_missing_body_becomes_an_empty_message_in_both_modes() {
    let notification = notification("Agent is waiting", None);

    assert_eq!(
        build_send_spec(&notification, &granted_permit()).message,
        ""
    );
    assert_eq!(
        build_send_spec(&notification, &SendMode::FireAndForget).message,
        ""
    );
}
