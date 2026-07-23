use super::failure_fingerprint::{failure_fingerprint, normalize_failure_evidence};
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
