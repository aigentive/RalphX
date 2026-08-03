use super::authority_audit::*;
use std::collections::{BTreeMap, BTreeSet, HashMap, VecDeque};
use std::path::Path;
use syn::visit::Visit;

/// Loads every production `.rs` file under `src-tauri/src` and the linked workspace crates.
///
/// `*_tests.rs` files are excluded: test bodies would inject edges no production caller has.
/// The cfg-gated-fixture exclusion applies per walk root, so a `#[cfg(test)]`-gated module in a
/// workspace crate is skipped there exactly as it is in the app crate.
///
/// Every I/O failure is a hard error. An unreadable directory or file shrinks the call graph
/// exactly the way an unparseable file does, and the parse path already panics rather than
/// skip (see [`CallGraph::build`]); silent skipping here would have been the same
/// silent-graph-shrinkage with a quieter failure mode.
pub fn load_production_sources() -> Vec<(String, String)> {
    let root = crate_src_root();
    let mut files = Vec::new();
    collect_rs_files(&root, &root, &mut files);

    for crate_name in LINKED_WORKSPACE_CRATES {
        let crate_root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("crates")
            .join(crate_name)
            .join("src");
        let mut crate_files = Vec::new();
        collect_rs_files(&crate_root, &crate_root, &mut crate_files);
        assert!(
            crate_files.len() >= MIN_WORKSPACE_CRATE_SOURCE_FILES,
            "authority audit loaded {} sources from workspace crate {crate_name}: the walk lost the crate",
            crate_files.len()
        );
        files.extend(
            crate_files
                .into_iter()
                .map(|(relative, source)| (format!("crates/{crate_name}/src/{relative}"), source)),
        );
    }

    files.sort_by(|a, b| a.0.cmp(&b.0));
    assert!(
        files.len() >= MIN_PRODUCTION_SOURCE_FILES,
        "authority audit loaded only {} production sources, below the {MIN_PRODUCTION_SOURCE_FILES} floor: the graph collapsed",
        files.len()
    );
    files
}

/// Whether a `cfg` attribute gates its item out of every production build.
///
/// Matched on whole words so `feature = "latest"` is not mistaken for a test gate; nested groups
/// are walked so `all(test, feature = "test-utils")` is caught as well as bare `cfg(test)`.
fn cfg_is_test_only(attr: &syn::Attribute) -> Option<String> {
    let syn::Meta::List(list) = &attr.meta else {
        return None;
    };
    if !list.path.is_ident("cfg") {
        return None;
    }
    let rendered = list.tokens.to_string();
    let is_test_only = rendered
        .split(|character: char| {
            !character.is_ascii_alphanumeric() && character != '_' && character != '-'
        })
        .any(|word| word == "test" || word == "test-utils");
    is_test_only.then(|| format!("cfg({rendered})"))
}

/// The module declarations in `dir`'s owning module file that production builds never compile.
///
/// The owning file is `mod.rs`, or the crate roots when `dir` is the walk root.
fn test_only_module_gates(root: &Path, dir: &Path) -> BTreeMap<String, String> {
    let owners: &[&str] = if dir == root {
        &["lib.rs", "main.rs"]
    } else {
        &["mod.rs"]
    };
    let mut gates = BTreeMap::new();
    for owner in owners {
        let path = dir.join(owner);
        // Compile-time root (`crate_src_root` / `CARGO_MANIFEST_DIR`) walked downward; the
        // only child component is a fixed owner name from the `owners` list above.
        // codeql[rust/path-injection]
        let Ok(source) = std::fs::read_to_string(&path) else {
            continue;
        };
        let Ok(parsed) = syn::parse_file(&source) else {
            panic!("authority audit could not parse {}", path.display());
        };
        for item in parsed.items {
            let syn::Item::Mod(module) = item else {
                continue;
            };
            // Only declarations (`mod foo;`) name another file; inline modules carry their own
            // bodies and are already scoped by the file they live in.
            if module.content.is_some() {
                continue;
            }
            if let Some(gate) = module.attrs.iter().find_map(cfg_is_test_only) {
                gates.insert(module.ident.to_string(), gate);
            }
        }
    }
    gates
}

/// Every module file the tree declares behind a test-only `cfg`, as relative walk paths.
///
/// Exposed so the general rule — fixtures never contribute authority rows — can be asserted
/// against the real tree, not just a synthetic fixture.
pub(crate) fn test_gated_module_files(root: &Path) -> BTreeMap<String, String> {
    let mut found = BTreeMap::new();
    collect_test_gated_module_files(root, root, &mut found);
    found
}

fn collect_test_gated_module_files(root: &Path, dir: &Path, out: &mut BTreeMap<String, String>) {
    for (name, gate) in test_only_module_gates(root, dir) {
        for candidate in [
            dir.join(format!("{name}.rs")),
            dir.join(&name).join("mod.rs"),
        ] {
            if candidate.is_file() {
                let relative = candidate
                    .strip_prefix(root)
                    .unwrap_or(&candidate)
                    .to_string_lossy()
                    .replace('\\', "/");
                out.insert(relative, gate.clone());
            }
        }
    }
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_test_gated_module_files(root, &path, out);
        }
    }
}

/// Recursive `.rs` walk. Public so the fail-closed behaviour is testable against a fixture
/// tree; production callers go through [`load_production_sources`].
///
/// Modules declared behind a test-only `cfg` are skipped. A fixture is not production authority
/// surface, and the failure is not one-directional: PR 3.2's `feature = "test-utils"` harness
/// fixture defined `start/0`, which arity-keyed dispatch fused with `ResearchProcess::start/0`
/// and thereby OVER-attributed `start_research` to the `workspace-bridge` surface — the same
/// collision could as easily have masked a real writer. Fixtures must never move authority
/// verdicts in either direction.
pub(crate) fn collect_rs_files(root: &Path, dir: &Path, out: &mut Vec<(String, String)>) {
    let test_only = test_only_module_gates(root, dir);
    // `dir` is the compile-time crate root or a descendant discovered by this same walk;
    // no runtime, env, request, or config value reaches it.
    // codeql[rust/path-injection]
    let entries = std::fs::read_dir(dir).unwrap_or_else(|error| {
        panic!(
            "authority audit could not read directory {}: {error}",
            dir.display()
        )
    });
    for entry in entries {
        let entry = entry.unwrap_or_else(|error| {
            panic!(
                "authority audit could not read a directory entry under {}: {error}",
                dir.display()
            )
        });
        let path = entry.path();
        let file_type = entry.file_type().unwrap_or_else(|error| {
            panic!("authority audit could not stat {}: {error}", path.display())
        });
        if file_type.is_dir() {
            let directory_name = path.file_name().and_then(|name| name.to_str());
            if matches!(directory_name, Some("tests" | "testing")) {
                continue;
            }
            if directory_name.is_some_and(|name| test_only.contains_key(name)) {
                continue;
            }
            collect_rs_files(root, &path, out);
            continue;
        }
        let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        if !name.ends_with(".rs")
            || name.ends_with("_tests.rs")
            || matches!(name, "tests.rs" | "mocks.rs")
        {
            continue;
        }
        if test_only.contains_key(name.trim_end_matches(".rs")) {
            continue;
        }
        // `path` is a `read_dir` entry under the compile-time crate root — the walk never
        // joins a runtime-supplied component.
        // codeql[rust/path-injection]
        let source = std::fs::read_to_string(&path).unwrap_or_else(|error| {
            panic!("authority audit could not read {}: {error}", path.display())
        });
        let relative = path
            .strip_prefix(root)
            .unwrap_or(&path)
            .to_string_lossy()
            .replace('\\', "/");
        out.push((relative, source));
    }
}

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

/// The graph must span every workspace crate the app crate actually links.
///
/// PR 3.1-b batch 7 recorded the fault this closes: the walk covered `src-tauri/src` only, so a
/// call into a `ralphx-domain` entity method had NO same-name definition in the graph and fell
/// through to the all-same-name fallback. `reopen_issue` was the observed instance — its
/// `issue.reopen(reason)` resolved to `SessionReopenService::reopen`, which reaches git, and the
/// command read as a process spawner. The mechanism is direction-agnostic: the same missing
/// definition could as easily have swallowed a real writer, so this is a soundness floor, not a
/// convenience.
///
/// `ralphx-workflow-runner` is deliberately absent: it is a standalone `main.rs` binary and is
/// NOT a dependency of the app crate, so none of its definitions are reachable from a Tauri
/// command closure. Admitting them would inject same-name candidates from a program the census
/// can never call — the exact over-attribution this walk exists to prevent.
#[test]
fn production_source_walk_spans_the_linked_workspace_crates() {
    let files = load_production_sources();
    let paths = files
        .iter()
        .map(|(path, _)| path.as_str())
        .collect::<BTreeSet<_>>();

    for expected in [
        "crates/ralphx-domain/src/entities/review_issue.rs",
        "crates/ralphx-events/src/event_bus.rs",
        "crates/ralphx-remote-protocol/src/lib.rs",
    ] {
        assert!(
            paths.contains(expected),
            "workspace crate walk missed {expected}"
        );
    }

    assert!(
        !paths
            .iter()
            .any(|path| path.starts_with("crates/ralphx-workflow-runner/")),
        "the workflow-runner binary is not linked by the app crate and must not contribute \
         same-name dispatch candidates"
    );

    // Every included crate contributed, so a crate silently dropping out of the walk fails here
    // rather than quietly re-opening the batch-7 masking hole.
    for crate_name in ["ralphx-domain", "ralphx-events", "ralphx-remote-protocol"] {
        let prefix = format!("crates/{crate_name}/src/");
        assert!(
            paths
                .iter()
                .filter(|path| path.starts_with(&prefix))
                .count()
                >= MIN_WORKSPACE_CRATE_SOURCE_FILES,
            "{crate_name} contributed too few sources: the walk lost the crate"
        );
    }
}

/// The batch-7 false positive clears once the domain crate is visible.
///
/// `ReviewIssueEntity::reopen` is a status-guarded field mutation. With the definition in the
/// graph the resolver can pick it on owner/arity instead of falling back to every `reopen`, and
/// `reopen_issue`'s closure must no longer reach a process-launch sink.
#[test]
fn domain_crate_visibility_clears_the_reopen_issue_fallback() {
    let graph = CallGraph::build(&load_production_sources());
    let closure = graph.closure(["reopen_issue".to_string()]);
    assert!(
        !tokens_reach_any(&closure.tokens, PROCESS_LAUNCH_SINKS),
        "`reopen_issue` still reaches a process-launch sink: the domain-crate definition did \
         not displace the all-same-name fallback"
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

// ---------------------------------------------------------------------------------------
// PR 3.1-b batch 7 — same-name delegation must stay in the closure.
//
// `resolve_dispatch` refuses to fuse two sibling commands whose bodies mention each other's
// registered name. That rule is right, but dropping the edge unconditionally also deletes the
// most common delegation shape in this codebase: a thin Tauri command calling an
// identically-named application-service function. For those commands every detector reads
// silent no matter what the delegate does — a MASKING fault, the direction batch 6 pinned as
// the one fixtures and graph shortcuts must never move a verdict in.
// ---------------------------------------------------------------------------------------

/// A command delegating to a same-named NON-command definition keeps the edge; a command
/// naming a sibling COMMAND still does not.
#[test]
fn same_name_delegation_to_a_non_command_stays_in_the_closure() {
    let files = vec![
        (
            "commands/registry.rs".to_string(),
            r#"
            pub fn handlers() {
                tauri::generate_handler![
                    commands::alpha::do_thing,
                    commands::alpha::other_thing,
                ]
            }
            "#
            .to_string(),
        ),
        (
            "commands/alpha.rs".to_string(),
            r#"
            #[tauri::command]
            pub async fn do_thing(id: String, state: State<'_, AppState>) -> Result<(), String> {
                ThingService::do_thing(&state, &id).await.map_err(|e| e.to_string())
            }

            #[tauri::command]
            pub async fn other_thing(id: String, state: State<'_, AppState>) -> Result<(), String> {
                do_thing(id, state).await
            }
            "#
            .to_string(),
        ),
        (
            "application/thing_service.rs".to_string(),
            r#"
            impl ThingService {
                pub async fn do_thing(state: &AppState, id: &TaskId) -> AppResult<()> {
                    let path = resolve_git_cli_path().await?;
                    let _ = path;
                    Ok(())
                }
            }
            "#
            .to_string(),
        ),
    ];

    let graph = CallGraph::build(&files);

    // The delegation edge survives, so the launch sink in the service body is visible.
    let delegating = graph.closure(["do_thing".to_string()]);
    assert!(
        tokens_reach_any(&delegating.tokens, PROCESS_LAUNCH_SINKS),
        "`do_thing` delegates to `ThingService::do_thing`, which resolves a git CLI path; \
         dropping that edge makes detector (c) vacuous for every delegating command. \
         tokens={:?}",
        delegating.tokens
    );

    // The anti-fusion rule it was defending still holds: naming a SIBLING COMMAND creates no
    // edge, so `other_thing` does not inherit `do_thing`'s authority.
    let sibling = graph.closure(["other_thing".to_string()]);
    assert!(
        !tokens_reach_any(&sibling.tokens, PROCESS_LAUNCH_SINKS),
        "`other_thing` names the sibling command `do_thing`; commands must not fuse. \
         tokens={:?}",
        sibling.tokens
    );
}

/// The live regression this found: `get_task_validation_summary` reaches a `git rev-parse`
/// through `TaskValidationService::get_task_validation_summary` → `GitService::get_head_sha`.
/// Pinned against the real tree so the general rule above cannot pass while the case that
/// motivated it regresses.
#[test]
fn validation_summary_command_reaches_its_service_delegate() {
    let graph = CallGraph::build(&load_production_sources());
    let closure = graph.closure(["get_task_validation_summary".to_string()]);
    assert!(
        closure.tokens.contains("get_head_sha"),
        "`get_task_validation_summary` must reach `GitService::get_head_sha` through its \
         same-named service delegate; a closure that stops at the command body reports every \
         detector clean. tokens={:?}",
        closure.tokens
    );
    assert!(
        tokens_reach_any(&closure.tokens, PROCESS_LAUNCH_SINKS),
        "detector (c) must fire on `get_task_validation_summary`: its delegate shells out to \
         `git rev-parse HEAD`"
    );
}

#[test]
#[ignore = "calibration probe"]
fn probe_same_name_delegation_blast_radius() {
    let files = load_production_sources();
    let graph = CallGraph::build(&files);
    let mut affected = Vec::new();
    for (name, targets) in graph.definitions_snapshot() {
        if !graph.registered_commands_snapshot().contains(name) {
            continue;
        }
        let non_command: Vec<_> = targets
            .iter()
            .filter(|t| !graph.file_of(t).is_some_and(|f| f.starts_with("commands/")))
            .cloned()
            .collect();
        if !non_command.is_empty() {
            affected.push((name, non_command));
        }
    }
    eprintln!("PROBE-DELEGATION affected_commands={}", affected.len());
    for (name, targets) in &affected {
        eprintln!("PROBE-DELEGATION   {name} -> {targets:?}");
    }
}
