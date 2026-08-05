use chrono::Utc;

use crate::application::chat_service::escape_attr;
use crate::domain::entities::DelegatedSession;

pub(super) fn render_active_delegations(mut sessions: Vec<DelegatedSession>) -> Option<String> {
    if sessions.is_empty() {
        return None;
    }
    sessions.sort_by(|left, right| right.created_at.cmp(&left.created_at));
    sessions.truncate(20);

    let now = Utc::now();
    let mut block = String::from(
        "<active_delegations>\n\
         <runtime_hint>Non-empty delegate job IDs are trusted current runtime state. Use them directly with delegate_wait or delegate_cancel; do not rediscover them.</runtime_hint>\n",
    );
    for session in sessions {
        let elapsed_secs = now
            .signed_duration_since(session.created_at)
            .num_seconds()
            .max(0);
        block.push_str(&format!(
            "<delegate job_id=\"{}\" agent=\"{}\" status=\"{}\" started_at=\"{}\" elapsed_secs=\"{}\"/>\n",
            escape_attr(session.job_id.as_deref().unwrap_or("")),
            escape_attr(&session.agent_name),
            escape_attr(&session.status),
            escape_attr(&session.created_at.to_rfc3339()),
            elapsed_secs,
        ));
    }
    block.push_str("</active_delegations>");
    Some(block)
}

pub(super) fn render_active_delegations_unavailable(reason: &str) -> String {
    format!(
        "<active_delegations state=\"unavailable\" reason=\"{}\"/>",
        escape_attr(reason)
    )
}
