use super::mcp_runtime_context::McpRuntimeContext;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

fn full_agent_env() -> HashMap<String, String> {
    HashMap::from([
        ("RALPHX_PROJECT_ID".to_string(), " project-1 ".to_string()),
        (
            "RALPHX_CONTEXT_TYPE".to_string(),
            " task_execution ".to_string(),
        ),
        ("RALPHX_CONTEXT_ID".to_string(), " context-1 ".to_string()),
        (
            "RALPHX_CONVERSATION_ID".to_string(),
            " conversation-1 ".to_string(),
        ),
        ("RALPHX_AGENT_RUN_ID".to_string(), " run-1 ".to_string()),
        ("RALPHX_TASK_ID".to_string(), " task-1 ".to_string()),
        ("RALPHX_TASK_STATE".to_string(), " executing ".to_string()),
        (
            "RALPHX_PARENT_CONVERSATION_ID".to_string(),
            " conversation-parent ".to_string(),
        ),
    ])
}

#[test]
fn from_agent_env_maps_all_a2_fields_and_trusts_the_working_directory_argument() {
    let mut env = full_agent_env();
    env.insert(
        "RALPHX_WORKING_DIRECTORY".to_string(),
        "/untrusted/from-env".to_string(),
    );

    let context = McpRuntimeContext::from_agent_env(&env, Path::new("/trusted/workspace"))
        .expect("project-scoped context");

    assert_eq!(context.project_id.as_deref(), Some("project-1"));
    assert_eq!(context.context_type.as_deref(), Some("task_execution"));
    assert_eq!(context.context_id.as_deref(), Some("context-1"));
    assert_eq!(context.conversation_id.as_deref(), Some("conversation-1"));
    assert_eq!(context.agent_run_id.as_deref(), Some("run-1"));
    assert_eq!(context.task_id.as_deref(), Some("task-1"));
    assert_eq!(context.task_state.as_deref(), Some("executing"));
    assert_eq!(
        context.parent_conversation_id.as_deref(),
        Some("conversation-parent")
    );
    assert_eq!(
        context.working_directory,
        Some(PathBuf::from("/trusted/workspace"))
    );
    assert_eq!(context.coordination_mode, None);
    assert_eq!(context.lead_session_id, None);
    assert!(context.filesystem_read_roots.is_empty());
    assert!(!context.enforce_filesystem_roots);
}

#[test]
fn from_agent_env_prefers_explicit_context_id_and_falls_back_to_task_id() {
    let mut env = HashMap::from([
        ("RALPHX_PROJECT_ID".to_string(), "project-1".to_string()),
        ("RALPHX_CONTEXT_ID".to_string(), "context-1".to_string()),
        ("RALPHX_TASK_ID".to_string(), "task-1".to_string()),
    ]);

    let explicit =
        McpRuntimeContext::from_agent_env(&env, Path::new("/workspace")).expect("explicit context");
    assert_eq!(explicit.context_id.as_deref(), Some("context-1"));
    assert_eq!(explicit.task_id.as_deref(), Some("task-1"));

    env.remove("RALPHX_CONTEXT_ID");
    let fallback = McpRuntimeContext::from_agent_env(&env, Path::new("/workspace"))
        .expect("task fallback context");
    assert_eq!(fallback.context_id.as_deref(), Some("task-1"));
    assert_eq!(fallback.task_id.as_deref(), Some("task-1"));
}

#[test]
fn from_agent_env_fails_closed_without_a_non_blank_project() {
    let context_only = HashMap::from([(
        "RALPHX_CONTEXT_ID".to_string(),
        "context-without-project".to_string(),
    )]);
    assert_eq!(
        McpRuntimeContext::from_agent_env(&context_only, Path::new("/workspace")),
        None
    );

    let blank_project = HashMap::from([
        ("RALPHX_PROJECT_ID".to_string(), " \t ".to_string()),
        ("RALPHX_TASK_ID".to_string(), "task-1".to_string()),
    ]);
    assert_eq!(
        McpRuntimeContext::from_agent_env(&blank_project, Path::new("/workspace")),
        None
    );
}

#[test]
fn from_agent_env_omits_blank_optional_values() {
    let env = HashMap::from([
        ("RALPHX_PROJECT_ID".to_string(), "project-1".to_string()),
        ("RALPHX_CONTEXT_TYPE".to_string(), " ".to_string()),
        ("RALPHX_CONTEXT_ID".to_string(), "\n".to_string()),
        ("RALPHX_TASK_ID".to_string(), "\t".to_string()),
        (
            "RALPHX_PARENT_CONVERSATION_ID".to_string(),
            "  ".to_string(),
        ),
    ]);

    let context = McpRuntimeContext::from_agent_env(&env, Path::new("/workspace"))
        .expect("project-only context");
    assert_eq!(context.context_type, None);
    assert_eq!(context.context_id, None);
    assert_eq!(context.task_id, None);
    assert_eq!(context.parent_conversation_id, None);
}
