use std::fs;
use std::path::{Path, PathBuf};

use super::persona_ingest::{
    build_persona_ingest_file_path, ingest_picked_root, PersonaIngestManifest, MAX_INGEST_FILES,
    MAX_INGEST_FILE_BYTES, MAX_INGEST_TOTAL_BYTES,
};

fn temp_dir() -> tempfile::TempDir {
    let current_dir = std::env::current_dir().expect("current checkout directory");
    tempfile::tempdir_in(current_dir).expect("checkout-local temporary directory")
}

fn fixture_path(root: &Path, component: &str) -> PathBuf {
    assert!(Path::new(component).is_relative());
    assert!(!component.contains('/') && !component.contains('\\'));
    root.join(component)
}

fn fixture_relative_path(root: &Path, relative: &str) -> PathBuf {
    let mut path = root.to_path_buf();
    for component in Path::new(relative).components() {
        let std::path::Component::Normal(component) = component else {
            panic!("fixture path must be relative and traversal-free");
        };
        let component = component.to_string_lossy();
        assert!(!component.is_empty());
        assert!(!component.contains('/') && !component.contains('\\'));
        path.push(component.as_ref());
    }
    path
}

fn write_fixture(root: &Path, relative: &str, contents: &[u8]) {
    let path = fixture_relative_path(root, relative);
    let parent = path.parent().expect("fixture file parent");
    // codeql[rust/path-injection]
    fs::create_dir_all(parent).expect("fixture parent");
    // codeql[rust/path-injection]
    fs::write(path, contents).expect("fixture file");
}

fn ingest_fixture(root: &Path, destination: &Path) -> PersonaIngestManifest {
    ingest_picked_root(root, destination).expect("fixture ingestion")
}

#[test]
fn rejects_parent_dir_entries() {
    let temp = temp_dir();
    let destination = fixture_path(temp.path(), "destination");
    assert!(build_persona_ingest_file_path(&destination, Path::new("../escape.txt")).is_err());
}

#[test]
fn rejects_absolute_path_entries() {
    let temp = temp_dir();
    let destination = fixture_path(temp.path(), "destination");
    assert!(build_persona_ingest_file_path(&destination, Path::new("/escape.txt")).is_err());
}

#[cfg(unix)]
#[test]
fn rejects_symlink_even_inside_root() {
    let temp = temp_dir();
    let source = fixture_path(temp.path(), "source");
    let destination = fixture_path(temp.path(), "destination");
    write_fixture(&source, "actual.txt", b"allowed text");
    let link = fixture_path(&source, "link.txt");
    std::os::unix::fs::symlink(fixture_path(&source, "actual.txt"), &link)
        .expect("fixture symlink");
    let manifest = ingest_fixture(&source, &destination);
    assert!(manifest
        .rejected
        .iter()
        .any(|entry| entry.path == "link.txt"));
    assert!(!manifest.copied.iter().any(|entry| entry.path == "link.txt"));
}

#[cfg(unix)]
#[test]
fn rejects_symlink_escaping_root() {
    let temp = temp_dir();
    let source = fixture_path(temp.path(), "source");
    let outside = fixture_path(temp.path(), "outside.txt");
    let destination = fixture_path(temp.path(), "destination");
    write_fixture(&source, "allowed.txt", b"allowed text");
    // codeql[rust/path-injection]
    fs::write(&outside, b"outside text").expect("outside fixture file");
    let link = fixture_path(&source, "escape.txt");
    std::os::unix::fs::symlink(&outside, &link).expect("escaping fixture symlink");
    let manifest = ingest_fixture(&source, &destination);
    assert!(manifest
        .rejected
        .iter()
        .any(|entry| entry.path == "escape.txt"));
    assert!(!manifest
        .copied
        .iter()
        .any(|entry| entry.path == "escape.txt"));
}

#[test]
fn skips_oversized_file_before_read_with_manifest_reason() {
    let temp = temp_dir();
    let source = fixture_path(temp.path(), "source");
    let destination = fixture_path(temp.path(), "destination");
    write_fixture(
        &source,
        "large.txt",
        &vec![b'x'; MAX_INGEST_FILE_BYTES as usize + 1],
    );
    let manifest = ingest_fixture(&source, &destination);
    assert!(manifest.skipped.iter().any(|entry| {
        entry.path == "large.txt"
            && entry
                .reason
                .as_deref()
                .is_some_and(|reason| reason.contains("file size"))
    }));
    let oversized_destination =
        build_persona_ingest_file_path(&destination, Path::new("large.txt"))
            .expect("oversized hashed destination");
    assert!(!oversized_destination.exists());
}

#[test]
fn skips_binary_file_via_type_allowlist() {
    let temp = temp_dir();
    let source = fixture_path(temp.path(), "source");
    let destination = fixture_path(temp.path(), "destination");
    write_fixture(
        &source,
        "image.png",
        b"not inspected because extension is disallowed",
    );
    let manifest = ingest_fixture(&source, &destination);
    assert!(manifest.skipped.iter().any(|entry| {
        entry.path == "image.png"
            && entry
                .reason
                .as_deref()
                .is_some_and(|reason| reason.contains("file type"))
    }));
}

#[test]
fn enforces_total_count_bytes_depth_caps() {
    let total = temp_dir();
    let total_source = fixture_path(total.path(), "source");
    let total_destination = fixture_path(total.path(), "destination");
    for index in 0..(MAX_INGEST_TOTAL_BYTES / MAX_INGEST_FILE_BYTES) {
        write_fixture(
            &total_source,
            &format!("chunk-{index}.txt"),
            &vec![b'a'; MAX_INGEST_FILE_BYTES as usize],
        );
    }
    write_fixture(&total_source, "overflow.txt", b"b");
    let total_manifest = ingest_fixture(&total_source, &total_destination);
    assert!(total_manifest.skipped.iter().any(|entry| {
        entry
            .reason
            .as_deref()
            .is_some_and(|reason| reason.contains("total byte"))
    }));

    let count = temp_dir();
    let count_source = fixture_path(count.path(), "source");
    let count_destination = fixture_path(count.path(), "destination");
    for index in 0..=MAX_INGEST_FILES {
        write_fixture(&count_source, &format!("file-{index}.txt"), b"x");
    }
    let count_manifest = ingest_fixture(&count_source, &count_destination);
    assert_eq!(count_manifest.copied.len(), MAX_INGEST_FILES as usize);
    assert!(count_manifest.skipped.iter().any(|entry| {
        entry
            .reason
            .as_deref()
            .is_some_and(|reason| reason.contains("file count"))
    }));

    let depth = temp_dir();
    let depth_source = fixture_path(depth.path(), "source");
    let depth_destination = fixture_path(depth.path(), "destination");
    let nested = (0..13).fold(depth_source.clone(), |path, index| {
        path.join(format!("d{index}"))
    });
    // codeql[rust/path-injection]
    fs::create_dir_all(&nested).expect("deep fixture parent");
    // codeql[rust/path-injection]
    fs::write(nested.join("deep.txt"), b"too deep").expect("deep fixture file");
    let depth_manifest = ingest_fixture(&depth_source, &depth_destination);
    assert!(depth_manifest.skipped.iter().any(|entry| {
        entry
            .reason
            .as_deref()
            .is_some_and(|reason| reason.contains("depth"))
    }));
}

#[test]
fn accepts_normal_tree_writes_hashed_destinations() {
    let temp = temp_dir();
    let source = fixture_path(temp.path(), "source");
    let destination = fixture_path(temp.path(), "destination");
    write_fixture(&source, "notes/readme.md", b"# Persona notes\n");
    write_fixture(&source, "config/settings.toml", b"name = 'Ada'\n");
    let manifest = ingest_fixture(&source, &destination);
    assert_eq!(manifest.copied.len(), 2);
    for entry in &manifest.copied {
        let copied_path = build_persona_ingest_file_path(&destination, Path::new(&entry.path))
            .expect("hashed destination");
        assert!(copied_path.exists());
        assert!(copied_path.to_string_lossy().contains("file-"));
        assert!(!copied_path.to_string_lossy().contains(&entry.path));
    }
    let readme = build_persona_ingest_file_path(&destination, Path::new("notes/readme.md"))
        .expect("readme destination");
    // codeql[rust/path-injection]
    assert_eq!(
        fs::read(readme).expect("copied readme"),
        b"# Persona notes\n"
    );
}

#[test]
fn manifest_reports_copied_skipped_rejected() {
    let temp = temp_dir();
    let source = fixture_path(temp.path(), "source");
    let destination = fixture_path(temp.path(), "destination");
    write_fixture(&source, "accepted.txt", b"text");
    write_fixture(&source, "skipped.png", b"image");
    #[cfg(unix)]
    std::os::unix::fs::symlink(
        fixture_path(&source, "accepted.txt"),
        fixture_path(&source, "rejected.txt"),
    )
    .expect("rejected fixture symlink");
    let manifest = ingest_fixture(&source, &destination);
    assert_eq!(manifest.copied.len(), 1);
    assert_eq!(manifest.skipped.len(), 1);
    #[cfg(unix)]
    assert_eq!(manifest.rejected.len(), 1);
    let manifest_path = destination.join("manifest.json");
    // codeql[rust/path-injection]
    let persisted: PersonaIngestManifest =
        serde_json::from_slice(&fs::read(manifest_path).expect("persisted manifest"))
            .expect("valid persisted manifest");
    assert_eq!(persisted, manifest);
}
