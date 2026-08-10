use super::*;
use std::io;
use std::path::PathBuf;

#[test]
fn parse_version_from_filename_extracts_semver() {
    assert_eq!(
        parse_version_from_filename(Some("v0.9.0.md")),
        Some("0.9.0".to_string())
    );
    assert_eq!(
        parse_version_from_filename(Some("v0.12.1.md")),
        Some("0.12.1".to_string())
    );
}

#[test]
fn parse_version_from_filename_rejects_invalid_names() {
    assert_eq!(parse_version_from_filename(Some("README.md")), None);
    assert_eq!(parse_version_from_filename(Some("v.md")), None);
    assert_eq!(parse_version_from_filename(Some("notes.txt")), None);
    assert_eq!(parse_version_from_filename(None), None);
}

#[test]
fn compare_semver_desc_sorts_correctly() {
    assert_eq!(
        compare_semver_desc("0.28.0", "0.1.0"),
        std::cmp::Ordering::Less
    );
    assert_eq!(
        compare_semver_desc("0.1.0", "0.28.0"),
        std::cmp::Ordering::Greater
    );
    assert_eq!(
        compare_semver_desc("0.9.0", "0.9.0"),
        std::cmp::Ordering::Equal
    );
    assert_eq!(
        compare_semver_desc("0.9.0", "0.10.0"),
        std::cmp::Ordering::Greater
    );
}

#[test]
fn collect_versions_deduplicates_and_sorts_descending() {
    let temp_a = tempfile::tempdir().unwrap();
    let temp_b = tempfile::tempdir().unwrap();

    for name in ["v0.1.0.md", "v0.9.0.md", "v0.28.0.md", "README.md"] {
        std::fs::write(temp_a.path().join(name), "").unwrap();
    }
    for name in ["v0.9.0.md", "v0.12.0.md"] {
        std::fs::write(temp_b.path().join(name), "").unwrap();
    }

    let versions = collect_versions_from_dirs(
        vec![temp_a.path().to_path_buf(), temp_b.path().to_path_buf()],
        |path| std::fs::read_dir(path),
    )
    .expect("both directories are readable");

    assert_eq!(versions, vec!["0.28.0", "0.12.0", "0.9.0", "0.1.0"]);
}

/// A release-notes root that is absent is NORMAL: the bundled resource dir does not exist in a
/// dev checkout and the repo root does not exist in a shipped bundle, so `list_release_notes_versions`
/// must keep tolerating `NotFound` and read the roots that do exist.
#[test]
fn collect_versions_tolerates_a_missing_directory() {
    let present = tempfile::tempdir().unwrap();
    std::fs::write(present.path().join("v1.2.0.md"), "").unwrap();
    let absent = present.path().join("definitely-not-here");

    let versions = collect_versions_from_dirs(vec![absent, present.path().to_path_buf()], |path| {
        std::fs::read_dir(path)
    })
    .expect("a missing root is not a failure");

    assert_eq!(versions, vec!["1.2.0"]);
}

/// ...but a root that exists and cannot be READ is a host failure, and must NOT be reported as
/// "this version does not exist". Before this test the reader was `std::fs::read_dir(path).ok()`,
/// so a permissions or I/O error produced an empty version list indistinguishable from a genuine
/// empty directory — the caller could not tell "no release notes" from "could not look".
#[test]
fn collect_versions_fails_closed_when_a_readable_directory_errors() {
    let present = tempfile::tempdir().unwrap();
    std::fs::write(present.path().join("v1.2.0.md"), "").unwrap();

    let error = collect_versions_from_dirs(vec![present.path().to_path_buf()], |_| {
        Err(std::io::Error::new(
            std::io::ErrorKind::PermissionDenied,
            "release notes directory is not readable",
        ))
    })
    .expect_err("a non-NotFound read failure must surface, not collapse into an empty list");

    assert_eq!(error.kind(), std::io::ErrorKind::PermissionDenied);
}

#[test]
fn release_notes_filename_normalizes_version() {
    assert_eq!(release_notes_filename("0.9.0").unwrap(), "v0.9.0.md");
    assert_eq!(release_notes_filename("v0.9.0").unwrap(), "v0.9.0.md");
    assert_eq!(
        release_notes_filename(" v0.9.0-beta_1 ").unwrap(),
        "v0.9.0-beta_1.md"
    );
}

#[test]
fn release_notes_filename_rejects_path_traversal() {
    assert!(release_notes_filename("../0.9.0").is_err());
    assert!(release_notes_filename("0.9.0/notes").is_err());
    assert!(release_notes_filename("0.9.0\\notes").is_err());
}

#[test]
fn release_notes_filename_rejects_empty_and_non_ascii_versions() {
    assert!(release_notes_filename("").is_err());
    assert!(release_notes_filename("v").is_err());
    assert!(release_notes_filename("0.9.0β").is_err());
}

#[test]
fn release_notes_candidates_include_bundled_and_development_paths() {
    let candidates = release_notes_candidates_from_roots(
        Some(PathBuf::from("bundle-root")),
        Some(PathBuf::from("repo-root")),
        "v0.9.0.md",
    );

    assert_eq!(
        candidates,
        vec![
            (
                PathBuf::from("bundle-root")
                    .join(RELEASE_NOTES_DIR)
                    .join("v0.9.0.md"),
                ReleaseNotesSource::BundledResource
            ),
            (
                PathBuf::from("repo-root")
                    .join(RELEASE_NOTES_DIR)
                    .join("v0.9.0.md"),
                ReleaseNotesSource::DevelopmentCheckout
            ),
        ]
    );
}

#[test]
fn reads_first_available_candidate() {
    let missing = PathBuf::from("missing").join("v0.9.0.md");
    let notes_path = PathBuf::from("notes").join("v0.9.0.md");
    let mut requested_paths = Vec::new();

    let response = read_release_notes_from_candidates_with_reader(
        "0.9.0",
        vec![
            (missing.clone(), ReleaseNotesSource::BundledResource),
            (notes_path.clone(), ReleaseNotesSource::DevelopmentCheckout),
        ],
        |path| {
            requested_paths.push(path.to_path_buf());
            if path == notes_path.as_path() {
                Ok("## Release\n\nDetails".to_string())
            } else {
                Err(io::Error::new(io::ErrorKind::NotFound, "missing"))
            }
        },
    );

    assert_eq!(response.version, "0.9.0");
    assert_eq!(response.body.as_deref(), Some("## Release\n\nDetails"));
    assert_eq!(response.source, ReleaseNotesSource::DevelopmentCheckout);
    assert_eq!(requested_paths, vec![missing, notes_path]);
}

#[test]
fn returns_missing_response_when_no_candidate_reads() {
    let response = read_release_notes_from_candidates_with_reader(
        "0.9.0",
        vec![(
            PathBuf::from("missing").join("v0.9.0.md"),
            ReleaseNotesSource::BundledResource,
        )],
        |_| Err(io::Error::new(io::ErrorKind::NotFound, "missing")),
    );

    assert_eq!(response.version, "0.9.0");
    assert_eq!(response.body, None);
    assert_eq!(response.source, ReleaseNotesSource::Missing);
}

#[test]
fn read_release_notes_from_candidates_reads_real_file() {
    let dir = tempfile::tempdir().unwrap();
    let notes_path = dir.path().join(RELEASE_NOTES_DIR).join("v0.9.0.md");
    std::fs::create_dir_all(notes_path.parent().unwrap()).unwrap();
    std::fs::write(&notes_path, "## v0.9.0\n\nReal file content").unwrap();

    let response = read_release_notes_from_candidates(
        "0.9.0",
        vec![(notes_path, ReleaseNotesSource::DevelopmentCheckout)],
    );

    assert_eq!(response.version, "0.9.0");
    assert_eq!(
        response.body.as_deref(),
        Some("## v0.9.0\n\nReal file content")
    );
    assert_eq!(response.source, ReleaseNotesSource::DevelopmentCheckout);
}

#[test]
fn read_release_notes_from_candidates_returns_missing_for_nonexistent() {
    let response = read_release_notes_from_candidates(
        "0.1.0",
        vec![(
            PathBuf::from("/nonexistent/release-notes/v0.1.0.md"),
            ReleaseNotesSource::BundledResource,
        )],
    );

    assert_eq!(response.version, "0.1.0");
    assert_eq!(response.body, None);
    assert_eq!(response.source, ReleaseNotesSource::Missing);
}
