use std::fs::File;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

/// Prefix of the line written at the top of every freshly rotated chunk.
pub const ROTATION_MARKER_PREFIX: &[u8] = b"[ralphx] log rotated; earlier output moved to ";

/// Written when rotation itself fails and the writer degrades to dropping output.
pub const CAP_SUPPRESSED_MARKER: &[u8] =
    b"[ralphx] file log cap reached; further output suppressed.\n";

const ROLLED_SUFFIX: &str = "_rolled.log";

/// A [`Write`] adapter that keeps the **newest** output of a per-launch log
/// while bounding total disk usage at the configured cap.
///
/// The cap is split into two chunks. Output goes to the active log until its
/// chunk is full; the writer then discards the previous rolled chunk, renames
/// the active log to `<stem>_rolled.log`, and reopens a fresh active log. At
/// least the most recent half of the cap is always on disk, and the newest line
/// is always in the active file.
///
/// Rotation happens at event boundaries: `tracing_appender::non_blocking` hands
/// this writer one complete formatted event per call, and an event that would
/// straddle the boundary is moved whole into the new chunk. Only an event larger
/// than a whole chunk is split.
///
/// Every rotation failure degrades to bounded suppression rather than
/// unbounded growth, and `write` never surfaces an error to the tracing
/// pipeline once the active file is gone.
pub struct RotatingCappedWriter {
    active: Option<File>,
    active_path: PathBuf,
    rolled_path: PathBuf,
    rotation_marker: Vec<u8>,
    chunk_bytes: usize,
    bytes_in_chunk: usize,
    suppressed: bool,
}

impl RotatingCappedWriter {
    /// Wraps an already-created active log file plus the path it was created at.
    ///
    /// `active_path` must be the actually-created path (collision suffixes
    /// included) so the rolled chunk always pairs with the real active file.
    pub fn new(active_file: File, active_path: PathBuf, max_bytes: u64) -> Self {
        let rolled_path = rolled_path_for(&active_path);
        let rotation_marker = rotation_marker_for(&rolled_path);
        let max_bytes = usize::try_from(max_bytes).unwrap_or(usize::MAX);
        // A chunk that cannot hold its own marker plus one byte of payload would
        // rotate forever, so an absurd hand-set cap is floored instead.
        let chunk_bytes = (max_bytes / 2).max(rotation_marker.len() + 1);

        Self {
            active: Some(active_file),
            active_path,
            rolled_path,
            rotation_marker,
            chunk_bytes,
            bytes_in_chunk: 0,
            suppressed: false,
        }
    }

    /// Bytes retained per chunk; total on-disk usage stays at or below twice this.
    pub fn chunk_bytes(&self) -> usize {
        self.chunk_bytes
    }

    fn write_active(&mut self, bytes: &[u8]) -> io::Result<()> {
        let Some(file) = self.active.as_mut() else {
            return Err(io::Error::new(
                io::ErrorKind::Other,
                "rotating log writer has no active file",
            ));
        };
        file.write_all(bytes)?;
        self.bytes_in_chunk += bytes.len();
        Ok(())
    }

    /// Enters the degraded mode: record it in whatever chunk room is left, then
    /// drop output. Always returns `false` so callers can `return` it directly.
    fn suppress(&mut self) -> bool {
        let room = self.chunk_bytes.saturating_sub(self.bytes_in_chunk);
        let marker = &CAP_SUPPRESSED_MARKER[..CAP_SUPPRESSED_MARKER.len().min(room)];
        if !marker.is_empty() {
            if let Some(file) = self.active.as_mut() {
                let _ = file.write_all(marker);
                let _ = file.flush();
            }
        }
        self.active = None;
        self.suppressed = true;
        false
    }

    /// Moves the full active chunk aside and reopens an empty one.
    ///
    /// Returns `false` when the writer degraded to suppression instead.
    fn rotate(&mut self) -> bool {
        if let Some(file) = self.active.as_mut() {
            let _ = file.flush();
        }

        // Both paths are children of the RalphX-owned log directory: the active
        // path comes from `create_file_log`, and the rolled path is that path's
        // stem plus a fixed constant suffix. `remove_file` unlinks a symlink at
        // the rolled path rather than following it.
        // codeql[rust/path-injection]
        match std::fs::remove_file(&self.rolled_path) {
            Ok(()) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(_) => return self.suppress(),
        }

        // codeql[rust/path-injection]
        if std::fs::rename(&self.active_path, &self.rolled_path).is_err() {
            return self.suppress();
        }

        // `create_new` preserves the `create_file_log` guarantee: never truncate
        // an existing file and never follow a pre-existing symlink.
        // codeql[rust/path-injection]
        let reopened = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&self.active_path);
        let Ok(reopened) = reopened else {
            // The handle we still hold now points at the rolled chunk, so the
            // suppression marker lands at the tail of the retained output.
            return self.suppress();
        };

        self.active = Some(reopened);
        self.bytes_in_chunk = 0;

        let marker = std::mem::take(&mut self.rotation_marker);
        let written = self.write_active(&marker);
        self.rotation_marker = marker;
        if written.is_err() {
            self.active = None;
            self.suppressed = true;
            return false;
        }

        true
    }
}

impl Write for RotatingCappedWriter {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        if buf.is_empty() || self.suppressed {
            return Ok(buf.len());
        }

        let mut offset = 0;
        let mut rotated_for_event = false;
        while offset < buf.len() {
            let remaining_chunk = self.chunk_bytes.saturating_sub(self.bytes_in_chunk);
            let remaining_event = buf.len() - offset;

            if remaining_chunk == 0 {
                if !self.rotate() {
                    return Ok(buf.len());
                }
                continue;
            }

            // Event-boundary rotation: a whole event that does not fit here but
            // would fit a fresh chunk moves over intact, so the line recording a
            // failure is never severed across two files.
            let fits_fresh_chunk =
                remaining_event <= self.chunk_bytes.saturating_sub(self.rotation_marker.len());
            if !rotated_for_event
                && offset == 0
                && self.bytes_in_chunk > 0
                && remaining_event > remaining_chunk
                && fits_fresh_chunk
            {
                if !self.rotate() {
                    return Ok(buf.len());
                }
                rotated_for_event = true;
                continue;
            }

            let accepted = remaining_chunk.min(remaining_event);
            if self.write_active(&buf[offset..offset + accepted]).is_err() {
                self.suppress();
                return Ok(buf.len());
            }
            offset += accepted;
        }

        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        match self.active.as_mut() {
            Some(file) => file.flush(),
            None => Ok(()),
        }
    }
}

fn rolled_path_for(active_path: &Path) -> PathBuf {
    let stem = active_path
        .file_stem()
        .map(|stem| stem.to_string_lossy().into_owned())
        .unwrap_or_else(|| "ralphx".to_string());
    let rolled_filename = format!("{stem}{ROLLED_SUFFIX}");
    match active_path.parent() {
        Some(parent) => parent.join(rolled_filename),
        None => PathBuf::from(rolled_filename),
    }
}

fn rotation_marker_for(rolled_path: &Path) -> Vec<u8> {
    let rolled_filename = rolled_path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();
    [
        ROTATION_MARKER_PREFIX,
        rolled_filename.as_bytes(),
        b"\n".as_slice(),
    ]
    .concat()
}

#[cfg(test)]
#[path = "rotating_capped_writer_tests.rs"]
mod tests;
