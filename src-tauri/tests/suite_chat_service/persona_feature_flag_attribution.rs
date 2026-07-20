use super::persona_feature_flag_support::*;

#[cfg(unix)]
#[tokio::test]
async fn persona_normal_send_persists_applied_attribution_without_body_and_emits_body_free_event() {
    let (run, events) =
        send_persona_attribution_fixture(Some(persona_attribution_fixture()), false).await;

    assert_eq!(run.persona_id.as_deref(), Some("persona-design-voice"));
    assert_eq!(run.persona_slug.as_deref(), Some("design-voice"));
    assert_eq!(run.persona_version, Some(2));
    assert_eq!(
        run.persona_content_hash.as_deref(),
        Some("persona-content-hash")
    );
    assert_eq!(run.persona_injected, Some(true));
    assert_eq!(run.persona_skipped_reason, None);
    let serialized_run = serde_json::to_string(&run).unwrap();
    assert!(!serialized_run.contains("SECRET_PERSONA_BODY_SENTINEL"));
    let applied = events
        .iter()
        .find(|payload| payload["persona_slug"] == "design-voice")
        .expect("persona applied event should be emitted");
    assert_eq!(applied["persona_id"], "persona-design-voice");
    assert_eq!(applied["version"], 2);
    assert_eq!(applied["run_id"], run.id.as_str());
    assert!(!applied.to_string().contains("SECRET_PERSONA_BODY_SENTINEL"));
}

#[cfg(unix)]
#[tokio::test]
async fn persona_native_agent_skip_persists_not_injected_reason() {
    let (run, events) =
        send_persona_attribution_fixture(Some(persona_attribution_fixture()), true).await;

    assert_eq!(run.persona_injected, Some(false));
    assert_eq!(
        run.persona_skipped_reason.as_deref(),
        Some("native_agent_flag")
    );
    let skipped = events
        .iter()
        .find(|payload| payload["reason"] == "native_agent_flag")
        .expect("persona skipped event should be emitted");
    assert_eq!(skipped["run_id"], run.id.as_str());
    assert_eq!(skipped["persona_slug"], "design-voice");
    assert!(!skipped.to_string().contains("SECRET_PERSONA_BODY_SENTINEL"));
}

#[cfg(unix)]
#[tokio::test]
async fn persona_absent_send_leaves_all_run_attribution_columns_null() {
    let (run, events) = send_persona_attribution_fixture(None, false).await;

    assert_eq!(run.persona_id, None);
    assert_eq!(run.persona_slug, None);
    assert_eq!(run.persona_version, None);
    assert_eq!(run.persona_content_hash, None);
    assert_eq!(run.persona_injected, None);
    assert_eq!(run.persona_skipped_reason, None);
    assert!(events.is_empty());
}
