use super::failure_fingerprint::{
    attach_recurrence_evidence, failure_fingerprint, normalize_failure_evidence, recurrence_key,
};
use crate::domain::entities::TaskOutcomeClass;

#[test]
fn fingerprint_ignores_volatile_representation_noise() {
    let first = "\u{1b}[31mCompile failed\u{1b}[0m at 2026-07-23T10:11:12Z \
                 request 123e4567-e89b-12d3-a456-426614174000 \
                 commit abcdef1234567890 in /Users/alice/project/src/main.rs";
    let second = "compile   failed at 2026-07-24T12:13:14+00:00 \
                  request 987e6543-e21b-12d3-a456-426614174999 \
                  commit 0123456789abcdef in C:\\work\\project\\src\\main.rs";

    assert_eq!(
        normalize_failure_evidence(first),
        normalize_failure_evidence(second)
    );
    assert_eq!(
        failure_fingerprint(&TaskOutcomeClass::MergeQaFailed, first),
        failure_fingerprint(&TaskOutcomeClass::MergeQaFailed, second)
    );
}

#[test]
fn fingerprint_changes_for_meaningful_class_or_detail() {
    let evidence = "compile failed: unresolved import widget";
    let same = failure_fingerprint(&TaskOutcomeClass::MergeQaFailed, evidence);
    let different_detail = failure_fingerprint(
        &TaskOutcomeClass::MergeQaFailed,
        "compile failed: unresolved import gadget",
    );
    let different_class = failure_fingerprint(&TaskOutcomeClass::MergeTimeout, evidence);

    assert_ne!(same, different_detail);
    assert_ne!(same, different_class);
    assert_eq!(same.len(), 64);
    assert!(same
        .bytes()
        .all(|value| value.is_ascii_hexdigit() && !value.is_ascii_uppercase()));
}

#[test]
fn recurrence_key_is_a_versioned_sorted_unique_token_set() {
    let first = "Merge failed: missing WIDGET widget in parser.rs";
    let reordered = "PARSER rs, widget missing in merge FAILED";
    let different = "Merge failed: missing gadget in parser.rs";

    let key = recurrence_key(first).expect("recurrence key");
    assert_eq!(recurrence_key(reordered).as_deref(), Some(key.as_str()));
    assert_ne!(recurrence_key(different).as_deref(), Some(key.as_str()));
    assert!(key.starts_with("token-set-v1:"));
    assert_eq!(key.len(), "token-set-v1:".len() + 64);
    assert_eq!(recurrence_key(" \n\t"), None);
}

#[test]
fn attaching_recurrence_metadata_preserves_raw_evidence_bytes() {
    let raw = "Failure at /Users/alice/project: WIDGET missing.\r\n";
    let mut evidence = serde_json::json!({ "raw": raw });

    let key = attach_recurrence_evidence(&mut evidence, raw, Some(" session-1 "))
        .expect("recurrence key");

    assert_eq!(evidence["raw"], raw);
    assert_eq!(evidence["recurrence_key"], key);
    assert_eq!(evidence["recurrence_session"], "session-1");
}
