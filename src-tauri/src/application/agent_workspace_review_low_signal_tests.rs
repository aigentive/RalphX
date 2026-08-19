use super::agent_workspace_review_low_signal::{
    low_signal_class, strip_low_signal_diff_sections, LowSignalClass,
};

#[test]
fn lockfiles_are_classified_by_exact_basename_and_full_extension() {
    for path in [
        "package-lock.json",
        "Cargo.lock",
        "flake.lock",
        "frontend/pnpm-lock.yaml",
        "go.sum",
        "some/nested/dir/yarn.lock",
    ] {
        assert_eq!(
            low_signal_class(path, false),
            Some(LowSignalClass::Lockfile),
            "{path} should classify as a lockfile"
        );
    }
}

/// The classifier must not swallow ordinary source that merely reads like a lockfile.
#[test]
fn source_files_that_merely_mention_lock_or_build_stay_reviewable() {
    for path in [
        "src/locksmith.rs",
        "src/lock.rs",
        "src/build.rs",
        "src/application/merge_lock.rs",
        "frontend/src/hooks/useLock.ts",
        "docs/dist-strategy.md",
        "src/snapshot_service.rs",
    ] {
        assert_eq!(
            low_signal_class(path, false),
            None,
            "{path} should stay reviewable"
        );
    }
}

#[test]
fn snapshots_are_classified_by_extension_or_directory() {
    assert_eq!(
        low_signal_class("frontend/src/__snapshots__/App.test.tsx.snap", false),
        Some(LowSignalClass::Snapshot)
    );
    assert_eq!(
        low_signal_class("tests/fixtures/output.ambr", false),
        Some(LowSignalClass::Snapshot)
    );
}

#[test]
fn assets_and_generated_output_are_classified() {
    assert_eq!(
        low_signal_class("frontend/public/logo.png", false),
        Some(LowSignalClass::Asset)
    );
    assert_eq!(
        low_signal_class("assets/Inter.woff2", false),
        Some(LowSignalClass::Asset)
    );
    assert_eq!(
        low_signal_class("dist/bundle.js", false),
        Some(LowSignalClass::Generated)
    );
    assert_eq!(
        low_signal_class("frontend/vendor/lib.min.js", false),
        Some(LowSignalClass::Generated)
    );
    assert_eq!(
        low_signal_class("src/api/client.generated.ts", false),
        Some(LowSignalClass::Generated)
    );
}

/// Git's own binary detection wins: a file it cannot diff has no reviewable hunks whatever its
/// extension suggests.
#[test]
fn git_reported_binary_wins_over_path_heuristics() {
    assert_eq!(
        low_signal_class("src/application/handler.rs", true),
        Some(LowSignalClass::Binary)
    );
}

#[test]
fn empty_paths_are_not_classified() {
    assert_eq!(low_signal_class("", false), None);
    assert_eq!(low_signal_class("   ", false), None);
}

const MIXED_DIFF: &str = "diff --git a/src/handler.rs b/src/handler.rs\n\
index 1111111..2222222 100644\n\
--- a/src/handler.rs\n\
+++ b/src/handler.rs\n\
@@ -1,2 +1,3 @@\n\
 fn handler() {}\n\
+fn added() {}\n\
diff --git a/Cargo.lock b/Cargo.lock\n\
index 3333333..4444444 100644\n\
--- a/Cargo.lock\n\
+++ b/Cargo.lock\n\
@@ -1,4 +1,4 @@\n\
-version = \"1.0.0\"\n\
+version = \"1.0.1\"\n\
diff --git a/src/other.rs b/src/other.rs\n\
index 5555555..6666666 100644\n\
--- a/src/other.rs\n\
+++ b/src/other.rs\n\
@@ -1,1 +1,2 @@\n\
+fn other() {}\n";

#[test]
fn low_signal_sections_are_dropped_and_source_sections_survive_intact() {
    let (filtered, dropped) = strip_low_signal_diff_sections(MIXED_DIFF);

    assert!(dropped);
    assert!(filtered.contains("src/handler.rs"));
    assert!(filtered.contains("+fn added() {}"));
    assert!(filtered.contains("src/other.rs"));
    assert!(filtered.contains("+fn other() {}"));
    assert!(!filtered.contains("Cargo.lock"));
    assert!(!filtered.contains("version = \"1.0.1\""));
}

#[test]
fn a_diff_with_no_low_signal_files_is_returned_unchanged() {
    let source_only = "diff --git a/src/a.rs b/src/a.rs\n@@ -1,1 +1,2 @@\n+fn a() {}\n";

    let (filtered, dropped) = strip_low_signal_diff_sections(source_only);

    assert!(!dropped);
    assert_eq!(filtered, source_only);
}

#[test]
fn an_empty_diff_is_handled_without_reporting_a_drop() {
    let (filtered, dropped) = strip_low_signal_diff_sections("");

    assert!(!dropped);
    assert_eq!(filtered, "");
}

/// The last file in a diff has no following header to terminate it, so its exclusion has to be
/// driven by end-of-input rather than the next boundary.
#[test]
fn a_trailing_low_signal_section_is_dropped_to_end_of_input() {
    let trailing = "diff --git a/src/a.rs b/src/a.rs\n\
@@ -1,1 +1,2 @@\n\
+fn a() {}\n\
diff --git a/pnpm-lock.yaml b/pnpm-lock.yaml\n\
@@ -1,1 +1,2 @@\n\
+  resolution: {integrity: sha512-xyz}\n";

    let (filtered, dropped) = strip_low_signal_diff_sections(trailing);

    assert!(dropped);
    assert!(filtered.contains("+fn a() {}"));
    assert!(!filtered.contains("pnpm-lock.yaml"));
    assert!(!filtered.contains("sha512-xyz"));
}

/// Renames carry different a/ and b/ paths; the b-side is what the inventory reports.
#[test]
fn renamed_files_classify_on_their_post_change_path() {
    let renamed = "diff --git a/src/handler.rs b/dist/handler.js\n\
@@ -1,1 +1,2 @@\n\
+generated\n";

    let (filtered, dropped) = strip_low_signal_diff_sections(renamed);

    assert!(dropped);
    assert!(!filtered.contains("generated"));
}
