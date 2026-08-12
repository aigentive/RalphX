//! Wire markers that identify a verification-result chat message.
//!
//! Both the application-layer reconciliation handoff (which writes them) and the
//! chat-message repositories (which detect them when filtering history) need the
//! exact same literals, so they live in the domain rather than in either side.

/// XML tag marker used to detect an already-injected verification-result message.
/// Used for legacy content-based dedup and agent-facing queued handoff payloads.
pub const VERIFICATION_RESULT_MARKER: &str = "<verification-result>";

/// Metadata key stamped on a verification-result chat message. Preferred over
/// [`VERIFICATION_RESULT_MARKER`] for dedup; the marker remains for legacy rows.
pub const VERIFICATION_RESULT_METADATA_KEY: &str = "verification_result";
