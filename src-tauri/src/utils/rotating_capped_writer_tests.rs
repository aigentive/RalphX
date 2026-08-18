use std::io::Write;
use std::path::{Path, PathBuf};

use crate::utils::rotating_capped_writer::{
    RotatingCappedWriter, CAP_SUPPRESSED_MARKER, ROTATION_MARKER_PREFIX,
};

const ACTIVE_FILENAME: &str = "ralphx_2026-08-11_00-00-00.log";
const ROLLED_FILENAME: &str = "ralphx_2026-08-11_00-00-00_rolled.log";

/// Opens a writer over a fresh active log inside `dir`, mirroring how
/// `create_file_log` hands the bootstrap an already-created file plus its path.
fn writer_in(dir: &Path, max_bytes: u64) -> (RotatingCappedWriter, PathBuf, PathBuf) {
    let active_path = dir.join(ACTIVE_FILENAME);
    let active_file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&active_path)
        .expect("active log file");
    let rolled_path = dir.join(ROLLED_FILENAME);
    let writer = RotatingCappedWriter::new(active_file, active_path.clone(), max_bytes);
    (writer, active_path, rolled_path)
}

fn file_len(path: &Path) -> u64 {
    std::fs::metadata(path).map(|meta| meta.len()).unwrap_or(0)
}

fn assert_within_cap(active: &Path, rolled: &Path, max_bytes: u64) {
    let total = file_len(active) + file_len(rolled);
    assert!(
        total <= max_bytes,
        "on-disk total {total} exceeded the configured cap {max_bytes}"
    );
}

fn expected_rotation_marker() -> Vec<u8> {
    [
        ROTATION_MARKER_PREFIX,
        ROLLED_FILENAME.as_bytes(),
        b"\n".as_slice(),
    ]
    .concat()
}

#[test]
fn writes_below_the_cap_land_verbatim_without_rotating() {
    let dir = tempfile::tempdir().expect("temp log directory");
    let (mut writer, active, rolled) = writer_in(dir.path(), 4096);

    writer.write_all(b"first line\n").expect("first write");
    writer.write_all(b"second line\n").expect("second write");
    writer.flush().expect("flush");

    assert_eq!(
        std::fs::read(&active).expect("active log"),
        b"first line\nsecond line\n"
    );
    assert!(!rolled.exists(), "no rotation should have happened yet");
    assert_within_cap(&active, &rolled, 4096);
}

#[test]
fn filling_a_chunk_rotates_and_starts_the_next_chunk_with_a_marker() {
    let dir = tempfile::tempdir().expect("temp log directory");
    let (mut writer, active, rolled) = writer_in(dir.path(), 400);
    let chunk = writer.chunk_bytes();

    let first_chunk = vec![b'a'; chunk];
    writer.write_all(&first_chunk).expect("chunk-filling write");
    assert!(!rolled.exists(), "rotation must be lazy, not eager");

    writer
        .write_all(b"tail bytes")
        .expect("post-rotation write");
    writer.flush().expect("flush");

    assert_eq!(std::fs::read(&rolled).expect("rolled log"), first_chunk);
    let active_bytes = std::fs::read(&active).expect("active log");
    assert_eq!(
        active_bytes,
        [expected_rotation_marker(), b"tail bytes".to_vec()].concat()
    );
    assert_within_cap(&active, &rolled, 400);
}

#[test]
fn writing_far_past_the_cap_preserves_the_tail_and_stays_bounded() {
    let dir = tempfile::tempdir().expect("temp log directory");
    let (mut writer, active, rolled) = writer_in(dir.path(), 400);
    let chunk = writer.chunk_bytes();

    let event_len = 32;
    let event_count = (5 * 2 * chunk) / event_len;
    let mut last_event = Vec::new();
    for index in 0..event_count {
        let event = format!("{index:0>width$}\n", width = event_len - 1).into_bytes();
        writer.write_all(&event).expect("patterned write");
        last_event = event;
    }
    writer.flush().expect("flush");

    let active_bytes = std::fs::read(&active).expect("active log");
    assert!(
        active_bytes.ends_with(&last_event),
        "the newest event must survive in the active chunk"
    );
    assert_within_cap(&active, &rolled, 400);
}

#[test]
fn a_single_event_larger_than_a_chunk_still_ends_with_its_tail_on_disk() {
    let dir = tempfile::tempdir().expect("temp log directory");
    let (mut writer, active, rolled) = writer_in(dir.path(), 400);
    let chunk = writer.chunk_bytes();

    let mut oversized = vec![b'x'; 3 * chunk];
    let tail = b"OVERSIZED-TAIL\n";
    let start = oversized.len() - tail.len();
    oversized[start..].copy_from_slice(tail);

    assert_eq!(
        writer.write(&oversized).expect("oversized write"),
        oversized.len(),
        "the writer must always report the full caller length"
    );
    writer.flush().expect("flush");

    let active_bytes = std::fs::read(&active).expect("active log");
    assert!(
        active_bytes.ends_with(tail),
        "the end of an oversized event must remain on disk"
    );
    assert_within_cap(&active, &rolled, 400);
}

#[test]
fn rotation_keeps_whole_events_on_each_side_of_the_boundary() {
    let dir = tempfile::tempdir().expect("temp log directory");
    let (mut writer, active, rolled) = writer_in(dir.path(), 400);
    let chunk = writer.chunk_bytes();
    let marker_len = expected_rotation_marker().len();

    // Two events fit one chunk, a third does not, and a single event still fits
    // a freshly rotated chunk after its marker.
    let event_len = 80;
    assert!(2 * event_len <= chunk && 3 * event_len > chunk);
    assert!(event_len <= chunk - marker_len);

    let events: Vec<Vec<u8>> = (0..3u8)
        .map(|index| {
            let mut event = vec![b'A' + index; event_len];
            event[event_len - 1] = b'\n';
            event
        })
        .collect();
    for event in &events {
        writer.write_all(event).expect("event write");
    }
    writer.flush().expect("flush");

    assert_eq!(
        std::fs::read(&rolled).expect("rolled log"),
        [events[0].clone(), events[1].clone()].concat(),
        "the rolled chunk must contain whole events only"
    );
    assert_eq!(
        std::fs::read(&active).expect("active log"),
        [expected_rotation_marker(), events[2].clone()].concat(),
        "the event that triggered rotation must land whole in the new chunk"
    );
    assert_within_cap(&active, &rolled, 400);
}

#[test]
fn rotation_reclaims_a_stale_rolled_chunk_before_renaming() {
    let dir = tempfile::tempdir().expect("temp log directory");
    let (mut writer, active, rolled) = writer_in(dir.path(), 400);
    let chunk = writer.chunk_bytes();
    std::fs::write(&rolled, b"stale content from an earlier rotation").expect("stale rolled chunk");

    let first_chunk = vec![b'a'; chunk];
    writer.write_all(&first_chunk).expect("chunk-filling write");
    writer
        .write_all(b"tail bytes")
        .expect("post-rotation write");
    writer.flush().expect("flush");

    assert_eq!(
        std::fs::read(&rolled).expect("rolled log"),
        first_chunk,
        "the stale chunk must be removed, not left in place by a failed rename"
    );
    assert!(dir.path().read_dir().expect("log dir").count() == 2);
    assert_within_cap(&active, &rolled, 400);
}

#[cfg(unix)]
#[test]
fn rotation_unlinks_a_symlinked_rolled_path_instead_of_following_it() {
    let dir = tempfile::tempdir().expect("temp log directory");
    let decoy_dir = tempfile::tempdir().expect("temp decoy directory");
    let decoy = decoy_dir.path().join("decoy.txt");
    std::fs::write(&decoy, b"decoy must survive").expect("decoy file");

    let (mut writer, active, rolled) = writer_in(dir.path(), 400);
    let chunk = writer.chunk_bytes();
    std::os::unix::fs::symlink(&decoy, &rolled).expect("symlinked rolled path");

    writer
        .write_all(&vec![b'a'; chunk])
        .expect("chunk-filling write");
    writer
        .write_all(b"tail bytes")
        .expect("post-rotation write");
    writer.flush().expect("flush");

    assert_eq!(
        std::fs::read(&decoy).expect("decoy file"),
        b"decoy must survive",
        "remove_file must unlink the symlink, never write through it"
    );
    let rolled_meta = std::fs::symlink_metadata(&rolled).expect("rolled metadata");
    assert!(
        rolled_meta.file_type().is_file(),
        "the rolled path must be a regular file after rotation"
    );
    assert_within_cap(&active, &rolled, 400);
}

#[cfg(unix)]
#[test]
fn a_failed_rotation_degrades_to_bounded_suppression() {
    use std::os::unix::fs::PermissionsExt;

    let dir = tempfile::tempdir().expect("temp log directory");
    let (mut writer, active, rolled) = writer_in(dir.path(), 400);
    let chunk = writer.chunk_bytes();
    let event_len = 80;

    let original_perms = std::fs::metadata(dir.path())
        .expect("log dir metadata")
        .permissions();
    std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o500))
        .expect("read-only log dir");

    // Running as root ignores directory permissions, so the failure cannot be
    // provoked; restore and skip rather than assert something untrue.
    let probe = dir.path().join("probe");
    if std::fs::File::create(&probe).is_ok() {
        let _ = std::fs::remove_file(&probe);
        std::fs::set_permissions(dir.path(), original_perms).expect("restore log dir");
        return;
    }

    for index in 0..3u8 {
        let mut event = vec![b'A' + index; event_len];
        event[event_len - 1] = b'\n';
        assert_eq!(
            writer
                .write(&event)
                .expect("writes must not surface errors"),
            event_len,
            "a failed rotation must not make the tracing pipeline see an error"
        );
    }
    writer.flush().expect("flush");

    let active_bytes = std::fs::read(&active).expect("active log");
    let suppression_room = chunk - 2 * event_len;
    assert_eq!(
        &active_bytes[2 * event_len..],
        &CAP_SUPPRESSED_MARKER[..suppression_room.min(CAP_SUPPRESSED_MARKER.len())],
        "suppression must record itself in the remaining chunk room"
    );
    assert!(!rolled.exists(), "no rolled chunk should exist");
    assert_within_cap(&active, &rolled, 400);

    // Prove the suppressed-write no-op contract: writes after suppression must
    // return Ok, append nothing, and leave the cap invariant intact.
    let mut post_suppress_event = vec![b'Z'; event_len];
    post_suppress_event[event_len - 1] = b'\n';
    assert_eq!(
        writer
            .write(&post_suppress_event)
            .expect("suppressed write must not surface an error"),
        event_len,
        "a suppressed writer must still report Ok(len) to the tracing pipeline"
    );
    let active_bytes_after = std::fs::read(&active).expect("active log after suppressed write");
    assert_eq!(
        active_bytes, active_bytes_after,
        "a suppressed writer must append nothing to the active file"
    );
    assert!(!rolled.exists(), "a suppressed writer must not attempt another rotation");
    writer.flush().expect("flush while suppressed must return Ok");
    assert_within_cap(&active, &rolled, 400);

    std::fs::set_permissions(dir.path(), original_perms).expect("restore log dir");
}

#[test]
fn an_absurdly_small_cap_is_floored_to_one_usable_chunk_instead_of_looping() {
    let dir = tempfile::tempdir().expect("temp log directory");
    let (mut writer, active, rolled) = writer_in(dir.path(), 8);
    let chunk = writer.chunk_bytes();
    let marker_len = expected_rotation_marker().len();

    assert_eq!(
        chunk,
        marker_len + 1,
        "a marker-only chunk would rotate forever, so the floor is deliberate"
    );

    writer
        .write_all(&vec![b'z'; 10 * chunk])
        .expect("write far past the floor");
    writer.flush().expect("flush");

    // The documented trade: a hand-set absurd cap yields two floored chunks
    // rather than the requested byte count.
    assert_within_cap(&active, &rolled, (2 * chunk) as u64);
}
