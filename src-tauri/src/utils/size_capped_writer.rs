use std::io::{self, Write};

pub const CAP_REACHED_MARKER: &[u8] =
    b"[ralphx] file log cap reached; further output suppressed.\n";

/// A [`Write`] adapter that accepts all caller bytes while retaining at most
/// `max_bytes` in the underlying log, including its single cap marker.
pub struct SizeCappedWriter<W: Write> {
    inner: W,
    max_bytes: usize,
    bytes_written: usize,
    capped: bool,
}

impl<W: Write> SizeCappedWriter<W> {
    pub fn new(inner: W, max_bytes: u64) -> Self {
        Self {
            inner,
            max_bytes: usize::try_from(max_bytes).unwrap_or(usize::MAX),
            bytes_written: 0,
            capped: false,
        }
    }

    fn write_marker(&mut self) -> io::Result<()> {
        let remaining = self.max_bytes.saturating_sub(self.bytes_written);
        let marker = &CAP_REACHED_MARKER[..CAP_REACHED_MARKER.len().min(remaining)];
        self.inner.write_all(marker)?;
        self.bytes_written += marker.len();
        self.capped = true;
        Ok(())
    }
}

impl<W: Write> Write for SizeCappedWriter<W> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        if buf.is_empty() || self.capped {
            return Ok(buf.len());
        }

        let content_cap = self.max_bytes.saturating_sub(CAP_REACHED_MARKER.len());
        let remaining = content_cap.saturating_sub(self.bytes_written);
        let accepted = remaining.min(buf.len());
        if accepted > 0 {
            self.inner.write_all(&buf[..accepted])?;
            self.bytes_written += accepted;
        }

        if accepted < buf.len() || self.bytes_written == content_cap {
            self.write_marker()?;
        }

        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        self.inner.flush()
    }
}

#[cfg(test)]
#[path = "size_capped_writer_tests.rs"]
mod tests;
