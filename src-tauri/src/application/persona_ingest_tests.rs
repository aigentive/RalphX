use std::fs;
use std::path::{Path, PathBuf};

use super::persona_ingest::{
    build_persona_ingest_file_path, ingest_picked_root, ingest_picked_roots,
    persona_ingest_conversation_path, persona_ingest_storage_path, PersonaIngestManifest,
    MAX_INGEST_FILES, MAX_INGEST_FILE_BYTES, MAX_INGEST_TOTAL_BYTES,
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

fn copied_path(destination: &Path, picked_root: &Path, relative: &str) -> PathBuf {
    let canonical_root = picked_root.canonicalize().expect("canonical picked root");
    build_persona_ingest_file_path(destination, &canonical_root, Path::new(relative))
        .expect("hashed destination")
}

fn stored_content_files(destination: &Path) -> Vec<PathBuf> {
    // codeql[rust/path-injection]
    fs::read_dir(destination)
        .expect("ingest destination")
        .filter_map(Result::ok)
        .map(|entry| entry.path().join("content"))
        .filter(|path| path.is_file())
        .collect()
}

#[test]
fn rejects_parent_dir_entries() {
    let temp = temp_dir();
    let destination = fixture_path(temp.path(), "destination");
    assert!(
        build_persona_ingest_file_path(&destination, temp.path(), Path::new("../escape.txt"))
            .is_err()
    );
}

#[test]
fn rejects_absolute_path_entries() {
    let temp = temp_dir();
    let destination = fixture_path(temp.path(), "destination");
    assert!(
        build_persona_ingest_file_path(&destination, temp.path(), Path::new("/escape.txt"))
            .is_err()
    );
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
    let oversized_destination = copied_path(&destination, &source, "large.txt");
    assert!(!oversized_destination.exists());
}

#[test]
fn copies_extensionless_and_unknown_extension_utf8_files() {
    let temp = temp_dir();
    let source = fixture_path(temp.path(), "source");
    let destination = fixture_path(temp.path(), "destination");
    write_fixture(&source, "STYLEGUIDE", b"Concise and direct.\n");
    write_fixture(
        &source,
        "persona.custom-format",
        b"Use terse review notes.\n",
    );

    let manifest = ingest_fixture(&source, &destination);

    assert_eq!(manifest.copied.len(), 2);
    assert!(manifest
        .copied
        .iter()
        .any(|entry| entry.path == "STYLEGUIDE"));
    assert!(manifest
        .copied
        .iter()
        .any(|entry| entry.path == "persona.custom-format"));
}

#[test]
fn skips_known_binary_extensions_and_dotfiles_before_content_read() {
    let temp = temp_dir();
    let source = fixture_path(temp.path(), "source");
    let destination = fixture_path(temp.path(), "destination");
    write_fixture(
        &source,
        "image.png",
        b"not inspected because extension is disallowed",
    );
    write_fixture(&source, ".DS_Store", b"otherwise valid UTF-8");
    let manifest = ingest_fixture(&source, &destination);
    for name in ["image.png", ".DS_Store"] {
        assert!(manifest.skipped.iter().any(|entry| {
            entry.path == name && entry.reason.as_deref() == Some("binary file type")
        }));
    }
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
        let copied_path = copied_path(&destination, &source, &entry.path);
        assert!(copied_path.exists());
        assert!(copied_path.to_string_lossy().contains("file-"));
        assert!(!copied_path.to_string_lossy().contains(&entry.path));
    }
    let readme = copied_path(&destination, &source, "notes/readme.md");
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

#[test]
fn ingest_skips_non_utf8_text_and_uses_app_owned_hashed_conversation_roots() {
    let temp = temp_dir();
    let source = fixture_path(temp.path(), "source");
    let app_data = fixture_path(temp.path(), "app-data");
    let storage = persona_ingest_storage_path(&app_data);
    let destination = persona_ingest_conversation_path(&storage, "conversation/identifier");
    write_fixture(&source, "binary.txt", &[0xff, 0xfe, 0x00]);

    let manifest = ingest_fixture(&source, &destination);

    assert!(manifest.copied.is_empty());
    assert!(manifest.skipped.iter().any(|entry| {
        entry.path == "binary.txt"
            && entry
                .reason
                .as_deref()
                .is_some_and(|reason| reason.contains("UTF-8"))
    }));
    assert!(destination.starts_with(storage));
    assert!(!destination
        .to_string_lossy()
        .contains("conversation/identifier"));
}

#[test]
fn ingesting_a_single_file_uses_its_filename_as_manifest_path() {
    let temp = temp_dir();
    let source = fixture_path(temp.path(), "single.md");
    let destination = fixture_path(temp.path(), "destination");
    // codeql[rust/path-injection]
    fs::write(&source, b"single file source").expect("single fixture file");

    let manifest = ingest_fixture(&source, &destination);

    assert_eq!(manifest.copied.len(), 1);
    assert_eq!(manifest.copied[0].path, "single.md");
    let copied = copied_path(&destination, &source, "single.md");
    // codeql[rust/path-injection]
    assert_eq!(
        fs::read(copied).expect("single file copy"),
        b"single file source"
    );
}

#[cfg(unix)]
#[test]
fn rejects_a_picked_file_that_is_itself_a_symlink() {
    let temp = temp_dir();
    let source = fixture_path(temp.path(), "actual.md");
    let picked = fixture_path(temp.path(), "picked.md");
    let destination = fixture_path(temp.path(), "destination");
    // codeql[rust/path-injection]
    fs::write(&source, b"source").expect("source file");
    std::os::unix::fs::symlink(&source, &picked).expect("picked symlink");

    let error = ingest_picked_root(&picked, &destination).expect_err("root symlink must reject");

    assert!(error.to_string().contains("symlink"));
    assert!(!destination.exists());
}

#[test]
fn batch_ingests_files_and_directories_and_rejects_vanished_entries_by_basename() {
    let temp = temp_dir();
    let picked_file = fixture_path(temp.path(), "single.md");
    let picked_dir = fixture_path(temp.path(), "folder");
    let missing = fixture_path(temp.path(), "vanished.secret.md");
    let destination = fixture_path(temp.path(), "destination");
    // codeql[rust/path-injection]
    fs::write(&picked_file, b"single").expect("single file");
    write_fixture(&picked_dir, "nested.md", b"nested");

    let manifest = ingest_picked_roots(&[picked_file, picked_dir, missing.clone()], &destination)
        .expect("valid batch entries should continue after a vanished path");

    assert_eq!(manifest.copied.len(), 2);
    assert!(manifest.rejected.iter().any(|entry| {
        entry.path == "vanished.secret.md"
            && !entry.path.contains(temp.path().to_string_lossy().as_ref())
    }));
}

#[test]
fn same_name_sources_use_distinct_destinations_and_repicks_refresh_in_place() {
    let temp = temp_dir();
    let source_a = fixture_path(temp.path(), "a");
    let source_b = fixture_path(temp.path(), "b");
    let file_a = fixture_path(&source_a, "README.md");
    let file_b = fixture_path(&source_b, "README.md");
    let destination = fixture_path(temp.path(), "destination");
    write_fixture(&source_a, "README.md", b"alpha");
    write_fixture(&source_b, "README.md", b"beta");

    let batch = ingest_picked_roots(&[file_a.clone(), file_b.clone()], &destination)
        .expect("same-name batch");
    assert_eq!(batch.copied.len(), 2);
    let stored = stored_content_files(&destination);
    assert_eq!(stored.len(), 2);
    let mut contents = stored
        .iter()
        .map(|path| fs::read_to_string(path).expect("stored content"))
        .collect::<Vec<_>>();
    contents.sort();
    assert_eq!(contents, ["alpha", "beta"]);

    // codeql[rust/path-injection]
    fs::write(&file_a, b"alpha revised").expect("revised source");
    let refreshed = ingest_picked_roots(&[file_a], &destination).expect("refreshed re-pick");
    assert_eq!(refreshed.copied[0].reason.as_deref(), Some("updated"));
    assert_eq!(stored_content_files(&destination).len(), 2);
    assert!(stored_content_files(&destination).iter().any(|path| {
        fs::read_to_string(path).is_ok_and(|contents| contents == "alpha revised")
    }));
}

#[test]
fn repeat_ingests_merge_manifest_and_seed_cumulative_file_usage() {
    let temp = temp_dir();
    let first = fixture_path(temp.path(), "first");
    let second = fixture_path(temp.path(), "second");
    let destination = fixture_path(temp.path(), "destination");
    for index in 0..(MAX_INGEST_FILES - 1) {
        write_fixture(&first, &format!("first-{index}.txt"), b"x");
    }
    write_fixture(&second, "allowed.txt", b"y");
    write_fixture(&second, "overflow.txt", b"z");

    ingest_picked_roots(&[first], &destination).expect("first ingest");
    let second_batch = ingest_picked_roots(&[second], &destination).expect("second ingest");

    assert_eq!(second_batch.copied.len(), 1);
    assert!(second_batch
        .skipped
        .iter()
        .any(|entry| { entry.reason.as_deref() == Some("file count exceeds ingest limit") }));
    assert_eq!(
        stored_content_files(&destination).len(),
        MAX_INGEST_FILES as usize
    );
    let persisted: PersonaIngestManifest = serde_json::from_slice(
        &fs::read(destination.join("manifest.json")).expect("cumulative manifest"),
    )
    .expect("valid cumulative manifest");
    assert!(persisted
        .copied
        .iter()
        .any(|entry| entry.path == "first-0.txt"));
    assert!(persisted
        .copied
        .iter()
        .any(|entry| entry.path == "allowed.txt"));
}

#[test]
fn batch_and_repeat_ingests_enforce_cumulative_total_bytes_without_repick_drift() {
    let temp = temp_dir();
    let first = fixture_path(temp.path(), "first");
    let second = fixture_path(temp.path(), "second");
    let third = fixture_path(temp.path(), "third");
    let destination = fixture_path(temp.path(), "destination");
    for (root, range) in [(&first, 0..11), (&second, 11..22), (&third, 22..33)] {
        for index in range {
            write_fixture(
                root,
                &format!("chunk-{index}.txt"),
                &vec![b'a'; MAX_INGEST_FILE_BYTES as usize],
            );
        }
    }

    let batch = ingest_picked_roots(&[first.clone(), second, third], &destination)
        .expect("three-root batch");
    assert_eq!(
        batch.copied.len(),
        (MAX_INGEST_TOTAL_BYTES / MAX_INGEST_FILE_BYTES) as usize
    );
    assert!(batch
        .skipped
        .iter()
        .any(|entry| { entry.reason.as_deref() == Some("total byte limit exceeded") }));

    let repick = ingest_picked_roots(&[first], &destination).expect("idempotent re-pick");
    assert_eq!(repick.copied.len(), 11);
    assert!(repick.copied.iter().all(|entry| entry.reason.is_none()));
    assert_eq!(stored_content_files(&destination).len(), 32);
}

#[cfg(unix)]
#[test]
fn ingest_refuses_to_replace_an_app_owned_manifest_symlink() {
    let temp = temp_dir();
    let source = fixture_path(temp.path(), "source");
    let destination = fixture_path(temp.path(), "destination");
    let manifest_target = fixture_path(temp.path(), "manifest-target.json");
    write_fixture(&source, "accepted.txt", b"accepted");
    // codeql[rust/path-injection]
    fs::create_dir_all(&destination).expect("destination root");
    // codeql[rust/path-injection]
    fs::write(&manifest_target, b"must remain untouched").expect("manifest target");
    std::os::unix::fs::symlink(&manifest_target, destination.join("manifest.json"))
        .expect("manifest symlink");

    let error = ingest_picked_root(&source, &destination)
        .expect_err("manifest symlinks must not be followed");

    assert!(error.to_string().contains("manifest"));
    // codeql[rust/path-injection]
    assert_eq!(
        fs::read(&manifest_target).expect("manifest target remains readable"),
        b"must remain untouched"
    );
}
