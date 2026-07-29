use std::io::{self, Write};
use std::sync::{Arc, Mutex};

use tracing_subscriber::{fmt, fmt::MakeWriter, prelude::*, Registry};

use crate::utils::redacting_writer::{RedactingMakeWriter, RedactingWriter};

/// A `MakeWriter` backed by a shared `Arc<Mutex<Vec<u8>>>` whose `Writer` type
/// does NOT borrow from `&self`, so it satisfies `for<'w> MakeWriter<'w>`.
#[derive(Clone)]
struct SharedBuf(Arc<Mutex<Vec<u8>>>);

impl io::Write for SharedBuf {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(buf);
        Ok(buf.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl<'a> MakeWriter<'a> for SharedBuf {
    type Writer = SharedBuf;
    fn make_writer(&'a self) -> SharedBuf {
        self.clone()
    }
}

/// Build a `RedactingWriter` wrapping a mutable `Vec<u8>` reference so we can
/// inspect the bytes after the writer is dropped.
fn make_writer(buf: &mut Vec<u8>) -> RedactingWriter<&mut Vec<u8>> {
    RedactingWriter::new(buf)
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[test]
fn single_complete_line() {
    let mut buf = Vec::new();
    {
        let mut w = make_writer(&mut buf);
        w.write_all(b"hello\n").unwrap();
        w.flush().unwrap();
    }
    assert_eq!(String::from_utf8(buf).unwrap(), "hello\n");
}

#[test]
fn single_secret_line() {
    let mut buf = Vec::new();
    {
        let mut w = make_writer(&mut buf);
        w.write_all(b"token: sk-ant-AAAAAAAAAAAAAAAAAAAAA\n").unwrap();
        w.flush().unwrap();
    }
    let out = String::from_utf8(buf).unwrap();
    assert!(
        out.contains("***REDACTED***"),
        "expected redaction in: {out:?}"
    );
    assert!(
        !out.contains("sk-ant-AAAAAAAAAAAAAAAAAAAAA"),
        "raw secret must not appear in: {out:?}"
    );
}

#[test]
fn multiple_lines_in_one_write() {
    let mut buf = Vec::new();
    {
        let mut w = make_writer(&mut buf);
        w.write_all(b"line1\nline2\n").unwrap();
        w.flush().unwrap();
    }
    let out = String::from_utf8(buf).unwrap();
    assert!(out.contains("line1\n"), "line1 missing from: {out:?}");
    assert!(out.contains("line2\n"), "line2 missing from: {out:?}");
}

#[test]
fn partial_line_accumulated() {
    // Use a Vec wrapped in a cell so we can observe it while the writer is live.
    // Simpler approach: use two separate scopes.
    let mut buf = Vec::new();
    {
        let mut w = make_writer(&mut buf);
        // First write: no newline — should not flush yet.
        let n1 = w.write(b"hel").unwrap();
        assert_eq!(n1, 3, "write should accept all bytes");
        // Second write: completes the line.
        w.write_all(b"lo\n").unwrap();
        w.flush().unwrap();
        // Verify inner writer is non-empty after the newline triggered the flush.
        // We can't inspect buf while w borrows it, so we rely on the post-drop check.
    }
    // After drop, buf should contain the complete line.
    assert_eq!(String::from_utf8(buf).unwrap(), "hello\n");
}

/// A secret split across two write() calls is still redacted because both
/// halves are assembled in the line buffer before the complete line is flushed.
#[test]
fn partial_line_secret_split_across_writes() {
    // Use an Anthropic key long enough to be caught by the real redactor (>=20 chars after prefix).
    let secret = "sk-ant-api03-XXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXX";
    let first_half = &secret[..secret.len() / 2];
    let second_half = &secret[secret.len() / 2..];

    let mut buf = Vec::new();
    {
        let mut w = make_writer(&mut buf);
        w.write_all(first_half.as_bytes()).unwrap();
        let full_line = format!("{second_half}\n");
        w.write_all(full_line.as_bytes()).unwrap();
        w.flush().unwrap();
    }
    let out = String::from_utf8(buf).unwrap();
    assert!(
        out.contains("***REDACTED***"),
        "split secret must be redacted, got: {out:?}"
    );
}

#[test]
fn flush_without_newline() {
    let mut buf = Vec::new();
    {
        let mut w = make_writer(&mut buf);
        w.write_all(b"partial").unwrap();
        // Flush explicitly — the content must reach the inner writer even without a newline.
        w.flush().unwrap();
    }
    assert_eq!(String::from_utf8(buf).unwrap(), "partial");
}

/// Two secrets on a single line — both must be redacted.
#[test]
fn multi_line_secret_on_one_line() {
    // Each secret has 20+ alphanum chars after the "sk-ant-" prefix to satisfy the regex.
    let line = "a=sk-ant-AAAAAAAAAAAAAAAAAAAA b=sk-ant-BBBBBBBBBBBBBBBBBBBB\n";
    let mut buf = Vec::new();
    {
        let mut w = make_writer(&mut buf);
        w.write_all(line.as_bytes()).unwrap();
        w.flush().unwrap();
    }
    let out = String::from_utf8(buf).unwrap();
    assert!(
        !out.contains("sk-ant-AAAAAAAAAAAAAAAAAAAA"),
        "first secret must not appear in: {out:?}"
    );
    assert!(
        !out.contains("sk-ant-BBBBBBBBBBBBBBBBBBBB"),
        "second secret must not appear in: {out:?}"
    );
}

#[test]
fn empty_write() {
    let mut buf = Vec::new();
    {
        let mut w = make_writer(&mut buf);
        let n = w.write(b"").unwrap();
        assert_eq!(n, 0, "empty write must return Ok(0)");
        w.flush().unwrap();
    }
    assert!(buf.is_empty(), "inner must be unchanged after empty write");
}

#[test]
fn drop_flushes_buffer() {
    let mut buf = Vec::new();
    {
        let mut w = make_writer(&mut buf);
        w.write_all(b"no newline here").unwrap();
        // Drop without explicit flush — Drop impl must flush the buffer.
    }
    assert_eq!(
        String::from_utf8(buf).unwrap(),
        "no newline here",
        "Drop must flush remaining buffer"
    );
}

// ─── Subscriber integration tests ────────────────────────────────────────────

/// Secret logged via a real `tracing` subscriber that uses `RedactingMakeWriter`
/// must be redacted in the output buffer.
#[test]
fn subscriber_integration_redacts_secrets() {
    let buf = SharedBuf(Arc::new(Mutex::new(Vec::new())));
    let writer = RedactingMakeWriter::new(buf.clone());

    let subscriber = Registry::default()
        .with(fmt::layer().with_writer(writer).with_ansi(false));

    tracing::subscriber::with_default(subscriber, || {
        tracing::info!("ANTHROPIC_AUTH_TOKEN: sk-ant-AAAAAAAAAAAAAAAAAAAAA");
    });

    let output = String::from_utf8(buf.0.lock().unwrap().clone()).unwrap();
    assert!(
        output.contains("***REDACTED***"),
        "expected redaction in subscriber output, got: {output:?}"
    );
    assert!(
        !output.contains("sk-ant-AAAAAAAAAAAAAAAAAAAAA"),
        "raw secret must not appear in subscriber output: {output:?}"
    );
}

/// Non-secret content logged through the subscriber must pass through unchanged.
#[test]
fn subscriber_integration_non_secret_unchanged() {
    let buf = SharedBuf(Arc::new(Mutex::new(Vec::new())));
    let writer = RedactingMakeWriter::new(buf.clone());

    let subscriber = Registry::default()
        .with(fmt::layer().with_writer(writer).with_ansi(false));

    tracing::subscriber::with_default(subscriber, || {
        tracing::info!("Hello, world!");
    });

    let output = String::from_utf8(buf.0.lock().unwrap().clone()).unwrap();
    assert!(
        output.contains("Hello, world!"),
        "non-secret content must be preserved in subscriber output, got: {output:?}"
    );
    assert!(
        !output.contains("***REDACTED***"),
        "non-secret content must not be redacted, got: {output:?}"
    );
}
