use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use chrono::Utc;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::application::automation::judge::{
    extract_automation_verdict_value, truncate_utf8_to_bytes, AUTOMATION_JUDGE_PROMPT_MAX_BYTES,
};
use crate::application::automation::service::{validate_finalizable, AutomationService};
use crate::application::automation::utility_agent::{
    invoke_automation_utility_agent, AutomationUtilityModelPolicy,
};
use crate::application::AppState;
use crate::domain::entities::{Automation, AutomationId};
use crate::error::{AppError, AppResult};
use crate::infrastructure::agents::claude::agent_names;

const REQUIRED_VERDICT_KEYS: &[&str] = &["decision", "reason", "confidence", "findings"];
const OUTPUT_CONTRACT_RESERVE_BYTES: usize = 8 * 1024;
const RETRY_INSTRUCTION: &str = "\n<retry_instruction truncated=\"false\">Previous decomposition verifier output was invalid. Return exactly one JSON object matching the output contract and no prose.</retry_instruction>\n";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum AutomationAuthoringMode {
    #[default]
    Reviewed,
    TrustedAutoFinalize,
}

impl AutomationAuthoringMode {
    pub fn parse(value: &str) -> Option<Self> {
        match value.trim() {
            "reviewed" => Some(Self::Reviewed),
            "trusted_auto_finalize" => Some(Self::TrustedAutoFinalize),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Reviewed => "reviewed",
            Self::TrustedAutoFinalize => "trusted_auto_finalize",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum AutomationDecompositionVerificationStatus {
    #[default]
    Unverified,
    Verified,
    NeedsRevision,
    Failed,
}

impl AutomationDecompositionVerificationStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Unverified => "unverified",
            Self::Verified => "verified",
            Self::NeedsRevision => "needs_revision",
            Self::Failed => "failed",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AutomationDecompositionInput {
    pub goal_prompt: String,
    pub goal_items_json: String,
    pub first_run_prompt: String,
    pub spec_artifact_id: String,
    pub spec_content: String,
    pub provider_harness: String,
    pub model_id: String,
    pub logical_effort: Option<String>,
    pub run_mode: String,
    pub base_ref_kind: String,
    pub base_ref: String,
    pub chain_mode: String,
    pub completion_signal: String,
    pub plan_approval_mode: String,
    pub pr_merge_mode: String,
    pub plan_deep_verification: bool,
    pub max_runs: i64,
    pub max_consecutive_failures: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AutomationAuthoringState {
    #[serde(default)]
    pub mode: AutomationAuthoringMode,
    #[serde(default)]
    pub verification_status: AutomationDecompositionVerificationStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub verified_input: Option<AutomationDecompositionInput>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub verdict_json: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub verified_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pending_goal_replan: Option<AutomationGoalReplanState>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AutomationGoalReplanStatus {
    Pending,
    Applied,
    Rejected,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AutomationGoalReplanState {
    pub source_run_id: String,
    pub base_goal_items_json: String,
    pub proposed_goal_items_json: String,
    pub reason: String,
    pub status: AutomationGoalReplanStatus,
    pub created_at: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub applied_at: Option<String>,
}

impl Default for AutomationAuthoringState {
    fn default() -> Self {
        Self {
            mode: AutomationAuthoringMode::Reviewed,
            verification_status: AutomationDecompositionVerificationStatus::Unverified,
            verified_input: None,
            verdict_json: None,
            verified_at: None,
            pending_goal_replan: None,
        }
    }
}

impl AutomationAuthoringState {
    pub fn trusted_unverified() -> Self {
        Self {
            mode: AutomationAuthoringMode::TrustedAutoFinalize,
            ..Self::default()
        }
    }

    pub fn verified(
        mode: AutomationAuthoringMode,
        input: AutomationDecompositionInput,
        verdict_json: String,
    ) -> Self {
        Self {
            mode,
            verification_status: AutomationDecompositionVerificationStatus::Verified,
            verified_input: Some(input),
            verdict_json: Some(verdict_json),
            verified_at: Some(Utc::now().to_rfc3339()),
            pending_goal_replan: None,
        }
    }

    pub fn needs_revision(
        mode: AutomationAuthoringMode,
        input: AutomationDecompositionInput,
        verdict_json: String,
    ) -> Self {
        Self {
            mode,
            verification_status: AutomationDecompositionVerificationStatus::NeedsRevision,
            verified_input: Some(input),
            verdict_json: Some(verdict_json),
            verified_at: None,
            pending_goal_replan: None,
        }
    }

    pub fn is_verified_for(&self, input: &AutomationDecompositionInput) -> bool {
        self.mode == AutomationAuthoringMode::TrustedAutoFinalize
            && self.verification_status == AutomationDecompositionVerificationStatus::Verified
            && self.verified_input.as_ref() == Some(input)
    }
}

pub fn parse_authoring_state(raw: Option<&str>) -> AppResult<AutomationAuthoringState> {
    let Some(raw) = raw.map(str::trim).filter(|value| !value.is_empty()) else {
        return Ok(AutomationAuthoringState::default());
    };
    serde_json::from_str(raw).map_err(|error| {
        AppError::Validation(format!("invalid automation authoring state: {error}"))
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AutomationDecompositionVerdictDecision {
    Approve,
    Revise,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AutomationDecompositionVerdictConfidence {
    Low,
    Medium,
    High,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AutomationDecompositionFindingSeverity {
    Critical,
    High,
    Medium,
    Low,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AutomationDecompositionFindingCategory {
    Coverage,
    PhaseBoundaries,
    Ordering,
    FirstRunAlignment,
    AutonomyRisk,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AutomationDecompositionFinding {
    pub severity: AutomationDecompositionFindingSeverity,
    pub category: AutomationDecompositionFindingCategory,
    pub description: String,
    #[serde(default)]
    pub goal_item_ids: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AutomationDecompositionVerdict {
    pub decision: AutomationDecompositionVerdictDecision,
    pub reason: String,
    pub confidence: AutomationDecompositionVerdictConfidence,
    pub findings: Vec<AutomationDecompositionFinding>,
}

#[derive(Debug, Clone)]
pub struct AutomationDecompositionVerifierInvocation {
    pub automation: Automation,
    pub input: AutomationDecompositionInput,
    pub retry_reminder: bool,
    pub timeout: Duration,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AutomationDecompositionVerifierInvocationOutput {
    pub raw_output: String,
    pub model_id: Option<String>,
}

#[async_trait]
pub trait AutomationDecompositionVerifierInvoker: Send + Sync {
    async fn invoke(
        &self,
        input: AutomationDecompositionVerifierInvocation,
    ) -> AppResult<AutomationDecompositionVerifierInvocationOutput>;
}

#[derive(Clone)]
pub struct HarnessAutomationDecompositionVerifierInvoker {
    state: AppState,
}

impl HarnessAutomationDecompositionVerifierInvoker {
    pub fn new(state: AppState) -> Self {
        Self { state }
    }
}

#[async_trait]
impl AutomationDecompositionVerifierInvoker for HarnessAutomationDecompositionVerifierInvoker {
    async fn invoke(
        &self,
        input: AutomationDecompositionVerifierInvocation,
    ) -> AppResult<AutomationDecompositionVerifierInvocationOutput> {
        let mut prompt = build_decomposition_verifier_prompt(&input.input)?;
        if input.retry_reminder
            && prompt.len() + RETRY_INSTRUCTION.len() <= AUTOMATION_JUDGE_PROMPT_MAX_BYTES
        {
            prompt.push_str(RETRY_INSTRUCTION);
        }
        let output = invoke_automation_utility_agent(
            &self.state,
            &input.automation,
            agent_names::AGENT_AUTOMATION_DECOMPOSITION_VERIFIER,
            "automation decomposition verifier",
            prompt,
            input.timeout,
            AutomationUtilityModelPolicy::LockedDefault,
        )
        .await?;
        Ok(AutomationDecompositionVerifierInvocationOutput {
            raw_output: output.raw_output,
            model_id: output.model_id,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AutomationDecompositionVerificationOutcome {
    pub automation: Automation,
    pub verdict: AutomationDecompositionVerdict,
}

#[derive(Clone)]
pub struct AutomationDecompositionVerifier {
    service: AutomationService,
    invoker: Arc<dyn AutomationDecompositionVerifierInvoker>,
    timeout: Duration,
}

impl AutomationDecompositionVerifier {
    pub fn new(
        service: AutomationService,
        invoker: Arc<dyn AutomationDecompositionVerifierInvoker>,
        timeout: Duration,
    ) -> Self {
        Self {
            service,
            invoker,
            timeout,
        }
    }

    pub async fn verify_and_finalize(
        &self,
        id: &AutomationId,
    ) -> AppResult<AutomationDecompositionVerificationOutcome> {
        let detail = self.service.get_automation_detail(id).await?;
        let automation = detail.automation;
        validate_finalizable(&automation)?;
        let authoring_state = parse_authoring_state(automation.authoring_state_json.as_deref())?;
        if authoring_state.mode != AutomationAuthoringMode::TrustedAutoFinalize {
            return Err(AppError::Validation(
                "decomposition verification is only available for trusted auto-finalize authoring"
                    .to_string(),
            ));
        }
        let input = self.service.load_decomposition_input(&automation).await?;
        let first_output = self
            .invoker
            .invoke(AutomationDecompositionVerifierInvocation {
                automation: automation.clone(),
                input: input.clone(),
                retry_reminder: false,
                timeout: self.timeout,
            })
            .await?;
        let verdict = match parse_decomposition_verdict(&first_output.raw_output) {
            Ok(verdict) => verdict,
            Err(_) => {
                let retry_output = self
                    .invoker
                    .invoke(AutomationDecompositionVerifierInvocation {
                        automation: automation.clone(),
                        input: input.clone(),
                        retry_reminder: true,
                        timeout: self.timeout,
                    })
                    .await?;
                parse_decomposition_verdict(&retry_output.raw_output)?
            }
        };
        let verdict_json = serde_json::to_string(&verdict).map_err(|error| {
            AppError::Infrastructure(format!(
                "failed to serialize decomposition verifier verdict: {error}"
            ))
        })?;
        let next_state = match verdict.decision {
            AutomationDecompositionVerdictDecision::Approve => {
                AutomationAuthoringState::verified(authoring_state.mode, input, verdict_json)
            }
            AutomationDecompositionVerdictDecision::Revise => {
                AutomationAuthoringState::needs_revision(authoring_state.mode, input, verdict_json)
            }
        };
        if !self
            .service
            .persist_authoring_state_if_unchanged(&automation, &next_state)
            .await?
        {
            return Err(AppError::Validation(
                "automation changed while decomposition verification was running; verify again"
                    .to_string(),
            ));
        }
        let automation = match verdict.decision {
            AutomationDecompositionVerdictDecision::Approve => self.service.finalize(id).await?,
            AutomationDecompositionVerdictDecision::Revise => {
                self.service.get_automation_detail(id).await?.automation
            }
        };
        Ok(AutomationDecompositionVerificationOutcome {
            automation,
            verdict,
        })
    }
}

pub fn parse_decomposition_verdict(output: &str) -> AppResult<AutomationDecompositionVerdict> {
    let value = extract_automation_verdict_value(output)?;
    validate_required_keys(&value)?;
    let mut verdict =
        serde_json::from_value::<AutomationDecompositionVerdict>(value).map_err(|error| {
            AppError::Validation(format!("invalid decomposition verifier JSON: {error}"))
        })?;
    verdict.reason = verdict.reason.trim().chars().take(1_000).collect();
    if verdict.reason.is_empty() {
        return Err(AppError::Validation(
            "decomposition verifier reason is required".to_string(),
        ));
    }
    for finding in &mut verdict.findings {
        finding.description = finding.description.trim().chars().take(1_000).collect();
        if finding.description.is_empty() {
            return Err(AppError::Validation(
                "decomposition verifier findings require descriptions".to_string(),
            ));
        }
        finding.goal_item_ids.retain(|id| !id.trim().is_empty());
    }
    match verdict.decision {
        AutomationDecompositionVerdictDecision::Approve => {
            if verdict
                .findings
                .iter()
                .any(|finding| finding.severity != AutomationDecompositionFindingSeverity::Low)
            {
                return Err(AppError::Validation(
                    "decomposition verifier cannot approve with blocking findings".to_string(),
                ));
            }
        }
        AutomationDecompositionVerdictDecision::Revise if verdict.findings.is_empty() => {
            return Err(AppError::Validation(
                "decomposition verifier revision requires at least one finding".to_string(),
            ));
        }
        AutomationDecompositionVerdictDecision::Revise => {}
    }
    Ok(verdict)
}

pub fn build_decomposition_verifier_prompt(
    input: &AutomationDecompositionInput,
) -> AppResult<String> {
    let output_contract = output_contract_section();
    if output_contract.len() > OUTPUT_CONTRACT_RESERVE_BYTES {
        return Err(AppError::Validation(
            "decomposition verifier output contract exceeds its reserved budget".to_string(),
        ));
    }
    let available = AUTOMATION_JUDGE_PROMPT_MAX_BYTES.saturating_sub(output_contract.len());
    let execution_policy = serde_json::to_string(&json!({
        "providerHarness": input.provider_harness,
        "modelId": input.model_id,
        "logicalEffort": input.logical_effort,
        "runMode": input.run_mode,
        "baseRefKind": input.base_ref_kind,
        "baseRef": input.base_ref,
        "chainMode": input.chain_mode,
        "completionSignal": input.completion_signal,
        "planApprovalMode": input.plan_approval_mode,
        "prMergeMode": input.pr_merge_mode,
        "planDeepVerification": input.plan_deep_verification,
        "maxRuns": input.max_runs,
        "maxConsecutiveFailures": input.max_consecutive_failures,
    }))
    .map_err(|error| {
        AppError::Infrastructure(format!(
            "failed to serialize decomposition execution policy: {error}"
        ))
    })?;
    let fixed = format!(
        "{}{}{}{}",
        xml_section("goal", &input.goal_prompt, &[("truncated", "false")]),
        xml_section(
            "goal_items",
            &input.goal_items_json,
            &[("truncated", "false")],
        ),
        xml_section(
            "first_run_prompt",
            &input.first_run_prompt,
            &[("truncated", "false")],
        ),
        xml_section(
            "execution_policy",
            &execution_policy,
            &[("truncated", "false")],
        ),
    );
    if fixed.len() >= available {
        return Err(AppError::Validation(
            "decomposition verifier fixed inputs exceed the prompt budget".to_string(),
        ));
    }
    let spec_attributes = [
        ("artifact_id", input.spec_artifact_id.as_str()),
        ("truncated", "false"),
    ];
    let spec_wrapper_bytes = xml_section("spec", "", &spec_attributes).len();
    let spec_budget = available
        .checked_sub(fixed.len() + spec_wrapper_bytes)
        .ok_or_else(|| {
            AppError::Validation(
                "decomposition verifier fixed inputs exceed the prompt budget".to_string(),
            )
        })?;
    let (spec, truncated) = truncate_utf8_to_bytes(&input.spec_content, spec_budget);
    let prompt = format!(
        "{fixed}{}{output_contract}",
        xml_section(
            "spec",
            &spec,
            &[
                ("artifact_id", input.spec_artifact_id.as_str()),
                ("truncated", if truncated { "true" } else { "false" }),
            ],
        )
    );
    if prompt.len() > AUTOMATION_JUDGE_PROMPT_MAX_BYTES {
        return Err(AppError::Validation(
            "decomposition verifier prompt exceeded the 64KB budget".to_string(),
        ));
    }
    Ok(prompt)
}

fn validate_required_keys(value: &Value) -> AppResult<()> {
    let object = value.as_object().ok_or_else(|| {
        AppError::Validation("decomposition verifier verdict must be a JSON object".to_string())
    })?;
    for key in REQUIRED_VERDICT_KEYS {
        if !object.contains_key(*key) {
            return Err(AppError::Validation(format!(
                "decomposition verifier verdict missing required key {key}"
            )));
        }
    }
    Ok(())
}

fn xml_section(tag: &str, content: &str, attributes: &[(&str, &str)]) -> String {
    let attributes = attributes
        .iter()
        .map(|(key, value)| format!(" {key}=\"{}\"", escape_xml_attribute(value)))
        .collect::<String>();
    format!("<{tag}{attributes}>\n{content}\n</{tag}>\n")
}

fn escape_xml_attribute(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('"', "&quot;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

fn output_contract_section() -> String {
    let contract = json!({
        "decision": "approve | revise",
        "reason": "concise evidence-based rationale",
        "confidence": "low | medium | high",
        "findings": [{
            "severity": "critical | high | medium | low",
            "category": "coverage | phase_boundaries | ordering | first_run_alignment | autonomy_risk",
            "description": "specific actionable gap",
            "goalItemIds": ["existing goal item ids only"]
        }]
    });
    format!(
        "<output_contract truncated=\"false\">\nEvaluate whether the phase decomposition covers the full spec, has independently deliverable boundaries, orders dependencies safely, aligns the first run to the first unfinished phase, and can proceed autonomously without hidden human decisions. Return approve only when no critical, high, or medium findings remain. Return exactly one JSON object and no prose.\n{}\n</output_contract>\n",
        serde_json::to_string_pretty(&contract)
            .expect("decomposition verifier output contract should serialize")
    )
}
