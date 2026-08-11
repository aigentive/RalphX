use std::path::Path;

use crate::application::chat_service::escape_attr;
use crate::domain::entities::ChatContextType;

pub(crate) fn task_runtime_state_for_context(
    context_type: ChatContextType,
    entity_status: Option<&str>,
) -> Option<&str> {
    match (context_type, entity_status) {
        (ChatContextType::TaskExecution, Some(task_state @ ("executing" | "re_executing"))) => {
            Some(task_state)
        }
        (ChatContextType::Review, Some(task_state @ "reviewing")) => Some(task_state),
        _ => None,
    }
}

pub(crate) fn build_task_runtime_context_prompt(
    context_type: ChatContextType,
    context_id: &str,
    entity_status: Option<&str>,
    project_id: Option<&str>,
    working_directory: &Path,
) -> Result<Option<String>, String> {
    let Some(task_state) = task_runtime_state_for_context(context_type, entity_status) else {
        return Ok(None);
    };
    if context_id.trim().is_empty() {
        return Err(format!(
            "{} task runtime context missing task identity",
            context_type
        ));
    }
    let project_id = project_id
        .filter(|id| !id.trim().is_empty())
        .ok_or_else(|| {
            format!(
                "{} task runtime context missing project identity for task {}",
                context_type, context_id
            )
        })?;
    if working_directory.as_os_str().is_empty() {
        return Err(format!(
            "{} task runtime context missing working directory for task {}",
            context_type, context_id
        ));
    }

    let mut context = String::from("<task_runtime_context>\n");
    context.push_str(&format!("<task_id>{}</task_id>\n", escape_attr(context_id)));
    context.push_str(&format!(
        "<project_id>{}</project_id>\n",
        escape_attr(project_id)
    ));
    context.push_str(&format!(
        "<context_type>{}</context_type>\n",
        escape_attr(&context_type.to_string())
    ));
    context.push_str(&format!(
        "<task_state>{}</task_state>\n",
        escape_attr(task_state)
    ));
    context.push_str(&format!(
        "<working_directory>{}</working_directory>\n",
        escape_attr(working_directory.to_string_lossy().as_ref())
    ));
    context.push_str(
        "<full_context_hint>Use get_task_context and related task MCP tools when full or fresh task details are required.</full_context_hint>\n",
    );
    context.push_str("</task_runtime_context>");
    Ok(Some(context))
}
