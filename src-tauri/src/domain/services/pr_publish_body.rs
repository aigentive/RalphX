use super::*;

pub(super) fn build_agent_workspace_pr_title(conversation: &ChatConversation) -> String {
    conversation
        .title
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty() && *value != "Untitled agent")
        .map(str::to_string)
        .unwrap_or_else(|| "Agent conversation changes".to_string())
}

pub(super) fn write_agent_workspace_pr_body(body: &str) -> AppResult<NamedTempFile> {
    let body_file = NamedTempFile::new().map_err(|e| {
        AppError::Infrastructure(format!("failed to create PR body temp file: {e}"))
    })?;
    use std::io::Write as _;
    (&body_file)
        .write_all(body.as_bytes())
        .map_err(|e| AppError::Infrastructure(format!("failed to write PR body temp file: {e}")))?;
    Ok(body_file)
}

pub(super) fn finalize_agent_workspace_pr_body(
    body: &str,
    plan_markdown: &Option<String>,
) -> String {
    match plan_markdown {
        Some(plan) if !plan.trim().is_empty() => {
            let editable_prefix = body.trim_end();
            let managed_prefix = format!("\n\n{RALPHX_MANAGED_PR_BODY_START}\n");
            let suffix = format!(
                "\n\n</details>\n\n{RALPHX_GENERATED_FOOTER}\n{RALPHX_MANAGED_PR_BODY_END}"
            );
            let plan_header = "<details>\n<summary>View full plan</summary>\n\n";
            let full_body = format!(
                "{editable_prefix}{managed_prefix}{plan_header}{}{suffix}",
                plan.trim()
            );
            if char_count(&full_body) <= GITHUB_PR_BODY_SOFT_LIMIT_CHARS {
                return full_body;
            }
            let fixed_chars = char_count(&managed_prefix)
                + char_count(plan_header)
                + char_count(&suffix)
                + char_count(PR_BODY_TRUNCATION_NOTICE);
            let available_content_chars =
                GITHUB_PR_BODY_SOFT_LIMIT_CHARS.saturating_sub(fixed_chars);
            let editable = truncate_chars(editable_prefix, available_content_chars);
            let remaining_plan_chars =
                available_content_chars.saturating_sub(char_count(editable.trim_end()));
            let truncated_plan = truncate_chars(plan.trim(), remaining_plan_chars);
            format!(
                "{}{managed_prefix}{plan_header}{}{PR_BODY_TRUNCATION_NOTICE}{suffix}",
                editable.trim_end(),
                truncated_plan.trim_end()
            )
        }
        _ => {
            let preserved_suffix = format!(
                "\n\n{RALPHX_MANAGED_PR_BODY_START}\n{RALPHX_GENERATED_FOOTER}\n\
                 {RALPHX_MANAGED_PR_BODY_END}"
            );
            let editable_prefix = body
                .trim_end()
                .strip_suffix(RALPHX_GENERATED_FOOTER)
                .map(str::trim_end)
                .unwrap_or(body);
            recompose_agent_workspace_pr_body_with_preserved_suffix(
                editable_prefix,
                &preserved_suffix,
            )
            .unwrap_or(preserved_suffix)
        }
    }
}

pub(super) fn recompose_agent_workspace_pr_body_with_preserved_suffix(
    editable_prefix: &str,
    preserved_suffix: &str,
) -> AppResult<String> {
    let editable_prefix = fit_editable_prefix_for_preserved_suffix(
        editable_prefix,
        preserved_suffix,
    )
    .ok_or_else(|| {
        AppError::Validation(
            "the preserved PR body suffix leaves no room for an editable description".to_string(),
        )
    })?;
    Ok(format!("{editable_prefix}{preserved_suffix}"))
}

fn char_count(text: &str) -> usize {
    text.chars().count()
}

fn truncate_chars(text: &str, max_chars: usize) -> String {
    text.chars().take(max_chars).collect()
}

pub(super) fn fit_plan_markdown_to_pr_body(
    prefix: &str,
    plan_markdown: &str,
    suffix: &str,
) -> String {
    let full_body = format!("{prefix}{plan_markdown}{suffix}");
    if char_count(&full_body) <= GITHUB_PR_BODY_SOFT_LIMIT_CHARS {
        return full_body;
    }

    let fixed_chars =
        char_count(prefix) + char_count(suffix) + char_count(PR_BODY_TRUNCATION_NOTICE);
    if fixed_chars >= GITHUB_PR_BODY_SOFT_LIMIT_CHARS {
        return truncate_chars(&full_body, GITHUB_PR_BODY_SOFT_LIMIT_CHARS);
    }

    let available_plan_chars = GITHUB_PR_BODY_SOFT_LIMIT_CHARS - fixed_chars;
    let truncated_plan = truncate_chars(plan_markdown, available_plan_chars);
    format!(
        "{prefix}{}{}{suffix}",
        truncated_plan.trim_end(),
        PR_BODY_TRUNCATION_NOTICE
    )
}
