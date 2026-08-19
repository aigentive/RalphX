//! Generic low-signal file classification for the Workspace Review packet.
//!
//! A large mechanical change is mostly lockfile churn, regenerated snapshots, and binary assets.
//! Spending the patch-excerpt budget on those starves the substantive hunks and pushes the
//! reviewer into an eight-delegate fan-out to read a diff it could have read in one pass.
//!
//! This classifier is deliberately generic — extensions, basenames, and directory segments only.
//! It encodes no repository-specific knowledge of which seams are "risky"; that judgment stays
//! entirely with the reviewer. Low-signal files are still listed in the inventory and still served
//! in full by `get_workspace_review_diff_page`; they are only omitted from the inline excerpt.

/// Why a file carries little per-line review signal.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum LowSignalClass {
    Lockfile,
    Binary,
    Snapshot,
    Asset,
    Generated,
}

impl LowSignalClass {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Lockfile => "lockfile",
            Self::Binary => "binary",
            Self::Snapshot => "snapshot",
            Self::Asset => "asset",
            Self::Generated => "generated",
        }
    }
}

/// Exact basenames that are always dependency lockfiles.
const LOCKFILE_BASENAMES: &[&str] = &[
    "package-lock.json",
    "yarn.lock",
    "pnpm-lock.yaml",
    "bun.lockb",
    "Cargo.lock",
    "Gemfile.lock",
    "composer.lock",
    "poetry.lock",
    "Pipfile.lock",
    "uv.lock",
    "flake.lock",
    "go.sum",
    "packages.lock.json",
];

/// Extensions for media, fonts, archives, and compiled artifacts.
const ASSET_EXTENSIONS: &[&str] = &[
    "png", "jpg", "jpeg", "gif", "webp", "bmp", "ico", "icns", "tiff", "avif", "svgz", "mp3",
    "mp4", "wav", "mov", "webm", "avi", "ogg", "woff", "woff2", "ttf", "otf", "eot", "pdf", "zip",
    "gz", "tgz", "bz2", "xz", "zst", "7z", "rar", "jar", "wasm", "so", "dylib", "dll", "exe",
    "bin", "class", "pyc", "o", "a",
];

/// Directory segments whose contents are build or dependency output.
const GENERATED_DIR_SEGMENTS: &[&str] = &[
    "node_modules",
    "dist",
    "build",
    "target",
    "vendor",
    "__pycache__",
    ".next",
    ".nuxt",
    "coverage",
];

/// Directory segments holding recorded test snapshots.
const SNAPSHOT_DIR_SEGMENTS: &[&str] = &["__snapshots__", "snapshots"];

/// Extensions for recorded test snapshots.
const SNAPSHOT_EXTENSIONS: &[&str] = &["snap", "ambr"];

/// Suffixes for minified or generated source, which no one reviews line by line.
const GENERATED_SUFFIXES: &[&str] = &[
    ".min.js",
    ".min.css",
    ".min.mjs",
    ".map",
    ".generated.ts",
    ".generated.js",
    ".g.dart",
    "_pb2.py",
    ".pb.go",
];

/// Classifies a changed file, or returns `None` when it deserves normal review attention.
///
/// `is_binary` comes from git's own binary detection and wins over path heuristics, since a file
/// git cannot diff has no reviewable hunks whatever its extension suggests.
pub fn low_signal_class(path: &str, is_binary: bool) -> Option<LowSignalClass> {
    if is_binary {
        return Some(LowSignalClass::Binary);
    }
    let path = path.trim();
    if path.is_empty() {
        return None;
    }
    let normalized = path.replace('\\', "/");
    let basename = normalized.rsplit('/').next().unwrap_or(&normalized);

    if LOCKFILE_BASENAMES
        .iter()
        .any(|candidate| basename.eq_ignore_ascii_case(candidate))
    {
        return Some(LowSignalClass::Lockfile);
    }

    let extension = basename
        .rsplit_once('.')
        .map(|(_, extension)| extension.to_ascii_lowercase())
        .unwrap_or_default();

    // `*.lock` is a lockfile only as a full extension: `locksmith.rs` and `lock.rs` are source.
    if extension == "lock" {
        return Some(LowSignalClass::Lockfile);
    }
    if SNAPSHOT_EXTENSIONS.contains(&extension.as_str()) {
        return Some(LowSignalClass::Snapshot);
    }
    if ASSET_EXTENSIONS.contains(&extension.as_str()) {
        return Some(LowSignalClass::Asset);
    }

    let segments = normalized.split('/').collect::<Vec<_>>();
    // Only directory segments count, never the filename itself: `src/build.rs` is source.
    let directories = &segments[..segments.len().saturating_sub(1)];
    if directories
        .iter()
        .any(|segment| SNAPSHOT_DIR_SEGMENTS.contains(segment))
    {
        return Some(LowSignalClass::Snapshot);
    }
    if directories
        .iter()
        .any(|segment| GENERATED_DIR_SEGMENTS.contains(segment))
    {
        return Some(LowSignalClass::Generated);
    }

    let lowercased = basename.to_ascii_lowercase();
    if GENERATED_SUFFIXES
        .iter()
        .any(|suffix| lowercased.ends_with(suffix))
    {
        return Some(LowSignalClass::Generated);
    }

    None
}

/// Drops low-signal files' hunks from a unified diff, keeping every other block byte-identical.
///
/// Returns the filtered diff and whether anything was dropped. Splits on `diff --git` headers,
/// which is the only file boundary a unified diff guarantees.
pub fn strip_low_signal_diff_sections(diff: &str) -> (String, bool) {
    if diff.trim().is_empty() {
        return (diff.to_string(), false);
    }
    let mut kept = String::with_capacity(diff.len());
    let mut dropped_any = false;
    let mut current_is_low_signal = false;
    let mut seen_header = false;

    for line in diff.split_inclusive('\n') {
        if let Some(path) = diff_git_header_path(line) {
            seen_header = true;
            current_is_low_signal = low_signal_class(&path, false).is_some();
            if current_is_low_signal {
                dropped_any = true;
                continue;
            }
        } else if current_is_low_signal {
            continue;
        }
        // Preamble before the first `diff --git` header belongs to no file; keep it.
        let _ = seen_header;
        kept.push_str(line);
    }
    (kept, dropped_any)
}

/// Extracts the b-side path from a `diff --git a/<path> b/<path>` header line.
fn diff_git_header_path(line: &str) -> Option<String> {
    let rest = line.trim_end_matches(['\n', '\r']).strip_prefix("diff --git ")?;
    // Take the b-side, which is the post-change path and the one the inventory reports.
    let b_index = rest.rfind(" b/")?;
    Some(rest[b_index + 3..].to_string())
}
