//! Secret redaction utilities for sanitizing log output.
//!
//! Provides a single [`redact`] function that replaces known secret patterns
//! (API keys, Bearer tokens, environment variables containing credentials) with
//! placeholder text so they never appear in log files or error messages.

use lazy_static::lazy_static;

lazy_static! {
    /// Ordered list of `(pattern, replacement)` pairs applied left-to-right.
    ///
    /// Order is significant: more-specific `sk-ant-` and `sk-or-v1-` prefixes
    /// must appear before the generic `sk-` catch-all so the longer prefix wins.
    static ref SECRET_REGEXES: Vec<(regex::Regex, &'static str)> = vec![
        // 1. Anthropic API key (most specific sk- prefix)
        (
            regex::Regex::new(r"sk-ant-[a-zA-Z0-9_-]{20,}").unwrap(),
            "sk-ant-***REDACTED***",
        ),
        // 2. OpenRouter API key
        (
            regex::Regex::new(r"sk-or-v1-[a-zA-Z0-9]{20,}").unwrap(),
            "sk-or-v1-***REDACTED***",
        ),
        // 3. RalphX live API key
        (
            regex::Regex::new(r"rxk_live_[a-zA-Z0-9]{20,}").unwrap(),
            "rxk_live_***REDACTED***",
        ),
        // 4. Generic OpenAI-style sk- key (catch-all — MUST be after sk-ant- and sk-or-v1-)
        (
            regex::Regex::new(r"sk-[a-zA-Z0-9]{20,}").unwrap(),
            "sk-***REDACTED***",
        ),
        // 5. Bearer token in HTTP Authorization header
        (
            regex::Regex::new(r"Bearer [a-zA-Z0-9_.\-]{20,}").unwrap(),
            "Bearer ***REDACTED***",
        ),
        // 6. ANTHROPIC_AUTH_TOKEN JSON key/value pair
        (
            regex::Regex::new(r#""ANTHROPIC_AUTH_TOKEN"\s*:\s*"[^"]+""#).unwrap(),
            r#""ANTHROPIC_AUTH_TOKEN":"***REDACTED***""#,
        ),
        // 7. ANTHROPIC_API_KEY JSON key/value pair
        (
            regex::Regex::new(r#""ANTHROPIC_API_KEY"\s*:\s*"[^"]+""#).unwrap(),
            r#""ANTHROPIC_API_KEY":"***REDACTED***""#,
        ),
        // 8. GitHub personal access token
        (
            regex::Regex::new(r"ghp_[a-zA-Z0-9]{20,}").unwrap(),
            "ghp_***REDACTED***",
        ),
        // 9. GitHub OAuth token
        (
            regex::Regex::new(r"gho_[a-zA-Z0-9]{20,}").unwrap(),
            "gho_***REDACTED***",
        ),
        // 10. Generic env var with sensitive name in JSON ("MY_API_KEY": "value")
        (
            regex::Regex::new(r#"(?i)"([A-Z_0-9]*(?:KEY|TOKEN|SECRET|PASSWORD|CREDENTIAL)[A-Z_0-9]*)"\s*:\s*"[^"]+""#).unwrap(),
            r#""$1":"***REDACTED***""#,
        ),
        // 11. Generic env var with sensitive name in assignment (MY_API_KEY=value)
        (
            regex::Regex::new(r#"(?i)\b([A-Z_0-9]*(?:KEY|TOKEN|SECRET|PASSWORD|CREDENTIAL)[A-Z_0-9]*)=("[^"]*"|\S+)"#).unwrap(),
            "$1=***REDACTED***",
        ),
    ];
}

/// Replace all known secret patterns in `input` with redacted placeholders.
///
/// Each pattern is applied in order so that longer/more-specific prefixes take
/// priority over shorter catch-all patterns. The original string is returned
/// unchanged (cloned) when no pattern matches.
///
/// # Examples
///
/// ```
/// use ralphx_lib::utils::secret_redactor::redact;
///
/// let safe = redact("key: sk-ant-AAAAAAAAAAAAAAAAAAAAA");
/// assert_eq!(safe, "key: sk-ant-***REDACTED***");
/// ```
pub fn redact(input: &str) -> String {
    // Fast path: if none of the patterns match, avoid allocating a new String.
    let any_match = SECRET_REGEXES.iter().any(|(re, _)| re.is_match(input));
    if !any_match {
        return input.to_owned();
    }

    let mut result = input.to_owned();
    for (re, replacement) in SECRET_REGEXES.iter() {
        result = re.replace_all(&result, *replacement).into_owned();
    }
    result
}

#[cfg(test)]
#[path = "secret_redactor_tests.rs"]
mod tests;
