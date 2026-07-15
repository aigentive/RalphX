use serde_json::json;

use super::decomposition_verifier::{
    build_decomposition_verifier_prompt, parse_decomposition_verdict, AutomationAuthoringMode,
    AutomationAuthoringState, AutomationDecompositionInput, AutomationDecompositionVerdictDecision,
    AutomationDecompositionVerificationStatus,
};

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
