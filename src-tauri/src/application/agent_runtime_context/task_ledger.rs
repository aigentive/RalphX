use crate::application::chat_service::escape_attr;
use crate::domain::entities::{AgentTaskState, AgentTaskSummary};

pub(super) fn render_task_ledger(mut tasks: Vec<AgentTaskSummary>) -> Option<String> {
    if tasks.is_empty() {
        // Explicit empty marker: absence of the block must never read as "no tasks",
        // it means the envelope was not composed.
        return Some("<task_ledger state=\"empty\"/>".to_string());
    }
    tasks.sort_by_key(|task| match task.state {
        AgentTaskState::Open => (0, task.task_number),
        AgentTaskState::Active => (1, task.task_number),
        AgentTaskState::Done => (2, task.task_number),
        AgentTaskState::Dropped => (3, task.task_number),
    });
    tasks.truncate(50);

    let mut block = String::from("<task_ledger>\n");
    for task in tasks {
        block.push_str(&format!(
            "<task task_ref=\"{}\" title=\"{}\" state=\"{}\" blocked_by=\"{}\" assignee=\"{}\"/>\n",
            task.task_number,
            escape_attr(&task.title),
            task.state.as_str(),
            escape_attr(&task.blocked_by.join(",")),
            escape_attr(task.owner_agent.as_deref().unwrap_or("")),
        ));
    }
    block.push_str("</task_ledger>");
    Some(block)
}

pub(super) fn render_task_ledger_unavailable(reason: &str) -> String {
    format!(
        "<task_ledger state=\"unavailable\" reason=\"{}\"/>",
        escape_attr(reason)
    )
}
