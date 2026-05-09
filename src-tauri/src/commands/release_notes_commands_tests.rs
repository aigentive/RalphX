use super::*;
use std::io;
use std::path::PathBuf;

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
