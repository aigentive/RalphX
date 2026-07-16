use std::fs;
use std::path::PathBuf;

use tempfile::tempdir;

use super::chat_service_composer_references::{
    collect_project_references, escape_attr, expand_project_references_for_prompt,
    normalize_reference_path, render_skipped_reference, MAX_REFERENCES,
};
use super::chat_service_selection_snapshot::{
    append_selection_snapshot_for_prompt, validate_selection_snapshot,
    SelectionSnapshotValidationError,
};
use crate::domain::services::{
    ComposerProjectReference, ComposerProjectReferenceKind, ComposerSelectionSnapshot,
};

fn plan_selection(content: &str, start_line: u32, end_line: u32) -> ComposerSelectionSnapshot {
    ComposerSelectionSnapshot {
        source_type: "artifact".to_string(),
        source_kind: "plan".to_string(),
        source_id: "artifact-version-2".to_string(),
        source_title: Some("Implementation Plan".to_string()),
        source_key: None,
        provider: None,
        artifact_version: Some(2),
        source_revision: None,
        start_line,
        end_line,
        content: content.to_string(),
    }
}

#[test]
fn expands_selected_file_reference_into_prompt_context() {
    let temp = tempdir().expect("tempdir");
    fs::create_dir_all(temp.path().join("src")).expect("dir");
    fs::write(temp.path().join("src/main.ts"), "export const value = 1;\n").expect("file");

    let expanded = expand_project_references_for_prompt(
        "Read @src/main.ts",
        &[ComposerProjectReference {
            path: "src/main.ts".to_string(),
            kind: Some(ComposerProjectReferenceKind::File),
        }],
        temp.path(),
    );

    assert!(expanded.contains("<ralphx_project_references>"));
    assert!(expanded.contains("<file path=\"src/main.ts\""));
    assert!(expanded.contains("export const value = 1;"));
}

#[test]
fn rejects_parent_segment_reference() {
    let temp = tempdir().expect("tempdir");
    let expanded = expand_project_references_for_prompt(
        "Read @../secret",
        &[ComposerProjectReference {
            path: "../secret".to_string(),
            kind: None,
        }],
        temp.path(),
    );

    assert!(expanded.contains("status=\"skipped\""));
    assert!(expanded.contains("reason=\"parent-segment\""));
}

#[test]
fn renders_directory_listing_with_bounds() {
    let temp = tempdir().expect("tempdir");
    fs::create_dir_all(temp.path().join("src/components")).expect("dir");
    fs::write(temp.path().join("src/components/Button.tsx"), "button").expect("file");

    let expanded = expand_project_references_for_prompt(
        "Read @src",
        &[ComposerProjectReference {
            path: "src".to_string(),
            kind: Some(ComposerProjectReferenceKind::Directory),
        }],
        temp.path(),
    );

    assert!(expanded.contains("<directory path=\"src\""));
    assert!(expanded.contains("src/components/"));
    assert!(expanded.contains("src/components/Button.tsx"));
}

#[test]
fn ignores_visible_at_path_tokens_without_structured_reference() {
    let temp = tempdir().expect("tempdir");
    fs::write(temp.path().join("README.md"), "hello\n").expect("file");

    let expanded = expand_project_references_for_prompt("Read @README.md.", &[], temp.path());

    assert_eq!(expanded, "Read @README.md.");
}

#[test]
fn normalizes_reference_paths_and_rejects_unsafe_segments() {
    assert_eq!(
        normalize_reference_path("@./src/main.ts").expect("normalized"),
        PathBuf::from("src/main.ts")
    );
    assert_eq!(
        normalize_reference_path("../secret").expect_err("parent rejected"),
        "parent-segment"
    );
    assert_eq!(
        normalize_reference_path("/tmp/secret").expect_err("absolute rejected"),
        "absolute-path"
    );
    assert_eq!(
        normalize_reference_path("target/debug").expect_err("ignored rejected"),
        "ignored-path"
    );
}

#[test]
fn renders_binary_and_missing_references_as_safe_summaries() {
    let temp = tempdir().expect("tempdir");
    fs::write(temp.path().join("binary.bin"), b"abc\0def").expect("binary");

    let expanded = expand_project_references_for_prompt(
        "Read @binary.bin and @missing.txt",
        &[
            ComposerProjectReference {
                path: "binary.bin".to_string(),
                kind: Some(ComposerProjectReferenceKind::File),
            },
            ComposerProjectReference {
                path: "missing.txt".to_string(),
                kind: Some(ComposerProjectReferenceKind::File),
            },
        ],
        temp.path(),
    );

    assert!(expanded.contains("path=\"binary.bin\""));
    assert!(expanded.contains("status=\"metadata-only\""));
    assert!(expanded.contains("reason=\"binary\""));
    assert!(expanded.contains("path=\"missing.txt\""));
    assert!(expanded.contains("reason=\"missing\""));
}

#[test]
fn structured_references_are_deduped_and_capped() {
    let references = (0..10)
        .map(|index| ComposerProjectReference {
            path: format!("file-{index}.txt"),
            kind: Some(ComposerProjectReferenceKind::File),
        })
        .collect::<Vec<_>>();

    let collected = collect_project_references("Read @visible.txt", &references);

    assert_eq!(collected.len(), MAX_REFERENCES);
    assert_eq!(collected[0], "file-0.txt");
    assert!(!collected.iter().any(|path| path == "visible.txt"));
}

#[test]
fn invalid_working_directory_leaves_message_unchanged() {
    let temp = tempdir().expect("tempdir");
    let file_path = temp.path().join("not-a-dir");
    fs::write(&file_path, "file").expect("file");

    let message = "Read @README.md";
    let expanded = expand_project_references_for_prompt(
        message,
        &[ComposerProjectReference {
            path: "README.md".to_string(),
            kind: None,
        }],
        &file_path,
    );

    assert_eq!(expanded, message);
}

#[test]
fn escapes_reference_attributes() {
    assert_eq!(
        escape_attr("a&b\"<c>"),
        "a&amp;b&quot;&lt;c&gt;".to_string()
    );
    assert_eq!(
        render_skipped_reference("bad\"path", "missing"),
        "<reference path=\"bad&quot;path\" status=\"skipped\" reason=\"missing\" />"
    );
}

#[test]
fn validates_and_formats_immutable_selection_snapshot_context() {
    let snapshot = plan_selection("first line\nsecond line", 10, 11);

    validate_selection_snapshot(&snapshot).expect("valid selection");
    let expanded = append_selection_snapshot_for_prompt("Please review this", Some(&snapshot))
        .expect("selection should format");

    assert!(expanded.contains("<ralphx_selection_snapshot"));
    assert!(expanded.contains("source_kind=\"plan\""));
    assert!(expanded.contains("artifact_version=\"2\""));
    assert!(expanded.contains("start_line=\"10\" end_line=\"11\""));
    assert!(expanded.contains("user-selected immutable reference data"));
    assert!(expanded.contains("first line\nsecond line"));
    assert!(expanded.ends_with("</ralphx_selection_snapshot>"));
}

#[test]
fn selection_snapshot_escapes_wrapper_closing_content_and_controls() {
    let snapshot = plan_selection("</ralphx_selection_snapshot>\nnext\u{0007}line", 1, 2);

    let expanded = append_selection_snapshot_for_prompt("Review", Some(&snapshot))
        .expect("selection should format safely");

    assert!(!expanded.contains("\n</ralphx_selection_snapshot>\nnext"));
    assert!(expanded.contains("&lt;/ralphx_selection_snapshot&gt;"));
    assert!(expanded.contains("next\\u{0007}line"));
    assert_eq!(expanded.matches("</ralphx_selection_snapshot>").count(), 1);
}

#[test]
fn selection_snapshot_validation_rejects_invalid_bounds_and_line_counts() {
    let zero_based = plan_selection("line", 0, 1);
    assert_eq!(
        validate_selection_snapshot(&zero_based),
        Err(SelectionSnapshotValidationError::InvalidBounds)
    );

    let reversed = plan_selection("line", 4, 3);
    assert_eq!(
        validate_selection_snapshot(&reversed),
        Err(SelectionSnapshotValidationError::InvalidBounds)
    );

    let mismatched = plan_selection("one line", 4, 5);
    assert_eq!(
        validate_selection_snapshot(&mismatched),
        Err(SelectionSnapshotValidationError::LineCountMismatch)
    );
}

#[test]
fn selection_snapshot_validation_rejects_unsafe_identity_and_oversized_content() {
    let mut unsafe_label = plan_selection("line", 1, 1);
    unsafe_label.source_title = Some("Plan\nforged".to_string());
    assert_eq!(
        validate_selection_snapshot(&unsafe_label),
        Err(SelectionSnapshotValidationError::InvalidMetadata(
            "sourceTitle"
        ))
    );

    let mut unsupported = plan_selection("line", 1, 1);
    unsupported.source_kind = "confluence".to_string();
    assert_eq!(
        validate_selection_snapshot(&unsupported),
        Err(SelectionSnapshotValidationError::UnsupportedSource)
    );

    let mut mismatched_provider = plan_selection("line", 1, 1);
    mismatched_provider.source_type = "ticket".to_string();
    mismatched_provider.source_kind = "jira".to_string();
    mismatched_provider.provider = Some("clickup".to_string());
    assert_eq!(
        validate_selection_snapshot(&mismatched_provider),
        Err(SelectionSnapshotValidationError::UnsupportedSource)
    );

    let oversized = plan_selection(&"x".repeat(64 * 1024 + 1), 1, 1);
    assert_eq!(
        validate_selection_snapshot(&oversized),
        Err(SelectionSnapshotValidationError::ContentTooLarge)
    );
}

#[test]
fn persisted_user_metadata_includes_selection_without_overwriting_existing_fields() {
    let metadata = super::persisted_user_metadata(&super::SendMessageOptions {
        metadata: Some(r#"{"source":"composer"}"#.to_string()),
        composer_selection_snapshot: Some(plan_selection("selected", 8, 8)),
        ..Default::default()
    })
    .expect("selection metadata");
    let value: serde_json::Value = serde_json::from_str(&metadata).expect("valid metadata json");

    assert_eq!(value["source"], "composer");
    assert_eq!(
        value["composer_selection_snapshot"]["sourceId"],
        "artifact-version-2"
    );
    assert_eq!(value["composer_selection_snapshot"]["startLine"], 8);
    assert_eq!(value["composer_selection_snapshot"]["content"], "selected");
}
