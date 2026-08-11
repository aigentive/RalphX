use crate::agents::AgentHarnessKind;

use super::{
    processed_tokens, AgentRun, AgentRunUsage, ChatConversationId, ProviderUsageSnapshot,
    UsageCapture, UsageProvenance,
};

fn usage(input: u64, output: u64, cache_creation: u64, cache_read: u64) -> AgentRunUsage {
    AgentRunUsage {
        input_tokens: Some(input),
        output_tokens: Some(output),
        cache_creation_tokens: Some(cache_creation),
        cache_read_tokens: Some(cache_read),
        estimated_usd: None,
    }
}

#[test]
fn codex_processed_tokens_do_not_add_cached_input_twice() {
    let captured = usage(9_116_803, 25_881, 0, 8_837_504);
    let second_capture = usage(9_877_122, 31_874, 0, 9_540_224);

    assert_eq!(
        processed_tokens(
            Some(AgentHarnessKind::Codex),
            &captured,
            Some(UsageProvenance::ProviderTurnDelta),
        ),
        Some(9_142_684),
    );
    assert_eq!(
        processed_tokens(
            Some(AgentHarnessKind::Codex),
            &second_capture,
            Some(UsageProvenance::ProviderTurnDelta),
        ),
        Some(9_908_996),
    );
}

#[test]
fn claude_processed_tokens_include_separate_cache_counters_once() {
    let captured = usage(13, 1_434, 127_826, 1_099_251);

    assert_eq!(
        processed_tokens(
            Some(AgentHarnessKind::Claude),
            &captured,
            Some(UsageProvenance::ProviderTurnDelta),
        ),
        Some(1_228_524),
    );
}

#[test]
fn processed_tokens_are_unavailable_without_harness_or_for_baseline_only_capture() {
    let captured = usage(10, 2, 3, 4);

    assert_eq!(
        processed_tokens(None, &captured, Some(UsageProvenance::ProviderTurnDelta)),
        None,
    );
    assert_eq!(
        processed_tokens(
            Some(AgentHarnessKind::Codex),
            &captured,
            Some(UsageProvenance::CumulativeBaselineOnly),
        ),
        None,
    );
}

#[test]
fn processed_tokens_fail_closed_on_overflow() {
    let captured = usage(u64::MAX, 1, 0, 0);

    assert_eq!(
        processed_tokens(
            Some(AgentHarnessKind::Codex),
            &captured,
            Some(UsageProvenance::DerivedCumulativeDelta),
        ),
        None,
    );
}

#[test]
fn baseline_capture_keeps_raw_snapshot_and_clears_normalized_usage() {
    let raw = ProviderUsageSnapshot::from_usage(usage(100, 20, 0, 80));
    let capture = UsageCapture::cumulative_baseline(raw.clone());

    assert!(capture.normalized.is_empty());
    assert_eq!(capture.provenance, UsageProvenance::CumulativeBaselineOnly);
    assert_eq!(capture.raw_snapshot, Some(raw));
}

#[test]
fn provenance_round_trips_through_persisted_string_values() {
    for provenance in [
        UsageProvenance::ProviderTurnDelta,
        UsageProvenance::DerivedCumulativeDelta,
        UsageProvenance::ProviderSnapshotFallback,
        UsageProvenance::CumulativeBaselineOnly,
    ] {
        let persisted = provenance.to_string();
        assert_eq!(persisted.parse(), Ok(provenance));
    }
}

#[test]
fn agent_run_exposes_the_shared_processed_token_semantics() {
    let mut run = AgentRun::new(ChatConversationId::new());
    run.harness = Some(AgentHarnessKind::Codex);
    run.input_tokens = Some(100);
    run.output_tokens = Some(10);
    run.cache_read_tokens = Some(90);
    run.usage_provenance = Some(UsageProvenance::ProviderTurnDelta);

    assert_eq!(run.processed_tokens(), Some(110));
}
