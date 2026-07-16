use serde_json::json;

use super::decomposition_verifier::{
    build_decomposition_verifier_prompt, parse_authoring_state, parse_decomposition_verdict,
    AutomationAuthoringMode, AutomationAuthoringState, AutomationDecompositionInput,
    AutomationDecompositionVerdictDecision, AutomationDecompositionVerificationStatus,
};
use super::judge::AUTOMATION_JUDGE_PROMPT_MAX_BYTES;

fn decomposition_input() -> AutomationDecompositionInput {
    AutomationDecompositionInput {
        goal_prompt: "Ship the automation pipeline in dependency-safe phases.".to_string(),
        goal_items_json: json!([
            { "id": "phase-1", "title": "Add the backend contract", "status": "pending" },
            { "id": "phase-2", "title": "Add the operator UI", "status": "pending" }
        ])
        .to_string(),
        first_run_prompt: "Implement phase 1 with focused tests and publish its PR.".to_string(),
        spec_artifact_id: "spec-1".to_string(),
        spec_content: "# Plan\n\nPhase 1 adds the backend. Phase 2 adds the UI.".to_string(),
        provider_harness: "codex".to_string(),
        model_id: "gpt-5.5".to_string(),
        logical_effort: Some("high".to_string()),
        run_mode: "edit".to_string(),
        base_ref_kind: "project_default".to_string(),
        base_ref: "main".to_string(),
        chain_mode: "merged_base".to_string(),
        completion_signal: "pr_merged".to_string(),
        plan_approval_mode: "automatic".to_string(),
        pr_merge_mode: "automatic".to_string(),
        plan_deep_verification: true,
        max_runs: 25,
        max_consecutive_failures: 3,
    }
}

#[test]
fn decomposition_prompt_carries_the_full_authoritative_contract() {
    let prompt = build_decomposition_verifier_prompt(&decomposition_input()).unwrap();

    assert!(prompt.contains("<goal "));
    assert!(prompt.contains("dependency-safe phases"));
    assert!(prompt.contains("<goal_items "));
    assert!(prompt.contains("phase-1"));
    assert!(prompt.contains("<first_run_prompt "));
    assert!(prompt.contains("<execution_policy "));
    assert!(prompt.contains("\"planApprovalMode\":\"automatic\""));
    assert!(prompt.contains("\"prMergeMode\":\"automatic\""));
    assert!(prompt.contains("<spec artifact_id=\"spec-1\""));
    assert!(prompt.contains("<output_contract "));
    assert!(prompt.contains("coverage"));
    assert!(prompt.contains("phase_boundaries"));
    assert!(prompt.contains("ordering"));
    assert!(prompt.contains("autonomy_risk"));
}

#[test]
fn authoring_mode_and_status_string_contracts_are_stable() {
    assert_eq!(
        AutomationAuthoringMode::parse(" reviewed "),
        Some(AutomationAuthoringMode::Reviewed)
    );
    assert_eq!(
        AutomationAuthoringMode::parse("trusted_auto_finalize"),
        Some(AutomationAuthoringMode::TrustedAutoFinalize)
    );
    assert_eq!(AutomationAuthoringMode::parse("legacy"), None);
    assert_eq!(AutomationAuthoringMode::Reviewed.as_str(), "reviewed");
    assert_eq!(
        AutomationAuthoringMode::TrustedAutoFinalize.as_str(),
        "trusted_auto_finalize"
    );
    assert_eq!(
        AutomationDecompositionVerificationStatus::Unverified.as_str(),
        "unverified"
    );
    assert_eq!(
        AutomationDecompositionVerificationStatus::Verified.as_str(),
        "verified"
    );
    assert_eq!(
        AutomationDecompositionVerificationStatus::NeedsRevision.as_str(),
        "needs_revision"
    );
    assert_eq!(
        AutomationDecompositionVerificationStatus::Failed.as_str(),
        "failed"
    );
}

#[test]
fn authoring_state_parser_defaults_blank_and_rejects_invalid_json() {
    let blank = parse_authoring_state(Some("   ")).unwrap();
    assert_eq!(blank, AutomationAuthoringState::default());
    assert_eq!(
        AutomationAuthoringState::trusted_unverified().mode,
        AutomationAuthoringMode::TrustedAutoFinalize
    );

    let error = parse_authoring_state(Some("{not-json")).unwrap_err();
    assert!(error
        .to_string()
        .contains("invalid automation authoring state"));
}

#[test]
fn decomposition_verdict_approves_only_without_blocking_findings() {
    let approved = parse_decomposition_verdict(
        &json!({
            "decision": "approve",
            "reason": "The phases cover the full goal with explicit boundaries.",
            "confidence": "high",
            "findings": []
        })
        .to_string(),
    )
    .unwrap();
    assert_eq!(
        approved.decision,
        AutomationDecompositionVerdictDecision::Approve
    );

    let error = parse_decomposition_verdict(
        &json!({
            "decision": "approve",
            "reason": "Looks good despite a missing recovery phase.",
            "confidence": "high",
            "findings": [{
                "severity": "high",
                "category": "coverage",
                "description": "Recovery work is absent.",
                "goalItemIds": []
            }]
        })
        .to_string(),
    )
    .unwrap_err();
    assert!(error.to_string().contains("blocking findings"));
}

#[test]
fn decomposition_verdict_revision_requires_actionable_findings() {
    let error = parse_decomposition_verdict(
        &json!({
            "decision": "revise",
            "reason": "The decomposition needs work.",
            "confidence": "medium",
            "findings": []
        })
        .to_string(),
    )
    .unwrap_err();
    assert!(error.to_string().contains("at least one finding"));
}

#[test]
fn decomposition_verdict_rejects_missing_required_keys_and_empty_text() {
    let non_object = parse_decomposition_verdict("[]").unwrap_err();
    assert!(non_object.to_string().contains("JSON"));

    let missing_key = parse_decomposition_verdict(
        &json!({
            "decision": "approve",
            "reason": "Complete.",
            "confidence": "high"
        })
        .to_string(),
    )
    .unwrap_err();
    assert!(missing_key.to_string().contains("missing required key"));

    let empty_reason = parse_decomposition_verdict(
        &json!({
            "decision": "approve",
            "reason": "   ",
            "confidence": "high",
            "findings": []
        })
        .to_string(),
    )
    .unwrap_err();
    assert!(empty_reason.to_string().contains("reason is required"));

    let empty_finding = parse_decomposition_verdict(
        &json!({
            "decision": "revise",
            "reason": "Needs a concrete split.",
            "confidence": "medium",
            "findings": [{
                "severity": "medium",
                "category": "phase_boundaries",
                "description": "   ",
                "goalItemIds": ["phase-1"]
            }]
        })
        .to_string(),
    )
    .unwrap_err();
    assert!(empty_finding
        .to_string()
        .contains("findings require descriptions"));

    let invalid_enum = parse_decomposition_verdict(
        &json!({
            "decision": "delegate",
            "reason": "Unknown action.",
            "confidence": "high",
            "findings": []
        })
        .to_string(),
    )
    .unwrap_err();
    assert!(invalid_enum
        .to_string()
        .contains("invalid decomposition verifier JSON"));
}

#[test]
fn decomposition_verdict_trims_and_allows_low_severity_approval_notes() {
    let long_reason = format!(" {} ", "r".repeat(1_200));
    let long_description = format!(" {} ", "d".repeat(1_200));

    let verdict = parse_decomposition_verdict(
        &json!({
            "decision": "approve",
            "reason": long_reason,
            "confidence": "medium",
            "findings": [{
                "severity": "low",
                "category": "autonomy_risk",
                "description": long_description,
                "goalItemIds": ["phase-1", "", "   ", "phase-2"]
            }]
        })
        .to_string(),
    )
    .unwrap();

    assert_eq!(
        verdict.decision,
        AutomationDecompositionVerdictDecision::Approve
    );
    assert_eq!(verdict.reason.len(), 1_000);
    assert_eq!(verdict.findings[0].description.len(), 1_000);
    assert_eq!(verdict.findings[0].goal_item_ids, ["phase-1", "phase-2"]);
}

#[test]
fn decomposition_prompt_truncates_spec_to_fit_budget() {
    let mut input = decomposition_input();
    input.spec_content = "spec ".repeat(40_000);

    let prompt = build_decomposition_verifier_prompt(&input).unwrap();

    assert!(prompt.len() <= AUTOMATION_JUDGE_PROMPT_MAX_BYTES);
    assert!(prompt.contains("<spec artifact_id=\"spec-1\" truncated=\"true\">"));
    assert!(prompt.contains("<output_contract truncated=\"false\">"));
}

#[test]
fn decomposition_prompt_escapes_spec_artifact_attribute_and_rejects_oversized_fixed_inputs() {
    let mut input = decomposition_input();
    input.spec_artifact_id = "spec-&-\"quoted\"-<tag>".to_string();

    let prompt = build_decomposition_verifier_prompt(&input).unwrap();

    assert!(prompt.contains("artifact_id=\"spec-&amp;-&quot;quoted&quot;-&lt;tag&gt;\""));

    input.goal_prompt = "goal ".repeat(20_000);
    let error = build_decomposition_verifier_prompt(&input).unwrap_err();
    assert!(error
        .to_string()
        .contains("fixed inputs exceed the prompt budget"));
}

#[test]
fn verified_authoring_state_is_fresh_only_for_the_exact_input_snapshot() {
    let input = decomposition_input();
    let state = AutomationAuthoringState::verified(
        AutomationAuthoringMode::TrustedAutoFinalize,
        input.clone(),
        json!({
            "decision": "approve",
            "reason": "Complete and dependency-safe.",
            "confidence": "high",
            "findings": []
        })
        .to_string(),
    );

    assert_eq!(
        state.verification_status,
        AutomationDecompositionVerificationStatus::Verified
    );
    assert!(state.is_verified_for(&input));

    let mut changed = input;
    changed.first_run_prompt.push_str(" Also change phase 2.");
    assert!(!state.is_verified_for(&changed));
}
