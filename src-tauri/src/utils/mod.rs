// Shared utility functions used across layers.

pub mod secret_redactor;
pub mod redacting_writer;
pub mod path_safety;
pub mod runtime_log_paths;
pub mod backend_endpoint;

/// Truncate a UTF-8 string to at most `max_bytes` bytes, respecting char boundaries.
///
/// Unlike `&s[..n]`, this never panics on multi-byte characters.
pub fn truncate_str(s: &str, max_bytes: usize) -> &str {
    if s.len() <= max_bytes {
        return s;
    }
    let mut end = max_bytes;
    while !s.is_char_boundary(end) && end > 0 {
        end -= 1;
    }
    &s[..end]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ascii_shorter_than_limit_returns_unchanged() {
        assert_eq!(truncate_str("hello", 10), "hello");
    }

    #[test]
    fn ascii_longer_than_limit_truncates() {
        assert_eq!(truncate_str("hello world", 5), "hello");
    }

    #[test]
    fn multibyte_truncates_at_char_boundary() {
        // "é" is 2 bytes (0xC3 0xA9). Truncating at 1 byte must step back to 0.
        let s = "é world";
        assert_eq!(truncate_str(s, 1), "");
        assert_eq!(truncate_str(s, 2), "é");
    }

    #[test]
    fn zero_max_bytes_returns_empty() {
        assert_eq!(truncate_str("hello", 0), "");
    }
}
