// Pure crypto functions for API key management.
// No infrastructure dependencies — safe to use from any layer.

/// Number of random alphanumeric characters in every raw credential this module mints.
///
/// 32 characters drawn from a 62-symbol alphabet is ~190 bits of entropy, which is why
/// [`hash_key`]'s unsalted SHA-256 is sufficient for credentials minted here and no KDF is
/// needed (remote-multi-env §4.1).
pub const RAW_KEY_RANDOM_CHARS: usize = 32;

/// Generate a new raw API key in the format: rxk_live_{32 random alphanumeric chars}
pub fn generate_raw_key() -> String {
    generate_prefixed_key("rxk_live_")
}

/// Generate a raw credential as `{prefix}{32 random alphanumeric chars}`.
///
/// Shared generator behind the :3848 API keys (`rxk_live_`) and the :3849 remote-access
/// credentials — device tokens (`rxd_live_`), pairing codes (`rxp_`), and WS tickets
/// (`rxt_`). Only the prefix differs so credentials stay visually distinguishable and
/// greppable while the entropy source stays in one place.
pub fn generate_prefixed_key(prefix: &str) -> String {
    use rand::Rng;
    const CHARSET: &[u8] = b"abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789";
    let mut rng = rand::thread_rng();
    let random_part: String = (0..RAW_KEY_RANDOM_CHARS)
        .map(|_| {
            let idx = rng.gen_range(0..CHARSET.len());
            CHARSET[idx] as char
        })
        .collect();
    format!("{prefix}{random_part}")
}

/// SHA-256 hash a raw key for storage (only hash is stored, never raw key)
pub fn hash_key(raw_key: &str) -> String {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(raw_key.as_bytes());
    format!("{:x}", hasher.finalize())
}

/// Extract the display prefix from a raw key (first 12 chars, e.g. "rxk_live_a3f2")
pub fn key_prefix(raw_key: &str) -> String {
    raw_key.chars().take(12).collect()
}
