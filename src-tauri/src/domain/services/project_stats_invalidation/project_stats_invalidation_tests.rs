use std::sync::{Arc, Mutex};

use crate::domain::services::project_stats_invalidation::{
    invalidate_project_stats, register_project_stats_invalidator, ProjectStatsInvalidator,
};

#[test]
fn onclock_invalidator_lifecycle() {
    // Case 1: before registration, calling invalidate must be a no-op (no panic).
    invalidate_project_stats("pre-register-project");

    // Case 2: first registration takes effect immediately.
    let calls: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
    let calls_clone = Arc::clone(&calls);
    let first: ProjectStatsInvalidator = Arc::new(move |project_id: &str| {
        calls_clone.lock().unwrap().push(project_id.to_string());
    });
    register_project_stats_invalidator(first);

    invalidate_project_stats("project-a");
    assert_eq!(
        *calls.lock().unwrap(),
        vec!["project-a".to_string()],
        "first registered invalidator must be called after registration"
    );

    // Case 3: re-registration is silently ignored; first invalidator stays.
    let second_calls: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
    let second_calls_clone = Arc::clone(&second_calls);
    let second: ProjectStatsInvalidator = Arc::new(move |project_id: &str| {
        second_calls_clone.lock().unwrap().push(project_id.to_string());
    });
    register_project_stats_invalidator(second);

    invalidate_project_stats("project-b");
    assert_eq!(
        *calls.lock().unwrap(),
        vec!["project-a".to_string(), "project-b".to_string()],
        "first invalidator must remain active after re-registration attempt"
    );
    assert!(
        second_calls.lock().unwrap().is_empty(),
        "second registration must be ignored; OnceLock keeps the first one"
    );
}
