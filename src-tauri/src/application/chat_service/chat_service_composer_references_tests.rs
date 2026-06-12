use std::fs;
use std::path::PathBuf;

use tempfile::tempdir;

use super::chat_service_composer_references::{
    collect_project_references, escape_attr, expand_project_references_for_prompt,
    normalize_reference_path, render_skipped_reference, MAX_REFERENCES,
};
use crate::domain::services::{ComposerProjectReference, ComposerProjectReferenceKind};

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
