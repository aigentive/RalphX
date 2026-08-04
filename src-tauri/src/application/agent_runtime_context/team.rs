use chrono::Utc;

use crate::application::chat_service::escape_attr;
use crate::domain::entities::{TeamMember, TeamMemberStatus, TeamSession, TeamSessionStatus};

const MAX_TEAM_STATE_MEMBERS: usize = 20;

pub(super) fn render_team_state(session: &TeamSession, mut members: Vec<TeamMember>) -> String {
    members.retain(|member| member.status != TeamMemberStatus::Stopped);
    members.sort_by(|left, right| {
        left.normalized_name
            .cmp(&right.normalized_name)
            .then_with(|| left.id.as_str().cmp(right.id.as_str()))
    });
    members.truncate(MAX_TEAM_STATE_MEMBERS);

    let mut block = format!(
        "<team_state session_id=\"{}\" status=\"{}\" concurrency=\"{}\" as_of=\"{}\">\n",
        escape_attr(session.id.as_str()),
        team_session_status(session.status),
        session.effective_concurrency,
        Utc::now().to_rfc3339(),
    );
    for member in members {
        block.push_str(&format!(
            "<member name=\"{}\" status=\"{}\" role=\"{}\" current_assignment=\"{}\" last_activity=\"{}\"/>\n",
            escape_attr(&member.name),
            team_member_status(member.status),
            escape_attr(&member.role_summary),
            escape_attr(
                member
                    .current_assignment_id
                    .as_ref()
                    .map_or("", |id| id.as_str()),
            ),
            member
                .last_activity_at
                .map_or_else(|| "unknown".to_string(), |at| at.to_rfc3339()),
        ));
    }
    block.push_str(
        "<runtime_hint>These members are resolved from trusted runtime context. Use team_send_message, team_assign, or team_stop_member with the listed member name.</runtime_hint>\n",
    );
    block.push_str("</team_state>");
    block
}

pub(super) fn render_team_state_unavailable(reason: &str) -> String {
    format!(
        "<team_state state=\"unavailable\" reason=\"{}\"/>",
        escape_attr(reason)
    )
}

fn team_member_status(status: TeamMemberStatus) -> &'static str {
    match status {
        TeamMemberStatus::Provisioning => "provisioning",
        TeamMemberStatus::Idle => "idle",
        TeamMemberStatus::Working => "working",
        TeamMemberStatus::AwaitingInput => "awaiting_input",
        TeamMemberStatus::AwaitingApproval => "awaiting_approval",
        TeamMemberStatus::Stopping => "stopping",
        TeamMemberStatus::Suspended => "suspended",
        TeamMemberStatus::Failed => "failed",
        TeamMemberStatus::Stopped => "stopped",
    }
}

fn team_session_status(status: TeamSessionStatus) -> &'static str {
    match status {
        TeamSessionStatus::Active => "active",
        TeamSessionStatus::Suspending => "suspending",
        TeamSessionStatus::Suspended => "suspended",
        TeamSessionStatus::Draining => "draining",
        TeamSessionStatus::Closed => "closed",
        TeamSessionStatus::Failed => "failed",
    }
}
