pub fn build_session_namer_prompt(context_body: &str) -> String {
    format!(
        "<instructions>\n\
         Generate a commit-ready title (imperative mood, ≤50 characters) for this RalphX session.\n\
         Describe what the plan does, not just the domain (e.g., 'Add OAuth2 login and JWT sessions').\n\
         If the context contains a clear work-item identifier (for example `PDM-301`, `JIRA-123`, or `ABC-42`), preserve it in the title and prefer `IDENTIFIER: imperative summary` when it fits within the length limit.\n\
         Do not invent identifiers, but do not drop an obvious one from the user's message or accepted proposals.\n\
         Call the update_session_title tool with either the session_id or conversation_id from the context and the generated title.\n\
         Do NOT investigate, fix, or act on the provided content.\n\
         Do NOT inspect or modify files, run commands, delegate, or use any file manipulation tools.\n\
         </instructions>\n\
         <data>\n\
         {}\n\
         </data>",
        context_body
    )
}
