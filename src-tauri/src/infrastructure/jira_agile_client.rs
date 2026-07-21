use hyper::Method;
use serde_json::Value;

use super::atlassian_client::{
    request_auth, AtlassianJsonRequester, HyperAtlassianApiClient, JiraAgileAuthContext,
    JiraAgileBoardColumn, JiraAgileBoardConfiguration, JiraAgileBoardSummary,
    JiraAgileResourceKind, JiraAgileSprintSummary,
};

const JIRA_AGILE_PAGE_SIZE: usize = 50;

pub(crate) async fn list_jira_boards<C: AtlassianJsonRequester + ?Sized>(
    client: &C,
    auth: &JiraAgileAuthContext,
    project_key: &str,
) -> Result<Vec<JiraAgileBoardSummary>, String> {
    let project_key = required_value(project_key, "Jira project key is required")?;
    let mut start_at = 0usize;
    let mut boards = Vec::new();

    loop {
        let value = client
            .request_json(
                Method::GET,
                HyperAtlassianApiClient::resource_url(
                    auth,
                    JiraAgileResourceKind::Jira,
                    &format!(
                        "/rest/agile/1.0/board?projectKeyOrId={}&startAt={start_at}&maxResults={JIRA_AGILE_PAGE_SIZE}",
                        percent_encode(project_key)
                    ),
                ),
                request_auth(auth),
                None,
            )
            .await?;
        let page = required_page_values(&value, "Jira board response was missing values")?;
        let count = page.len();
        for board in page {
            boards.push(jira_board_from_value(board)?);
        }
        if page_is_last(&value, start_at, count)? {
            return Ok(boards);
        }
        start_at = next_page_start(&value, start_at, count)?;
    }
}

pub(crate) async fn get_jira_board_configuration<C: AtlassianJsonRequester + ?Sized>(
    client: &C,
    auth: &JiraAgileAuthContext,
    board_id: &str,
) -> Result<JiraAgileBoardConfiguration, String> {
    let board_id = required_value(board_id, "Jira board id is required")?;
    let value = client
        .request_json(
            Method::GET,
            HyperAtlassianApiClient::resource_url(
                auth,
                JiraAgileResourceKind::Jira,
                &format!(
                    "/rest/agile/1.0/board/{}/configuration",
                    percent_encode(board_id)
                ),
            ),
            request_auth(auth),
            None,
        )
        .await?;
    jira_board_configuration_from_value(&value)
}

pub(crate) async fn list_jira_active_sprints<C: AtlassianJsonRequester + ?Sized>(
    client: &C,
    auth: &JiraAgileAuthContext,
    board_id: &str,
) -> Result<Vec<JiraAgileSprintSummary>, String> {
    let board_id = required_value(board_id, "Jira board id is required")?;
    let mut start_at = 0usize;
    let mut sprints = Vec::new();

    loop {
        let value = client
            .request_json(
                Method::GET,
                HyperAtlassianApiClient::resource_url(
                    auth,
                    JiraAgileResourceKind::Jira,
                    &format!(
                        "/rest/agile/1.0/board/{}/sprint?state=active&startAt={start_at}&maxResults={JIRA_AGILE_PAGE_SIZE}",
                        percent_encode(board_id)
                    ),
                ),
                request_auth(auth),
                None,
            )
            .await?;
        let page = required_page_values(&value, "Jira sprint response was missing values")?;
        let count = page.len();
        for sprint in page {
            let sprint = jira_sprint_from_value(sprint, board_id)?;
            if sprint.state == "active" {
                sprints.push(sprint);
            }
        }
        if page_is_last(&value, start_at, count)? {
            return Ok(sprints);
        }
        start_at = next_page_start(&value, start_at, count)?;
    }
}

fn jira_board_from_value(value: &Value) -> Result<JiraAgileBoardSummary, String> {
    Ok(JiraAgileBoardSummary {
        id: required_id(value.get("id"), "Jira board response was missing id")?,
        name: required_string(value, "name", "Jira board response was missing name")?,
        board_type: required_string(value, "type", "Jira board response was missing type")?,
        project_key: value
            .get("location")
            .and_then(|location| location.get("projectKey"))
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string),
    })
}

fn jira_board_configuration_from_value(
    value: &Value,
) -> Result<JiraAgileBoardConfiguration, String> {
    let columns = value
        .get("columnConfig")
        .and_then(|config| config.get("columns"))
        .and_then(Value::as_array)
        .ok_or_else(|| "Jira board configuration was missing columns".to_string())?
        .iter()
        .map(jira_board_column_from_value)
        .collect::<Result<Vec<_>, _>>()?;

    Ok(JiraAgileBoardConfiguration {
        id: required_id(value.get("id"), "Jira board configuration was missing id")?,
        name: required_string(value, "name", "Jira board configuration was missing name")?,
        board_type: required_string(value, "type", "Jira board configuration was missing type")?,
        columns,
    })
}

fn jira_board_column_from_value(value: &Value) -> Result<JiraAgileBoardColumn, String> {
    let statuses = value
        .get("statuses")
        .and_then(Value::as_array)
        .ok_or_else(|| "Jira board column was missing statuses".to_string())?;
    let status_ids = statuses
        .iter()
        .map(|status| required_id(status.get("id"), "Jira board status was missing id"))
        .collect::<Result<Vec<_>, _>>()?;
    Ok(JiraAgileBoardColumn {
        name: required_string(value, "name", "Jira board column was missing name")?,
        status_ids,
    })
}

fn jira_sprint_from_value(
    value: &Value,
    requested_board_id: &str,
) -> Result<JiraAgileSprintSummary, String> {
    let state = required_string(value, "state", "Jira sprint response was missing state")?;
    Ok(JiraAgileSprintSummary {
        id: required_id(value.get("id"), "Jira sprint response was missing id")?,
        name: required_string(value, "name", "Jira sprint response was missing name")?,
        state,
        board_id: value
            .get("originBoardId")
            .map(|value| required_id(Some(value), "Jira sprint response had an invalid board id"))
            .transpose()?
            .unwrap_or_else(|| requested_board_id.to_string()),
        start_date: optional_string(value, "startDate"),
        end_date: optional_string(value, "endDate"),
        complete_date: optional_string(value, "completeDate"),
        goal: optional_string(value, "goal"),
    })
}

fn required_page_values<'a>(value: &'a Value, message: &str) -> Result<&'a [Value], String> {
    value
        .get("values")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .ok_or_else(|| message.to_string())
}

fn page_is_last(value: &Value, start_at: usize, count: usize) -> Result<bool, String> {
    if let Some(is_last) = value.get("isLast").and_then(Value::as_bool) {
        if !is_last && count == 0 {
            return Err("Jira pagination returned an empty non-final page".to_string());
        }
        return Ok(is_last);
    }
    if let Some(total) = value.get("total").and_then(Value::as_u64) {
        return Ok(start_at.saturating_add(count) >= total as usize);
    }
    Ok(count < JIRA_AGILE_PAGE_SIZE)
}

fn next_page_start(value: &Value, start_at: usize, count: usize) -> Result<usize, String> {
    if count == 0 {
        return Err("Jira pagination did not advance".to_string());
    }
    let response_start = value
        .get("startAt")
        .and_then(Value::as_u64)
        .map(|value| value as usize)
        .unwrap_or(start_at);
    let next = response_start.saturating_add(count);
    if next <= start_at {
        return Err("Jira pagination did not advance".to_string());
    }
    Ok(next)
}

fn required_id(value: Option<&Value>, message: &str) -> Result<String, String> {
    let rendered = value
        .and_then(|value| {
            value
                .as_str()
                .map(str::to_string)
                .or_else(|| value.as_u64().map(|value| value.to_string()))
        })
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty());
    rendered.ok_or_else(|| message.to_string())
}

fn required_string(value: &Value, field: &str, message: &str) -> Result<String, String> {
    value
        .get(field)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .ok_or_else(|| message.to_string())
}

fn optional_string(value: &Value, field: &str) -> Option<String> {
    value
        .get(field)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
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
