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
    let message = panic
        .downcast_ref::<String>()
        .map(String::as_str)
        .or_else(|| panic.downcast_ref::<&str>().copied())
        .expect("panic has a string message");
    assert!(
        message.contains("commands::broken::bad()"),
        "panic must name the malformed segment: {message}"
    );
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
