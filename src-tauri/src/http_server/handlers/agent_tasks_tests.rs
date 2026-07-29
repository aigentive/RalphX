use super::agent_tasks::resolve_scope;
use crate::http_server::AgentTaskContextFields;

#[test]
fn resolve_scope_rejects_bare_project_fallback_without_explicit_scope() {
    let error = resolve_scope(&AgentTaskContextFields {
        context_type: None,
        context_id: None,
        project_id: Some("project-id".to_string()),
        actor_agent: Some("ralphx-general-worker".to_string()),
    })
    .expect_err("conversation runtimes without identity must not share a project ledger");

    assert!(error.contains("context_type and context_id are required"));
}

#[test]
fn resolve_scope_preserves_explicit_project_scope() {
    let scope = resolve_scope(&AgentTaskContextFields {
        context_type: Some("project".to_string()),
        context_id: Some("project-id".to_string()),
        project_id: Some("project-id".to_string()),
        actor_agent: Some("ralphx-external-mcp".to_string()),
    })
    .expect("explicit project-scoped external callers remain supported");

    assert_eq!(scope.scope_type, "project");
    assert_eq!(scope.scope_id, "project-id");
    assert_eq!(scope.project_id.expect("project id").as_str(), "project-id");
}
