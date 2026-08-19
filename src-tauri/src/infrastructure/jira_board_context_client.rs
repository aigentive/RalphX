//! Fetches a Jira Software board's active-sprint summary (grouped by board
//! column) for the `"jira_board"` composer reference kind. Kept as its own
//! sibling module — mirroring `jira_agile_client.rs` — instead of growing
//! `atlassian_client.rs`, which is already well over its file-size budget.

use hyper::Method;
use serde_json::Value;

use crate::domain::integrations::AtlassianResourceContent;
use crate::domain::services::ComposerIntegrationReference;

use super::atlassian_client::{
    request_auth, AtlassianJsonRequester, HyperAtlassianApiClient, JiraAgileAuthContext,
    JiraAgileBoardColumn, JiraAgileBoardConfiguration, JiraAgileResourceKind,
    JiraAgileSprintSummary,
};
use super::jira_agile_client::{get_jira_board_configuration, list_jira_active_sprints};

const JIRA_SPRINT_ISSUE_PAGE_SIZE: usize = 100;

/// Caps how many issues a single sprint renders. Mirrors
/// `jira_agile_client.rs`'s `list_jira_sprint_issues` cap so board-context
/// expansion never issues unbounded sequential paged calls for a large
/// sprint.
const BOARD_SPRINT_ISSUE_LIMIT: usize = 50;

struct SprintIssue {
    key: String,
    summary: String,
    status_id: Option<String>,
    status_name: String,
    assignee: Option<String>,
}

pub(crate) async fn fetch_jira_board_context<C: AtlassianJsonRequester + ?Sized>(
    client: &C,
    auth: &JiraAgileAuthContext,
    reference: &ComposerIntegrationReference,
) -> Result<AtlassianResourceContent, String> {
    let board_id = required_value(&reference.id, "Jira board id is required")?;
    let configuration = get_jira_board_configuration(client, auth, board_id).await?;
    let active_sprints = list_jira_active_sprints(client, auth, board_id).await?;

    let mut sections = vec![format!("# Board: {}", configuration.name)];
    if active_sprints.is_empty() {
        sections.push(render_no_active_sprint());
    } else {
        for sprint in &active_sprints {
            match list_sprint_issues(client, auth, &sprint.id).await {
                Ok((issues, total)) => {
                    sections.push(render_sprint_section(&configuration, sprint, &issues, total));
                }
                Err(error) => {
                    tracing::warn!(
                        sprint_id = %sprint.id,
                        error = %error,
                        "Jira board context: sprint issue lookup failed"
                    );
                    sections.push(render_sprint_unavailable(sprint));
                }
            }
        }
    }

    Ok(AtlassianResourceContent {
        kind: JiraAgileResourceKind::Jira,
        id: reference.id.clone(),
        key: None,
        title: format!("Board: {}", configuration.name),
        url: reference.url.clone(),
        body: sections.join("\n\n"),
        status: None,
        assignee: None,
        reporter: None,
        updated_at_remote: None,
        description_markdown: None,
        description_text: None,
        acceptance_criteria_markdown: None,
        acceptance_criteria_text: None,
        comments: Vec::new(),
        attachments: Vec::new(),
        issue_type: None,
        labels: Vec::new(),
        priority: None,
        parent_key: None,
        children: Vec::new(),
    })
}

fn render_no_active_sprint() -> String {
    "No active sprint for this board.\n\nBacklog: this board has no active sprint; check the \
     board's backlog in Jira for unscheduled work."
        .to_string()
}

/// Rendered in place of a sprint's issue table when the secondary
/// issue-fetch call fails, so one bad sprint never discards the board name
/// or any other sprint already rendered.
fn render_sprint_unavailable(sprint: &JiraAgileSprintSummary) -> String {
    let mut lines = vec![format!("## Sprint: {} (active)", sprint.name)];
    if let Some(goal) = sprint
        .goal
        .as_deref()
        .map(str::trim)
        .filter(|g| !g.is_empty())
    {
        lines.push(format!("Goal: {goal}"));
    }
    lines.push("Issues unavailable: fetching this sprint's issues failed.".to_string());
    lines.join("\n")
}

fn render_sprint_section(
    configuration: &JiraAgileBoardConfiguration,
    sprint: &JiraAgileSprintSummary,
    issues: &[SprintIssue],
    total: Option<usize>,
) -> String {
    let mut lines = vec![format!("## Sprint: {} (active)", sprint.name)];
    if let Some(goal) = sprint
        .goal
        .as_deref()
        .map(str::trim)
        .filter(|g| !g.is_empty())
    {
        lines.push(format!("Goal: {goal}"));
    }
    if let Some(total) = total {
        if total > issues.len() {
            lines.push(format!("({} shown of {total})", issues.len()));
        }
    }

    for column in &configuration.columns {
        lines.push(format!("### {}", column.name));
        let column_issues = issues_for_column(issues, column);
        push_issue_lines(&mut lines, &column_issues);
    }

    let unmapped = unmapped_issues(configuration, issues);
    if !unmapped.is_empty() {
        lines.push("### Other".to_string());
        push_issue_lines(&mut lines, &unmapped);
    }

    lines.join("\n")
}

fn issues_for_column<'a>(
    issues: &'a [SprintIssue],
    column: &JiraAgileBoardColumn,
) -> Vec<&'a SprintIssue> {
    issues
        .iter()
        .filter(|issue| {
            issue
                .status_id
                .as_deref()
                .is_some_and(|status_id| column.status_ids.iter().any(|id| id == status_id))
        })
        .collect()
}

fn unmapped_issues<'a>(
    configuration: &JiraAgileBoardConfiguration,
    issues: &'a [SprintIssue],
) -> Vec<&'a SprintIssue> {
    issues
        .iter()
        .filter(|issue| {
            !issue.status_id.as_deref().is_some_and(|status_id| {
                configuration
                    .columns
                    .iter()
                    .any(|column| column.status_ids.iter().any(|id| id == status_id))
            })
        })
        .collect()
}

fn push_issue_lines(lines: &mut Vec<String>, issues: &[&SprintIssue]) {
    if issues.is_empty() {
        lines.push("(none)".to_string());
        return;
    }
    for issue in issues {
        let assignee = issue.assignee.as_deref().unwrap_or("Unassigned");
        lines.push(format!(
            "- {}: {} ({}) — {}",
            issue.key, issue.summary, issue.status_name, assignee
        ));
    }
}

/// Fetches a sprint's issues, capped at [`BOARD_SPRINT_ISSUE_LIMIT`]. Returns
/// the API's own `total` (when Jira reports one) alongside the capped
/// issues so callers can render a "(N shown of M)" note on truncation.
async fn list_sprint_issues<C: AtlassianJsonRequester + ?Sized>(
    client: &C,
    auth: &JiraAgileAuthContext,
    sprint_id: &str,
) -> Result<(Vec<SprintIssue>, Option<usize>), String> {
    let mut start_at = 0usize;
    let mut issues = Vec::new();
    let mut total = None;

    loop {
        let value = client
            .request_json(
                Method::GET,
                HyperAtlassianApiClient::resource_url(
                    auth,
                    JiraAgileResourceKind::Jira,
                    &format!(
                        "/rest/agile/1.0/sprint/{}/issue?fields=summary,status,assignee&startAt={start_at}&maxResults={JIRA_SPRINT_ISSUE_PAGE_SIZE}",
                        percent_encode(sprint_id)
                    ),
                ),
                request_auth(auth),
                None,
            )
            .await?;
        let page = value
            .get("issues")
            .and_then(Value::as_array)
            .ok_or_else(|| "Jira sprint issue response was missing issues".to_string())?;
        let count = page.len();
        for issue in page {
            issues.push(sprint_issue_from_value(issue)?);
        }
        total = value
            .get("total")
            .and_then(Value::as_u64)
            .map(|value| value as usize)
            .or(total);
        if issues.len() >= BOARD_SPRINT_ISSUE_LIMIT
            || sprint_issue_page_is_last(&value, start_at, count)
        {
            issues.truncate(BOARD_SPRINT_ISSUE_LIMIT);
            return Ok((issues, total));
        }
        start_at = start_at.saturating_add(count);
    }
}

fn sprint_issue_page_is_last(value: &Value, start_at: usize, count: usize) -> bool {
    if count == 0 {
        return true;
    }
    match value.get("total").and_then(Value::as_u64) {
        Some(total) => start_at.saturating_add(count) >= total as usize,
        None => count < JIRA_SPRINT_ISSUE_PAGE_SIZE,
    }
}

fn sprint_issue_from_value(value: &Value) -> Result<SprintIssue, String> {
    let key = value
        .get("key")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| "Jira sprint issue response was missing key".to_string())?
        .to_string();
    let fields = value.get("fields").unwrap_or(&Value::Null);
    let summary = fields
        .get("summary")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or(&key)
        .to_string();
    let status = fields.get("status");
    let status_id = status
        .and_then(|status| status.get("id"))
        .and_then(Value::as_str)
        .map(str::to_string);
    let status_name = status
        .and_then(|status| status.get("name"))
        .and_then(Value::as_str)
        .map(str::to_string)
        .unwrap_or_else(|| "Unknown status".to_string());
    let assignee = fields
        .get("assignee")
        .filter(|value| !value.is_null())
        .and_then(|assignee| assignee.get("displayName"))
        .and_then(Value::as_str)
        .map(str::to_string);

    Ok(SprintIssue {
        key,
        summary,
        status_id,
        status_name,
        assignee,
    })
}

fn required_value<'a>(value: &'a str, message: &str) -> Result<&'a str, String> {
    let value = value.trim();
    if value.is_empty() {
        Err(message.to_string())
    } else {
        Ok(value)
    }
}

fn percent_encode(value: &str) -> String {
    value
        .bytes()
        .flat_map(|byte| match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                vec![byte as char]
            }
            _ => format!("%{byte:02X}").chars().collect(),
        })
        .collect()
}
