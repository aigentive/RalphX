use super::*;

#[test]
fn release_notes_filename_normalizes_version() {
    assert_eq!(release_notes_filename("0.9.0").unwrap(), "v0.9.0.md");
    assert_eq!(release_notes_filename("v0.9.0").unwrap(), "v0.9.0.md");
}

#[test]
fn release_notes_filename_rejects_path_traversal() {
    assert!(release_notes_filename("../0.9.0").is_err());
    assert!(release_notes_filename("0.9.0/notes").is_err());
    assert!(release_notes_filename("0.9.0\\notes").is_err());
}

#[test]
fn reads_first_available_candidate() {
    let root =
        std::env::temp_dir().join(format!("ralphx-release-notes-test-{}", std::process::id()));
    let missing = root.join("missing").join("v0.9.0.md");
    let notes_dir = root.join("notes");
    let notes_path = notes_dir.join("v0.9.0.md");
    std::fs::create_dir_all(&notes_dir).unwrap();
    std::fs::write(&notes_path, "## Release\n\nDetails").unwrap();

    let response = read_release_notes_from_candidates(
        "0.9.0",
        vec![
            (missing, ReleaseNotesSource::BundledResource),
            (notes_path, ReleaseNotesSource::DevelopmentCheckout),
        ],
    );

    assert_eq!(response.version, "0.9.0");
    assert_eq!(response.body.as_deref(), Some("## Release\n\nDetails"));
    assert_eq!(response.source, ReleaseNotesSource::DevelopmentCheckout);

    let _ = std::fs::remove_dir_all(root);
}
