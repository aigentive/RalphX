// Tests for WebhookService registration logic
// Validates match-all behavior when no explicit project_ids provided

use std::sync::Arc;

use crate::application::WebhookService;
use crate::domain::repositories::WebhookRegistrationRepository;
use crate::infrastructure::memory::MemoryWebhookRegistrationRepository;

fn make_repo() -> Arc<MemoryWebhookRegistrationRepository> {
    Arc::new(MemoryWebhookRegistrationRepository::new())
}

fn make_service(repo: Arc<MemoryWebhookRegistrationRepository>) -> WebhookService {
    WebhookService::new(repo as Arc<dyn WebhookRegistrationRepository>)
}

#[tokio::test]
async fn register_without_project_ids_stores_empty_array() {
    let repo = make_repo();
    let svc = make_service(Arc::clone(&repo));

    let registration = svc
        .register(
            "key-1",
            "https://example.com/hook",
            None,
            vec![], // no project_ids requested
            &["proj-a".to_string(), "proj-b".to_string()], // authorized scope
        )
        .await
        .expect("registration should succeed");

    assert_eq!(
        registration.project_ids, "[]",
        "project_ids should be '[]' (match-all) when none requested"
    );
}

#[tokio::test]
async fn register_with_explicit_project_ids_stores_them() {
    let repo = make_repo();
    let svc = make_service(Arc::clone(&repo));

    let registration = svc
        .register(
            "key-1",
            "https://example.com/hook",
            None,
            vec!["proj-a".to_string()], // explicit project_ids
            &["proj-a".to_string(), "proj-b".to_string()],
        )
        .await
        .expect("registration should succeed");

    let stored: Vec<String> =
        serde_json::from_str(&registration.project_ids).expect("valid JSON");
    assert_eq!(stored, vec!["proj-a"]);
}

#[tokio::test]
async fn match_all_webhook_returned_for_any_project() {
    let repo = make_repo();
    let svc = make_service(Arc::clone(&repo));

    // Register without project_ids → match-all (empty '[]')
    svc.register(
        "key-1",
        "https://example.com/hook",
        None,
        vec![],
        &[],
    )
    .await
    .expect("registration should succeed");

    // Should match project-x even though it wasn't specified
    let for_x = repo
        .list_active_for_project("project-x")
        .await
        .expect("query should succeed");
    assert_eq!(
        for_x.len(),
        1,
        "match-all webhook should appear for project-x"
    );

    // Should also match project-y
    let for_y = repo
        .list_active_for_project("project-y")
        .await
        .expect("query should succeed");
    assert_eq!(
        for_y.len(),
        1,
        "match-all webhook should appear for project-y"
    );
}

#[tokio::test]
async fn scoped_webhook_not_returned_for_other_project() {
    let repo = make_repo();
    let svc = make_service(Arc::clone(&repo));

    // Register with explicit project_ids
    svc.register(
        "key-1",
        "https://example.com/hook",
        None,
        vec!["proj-a".to_string()],
        &["proj-a".to_string()],
    )
    .await
    .expect("registration should succeed");

    let for_a = repo
        .list_active_for_project("proj-a")
        .await
        .expect("query should succeed");
    assert_eq!(for_a.len(), 1, "webhook should appear for proj-a");

    let for_b = repo
        .list_active_for_project("proj-b")
        .await
        .expect("query should succeed");
    assert_eq!(for_b.len(), 0, "webhook should NOT appear for proj-b");
}

#[tokio::test]
async fn register_reregistration_refreshes_project_ids() {
    let repo = make_repo();
    let svc = make_service(Arc::clone(&repo));

    // First registration: proj-a only
    let first = svc
        .register(
            "key-1",
            "https://example.com/hook",
            None,
            vec!["proj-a".to_string()],
            &["proj-a".to_string(), "proj-b".to_string()],
        )
        .await
        .expect("first registration should succeed");

    let first_id = first.id.clone();
    let stored_first: Vec<String> =
        serde_json::from_str(&first.project_ids).expect("valid JSON");
    assert_eq!(stored_first, vec!["proj-a"]);

    // Second registration (same URL+api_key): expand scope to include proj-b
    let second = svc
        .register(
            "key-1",
            "https://example.com/hook",
            None,
            vec!["proj-a".to_string(), "proj-b".to_string()],
            &["proj-a".to_string(), "proj-b".to_string()],
        )
        .await
        .expect("re-registration should succeed");

    // Same id preserved
    assert_eq!(second.id, first_id, "Re-registration must preserve webhook id");
    // project_ids refreshed to include proj-b
    let stored_second: Vec<String> =
        serde_json::from_str(&second.project_ids).expect("valid JSON");
    assert!(
        stored_second.contains(&"proj-b".to_string()),
        "Re-registration must include newly-added project in project_ids"
    );

    // Verify new project appears in list_active_for_project
    let for_b = repo
        .list_active_for_project("proj-b")
        .await
        .expect("query should succeed");
    assert_eq!(
        for_b.len(),
        1,
        "Webhook must appear for proj-b after re-registration"
    );
}

#[tokio::test]
async fn register_rejects_out_of_scope_project_ids() {
    let repo = make_repo();
    let svc = make_service(Arc::clone(&repo));

    let result = svc
        .register(
            "key-1",
            "https://example.com/hook",
            None,
            vec!["proj-z".to_string()], // not in authorized scope
            &["proj-a".to_string(), "proj-b".to_string()],
        )
        .await;

    assert!(
        result.is_err(),
        "should reject project_ids outside authorized scope"
    );
}
