use event_manifest_scanner::{
    reviewed_unmatched_events, scan_consumed_source, scan_rust_source,
    verify_unmatched_event_coverage, ScanError,
};

fn names(source: &str) -> Vec<String> {
    scan_rust_source("fixture.rs", source)
        .expect("fixture scans")
        .into_iter()
        .map(|site| site.name)
        .collect()
}

#[test]
fn resolves_all_required_receiver_and_wrapper_shapes() {
    let names = names(include_str!("fixtures/receiver_shapes.rs"));
    for expected in [
        "task:created",
        "agent:run_completed",
        "task:deleted",
        "agent:chunk",
        "notification:created",
        "task:status_changed",
        "task:archived",
        "execution:queue_changed",
        "ticketing:cache_invalidated",
        "task:restored",
        "task:merge_progress",
        "agent:message_queued",
        "review:update",
    ] {
        assert!(names.iter().any(|name| name == expected), "missing {expected}");
    }
}

#[test]
fn rejects_dynamic_emit_names() {
    let error = scan_rust_source("dynamic.rs", include_str!("fixtures/dynamic_emit.rs"))
        .expect_err("dynamic event name must fail closed");
    assert!(matches!(error, ScanError::UnresolvedEmit { .. }));
}

#[test]
fn rejects_unregistered_wrappers_called_with_event_names() {
    let error = scan_rust_source(
        "unregistered.rs",
        include_str!("fixtures/unregistered_wrapper.rs"),
    )
    .expect_err("wrapper contract must fail");
    assert!(matches!(error, ScanError::UnregisteredWrapper { .. }));
}

#[test]
fn resolves_direct_and_static_mapped_event_bus_subscriptions() {
    let names = scan_consumed_source(
        "subscriptions.tsx",
        include_str!("fixtures/subscriptions.tsx"),
    )
    .expect("static subscriptions scan");
    assert_eq!(
        names,
        vec![
            "agent:run_completed".to_owned(),
            "agent:run_started".to_owned(),
            "notification:created".to_owned(),
        ]
    );
}

#[test]
fn rejects_dynamic_event_bus_subscriptions() {
    let error = scan_consumed_source(
        "dynamic_subscription.ts",
        include_str!("fixtures/dynamic_subscription.ts"),
    )
    .expect_err("dynamic subscription must fail closed");
    assert!(error.to_string().contains("unresolved EventBus subscription"));
}

#[test]
fn renders_reason_coded_reviewed_unmatched_event_gaps() {
    let gaps = reviewed_unmatched_events();
    assert_eq!(gaps.len(), 11);
    assert!(gaps.iter().any(|gap| gap.name() == "execution:stderr"));
    assert!(serde_json::to_value(&gaps)
        .expect("gaps serialize")
        .as_array()
        .expect("gap list")
        .iter()
        .all(|gap| gap.get("reason_code").is_some() && gap.get("reason").is_some()));
}

#[test]
fn rejects_new_unreviewed_unmatched_classification() {
    let error = verify_unmatched_event_coverage(&["new:event"])
        .expect_err("unknown unmatched event must fail CI");
    assert!(error.to_string().contains("no reviewed gap entry"));
}
