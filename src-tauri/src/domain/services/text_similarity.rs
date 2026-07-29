//! Shared text similarity utilities.
//!
//! Provides tokenization and Jaccard similarity functions used by both
//! gap fingerprinting (plan verification) and session dedup (external MCP).

use std::collections::HashSet;

const STOP_WORDS: &[&str] = &[
    "a", "an", "the", "is", "are", "was", "were", "be", "been", "has", "have", "had", "do",
    "does", "did", "will", "would", "should", "could", "can", "may", "might", "on", "in", "at",
    "for", "of", "to", "by", "with", "from", "that", "this", "these", "those", "it", "its",
    "and", "but", "or", "so", "yet",
];
// NEVER strip: "no", "not", "missing", "lacks", "without", "absent"

fn stem(word: &str) -> String {
    for (suffix, replacement) in &[
        ("ation", ""),
        ("ating", ""),
        ("ting", ""),
        ("ing", ""),
        ("ment", ""),
        ("ness", ""),
        ("able", ""),
        ("ible", ""),
        ("ed", ""),
        ("er", ""),
        ("es", ""),
        ("s", ""),
    ] {
        if word.len() > suffix.len() + 2 && word.ends_with(suffix) {
            return word[..word.len() - suffix.len()].to_string() + replacement;
        }
    }
    word.to_string()
}

/// Tokenize text for similarity comparison.
///
/// Pipeline:
/// 1. Lowercase + strip non-alphanumeric (keep whitespace)
/// 2. Stop-word removal (preserves negation: "not", "missing", "lacks", etc.)
/// 3. Suffix stemming (12 rules, longest-first)
///
/// Returns a HashSet of stemmed tokens for use with `jaccard_similarity`.
pub fn tokenize_for_similarity(text: &str) -> HashSet<String> {
    // Layer 1: lowercase + strip punctuation
    let cleaned: String = text
        .to_lowercase()
        .chars()
        .filter(|c| c.is_alphanumeric() || c.is_whitespace())
        .collect();

    // Layer 2: stop-word stripping
    let words: Vec<&str> = cleaned
        .split_whitespace()
        .filter(|w| !STOP_WORDS.contains(w))
        .collect();

    // Layer 3: suffix stemming
    words.iter().map(|w| stem(w)).collect()
}

/// Compute Jaccard similarity between two sets of strings.
///
/// Returns 1.0 for empty sets (two empty sets are considered identical).
pub fn jaccard_similarity(set_a: &HashSet<String>, set_b: &HashSet<String>) -> f64 {
    let intersection = set_a.intersection(set_b).count();
    let union = set_a.union(set_b).count();
    if union == 0 {
        return 1.0;
    }
    intersection as f64 / union as f64
}

#[cfg(test)]
#[path = "text_similarity_tests.rs"]
mod tests;
