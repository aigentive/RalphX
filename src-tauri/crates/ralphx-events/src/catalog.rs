pub const AGENT_TASK_COMPLETED: &str = "agent:task_completed";
pub const AGENT_TASK_STARTED: &str = "agent:task_started";
pub const AGENT_RUN_COMPLETED: &str = "agent:run_completed";
pub const AGENT_TURN_COMPLETED: &str = "agent:turn_completed";
pub const EXTERNAL_MCP_STATUS: &str = "external-mcp:status";

pub fn is_agent_completion_event(event: &str) -> bool {
    matches!(event, AGENT_RUN_COMPLETED | AGENT_TURN_COMPLETED)
}
