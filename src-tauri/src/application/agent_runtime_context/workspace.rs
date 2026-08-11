use crate::application::chat_service::escape_attr;
use crate::domain::entities::AgentConversationWorkspace;

pub(crate) fn format_agent_workspace_source_pull_request_prompt_context(
    workspace: &AgentConversationWorkspace,
) -> Option<String> {
    let mut block = format!(
        "<agent_workspace_context>\n\
         <current_workspace>\n\
         <conversation_id>{}</conversation_id>\n\
         <project_id>{}</project_id>\n\
         <mode>{}</mode>\n\
         <branch_name>{}</branch_name>\n\
         <base_ref>{}</base_ref>\n\
         <worktree_path>{}</worktree_path>\n",
        escape_attr(&workspace.conversation_id.as_str()),
        escape_attr(workspace.project_id.as_str()),
        workspace.mode,
        escape_attr(&workspace.branch_name),
        escape_attr(&workspace.base_ref),
        escape_attr(&workspace.worktree_path),
    );
    if let Some(session_id) = workspace.linked_ideation_session_id.as_ref() {
        block.push_str(&format!(
            "         <linked_ideation_session_id>{}</linked_ideation_session_id>\n",
            escape_attr(session_id.as_str())
        ));
    }
    if let Some(plan_branch_id) = workspace.linked_plan_branch_id.as_ref() {
        block.push_str(&format!(
            "         <linked_plan_branch_id>{}</linked_plan_branch_id>\n",
            escape_attr(plan_branch_id.as_str())
        ));
    }
    block.push_str("         </current_workspace>\n");

    let Some(source) = workspace.source_pull_request.as_ref() else {
        block.push_str("</agent_workspace_context>");
        return Some(block);
    };

    block.push_str(&format!(
        "         <source_pull_request>\n\
         <origin_hint>This agent workspace is based on branch {} of PR #{}.</origin_hint>\n\
         <number>{}</number>\n\
         <head_branch>{}</head_branch>\n\
         <workspace_base_ref>{}</workspace_base_ref>\n",
        escape_attr(&source.head_ref_name),
        source.number,
        source.number,
        escape_attr(&source.head_ref_name),
        escape_attr(&workspace.base_ref)
    ));
    if let Some(title) = source
        .title
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        block.push_str(&format!("         <title>{}</title>\n", escape_attr(title)));
    }
    if let Some(url) = source
        .url
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        block.push_str(&format!("         <url>{}</url>\n", escape_attr(url)));
    }
    if let Some(base_ref) = source
        .base_ref_name
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        block.push_str(&format!(
            "         <original_pr_base_branch>{}</original_pr_base_branch>\n",
            escape_attr(base_ref)
        ));
    }
    if let Some(head_sha) = source
        .head_ref_oid
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        block.push_str(&format!(
            "         <source_pr_head_sha>{}</source_pr_head_sha>\n",
            escape_attr(head_sha)
        ));
    }
    block.push_str(&format!(
        "         <publish_target_hint>If this workspace publishes changes, RalphX creates a new pull request targeting branch {}, which is the source PR head branch.</publish_target_hint>\n\
         </source_pull_request>\n\
         </agent_workspace_context>",
        escape_attr(&source.head_ref_name)
    ));
    Some(block)
}
