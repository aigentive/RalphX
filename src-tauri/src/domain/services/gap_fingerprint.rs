//! 4-layer normalization pipeline for gap description fingerprinting.
//! Used to detect duplicate/convergent gaps across verification rounds.
use sha2::{Digest, Sha256};

use super::text_similarity::tokenize_for_similarity;
pub use super::text_similarity::jaccard_similarity;

/// Compute a 12-character hex fingerprint for a gap description.
///
/// Pipeline:
/// 1. Lowercase + strip non-alphanumeric (keep whitespace)
/// 2. Stop-word removal (preserves negation words like "not", "missing")
/// 3. Suffix stemming (12 rules, longest-first)
/// 4. Sort + SHA-256 (first 12 hex chars)
pub fn gap_fingerprint(description: &str) -> String {
    let mut tokens: Vec<String> = tokenize_for_similarity(description).into_iter().collect();

    // Layer 4: sort + sha256 (first 12 hex chars)
    tokens.sort();
    let joined = tokens.join(" ");
    let mut hasher = Sha256::new();
    hasher.update(joined.as_bytes());
    let hash = hasher.finalize();
    format!("{:x}", hash)[..12].to_string()
}

/// Compute aggregate gap score: critical*10 + high*3 + medium*1
pub fn gap_score(gaps: &[crate::domain::entities::ideation::VerificationGap]) -> u32 {
    gaps.iter().fold(0u32, |acc, g| {
        acc + match g.severity.as_str() {
            "critical" => 10,
            "high" => 3,
            "medium" => 1,
            _ => 0,
        }
    })
}

#[cfg(test)]
#[path = "gap_fingerprint_tests.rs"]
mod tests;
