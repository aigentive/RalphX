use crate::application::git_service::clone::CLONE_URL_INVALID;
use crate::application::git_service::clone_url::normalize_clone_url;

/// The blueprint's normalization table, one case per row.
#[test]
fn accepted_url_shapes_normalize_as_specified() {
    let cases: &[(&str, &str, &str, Option<&str>)] = &[
        ("https://host/o/r.git", "https://host/o/r.git", "r", None),
        ("https://host/o/r", "https://host/o/r", "r", None),
        ("git@host:o/r.git", "git@host:o/r.git", "r", None),
        (
            "ssh://git@host/o/r.git",
            "ssh://git@host/o/r.git",
            "r",
            None,
        ),
        ("o/r", "https://github.com/o/r.git", "r", None),
        (
            "https://github.com/o/r/tree/feature",
            "https://github.com/o/r.git",
            "r",
            Some("feature"),
        ),
    ];

    for (input, expected_url, expected_folder, expected_branch) in cases {
        let normalized = normalize_clone_url(input)
            .unwrap_or_else(|error| panic!("{input} should normalize, got {error:?}"));
        assert_eq!(&normalized.url, expected_url, "url for {input}");
        assert_eq!(
            &normalized.folder_name, expected_folder,
            "folder for {input}"
        );
        assert_eq!(
            normalized.branch.as_deref(),
            *expected_branch,
            "branch for {input}"
        );
    }
}

#[test]
fn a_branch_page_with_slashes_keeps_the_whole_branch_name() {
    let normalized = normalize_clone_url("https://github.com/o/r/tree/release/1.2")
        .expect("a nested branch page should normalize");

    assert_eq!(normalized.branch.as_deref(), Some("release/1.2"));
    assert_eq!(normalized.url, "https://github.com/o/r.git");
}

#[test]
fn trailing_slashes_do_not_produce_an_empty_folder_name() {
    let normalized =
        normalize_clone_url("https://host/o/r.git/").expect("a trailing slash should be tolerated");

    assert_eq!(normalized.folder_name, "r");
}

/// Local clones are explicitly out of scope: "this folder is already here" is a
/// different intent in the wizard.
#[test]
fn local_and_file_targets_are_rejected() {
    for input in [
        "file:///tmp/repo",
        "/tmp/repo",
        "./repo",
        "../repo",
        "file://./repo",
    ] {
        let error = normalize_clone_url(input).expect_err(&format!("{input} should be rejected"));
        assert_eq!(error.code, CLONE_URL_INVALID, "code for {input}");
    }
}

#[test]
fn unusable_shapes_are_rejected_before_any_spawn() {
    for input in [
        "",
        "   ",
        "not a url",
        "o/r/extra",
        "o/..",
        "../r",
        "https://github.com/o/r?tab=readme",
        "https://host/",
        "-o/r",
    ] {
        let error = normalize_clone_url(input).expect_err(&format!("{input:?} should be rejected"));
        assert_eq!(error.code, CLONE_URL_INVALID, "code for {input:?}");
    }
}

#[test]
fn shorthand_tolerates_a_dot_git_suffix() {
    let normalized = normalize_clone_url("owner/repo.git").expect("shorthand should normalize");

    assert_eq!(normalized.url, "https://github.com/owner/repo.git");
    assert_eq!(normalized.folder_name, "repo");
}
