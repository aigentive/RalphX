use super::authority_audit::*;
use std::collections::{BTreeSet, HashMap, VecDeque};

fn trace(graph: &CallGraph, root: &str) -> Option<Vec<String>> {
    let sinks: BTreeSet<String> = TRANSITION_SINKS
        .iter()
        .chain(SCHEDULER_SINKS.iter())
        .chain(STEER_SINKS.iter())
        .map(|s| s.to_string())
        .collect();
    let mut parents: HashMap<String, String> = HashMap::new();
    let mut seen: BTreeSet<String> = BTreeSet::new();
    let mut queue = VecDeque::from([root.to_string()]);
    seen.insert(root.to_string());
    while let Some(name) = queue.pop_front() {
        let Some(node) = graph.nodes.get(&name) else {
            continue;
        };
        if !node.sink_hits.is_empty() && name != root {
            let mut path = vec![format!(
                "{name} !! {:?}",
                node.sink_hits.iter().map(|h| h.sink.clone()).collect::<Vec<_>>()
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
        "pause_task",
    ] {
        match trace(&graph, root) {
            Some(path) => eprintln!("PROBE {root} => {}", path.join(" -> ")),
            None => eprintln!("PROBE {root} => NO SINK"),
        }
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
