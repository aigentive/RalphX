use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::application::automation::judge::{
    extract_automation_verdict_value, first_non_done_goal_item_value, truncate_utf8_to_bytes,
    AutomationJudgeAttachmentContext, AUTOMATION_JUDGE_PROMPT_MAX_BYTES, SPEC_ATTACHMENT_MAX_BYTES,
};
use crate::domain::entities::{Automation, AutomationRun};
use crate::error::{AppError, AppResult};

pub const AUTOMATION_PLAN_JUDGE_PROMPT_MAX_BYTES: usize = AUTOMATION_JUDGE_PROMPT_MAX_BYTES;
const AUTOMATION_PLAN_JUDGE_RETRY_INSTRUCTION: &str = "\n<retry_instruction truncated=\"false\">\nPrevious plan judge output was invalid. Return exactly one JSON object matching the output_contract and no prose.\n</retry_instruction>\n";
const GOAL_MAX_BYTES: usize = 10 * 1024;
const GOAL_ITEMS_MAX_BYTES: usize = 12 * 1024;
const RUN_PROMPT_MAX_BYTES: usize = 8 * 1024;
const PLAN_MAX_BYTES: usize = 28 * 1024;
const VERIFICATION_MAX_BYTES: usize = 8 * 1024;
const PREVIOUS_VERDICT_MAX_BYTES: usize = 8 * 1024;
const MIN_REVISION_INSTRUCTIONS_CHARS: usize = 40;

const REQUIRED_PLAN_VERDICT_KEYS: &[&str] =
    &["decision", "reason", "confidence", "evaluatedArtifactId"];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BuildAutomationPlanJudgePromptInput<'a> {
    pub automation: &'a Automation,
    pub run: &'a AutomationRun,
    pub evaluated_artifact_id: &'a str,
    pub plan_content: &'a str,
    pub verification_context: Option<&'a AutomationPlanVerificationJudgeContext>,
    pub spec_attachments: &'a [AutomationJudgeAttachmentContext],
    pub previous_verdict_json: Option<&'a str>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AutomationPlanVerificationJudgeContext {
    pub status: String,
    pub in_progress: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub generation: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub current_round: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_rounds: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub convergence_reason: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub gap_count: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub gap_score: Option<u32>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub gaps: Vec<AutomationPlanVerificationGapSummary>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub unavailable_reason: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AutomationPlanVerificationGapSummary {
    pub severity: String,
    pub category: String,
    pub description: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub why_it_matters: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AutomationPlanJudgeDecision {
    Approve,
    Revise,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AutomationPlanJudgeConfidence {
    Low,
    Medium,
    High,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AutomationPlanJudgeVerdict {
    pub decision: AutomationPlanJudgeDecision,
    pub reason: String,
    pub confidence: AutomationPlanJudgeConfidence,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub revision_instructions: Option<String>,
    pub evaluated_artifact_id: String,
    /// Backend-stamped artifact version at judge invocation time; not part of
    /// the model output contract. `None` on legacy stored verdicts, which must
    /// fail closed against the current artifact version.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub evaluated_artifact_version: Option<u32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AutomationPlanJudgeValidationContext<'a> {
    pub expected_artifact_id: Option<&'a str>,
}

pub fn build_automation_plan_judge_prompt(
    input: BuildAutomationPlanJudgePromptInput<'_>,
) -> AppResult<String> {
    let output_contract = output_contract_section();
    if output_contract.len() > AUTOMATION_PLAN_JUDGE_PROMPT_MAX_BYTES {
        return Err(AppError::Validation(
            "automation plan judge output contract exceeds the 64KB argv-safe budget".to_string(),
        ));
    }

    let mut remaining = AUTOMATION_PLAN_JUDGE_PROMPT_MAX_BYTES - output_contract.len();
    let goal = budgeted_xml_section(
        "goal",
        input.automation.goal_prompt.trim(),
        GOAL_MAX_BYTES,
        remaining,
        &[],
    );
    remaining = remaining.saturating_sub(goal.len());

    let goal_items = budgeted_xml_section(
        "goal_items",
        &format_goal_items(input.automation.goal_items_json.as_deref()),
        GOAL_ITEMS_MAX_BYTES,
        remaining,
        &[],
    );
    remaining = remaining.saturating_sub(goal_items.len());

    let spec = if input.spec_attachments.is_empty() {
        String::new()
    } else {
        budgeted_xml_section(
            "spec",
            &format_spec_attachments(input.spec_attachments),
            SPEC_ATTACHMENT_MAX_BYTES,
            remaining,
            &[],
        )
    };
    remaining = remaining.saturating_sub(spec.len());

    let run_prompt = budgeted_xml_section(
        "run_prompt",
        &input.run.run_prompt,
        RUN_PROMPT_MAX_BYTES,
        remaining,
        &[],
    );
    remaining = remaining.saturating_sub(run_prompt.len());

    let plan = budgeted_xml_section(
        "plan",
        input.plan_content,
        PLAN_MAX_BYTES,
        remaining,
        &[("artifact_id", input.evaluated_artifact_id)],
    );
    remaining = remaining.saturating_sub(plan.len());

    let verification = input
        .verification_context
        .map(|context| {
            budgeted_xml_section(
                "verification",
                &format_verification_context(context),
                VERIFICATION_MAX_BYTES,
                remaining,
                &[],
            )
        })
        .unwrap_or_default();
    remaining = remaining.saturating_sub(verification.len());

    let previous_verdict = budgeted_xml_section(
        "previous_verdict",
        &format_previous_verdict(input.run, input.previous_verdict_json),
        PREVIOUS_VERDICT_MAX_BYTES,
        remaining,
        &[],
    );

    let prompt =
        format!("{goal}{goal_items}{spec}{run_prompt}{plan}{verification}{previous_verdict}{output_contract}");
    if prompt.len() > AUTOMATION_PLAN_JUDGE_PROMPT_MAX_BYTES {
        return Err(AppError::Validation(
            "automation plan judge prompt exceeded the 64KB argv-safe budget".to_string(),
        ));
    }
    Ok(prompt)
}

pub fn append_automation_plan_judge_retry_instruction(prompt: &mut String) -> bool {
    if prompt.len() + AUTOMATION_PLAN_JUDGE_RETRY_INSTRUCTION.len()
        > AUTOMATION_PLAN_JUDGE_PROMPT_MAX_BYTES
    {
        return false;
    }
    prompt.push_str(AUTOMATION_PLAN_JUDGE_RETRY_INSTRUCTION);
    true
}

pub fn parse_automation_plan_judge_verdict(
    output: &str,
    context: AutomationPlanJudgeValidationContext<'_>,
) -> AppResult<AutomationPlanJudgeVerdict> {
    let value = extract_automation_verdict_value(output)?;
    validate_required_plan_verdict_keys(&value)?;
    let has_revision_instructions = value
        .as_object()
        .is_some_and(|object| object.contains_key("revisionInstructions"));
    let verdict = serde_json::from_value::<AutomationPlanJudgeVerdict>(value).map_err(|error| {
        AppError::Validation(format!("invalid plan judge verdict JSON: {error}"))
    })?;
    validate_automation_plan_judge_verdict(verdict, has_revision_instructions, context)
}

pub fn validate_automation_plan_judge_verdict(
    mut verdict: AutomationPlanJudgeVerdict,
    has_revision_instructions_key: bool,
    context: AutomationPlanJudgeValidationContext<'_>,
) -> AppResult<AutomationPlanJudgeVerdict> {
    verdict.reason = verdict.reason.trim().chars().take(1000).collect();
    if verdict.reason.is_empty() {
        return Err(AppError::Validation(
            "plan judge verdict reason is required".to_string(),
        ));
    }
    verdict.evaluated_artifact_id = verdict.evaluated_artifact_id.trim().to_string();
    if verdict.evaluated_artifact_id.is_empty() {
        return Err(AppError::Validation(
            "plan judge verdict evaluatedArtifactId is required".to_string(),
        ));
    }
    if let Some(expected) = context.expected_artifact_id {
        if verdict.evaluated_artifact_id != expected {
            return Err(AppError::Validation(format!(
                "plan judge verdict evaluatedArtifactId {} did not match prompt artifact id {}",
                verdict.evaluated_artifact_id, expected
            )));
        }
    }

    match verdict.decision {
        AutomationPlanJudgeDecision::Approve => {
            if has_revision_instructions_key {
                return Err(AppError::Validation(
                    "plan judge approve verdict must not include revisionInstructions".to_string(),
                ));
            }
            verdict.revision_instructions = None;
        }
        AutomationPlanJudgeDecision::Revise => {
            let instructions = verdict
                .revision_instructions
                .as_deref()
                .map(str::trim)
                .unwrap_or("");
            if instructions.chars().count() < MIN_REVISION_INSTRUCTIONS_CHARS {
                return Err(AppError::Validation(
                    "plan judge revise verdict requires substantive revisionInstructions"
                        .to_string(),
                ));
            }
            verdict.revision_instructions = Some(instructions.chars().take(4000).collect());
        }
    }

    Ok(verdict)
}

fn validate_required_plan_verdict_keys(value: &Value) -> AppResult<()> {
    let object = value.as_object().ok_or_else(|| {
        AppError::Validation("plan judge verdict must be a JSON object".to_string())
    })?;
    for key in REQUIRED_PLAN_VERDICT_KEYS {
        if !object.contains_key(*key) {
            return Err(AppError::Validation(format!(
                "plan judge verdict missing required key {key}"
            )));
        }
    }
    Ok(())
}

fn budgeted_xml_section(
    tag: &str,
    raw: &str,
    cap: usize,
    remaining: usize,
    attrs: &[(&str, &str)],
) -> String {
    if remaining == 0 {
        return String::new();
    }
    let mut max_body = raw.len().min(cap);
    loop {
        let (body, body_truncated) = truncate_utf8_to_bytes(raw, max_body);
        let truncated = body_truncated || raw.len() > cap;
        let section = xml_section(tag, &body, truncated, attrs);
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

fn xml_section(tag: &str, body: &str, truncated: bool, attrs: &[(&str, &str)]) -> String {
    let attrs = attrs
        .iter()
        .map(|(key, value)| format!(" {key}=\"{}\"", escape_xml_attr(value)))
        .collect::<String>();
    format!("\n<{tag}{attrs} truncated=\"{truncated}\">\n{body}\n</{tag}>\n")
}

fn escape_xml_attr(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('"', "&quot;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

fn format_goal_items(goal_items_json: Option<&str>) -> String {
    let raw = goal_items_json
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("[]");
    let parsed = serde_json::from_str::<Value>(raw);
    let current_phase = parsed
        .as_ref()
        .ok()
        .and_then(first_non_done_goal_item_value);
    serde_json::to_string_pretty(&json!({
        "items": parsed.unwrap_or_else(|_| Value::String(raw.to_string())),
        "currentPhase": current_phase,
    }))
    .expect("goal items context JSON should serialize")
}

fn format_spec_attachments(attachments: &[AutomationJudgeAttachmentContext]) -> String {
    serde_json::to_string_pretty(&attachments).expect("spec attachment JSON should serialize")
}

fn format_verification_context(context: &AutomationPlanVerificationJudgeContext) -> String {
    serde_json::to_string_pretty(context).expect("verification context JSON should serialize")
}

fn format_previous_verdict(run: &AutomationRun, previous_verdict_json: Option<&str>) -> String {
    serde_json::to_string_pretty(&json!({
        "planRevisionRound": run.plan_revision_round,
        "verdict": previous_verdict_json
            .and_then(|value| serde_json::from_str::<Value>(value).ok())
            .unwrap_or(Value::Null),
    }))
    .expect("previous verdict context JSON should serialize")
}

fn output_contract_section() -> String {
    r#"
<output_contract truncated="false">
Return exactly one JSON object:
{
  "decision": "approve" | "revise",
  "reason": "string, <= 1000 chars",
  "confidence": "low" | "medium" | "high",
  "revisionInstructions": "string, required and at least 40 chars iff decision is revise, absent on approve",
  "evaluatedArtifactId": "the exact plan artifact id from the plan section"
}

Rules:
- Judge only the run plan against the automation goal, goal items, current phase, run prompt, advisory spec context, and advisory verification outcome.
- Verification gap findings inform the verdict but never mandate revise on their own; if verification is unavailable, proceed on the other evidence.
- Choose approve only when the plan is aligned with the current phase, scoped, plausible, and ready for implementation.
- Choose revise when the plan is missing required scope, recovery, validation, phase alignment, or feasibility detail.
- `evaluatedArtifactId` must exactly match the `artifact_id` attribute on the plan section.
- Do not include markdown fences, prose, tool calls, or fields outside the JSON verdict.
</output_contract>
"#
    .to_string()
}
