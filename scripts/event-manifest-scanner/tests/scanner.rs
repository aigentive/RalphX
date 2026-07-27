use event_manifest_scanner::{
    reviewed_unmatched_events, scan_consumed_source, scan_production_rust_tree, scan_rust_source,
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
fn resolves_qualified_constants_and_rejects_ambiguous_leaf_constants() {
    let source = r#"
        mod module_a { pub const EVENT: &str = "task:created"; }
        mod module_b { pub const EVENT: &str = "task:deleted"; }
        fn emits(app: &App) {
            app.emit(module_a::EVENT, ()).unwrap();
            app.emit(module_b::EVENT, ()).unwrap();
        }
    "#;
    assert_eq!(
        names(source),
        vec!["task:created".to_owned(), "task:deleted".to_owned()]
    );

    let ambiguous = r#"
        mod module_a { pub const EVENT: &str = "task:created"; }
        mod module_b { pub const EVENT: &str = "task:deleted"; }
        fn emits(app: &App) { app.emit(EVENT, ()).unwrap(); }
    "#;
    let error = scan_rust_source("ambiguous.rs", ambiguous)
        .expect_err("an ambiguous bare constant must fail closed");
    assert!(matches!(error, ScanError::UnresolvedEmit { .. }));
}

#[test]
fn resolves_qualified_constants_across_module_files_and_rejects_bare_leaf() {
    let root = tempfile::tempdir().expect("temp root");
    std::fs::write(
        root.path().join("module_a.rs"),
        "pub const EVENT: &str = \"task:created\";",
    )
    .expect("module a");
    std::fs::write(
        root.path().join("module_b.rs"),
        "pub const EVENT: &str = \"task:deleted\";",
    )
    .expect("module b");
    std::fs::write(
        root.path().join("caller.rs"),
        r#"
            fn emits(app: &App) {
                app.emit(module_a::EVENT, ()).unwrap();
                app.emit(module_b::EVENT, ()).unwrap();
            }
        "#,
    )
    .expect("caller");

    let names = scan_production_rust_tree(root.path())
        .expect("qualified multi-file constants resolve")
        .into_iter()
        .map(|site| site.name)
        .collect::<Vec<_>>();
    assert_eq!(names, vec!["task:created", "task:deleted"]);

    std::fs::write(
        root.path().join("caller.rs"),
        "fn emits(app: &App) { app.emit(EVENT, ()).unwrap(); }",
    )
    .expect("ambiguous caller");
    let error = scan_production_rust_tree(root.path())
        .expect_err("bare multi-file constant must be ambiguous");
    assert!(error.to_string().contains("unresolved event name"));
}

#[test]
fn production_census_excludes_test_only_emit_sites() {
    let root = tempfile::tempdir().expect("temp root");
    std::fs::write(
        root.path().join("production.rs"),
        "fn no_events() {}",
    )
    .expect("production source");
    std::fs::create_dir(root.path().join("tests")).expect("tests directory");
    std::fs::write(
        root.path().join("tests/test_only.rs"),
        "fn test_only(app: &App) { app.emit(\"agent:run_started\", ()).unwrap(); }",
    )
    .expect("test source");

    let sites = scan_production_rust_tree(root.path()).expect("scan production tree");
    assert!(sites.is_empty(), "test-only emit must not enter the production census");
    let error = verify_unmatched_event_coverage(&["agent:run_started"])
        .expect_err("test-only classified name remains unmatched");
    assert!(error.to_string().contains("no reviewed gap entry"));
}

#[test]
fn rejects_wrapper_defined_in_one_file_and_called_in_another() {
    let root = tempfile::tempdir().expect("temp root");
    std::fs::write(
        root.path().join("wrapper.rs"),
        "fn unregistered(app: &App, event: &str) { app.emit(event, ()).unwrap(); }",
    )
    .expect("wrapper source");
    std::fs::write(
        root.path().join("caller.rs"),
        "fn call(app: &App) { unregistered(app, \"task:created\"); }",
    )
    .expect("caller source");

    let error = scan_production_rust_tree(root.path())
        .expect_err("cross-file forwarding wrapper must be registered");
    let message = error.to_string();
    assert!(message.contains("unregistered"), "{message}");
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
    let rendered_value = serde_json::to_value(&gaps).expect("gaps serialize");
    let rendered = rendered_value
        .as_array()
        .expect("gap list");
    assert!(rendered
        .iter()
        .all(|gap| gap.get("reason_code").is_some() && gap.get("reason").is_some()));
    assert!(rendered.iter().any(|gap| {
        gap.get("name").and_then(serde_json::Value::as_str) == Some("execution:error")
            && gap.get("reason").and_then(serde_json::Value::as_str)
                == Some("frontend consumer exists; no current Tauri event producer")
    }));
    assert!(rendered.iter().any(|gap| {
        gap.get("name").and_then(serde_json::Value::as_str) == Some("supervisor:event")
            && gap.get("reason").and_then(serde_json::Value::as_str)
                == Some("frontend consumer exists; backend has internal Supervisor EventBus but no Tauri bridge")
    }));
}

#[test]
fn rejects_new_unreviewed_unmatched_classification() {
    let error = verify_unmatched_event_coverage(&["new:event"])
        .expect_err("unknown unmatched event must fail CI");
    assert!(error.to_string().contains("no reviewed gap entry"));
}
