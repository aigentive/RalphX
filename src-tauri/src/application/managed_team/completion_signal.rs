//! Coordinator-facing completion signal emitted after Team assignment settlement.

use crate::application::managed_team::{
    ManagedTeamMessageRequest, ManagedTeamMessageSender, ManagedTeamMessageTarget,
    ManagedTeamService,
};
use crate::domain::entities::{
    AgentTaskAssignmentTerminalStatus, AgentTaskAssignmentView, TeamMember, TeamMessageKind,
};

impl ManagedTeamService {
    /// Notifies the Team coordinator that an exact assignment has reached a
    /// terminal status, so a woken coordinator observes settled state instead
    /// of relying on polling. The idempotency key is deterministic on
    /// `(assignment_id, terminal_status)`: a replayed settlement (e.g. from
    /// recovery) returns the original envelope instead of a second wake.
    ///
    /// Non-fatal by design: settlement authority (`settle_member_assignment`)
    /// must never depend on this notification succeeding. A refused send from
    /// `admit_dispatch` on a fenced/closing session is an expected skip, not
    /// an error.
    pub(super) async fn notify_coordinator_assignment_settled(
        &self,
        assignment: &AgentTaskAssignmentView,
        member: &TeamMember,
        terminal_status: AgentTaskAssignmentTerminalStatus,
        reason: Option<&str>,
    ) {
        let Some(team_id) = assignment.assignment.team_id.clone() else {
            tracing::warn!(
                assignment_id = assignment.assignment.id.as_str(),
                "settlement completion signal skipped: assignment has no Team identity"
            );
            return;
        };
        let content = format!(
            "Member '{}' finished task #{} ({}) with status {:?}{}",
            member.name,
            assignment.task.task_number,
            assignment.task.title,
            terminal_status,
            reason
                .map(|reason| format!(": {reason}"))
                .unwrap_or_default(),
        );
        let kind = match terminal_status {
            AgentTaskAssignmentTerminalStatus::Completed => TeamMessageKind::Result,
            AgentTaskAssignmentTerminalStatus::Failed
            | AgentTaskAssignmentTerminalStatus::Cancelled => TeamMessageKind::Status,
        };
        let idempotency_key = format!(
            "team_assignment_settled:{}:{:?}",
            assignment.assignment.id, terminal_status
        );
        if let Err(error) = self
            .send_team_message(ManagedTeamMessageRequest {
                team_id,
                sender: ManagedTeamMessageSender::System {
                    assignment_id: assignment.assignment.id.clone(),
                },
                target: ManagedTeamMessageTarget::Coordinator,
                kind,
                content,
                idempotency_key,
            })
            .await
        {
            tracing::warn!(
                %error,
                assignment_id = assignment.assignment.id.as_str(),
                member_id = member.id.as_str(),
                "settlement completion signal to coordinator failed; settlement itself is unaffected"
            );
        }
    }
}
