use std::io::Write;

use crate::utils::size_capped_writer::{SizeCappedWriter, CAP_REACHED_MARKER};

#[test]
fn stops_at_the_configured_cap_and_emits_one_marker() {
    let mut bytes = Vec::new();
    {
        let mut writer = SizeCappedWriter::new(&mut bytes, (CAP_REACHED_MARKER.len() + 4) as u64);
        assert_eq!(writer.write(b"abcd").expect("first write"), 4);
        assert_eq!(writer.write(b"efgh").expect("capped write"), 4);
        assert_eq!(writer.write(b"ijkl").expect("suppressed write"), 4);
        writer.flush().expect("flush");
    }

    assert_eq!(bytes, [b"abcd".as_slice(), CAP_REACHED_MARKER].concat());
}

#[test]
fn never_exceeds_a_tiny_configured_cap() {
    let mut bytes = Vec::new();
    {
        let mut writer = SizeCappedWriter::new(&mut bytes, 5);
        writer.write_all(b"this is discarded").expect("write");
    }

    assert_eq!(bytes.len(), 5);
}
