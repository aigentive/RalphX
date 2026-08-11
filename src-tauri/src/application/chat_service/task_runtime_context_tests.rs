use super::chat_service_context::{
    build_task_runtime_context_prompt, task_runtime_state_for_context,
};
use crate::domain::entities::ChatContextType;
use std::path::Path;

#[test]
fn task_runtime_state_maps_task_execution_and_review_states() {
    assert_eq!(
        task_runtime_state_for_context(ChatContextType::TaskExecution, Some("executing")),
        Some("executing")
    );
    assert_eq!(
        task_runtime_state_for_context(ChatContextType::TaskExecution, Some("re_executing")),
        Some("re_executing")
    );
    assert_eq!(
        task_runtime_state_for_context(ChatContextType::Review, Some("reviewing")),
        Some("reviewing")
    );
}

#[test]
fn task_runtime_state_ignores_unmapped_contexts_and_statuses() {
    assert_eq!(
        task_runtime_state_for_context(ChatContextType::TaskExecution, Some("ready")),
        None
    );
    assert_eq!(
        task_runtime_state_for_context(ChatContextType::Review, Some("approved")),
        None
    );
    assert_eq!(
        task_runtime_state_for_context(ChatContextType::Project, Some("executing")),
        None
    );
    assert_eq!(
        task_runtime_state_for_context(ChatContextType::TaskExecution, None),
        None
    );
}

#[test]
fn task_runtime_context_prompt_contains_compact_escaped_task_fields() {
    let prompt = build_task_runtime_context_prompt(
        ChatContextType::TaskExecution,
        "task<&>",
        Some("executing"),
        Some("project<&>"),
        Path::new("/tmp/ralphx-work<&>"),
    )
    .expect("prompt build should succeed")
    .expect("runtime context should be present");

    assert!(prompt.starts_with("<task_runtime_context>"));
    assert!(prompt.contains("<task_id>task&lt;&amp;&gt;</task_id>"));
    assert!(prompt.contains("<project_id>project&lt;&amp;&gt;</project_id>"));
    assert!(prompt.contains("<context_type>task_execution</context_type>"));
    assert!(prompt.contains("<task_state>executing</task_state>"));
    assert!(prompt.contains("<working_directory>/tmp/ralphx-work&lt;&amp;&gt;</working_directory>"));
    assert!(prompt.contains("Use get_task_context and related task MCP tools"));
    assert!(prompt.ends_with("</task_runtime_context>"));
}

#[test]
fn task_runtime_context_prompt_returns_none_when_no_runtime_state_applies() {
    let prompt = build_task_runtime_context_prompt(
        ChatContextType::Project,
        "project-1",
        Some("executing"),
        Some("project-1"),
        Path::new("/tmp/ralphx"),
    )
    .expect("non-task context should not fail");

    assert!(prompt.is_none());
}

#[test]
fn task_runtime_context_prompt_fails_closed_without_task_identity() {
    let error = build_task_runtime_context_prompt(
        ChatContextType::TaskExecution,
        "   ",
        Some("executing"),
        Some("project-1"),
        Path::new("/tmp/ralphx"),
    )
    .expect_err("empty task identity should fail");

    assert!(error.contains("missing task identity"));
}

#[test]
fn task_runtime_context_prompt_fails_closed_without_project_identity() {
    let error = build_task_runtime_context_prompt(
        ChatContextType::Review,
        "task-1",
        Some("reviewing"),
        Some(""),
        Path::new("/tmp/ralphx"),
    )
    .expect_err("empty project identity should fail");

    assert!(error.contains("missing project identity"));
}

#[test]
fn task_runtime_context_prompt_fails_closed_without_working_directory() {
    let error = build_task_runtime_context_prompt(
        ChatContextType::TaskExecution,
        "task-1",
        Some("executing"),
        Some("project-1"),
        Path::new(""),
    )
    .expect_err("empty working directory should fail");

    assert!(error.contains("missing working directory"));
}
