use super::authority_audit::*;
use std::collections::{BTreeSet, HashMap, VecDeque};
use syn::visit::Visit;

#[test]
fn registered_command_parser_handles_layout_variants() {
    let source = r#"
        pub fn handlers() {
            tauri::generate_handler![
                commands::alpha::one, commands::beta::two,
                commands::gamma::three, // inline explanation
                #[cfg(debug_assertions)]
                commands::delta::four,
                greet,
            ]
        }
    "#;

    assert_eq!(
        parse_registered_commands(source),
        vec![
            ("one".to_string(), "alpha".to_string()),
            ("two".to_string(), "beta".to_string()),
            ("three".to_string(), "gamma".to_string()),
            ("four".to_string(), "delta".to_string()),
            ("greet".to_string(), "root".to_string()),
        ]
    );
}

fn panic_message(panic: &(dyn std::any::Any + Send)) -> String {
    panic
        .downcast_ref::<String>()
        .map(String::as_str)
        .or_else(|| panic.downcast_ref::<&str>().copied())
        .expect("panic has a string message")
        .to_string()
}

#[test]
fn registered_command_parser_panics_with_the_malformed_segment() {
    let source = r#"
        tauri::generate_handler![
            commands::alpha::good,
            commands::broken::bad(),
        ]
    "#;

    let panic = std::panic::catch_unwind(|| parse_registered_commands(source))
        .expect_err("malformed census segment must fail closed");
    let message = panic_message(panic.as_ref());
    assert!(
        message.contains("commands::broken::bad()"),
        "panic must name the malformed segment: {message}"
    );
}

/// The graph is a floor only if it is the whole program. An unreadable directory used to be
/// skipped silently, which is the same silent-graph-shrinkage the parse-failure panic exists to
/// prevent — and it could be baked into the checked-in manifest by a regeneration run.
#[cfg(unix)]
#[test]
fn unreadable_source_directory_is_a_hard_error() {
    use std::os::unix::fs::PermissionsExt;

    let fixture = tempfile::tempdir().expect("fixture root is creatable");
    let root = fixture.path().to_path_buf();
    std::fs::write(root.join("readable.rs"), "fn readable() {}\n").expect("seed readable source");
    let locked = root.join("locked");
    std::fs::create_dir(&locked).expect("fixture subdirectory is creatable");
    std::fs::write(locked.join("hidden.rs"), "fn hidden() {}\n").expect("seed hidden source");
    std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o000))
        .expect("directory permissions are settable");

    let walk_root = root.clone();
    let outcome = std::panic::catch_unwind(|| {
        let mut out = Vec::new();
        collect_rs_files(&walk_root, &walk_root, &mut out);
        out
    });
    std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o755))
        .expect("directory permissions are restorable");

    let panic = outcome.expect_err("an unreadable directory must fail closed, not shrink silently");
    let message = panic_message(panic.as_ref());
    assert!(
        message.contains("could not read directory"),
        "panic must name the unreadable directory: {message}"
    );
}

#[cfg(unix)]
#[test]
fn unreadable_source_file_is_a_hard_error() {
    use std::os::unix::fs::PermissionsExt;

    let fixture = tempfile::tempdir().expect("fixture root is creatable");
    let root = fixture.path().to_path_buf();
    let locked = root.join("locked.rs");
    std::fs::write(&locked, "fn locked() {}\n").expect("seed locked source");
    std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o000))
        .expect("file permissions are settable");

    let walk_root = root.clone();
    let outcome = std::panic::catch_unwind(|| {
        let mut out = Vec::new();
        collect_rs_files(&walk_root, &walk_root, &mut out);
        out
    });
    std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o644))
        .expect("file permissions are restorable");

    let panic = outcome.expect_err("an unreadable source file must fail closed");
    let message = panic_message(panic.as_ref());
    assert!(
        message.contains("could not read") && message.contains("locked.rs"),
        "panic must name the unreadable file: {message}"
    );
}

/// A fixture is not production authority surface.
///
/// The walk is a filesystem walk, but the graph it feeds must be the PRODUCTION program. A module
/// compiled only under `cfg(test)` or the `test-utils` feature is not in that program, and letting
/// it in donates call edges no production caller has. The exclusion is keyed on the `mod` item's
/// own `cfg` rather than on a filename, so the next fixture to arrive under a different name
/// cannot regress this silently.
#[test]
fn test_gated_module_declarations_are_excluded_from_the_walk() {
    let fixture = tempfile::tempdir().expect("fixture root is creatable");
    let root = fixture.path().to_path_buf();
    // The crate root is owned by `lib.rs`/`main.rs`; every other directory is owned by `mod.rs`.
    // Both owner forms are exercised, and a gated *directory* module as well as a gated file.
    std::fs::write(
        root.join("lib.rs"),
        r#"
            pub mod inner;
            #[cfg(feature = "test-utils")]
            pub mod fixtures;
        "#,
    )
    .expect("seed crate root");
    let inner = root.join("inner");
    std::fs::create_dir(&inner).expect("fixture subdirectory is creatable");
    std::fs::write(
        inner.join("mod.rs"),
        r#"
            pub mod real;
            #[cfg(feature = "test-utils")]
            pub mod harness;
            #[cfg(test)]
            mod scratch;
            #[cfg(all(test, feature = "test-utils"))]
            mod both;
            #[cfg(unix)]
            pub mod platform;
            #[cfg(feature = "latest")]
            pub mod newest;
        "#,
    )
    .expect("seed owning module file");
    for name in ["real", "harness", "scratch", "both", "platform", "newest"] {
        std::fs::write(
            inner.join(format!("{name}.rs")),
            format!("fn {name}() {{}}\n"),
        )
        .expect("seed module source");
    }
    let fixtures = root.join("fixtures");
    std::fs::create_dir(&fixtures).expect("gated module directory is creatable");
    std::fs::write(fixtures.join("mod.rs"), "pub mod leaf;\n").expect("seed gated module dir");
    std::fs::write(fixtures.join("leaf.rs"), "fn leaf() {}\n").expect("seed gated leaf");

    let mut out = Vec::new();
    collect_rs_files(&root, &root, &mut out);
    let scanned = out
        .iter()
        .map(|(path, _)| path.as_str())
        .collect::<BTreeSet<_>>();

    for kept in ["inner/real.rs", "inner/platform.rs", "inner/newest.rs"] {
        assert!(
            scanned.contains(kept),
            "{kept} is not test-gated and must stay in the production graph: {scanned:?}"
        );
    }
    for gated in [
        "inner/harness.rs",
        "inner/scratch.rs",
        "inner/both.rs",
        "fixtures/mod.rs",
        "fixtures/leaf.rs",
    ] {
        assert!(
            !scanned.contains(gated),
            "{gated} sits behind a test-only cfg and must never be scanned: {scanned:?}"
        );
    }

    let gates = test_gated_module_files(&root);
    assert_eq!(
        gates.keys().map(String::as_str).collect::<BTreeSet<_>>(),
        BTreeSet::from([
            "fixtures/mod.rs",
            "inner/both.rs",
            "inner/harness.rs",
            "inner/scratch.rs",
        ]),
        "the gate enumeration and the walk exclusion must agree"
    );
}

/// The general rule, enforced against the real tree.
///
/// PR 3.2 added `remote_server/harness.rs` behind `#[cfg(feature = "test-utils")]`. Because the
/// loader excluded only `*_tests.rs`/`tests.rs`/`mocks.rs`, the fixture's `start/0` collided with
/// `ResearchProcess::start/0` in arity-keyed dispatch and mis-attributed `start_research` to the
/// `workspace-bridge` state surface. Fixtures must never contribute authority rows — in EITHER
/// direction; the same collision could as easily have hidden a real writer.
#[test]
fn no_test_gated_module_reaches_the_production_source_load() {
    let scanned = load_production_sources()
        .into_iter()
        .map(|(path, _)| path)
        .collect::<BTreeSet<_>>();
    for (path, gated) in test_gated_module_files(&crate_src_root()) {
        assert!(
            !scanned.contains(&path),
            "{path} is declared behind `{gated}` and must not feed the authority graph"
        );
    }
    assert!(
        !scanned.contains("remote_server/harness.rs"),
        "the PR 3.2 chat-stream fixture must stay out of the production authority graph"
    );
}

#[test]
fn production_source_load_reaches_the_whole_tree() {
    let files = load_production_sources();
    assert!(
        files.len() >= MIN_PRODUCTION_SOURCE_FILES,
        "production source load fell below the collapse floor: {}",
        files.len()
    );
    // Recursion actually reached deep leaves, not just the crate-root files.
    for expected in [
        "commands/registry.rs",
        "remote_server/registry.rs",
        "application/ready_task_scheduler.rs",
    ] {
        assert!(
            files.iter().any(|(path, _)| path == expected),
            "production source walk missed {expected}"
        );
    }
}

#[test]
fn method_spawn_and_listen_shapes_are_inventory_roots_with_body_authority() {
    let source = r#"
        fn roots(handle: Handle, app: App) {
            handle.spawn(async { send_message(); });
            handle.spawn_blocking(|| send_message());
            app.listen("event", move |_| { send_message(); });
        }
        fn inert(handle: Handle, app: App) {
            handle.spawn(async { inspect(); });
            handle.spawn_blocking(|| inspect());
            app.listen("event", move |_| { inspect(); });
        }
    "#;
    let graph = CallGraph::build(&[("synthetic.rs".to_string(), source.to_string())]);

    let authoritative = graph
        .loop_roots
        .iter()
        .filter(|root| root.enclosing_fn.ends_with("::roots"))
        .collect::<Vec<_>>();
    assert_eq!(
        authoritative.len(),
        3,
        "spawn, spawn_blocking, and listen must each be discovered"
    );
    assert!(
        authoritative
            .iter()
            .all(|root| closure_is_arming(&graph.loop_closure(root))),
        "each send_message body must be authority-bearing"
    );

    let inert = graph
        .loop_roots
        .iter()
        .filter(|root| root.enclosing_fn.ends_with("::inert"))
        .collect::<Vec<_>>();
    assert_eq!(
        inert.len(),
        3,
        "inert forms must still be present in the inventory"
    );
    assert!(
        inert
            .iter()
            .all(|root| !closure_is_arming(&graph.loop_closure(root))),
        "inert bodies must not acquire authority"
    );
}

#[test]
fn agent_spawner_shape_is_mutually_exclusive_with_method_loop_spawn() {
    let source = r#"fn shapes(spawner: Spawner, handle: Handle, id: Id) {
            spawner.spawn("worker", id);
            handle.spawn(async { send_message(); });
        }"#;
    let file = syn::parse_file(source).unwrap();
    let mut calls = Vec::new();
    struct MethodCollector<'a>(&'a mut Vec<syn::ExprMethodCall>);
    impl<'ast> syn::visit::Visit<'ast> for MethodCollector<'_> {
        fn visit_expr_method_call(&mut self, node: &'ast syn::ExprMethodCall) {
            self.0.push(node.clone());
            syn::visit::visit_expr_method_call(self, node);
        }
    }
    MethodCollector(&mut calls).visit_file(&file);

    assert!(is_agent_spawn_method(
        &calls[0].method.to_string(),
        &calls[0].args
    ));
    assert!(!is_method_background_spawn(
        &calls[0].method.to_string(),
        &calls[0].args
    ));
    assert!(!is_agent_spawn_method(
        &calls[1].method.to_string(),
        &calls[1].args
    ));
    assert!(is_method_background_spawn(
        &calls[1].method.to_string(),
        &calls[1].args
    ));

    let graph = CallGraph::build(&[("synthetic.rs".to_string(), source.to_string())]);
    assert_eq!(
        graph.loop_roots.len(),
        1,
        "AgentSpawner string-first call must not be double-counted as a loop"
    );
}

#[test]
fn failed_rearms_while_cancelled_and_archived_remain_halting() {
    let hit = |target: &str| SinkHit {
        sink: "transition_task".to_string(),
        targets: BTreeSet::from([target.to_string()]),
    };

    assert_eq!(
        verdict_for(&hit("Failed")),
        HitVerdict::Arming,
        "Failed is scanned by execution reconciliation and auto-retries to Ready"
    );
    assert_eq!(
        verdict_for(&hit("Cancelled")),
        HitVerdict::Halting,
        "Cancelled stops pollers and has no on-enter spawn action"
    );
    assert_eq!(
        verdict_for(&hit("Archived")),
        HitVerdict::Halting,
        "Archived has no task on-enter action or reconciliation scan"
    );
}

fn trace(graph: &CallGraph, root: &str) -> Option<Vec<String>> {
    let sinks: BTreeSet<String> = TRANSITION_SINKS
        .iter()
        .chain(SCHEDULER_SINKS.iter())
        .chain(STEER_SINKS.iter())
        .map(|s| s.to_string())
        .collect();
    let mut parents: HashMap<String, String> = HashMap::new();
    let mut seen: BTreeSet<String> = BTreeSet::new();
    let mut queue = VecDeque::new();
    for resolved in graph.roots_named(root) {
        seen.insert(resolved.clone());
        queue.push_back(resolved);
    }
    while let Some(name) = queue.pop_front() {
        let Some(node) = graph.nodes.get(&name) else {
            continue;
        };
        if node
            .sink_hits
            .iter()
            .any(|hit| verdict_for(hit) == HitVerdict::Arming)
            && name != root
        {
            let mut path = vec![format!(
                "{name} !! {:?}",
                node.sink_hits
                    .iter()
                    .map(|h| h.sink.clone())
                    .collect::<Vec<_>>()
            )];
            let mut cur = name.clone();
            while let Some(p) = parents.get(&cur) {
                path.push(p.clone());
                cur = p.clone();
            }
            path.reverse();
            return Some(path);
        }
        for callee in &node.callees {
            if sinks.contains(callee) || seen.contains(callee) {
                continue;
            }
            seen.insert(callee.clone());
            parents.insert(callee.clone(), name.clone());
            queue.push_back(callee.clone());
        }
    }
    None
}

#[test]
#[ignore = "calibration probe"]
fn probe_paths() {
    let files = load_production_sources();
    let graph = CallGraph::build(&files);
    eprintln!("PROBE files={} nodes={}", files.len(), graph.nodes.len());
    for root in [
        "list_tasks",
        "get_task",
        "health_check",
        "list_projects",
        "get_notification_settings",
        "list_remote_advertised_endpoints",
        "list_remote_audit_entries",
        "pause_task",
        "block_task",
        "stop_task",
        "pause_tasks_in_group",
        "deny_permission_request",
    ] {
        let closure = graph.closure([root.to_string()]);
        let verdict = closure_is_arming(&closure);
        match trace(&graph, root) {
            Some(path) => eprintln!("PROBE {root} arming={verdict} => {}", path.join(" -> ")),
            None => eprintln!("PROBE {root} arming={verdict} => NO SINK"),
        }
        eprintln!("PROBE {root} hits={:?}", closure.sink_hits);
    }
    // Which node names are defined the most times (name-collision pressure)?
    let mut fanout: Vec<(usize, String)> = graph
        .nodes
        .iter()
        .map(|(name, node)| (node.callees.len(), name.clone()))
        .collect();
    fanout.sort_by(|a, b| b.0.cmp(&a.0));
    eprintln!("PROBE top-fanout {:?}", &fanout[..20.min(fanout.len())]);
}
