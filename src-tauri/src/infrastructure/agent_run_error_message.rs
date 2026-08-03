// Shared bound for persisted agent-run terminal causes.
// Both the SQLite and memory `AgentRunRepository::fail` implementations apply it so a failed run
// records an equivalent cause regardless of which repository backs the run.

/// Upper bound for `agent_runs.error_message`. Terminal causes are diagnostics,
/// not transcripts — a real incident stored 124KB of successful tool output here.
const MAX_PERSISTED_ERROR_MESSAGE_BYTES: usize = 8 * 1024;

/// Bound a terminal cause before it reaches `agent_runs.error_message`.
/// Keeps the tail, where the terminal detail usually is.
pub(crate) fn truncate_persisted_error_message(error_message: &str) -> String {
    if error_message.len() <= MAX_PERSISTED_ERROR_MESSAGE_BYTES {
        return error_message.to_string();
    }

    let mut tail_start = error_message.len() - MAX_PERSISTED_ERROR_MESSAGE_BYTES;
    while tail_start < error_message.len() && !error_message.is_char_boundary(tail_start) {
        tail_start += 1;
    }
    format!(
        "... {tail_start} bytes elided ...\n{}",
        &error_message[tail_start..]
    )
}
