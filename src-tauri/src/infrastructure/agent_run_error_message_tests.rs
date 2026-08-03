use crate::infrastructure::agent_run_error_message::truncate_persisted_error_message;

/// Mirrors `MAX_PERSISTED_ERROR_MESSAGE_BYTES`, which is private to the production module.
const MAX_BYTES: usize = 8 * 1024;

#[test]
fn causes_within_the_bound_are_persisted_verbatim() {
    let cause = "Codex stream ended without a completion signal";

    assert_eq!(truncate_persisted_error_message(cause), cause);
}

#[test]
fn a_cause_at_exactly_the_bound_is_persisted_verbatim() {
    let cause = "x".repeat(MAX_BYTES);

    let truncated = truncate_persisted_error_message(&cause);

    assert_eq!(truncated, cause);
    assert!(!truncated.contains("bytes elided"));
}

#[test]
fn an_oversized_cause_keeps_the_terminal_tail_and_announces_the_elision() {
    // The original incident stored 124KB of successful tool output here.
    let cause = format!("{}TERMINAL-DETAIL", "noise\n".repeat(30_000));
    assert!(cause.len() > 100_000);

    let truncated = truncate_persisted_error_message(&cause);

    let elided = cause.len() - MAX_BYTES;
    assert!(
        truncated.starts_with(&format!("... {elided} bytes elided ...\n")),
        "elision must be explicit and state the dropped byte count, got {:?}",
        truncated.chars().take(40).collect::<String>()
    );
    assert!(
        truncated.ends_with("TERMINAL-DETAIL"),
        "the terminal detail at the tail must survive truncation"
    );
    assert_eq!(
        truncated.len() - truncated.find('\n').expect("header ends in a newline") - 1,
        MAX_BYTES,
        "exactly the trailing bound must be retained"
    );
}

/// The byte cut lands mid-codepoint for any multibyte cause whose length is not
/// congruent to the bound. Slicing there would panic, so the helper must walk
/// forward to the next boundary before it slices.
#[test]
fn an_oversized_multibyte_cause_advances_the_cut_to_a_char_boundary() {
    // 3-byte chars: len is a multiple of 3, and 8192 is not, so
    // `len - MAX_BYTES` can never land on a boundary.
    let cause = "€".repeat(3_000);
    assert_eq!(cause.len(), 9_000);
    let naive_cut = cause.len() - MAX_BYTES;
    assert!(
        !cause.is_char_boundary(naive_cut),
        "this test is only meaningful while the naive cut splits a codepoint"
    );

    let truncated = truncate_persisted_error_message(&cause);

    // 808 -> 810: the next boundary at or after the naive cut.
    let boundary_cut = 810;
    assert!(truncated.starts_with(&format!("... {boundary_cut} bytes elided ...\n")));
    assert!(
        truncated.ends_with(&"€".repeat((cause.len() - boundary_cut) / 3)),
        "the retained tail must decode as whole codepoints"
    );
}
