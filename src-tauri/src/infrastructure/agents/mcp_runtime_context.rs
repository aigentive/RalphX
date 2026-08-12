use std::path::PathBuf;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct McpRuntimeContext {
    pub context_type: Option<String>,
    pub context_id: Option<String>,
    pub conversation_id: Option<String>,
    pub coordination_mode: Option<String>,
    pub agent_run_id: Option<String>,
    pub task_id: Option<String>,
    pub task_state: Option<String>,
    pub project_id: Option<String>,
    pub working_directory: Option<PathBuf>,
    pub filesystem_read_roots: Vec<PathBuf>,
    pub enforce_filesystem_roots: bool,
    pub lead_session_id: Option<String>,
    pub parent_conversation_id: Option<String>,
    /// Runtime-injected MCP tool grants that are additive to the canonical
    /// per-agent allowlist.
    ///
    /// Used for role-tiered grants that cannot live in canonical `agent.yaml`
    /// because they depend on run/project/role context. Bare snake_case names.
    /// Unlike `explicit_allowed_tools`, these append rather than replace.
    pub extra_allowed_mcp_tools: Vec<String>,
}

pub fn append_mcp_runtime_query(url: &mut String, runtime_context: Option<&McpRuntimeContext>) {
    let Some(runtime_context) = runtime_context else {
        return;
    };

    let mut params: Vec<(&str, &str)> = Vec::new();
    if let Some(context_type) = runtime_context.context_type.as_deref() {
        params.push(("context_type", context_type));
    }
    if let Some(context_id) = runtime_context.context_id.as_deref() {
        params.push(("context_id", context_id));
    }
    if let Some(conversation_id) = runtime_context.conversation_id.as_deref() {
        params.push(("conversation_id", conversation_id));
    }
    if let Some(coordination_mode) = runtime_context.coordination_mode.as_deref() {
        params.push(("coordination_mode", coordination_mode));
    }
    if let Some(agent_run_id) = runtime_context.agent_run_id.as_deref() {
        params.push(("agent_run_id", agent_run_id));
    }
    if let Some(project_id) = runtime_context.project_id.as_deref() {
        params.push(("project_id", project_id));
    }
    if let Some(task_state) = runtime_context.task_state.as_deref() {
        params.push(("task_state", task_state));
    }
    if let Some(parent_conversation_id) = runtime_context.parent_conversation_id.as_deref() {
        params.push(("parent_conversation_id", parent_conversation_id));
    }

    if params.is_empty() {
        return;
    }

    url.push('?');
    for (index, (key, value)) in params.into_iter().enumerate() {
        if index > 0 {
            url.push('&');
        }
        url.push_str(key);
        url.push('=');
        url.push_str(&encode_query_component(value));
    }
}

pub fn append_mcp_runtime_args(
    args: &mut Vec<String>,
    runtime_context: Option<&McpRuntimeContext>,
) {
    let Some(runtime_context) = runtime_context else {
        return;
    };

    if let Some(context_type) = runtime_context.context_type.as_deref() {
        args.push("--context-type".to_string());
        args.push(context_type.to_string());
    }
    if let Some(context_id) = runtime_context.context_id.as_deref() {
        args.push("--context-id".to_string());
        args.push(context_id.to_string());
    }
    if let Some(conversation_id) = runtime_context.conversation_id.as_deref() {
        args.push("--conversation-id".to_string());
        args.push(conversation_id.to_string());
    }
    if let Some(coordination_mode) = runtime_context.coordination_mode.as_deref() {
        args.push("--coordination-mode".to_string());
        args.push(coordination_mode.to_string());
    }
    if let Some(agent_run_id) = runtime_context.agent_run_id.as_deref() {
        args.push("--agent-run-id".to_string());
        args.push(agent_run_id.to_string());
    }
    if let Some(task_id) = runtime_context.task_id.as_deref() {
        args.push("--task-id".to_string());
        args.push(task_id.to_string());
    }
    if let Some(task_state) = runtime_context.task_state.as_deref() {
        args.push("--task-state".to_string());
        args.push(task_state.to_string());
    }
    if let Some(project_id) = runtime_context.project_id.as_deref() {
        args.push("--project-id".to_string());
        args.push(project_id.to_string());
    }
    if let Some(working_directory) = runtime_context.working_directory.as_ref() {
        args.push("--working-directory".to_string());
        args.push(working_directory.to_string_lossy().into_owned());
    }
    for filesystem_read_root in &runtime_context.filesystem_read_roots {
        args.push("--filesystem-read-root".to_string());
        args.push(filesystem_read_root.to_string_lossy().into_owned());
    }
    if runtime_context.enforce_filesystem_roots {
        args.push("--filesystem-enforced".to_string());
        args.push("1".to_string());
    }
    if let Some(lead_session_id) = runtime_context.lead_session_id.as_deref() {
        args.push("--lead-session-id".to_string());
        args.push(lead_session_id.to_string());
    }
    if let Some(parent_conversation_id) = runtime_context.parent_conversation_id.as_deref() {
        args.push("--parent-conversation-id".to_string());
        args.push(parent_conversation_id.to_string());
    }
}

fn encode_query_component(value: &str) -> String {
    let mut encoded = String::new();
    for byte in value.as_bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                encoded.push(*byte as char);
            }
            _ => encoded.push_str(&format!("%{byte:02X}")),
        }
    }
    encoded
}
