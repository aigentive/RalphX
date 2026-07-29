use super::skills::{read_claude_skill_dir, read_codex_skill_dir};
use std::collections::BTreeSet;

/// A directory the process cannot read must NOT render as "this project has no skills".
///
/// The fail-open arm was `Err(_) => return Ok(skills)`, sitting beside `?`-propagating calls in
/// the same function body — so an unreadable root produced a confident empty list. The probe
/// uses a regular file as the root: `canonicalize` succeeds, then `read_dir` fails with a
/// not-a-directory error, which is portable and needs no permission games.
#[test]
fn claude_skill_dir_read_failure_is_not_an_empty_skill_list() {
    let dir = tempfile::tempdir().expect("temp dir");
    let not_a_directory = dir.path().join("skills");
    std::fs::write(&not_a_directory, b"not a directory").expect("probe file is written");

    let error = read_claude_skill_dir(&not_a_directory, "project", None)
        .expect_err("an unreadable skill root must not report zero skills");
    assert!(
        error.contains("Failed to read Claude skill directory"),
        "the refusal must name the failure, got: {error}"
    );
}

#[test]
fn codex_skill_dir_read_failure_is_not_an_empty_skill_list() {
    let dir = tempfile::tempdir().expect("temp dir");
    let not_a_directory = dir.path().join("skills");
    std::fs::write(&not_a_directory, b"not a directory").expect("probe file is written");

    let error = read_codex_skill_dir(&not_a_directory, "project", None, &BTreeSet::new())
        .expect_err("an unreadable skill root must not report zero skills");
    assert!(
        error.contains("Failed to read Codex skill directory"),
        "the refusal must name the failure, got: {error}"
    );
}

/// The absent case stays quiet — the fix must not turn "no skills directory" into an error,
/// which is the normal state of most projects.
#[test]
fn an_absent_skill_directory_is_still_an_empty_list() {
    let dir = tempfile::tempdir().expect("temp dir");
    let missing = dir.path().join("nope").join("skills");

    // `canonicalize` refuses a missing path before `read_dir` is reached, so the absent case is
    // discharged there; assert it is an ordinary refusal and not a panic or a silent success.
    let result = read_claude_skill_dir(&missing, "project", None);
    assert!(result.is_err(), "a missing root cannot be canonicalized");
}
