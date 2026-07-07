use std::collections::{BTreeMap, HashSet};

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::domain::entities::{Automation, AutomationRun, AutomationRunStatus};
use crate::error::{AppError, AppResult};

pub const AUTOMATION_JUDGE_PROMPT_MAX_BYTES: usize = 64 * 1024;
const AUTOMATION_JUDGE_RETRY_INSTRUCTION: &str = "\n<retry_instruction truncated=\"false\">\nPrevious judge output was invalid. Return exactly one JSON object matching the output_contract and no prose.\n</retry_instruction>\n";

const SETUP_ANALYSIS_MAX_BYTES: usize = 8 * 1024;
const ORIGINAL_INPUTS_MAX_BYTES: usize = 12 * 1024;
const RUN_HISTORY_MAX_BYTES: usize = 18 * 1024;
const PREVIOUS_RUN_MAX_BYTES: usize = 16 * 1024;
const PREVIOUS_RUN_SUMMARY_MAX_BYTES: usize = 8 * 1024;
const MIN_CONTINUE_PROMPT_CHARS: usize = 40;

const REQUIRED_VERDICT_KEYS: &[&str] = &[
    "decision",
    "goalMet",
    "reason",
    "confidence",
    "goalProgress",
    "updatedItemStatuses",
    "nextRunPrompt",
    "nextBaseBranch",
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BuildAutomationJudgePromptInput<'a> {
    pub automation: &'a Automation,
    pub runs: &'a [AutomationRun],
    pub previous_run: &'a AutomationRun,
    pub attachments: &'a [AutomationJudgeAttachmentContext],
    pub context_refs: &'a [AutomationJudgeContextRefSummary],
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AutomationJudgeAttachmentContext {
    pub file_name: String,
    pub mime_type: Option<String>,
    pub file_size: Option<i64>,
    pub text_content: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AutomationJudgeContextRefSummary {
    pub provider: String,
    pub kind: String,
    pub key: String,
    pub title: Option<String>,
    pub url: Option<String>,
    pub summary_excerpt: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AutomationJudgeDecision {
    Continue,
    Stop,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AutomationJudgeNextBaseBranch {
    AutomationBase,
    PreviousPrHead,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AutomationGoalItemStatus {
    Pending,
    InProgress,
    Done,
    Skipped,
}

impl AutomationGoalItemStatus {
    fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::InProgress => "in_progress",
            Self::Done => "done",
            Self::Skipped => "skipped",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AutomationJudgeGoalProgress {
    pub completed_items: i64,
    pub total_items: i64,
    pub summary: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AutomationJudgeItemStatusUpdate {
    pub id: String,
    pub status: AutomationGoalItemStatus,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AutomationJudgeVerdict {
    pub decision: AutomationJudgeDecision,
    pub goal_met: bool,
    pub reason: String,
    pub confidence: f64,
    pub goal_progress: Option<AutomationJudgeGoalProgress>,
    pub updated_item_statuses: Option<Vec<AutomationJudgeItemStatusUpdate>>,
    pub next_run_prompt: Option<String>,
    pub next_base_branch: Option<AutomationJudgeNextBaseBranch>,
}

pub struct AutomationJudgeValidationContext<'a> {
    pub automation: &'a Automation,
    pub previous_run: &'a AutomationRun,
}

pub fn build_automation_judge_prompt(
    input: BuildAutomationJudgePromptInput<'_>,
) -> AppResult<String> {
    let fixed_start = format!(
        "{}{}{}",
        xml_section("goal", input.automation.goal_prompt.trim(), false),
        xml_section(
            "automation_config",
            &format_automation_config(&input),
            false
        ),
        xml_section(
            "goal_items",
            input
                .automation
                .goal_items_json
                .as_deref()
                .unwrap_or("null"),
            false
        )
    );
    let fixed_tail = format!(
        "{}{}",
        xml_section(
            "remaining_work_signal",
            &format_remaining_signal(&input),
            false
        ),
        output_contract_section()
    );
    let fixed_bytes = fixed_start.len() + fixed_tail.len();
    if fixed_bytes > AUTOMATION_JUDGE_PROMPT_MAX_BYTES {
        return Err(AppError::Validation(
            "automation judge prompt fixed sections exceed the 64KB argv-safe budget".to_string(),
        ));
    }

    let mut remaining = AUTOMATION_JUDGE_PROMPT_MAX_BYTES - fixed_bytes;
    let setup = budgeted_xml_section(
        "setup_analysis",
        input
            .automation
            .setup_analysis_summary
            .as_deref()
            .unwrap_or(""),
        SETUP_ANALYSIS_MAX_BYTES,
        remaining,
    );
    remaining = remaining.saturating_sub(setup.len());

    let previous_run = budgeted_xml_section(
        "previous_run",
        &format_previous_run(input.previous_run),
        PREVIOUS_RUN_MAX_BYTES,
        remaining,
    );
    remaining = remaining.saturating_sub(previous_run.len());

    let original_inputs = budgeted_xml_section(
        "original_inputs",
        &format_original_inputs(input.attachments, input.context_refs),
        ORIGINAL_INPUTS_MAX_BYTES,
        remaining,
    );
    remaining = remaining.saturating_sub(original_inputs.len());

    let run_history = budgeted_xml_section(
        "run_history",
        &format_run_history(input.runs, input.previous_run),
        RUN_HISTORY_MAX_BYTES,
        remaining,
    );

    let prompt =
        format!("{fixed_start}{setup}{previous_run}{original_inputs}{run_history}{fixed_tail}");
    if prompt.len() > AUTOMATION_JUDGE_PROMPT_MAX_BYTES {
        return Err(AppError::Validation(
            "automation judge prompt exceeded the 64KB argv-safe budget".to_string(),
        ));
    }
    Ok(prompt)
}

pub fn append_automation_judge_retry_instruction(prompt: &mut String) -> bool {
    if prompt.len() + AUTOMATION_JUDGE_RETRY_INSTRUCTION.len() > AUTOMATION_JUDGE_PROMPT_MAX_BYTES {
        return false;
    }
    prompt.push_str(AUTOMATION_JUDGE_RETRY_INSTRUCTION);
    true
}

pub fn parse_automation_judge_verdict(
    output: &str,
    context: AutomationJudgeValidationContext<'_>,
) -> AppResult<AutomationJudgeVerdict> {
    let value = extract_automation_verdict_value(output)?;
    validate_required_verdict_keys(&value)?;
    let verdict = serde_json::from_value::<AutomationJudgeVerdict>(value)
        .map_err(|error| AppError::Validation(format!("invalid judge verdict JSON: {error}")))?;
    validate_automation_judge_verdict(verdict, context)
}

pub fn validate_automation_judge_verdict(
    mut verdict: AutomationJudgeVerdict,
    context: AutomationJudgeValidationContext<'_>,
) -> AppResult<AutomationJudgeVerdict> {
    let goal_item_ids = collect_goal_item_ids(context.automation.goal_items_json.as_deref())?;
    if let Some(updates) = verdict.updated_item_statuses.as_deref() {
        for update in updates {
            if !goal_item_ids.contains(&update.id) {
                return Err(AppError::Validation(format!(
                    "judge verdict updatedItemStatuses references unknown goal item id {}",
                    update.id
                )));
            }
        }
    }

    verdict.reason = verdict.reason.trim().chars().take(1000).collect();
    if verdict.reason.is_empty() {
        return Err(AppError::Validation(
            "judge verdict reason is required".to_string(),
        ));
    }
    verdict.confidence = verdict.confidence.clamp(0.0, 1.0);

    match verdict.decision {
        AutomationJudgeDecision::Continue => {
            let prompt = verdict
                .next_run_prompt
                .as_deref()
                .map(str::trim)
                .unwrap_or("");
            if prompt.chars().count() < MIN_CONTINUE_PROMPT_CHARS {
                return Err(AppError::Validation(
                    "judge verdict continue requires a substantive nextRunPrompt".to_string(),
                ));
            }
            match verdict.next_base_branch {
                Some(AutomationJudgeNextBaseBranch::AutomationBase) => {
                    if context.automation.chain_mode == "pr_head_stacked" {
                        return Err(AppError::Validation(
                            "judge verdict automation_base is not valid for stacked chaining"
                                .to_string(),
                        ));
                    }
                }
                Some(AutomationJudgeNextBaseBranch::PreviousPrHead) => {
                    if context.automation.chain_mode != "pr_head_stacked"
                        || context
                            .previous_run
                            .pr_head_ref_name
                            .as_deref()
                            .map(str::trim)
                            .unwrap_or("")
                            .is_empty()
                    {
                        return Err(AppError::Validation(
                            "judge verdict previous_pr_head is only valid for stacked chaining with a previous PR head".to_string(),
                        ));
                    }
                }
                None => {
                    return Err(AppError::Validation(
                        "judge verdict continue requires nextBaseBranch".to_string(),
                    ));
                }
            }
        }
        AutomationJudgeDecision::Stop => {
            if verdict
                .next_run_prompt
                .as_deref()
                .map(str::trim)
                .is_some_and(|prompt| !prompt.is_empty())
                || verdict.next_base_branch.is_some()
            {
                return Err(AppError::Validation(
                    "judge verdict stop must not include a next run prompt or base".to_string(),
                ));
            }
        }
    }

    Ok(verdict)
}

pub fn apply_updated_item_statuses(
    goal_items_json: Option<&str>,
    updates: Option<&[AutomationJudgeItemStatusUpdate]>,
) -> AppResult<Option<String>> {
    let Some(updates) = updates.filter(|updates| !updates.is_empty()) else {
        return Ok(goal_items_json.map(str::to_string));
    };
    let Some(goal_items_json) = goal_items_json else {
        return Err(AppError::Validation(
            "judge status updates require goal_items_json".to_string(),
        ));
    };
    let mut value = parse_goal_items_json(goal_items_json)?;
    let updates_by_id = updates
        .iter()
        .map(|update| (update.id.clone(), update.status.as_str().to_string()))
        .collect::<BTreeMap<_, _>>();
    let changed = apply_goal_item_updates(&mut value, &updates_by_id);
    if changed != updates_by_id.len() {
        return Err(AppError::Validation(
            "judge status updates did not match stored goal item ids".to_string(),
        ));
    }
    serde_json::to_string(&value)
        .map(Some)
        .map_err(|error| AppError::Validation(format!("failed to serialize goal items: {error}")))
}

pub fn automation_judge_loop_suspected(
    previous_run: &AutomationRun,
    verdict: &AutomationJudgeVerdict,
) -> bool {
    if verdict.decision != AutomationJudgeDecision::Continue {
        return false;
    }
    if previous_run.status == AutomationRunStatus::Merged {
        return false;
    }
    let Some(next_prompt) = verdict.next_run_prompt.as_deref() else {
        return false;
    };
    normalized_prompt_fingerprint(&previous_run.run_prompt)
        == normalized_prompt_fingerprint(next_prompt)
}

fn extract_automation_verdict_value(output: &str) -> AppResult<Value> {
    let trimmed = output.trim();
    if let Ok(value) = serde_json::from_str::<Value>(trimmed) {
        if value.is_object() {
            return Ok(value);
        }
    }
    if let Some(value) = extract_fenced_json_object(trimmed) {
        return Ok(value);
    }
    if let Some(value) = extract_last_json_object(trimmed) {
        return Ok(value);
    }
    Err(AppError::Validation(
        "automation judge output did not contain a JSON object".to_string(),
    ))
}

fn extract_fenced_json_object(text: &str) -> Option<Value> {
    let mut rest = text;
    while let Some(start) = rest.find("```") {
        let after_open = &rest[start + 3..];
        let after_lang = after_open
            .strip_prefix("json")
            .or_else(|| after_open.strip_prefix("JSON"))?;
        let content_start = after_lang.strip_prefix('\n').unwrap_or(after_lang);
        let end = content_start.find("```")?;
        let candidate = content_start[..end].trim();
        if let Ok(value) = serde_json::from_str::<Value>(candidate) {
            if value.is_object() {
                return Some(value);
            }
        }
        rest = &content_start[end + 3..];
    }
    None
}

fn extract_last_json_object(text: &str) -> Option<Value> {
    let mut last = None;
    for (start, ch) in text.char_indices() {
        if ch != '{' {
            continue;
        }
        if let Some(end) = matching_json_object_end(&text[start..]) {
            let candidate = &text[start..start + end];
            if let Ok(value) = serde_json::from_str::<Value>(candidate) {
                if value.is_object() {
                    last = Some(value);
                }
            }
        }
    }
    last
}

fn matching_json_object_end(text: &str) -> Option<usize> {
    let mut depth = 0usize;
    let mut in_string = false;
    let mut escaped = false;
    for (offset, ch) in text.char_indices() {
        if in_string {
            if escaped {
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == '"' {
                in_string = false;
            }
            continue;
        }
        match ch {
            '"' => in_string = true,
            '{' => depth += 1,
            '}' => {
                depth = depth.checked_sub(1)?;
                if depth == 0 {
                    return Some(offset + ch.len_utf8());
                }
            }
            _ => {}
        }
    }
    None
}

fn validate_required_verdict_keys(value: &Value) -> AppResult<()> {
    let object = value
        .as_object()
        .ok_or_else(|| AppError::Validation("judge verdict must be a JSON object".to_string()))?;
    for key in REQUIRED_VERDICT_KEYS {
        if !object.contains_key(*key) {
            return Err(AppError::Validation(format!(
                "judge verdict missing required key {key}"
            )));
        }
    }
    Ok(())
}

fn collect_goal_item_ids(goal_items_json: Option<&str>) -> AppResult<HashSet<String>> {
    let Some(goal_items_json) = goal_items_json.filter(|value| !value.trim().is_empty()) else {
        return Ok(HashSet::new());
    };
    let value = parse_goal_items_json(goal_items_json)?;
    let mut ids = HashSet::new();
    collect_ids_from_value(&value, &mut ids);
    Ok(ids)
}

fn parse_goal_items_json(goal_items_json: &str) -> AppResult<Value> {
    serde_json::from_str::<Value>(goal_items_json).map_err(|error| {
        AppError::Validation(format!(
            "automation goal_items_json is invalid JSON: {error}"
        ))
    })
}

fn collect_ids_from_value(value: &Value, ids: &mut HashSet<String>) {
    match value {
        Value::Object(object) => {
            if let Some(id) = object.get("id").and_then(Value::as_str) {
                ids.insert(id.to_string());
            }
            for value in object.values() {
                collect_ids_from_value(value, ids);
            }
        }
        Value::Array(values) => {
            for value in values {
                collect_ids_from_value(value, ids);
            }
        }
        _ => {}
    }
}

fn apply_goal_item_updates(value: &mut Value, updates: &BTreeMap<String, String>) -> usize {
    match value {
        Value::Object(object) => {
            let mut changed = 0;
            let id = object.get("id").and_then(Value::as_str).map(str::to_string);
            if let Some(id) = id {
                if let Some(status) = updates.get(&id) {
                    object.insert("status".to_string(), Value::String(status.clone()));
                    changed += 1;
                }
            }
            for value in object.values_mut() {
                changed += apply_goal_item_updates(value, updates);
            }
            changed
        }
        Value::Array(values) => values
            .iter_mut()
            .map(|value| apply_goal_item_updates(value, updates))
            .sum(),
        _ => 0,
    }
}

fn xml_section(tag: &str, body: &str, truncated: bool) -> String {
    format!("\n<{tag} truncated=\"{truncated}\">\n{body}\n</{tag}>\n")
}

fn budgeted_xml_section(tag: &str, raw: &str, cap: usize, remaining: usize) -> String {
    if remaining == 0 {
        return String::new();
    }
    let mut max_body = raw.len().min(cap);
    loop {
        let (body, body_truncated) = truncate_utf8_to_bytes(raw, max_body);
        let truncated = body_truncated || raw.len() > cap;
        let section = xml_section(tag, &body, truncated);
        if section.len() <= remaining {
            return section;
        }
        if max_body == 0 {
            return String::new();
        }
        let overflow = section.len() - remaining;
        max_body = max_body.saturating_sub(overflow.max(1));
    }
}

fn truncate_utf8_to_bytes(value: &str, max_bytes: usize) -> (String, bool) {
    if value.len() <= max_bytes {
        return (value.to_string(), false);
    }
    let mut end = max_bytes;
    while end > 0 && !value.is_char_boundary(end) {
        end -= 1;
    }
    (value[..end].to_string(), true)
}

fn format_automation_config(input: &BuildAutomationJudgePromptInput<'_>) -> String {
    let automation = input.automation;
    serde_json::to_string_pretty(&json!({
        "name": automation.name,
        "runMode": automation.run_mode,
        "chainMode": automation.chain_mode,
        "baseRefKind": automation.base_ref_kind,
        "baseRef": automation.base_ref,
        "baseDisplayName": automation.base_display_name,
        "providerHarness": automation.provider_harness,
        "modelId": automation.model_id,
        "logicalEffort": automation.logical_effort,
        "maxRuns": automation.max_runs,
        "maxConsecutiveFailures": automation.max_consecutive_failures,
        "runsUsed": input.runs.len(),
        "completionSignal": automation.completion_signal,
    }))
    .expect("automation config JSON should serialize")
}

fn format_original_inputs(
    attachments: &[AutomationJudgeAttachmentContext],
    context_refs: &[AutomationJudgeContextRefSummary],
) -> String {
    serde_json::to_string_pretty(&json!({
        "attachments": attachments,
        "contextRefs": context_refs,
    }))
    .expect("original input JSON should serialize")
}

fn format_run_history(runs: &[AutomationRun], previous_run: &AutomationRun) -> String {
    let values = runs
        .iter()
        .filter(|run| run.id != previous_run.id)
        .map(|run| {
            json!({
                "runIndex": run.run_index,
                "status": run.status.as_str(),
                "judgeState": run.judge_state.as_str(),
                "promptAuthor": run.prompt_author.as_str(),
                "runPrompt": truncate_utf8_to_bytes(&run.run_prompt, 1024).0,
                "prNumber": run.pr_number,
                "prTitle": run.pr_title,
                "prUrl": run.pr_url,
                "prHeadRefName": run.pr_head_ref_name,
                "prBaseRefName": run.pr_base_ref_name,
                "mergeCommitSha": run.merge_commit_sha,
                "agentSummary": run.agent_summary.as_deref().map(|summary| {
                    truncate_utf8_to_bytes(summary, 1024).0
                }),
                "judgeVerdict": run.judge_verdict_json,
            })
        })
        .collect::<Vec<_>>();
    serde_json::to_string_pretty(&values).expect("run history JSON should serialize")
}

fn format_previous_run(run: &AutomationRun) -> String {
    serde_json::to_string_pretty(&json!({
        "runIndex": run.run_index,
        "status": run.status.as_str(),
        "judgeState": run.judge_state.as_str(),
        "runPrompt": run.run_prompt,
        "promptAuthor": run.prompt_author.as_str(),
        "prNumber": run.pr_number,
        "prTitle": run.pr_title,
        "prUrl": run.pr_url,
        "prHeadRefName": run.pr_head_ref_name,
        "prBaseRefName": run.pr_base_ref_name,
        "mergedAt": run.pr_merged_at,
        "mergeCommitSha": run.merge_commit_sha,
        "diffStats": run.diff_stats_json,
        "agentSummary": run.agent_summary.as_deref().map(|summary| {
            truncate_utf8_to_bytes(summary, PREVIOUS_RUN_SUMMARY_MAX_BYTES).0
        }),
        "errorCode": run.error_code,
        "errorDetail": run.error_detail,
        "branchName": run.branch_name,
        "baseRefKind": run.base_ref_kind,
        "baseRefUsed": run.base_ref_used,
    }))
    .expect("previous run JSON should serialize")
}

fn format_remaining_signal(input: &BuildAutomationJudgePromptInput<'_>) -> String {
    let stats = goal_item_stats(input.automation.goal_items_json.as_deref());
    let runs_used = input.runs.len() as i64;
    serde_json::to_string_pretty(&json!({
        "goalItemsTotal": stats.total,
        "goalItemsDone": stats.done,
        "goalItemsPending": stats.pending,
        "runsUsed": runs_used,
        "runsRemaining": (input.automation.max_runs - runs_used).max(0),
        "consecutiveFailureCount": consecutive_failure_count(input.runs),
    }))
    .expect("remaining signal JSON should serialize")
}

#[derive(Default)]
struct GoalItemStats {
    total: i64,
    done: i64,
    pending: i64,
}

fn goal_item_stats(goal_items_json: Option<&str>) -> GoalItemStats {
    let Some(goal_items_json) = goal_items_json else {
        return GoalItemStats::default();
    };
    let Ok(value) = serde_json::from_str::<Value>(goal_items_json) else {
        return GoalItemStats::default();
    };
    let mut stats = GoalItemStats::default();
    collect_status_stats(&value, &mut stats);
    stats
}

fn collect_status_stats(value: &Value, stats: &mut GoalItemStats) {
    match value {
        Value::Object(object) => {
            if let Some(status) = object.get("status").and_then(Value::as_str) {
                stats.total += 1;
                match status {
                    "done" | "skipped" => stats.done += 1,
                    "pending" | "in_progress" => stats.pending += 1,
                    _ => {}
                }
            }
            for value in object.values() {
                collect_status_stats(value, stats);
            }
        }
        Value::Array(values) => {
            for value in values {
                collect_status_stats(value, stats);
            }
        }
        _ => {}
    }
}

fn consecutive_failure_count(runs: &[AutomationRun]) -> i64 {
    let mut count = 0;
    for run in runs.iter().rev() {
        match run.status {
            AutomationRunStatus::AgentFailed | AutomationRunStatus::PrClosed => count += 1,
            AutomationRunStatus::Merged => break,
            _ => {}
        }
    }
    count
}

fn output_contract_section() -> String {
    r#"
<output_contract truncated="false">
Return exactly one JSON object:
{
  "decision": "continue" | "stop",
  "goalMet": true | false,
  "reason": "string, <= 1000 chars",
  "confidence": 0.0-1.0,
  "goalProgress": { "completedItems": 7, "totalItems": 20, "summary": "string" } | null,
  "updatedItemStatuses": [ { "id": "string", "status": "pending"|"in_progress"|"done"|"skipped" } ] | null,
  "nextRunPrompt": "string" | null,
  "nextBaseBranch": "automation_base" | "previous_pr_head" | null
}

Rules:
- `continue` requires `nextRunPrompt` and `nextBaseBranch`.
- `previous_pr_head` is valid only for `chainMode=pr_head_stacked` with a valid previous PR head.
- `updatedItemStatuses` ids must exist in `goal_items`.
- If there is no concrete unfinished work, choose `stop`.
- The next run prompt must be self-contained and may cite attached specs by file or section.
</output_contract>
"#
    .to_string()
}

fn normalized_prompt_fingerprint(value: &str) -> String {
    value
        .split_whitespace()
        .flat_map(|part| {
            part.chars()
                .filter(|ch| ch.is_alphanumeric())
                .flat_map(char::to_lowercase)
        })
        .collect()
}
