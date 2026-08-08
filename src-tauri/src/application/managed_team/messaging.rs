//! Durable Team-message routing, queued delivery projection, and wake claims.

use chrono::Utc;

use crate::domain::entities::{
    normalize_team_member_name, AgentRunId, AgentTaskAssignmentId, ChatConversationId, TeamMember,
    TeamMemberId, TeamMemberStatus, TeamMessage, TeamMessageActorKind, TeamMessageDelivery,
    TeamMessageDeliveryId, TeamMessageDeliveryStatus, TeamMessageEnvelopeTarget, TeamMessageId,
    TeamMessageKind, TeamRunBindingStatus, TeamSessionId, TeamWakeBatch, TeamWakeBatchId,
    TeamWakeBatchStatus,
};
use crate::error::{AppError, AppResult};

use super::lifecycle::new_coordinator_wake_run_binding;
use super::service::ManagedTeamService;

const MESSAGE_CREATE_ATTEMPTS: usize = 4;
const WAKE_BATCH_CLAIM_LIMIT: u32 = 16;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ManagedTeamMessageTarget {
    Coordinator,
    MemberName(String),
    Broadcast,
}

#[derive(Debug, Clone)]
pub enum ManagedTeamMessageSender {
    Coordinator {
        conversation_id: ChatConversationId,
        source_run_id: Option<AgentRunId>,
    },
    Member {
        member_id: TeamMemberId,
        generation: i64,
        source_run_id: AgentRunId,
    },
    /// Backend-originated notifications (e.g. settlement completion signals)
    /// that carry no run authority of their own; validated only against Team
    /// ownership of the referenced assignment.
    System {
        assignment_id: AgentTaskAssignmentId,
    },
}

#[derive(Debug, Clone)]
pub struct ManagedTeamMessageRequest {
    pub team_id: TeamSessionId,
    pub sender: ManagedTeamMessageSender,
    pub target: ManagedTeamMessageTarget,
    pub kind: TeamMessageKind,
    pub content: String,
    pub idempotency_key: String,
}

impl ManagedTeamService {
    /// Creates one immutable Team envelope plus all resolved recipient rows.
    /// Replays of the same transport-owned idempotency key return the original
    /// envelope and re-attempt queue projection instead of creating a new turn.
    pub async fn send_team_message(
        &self,
        request: ManagedTeamMessageRequest,
    ) -> AppResult<(TeamMessage, Vec<TeamMessageDelivery>)> {
        if request.content.trim().is_empty() || request.idempotency_key.trim().is_empty() {
            return Err(AppError::Validation(
                "managed Team message content and idempotency key are required".to_string(),
            ));
        }
        // Check the live fence before creating any delivery effect. A queued
        // message can lead to a new member/coordinator turn, so it is part of
        // dispatch admission rather than a harmless audit write.
        self.admit_dispatch(&request.team_id).await?;
        let session = self.open_session(&request.team_id).await?;
        self.validate_message_sender(&session, &request.sender)
            .await?;

        let existing = self
            .message_repo
            .get_envelope_by_idempotency_key(&session.id, &request.idempotency_key)
            .await?;
        let (message, deliveries) = match existing {
            Some(envelope) => envelope,
            None => self.create_message_envelope(&session.id, &request).await?,
        };

        let queued = self.project_actionable_deliveries(&session.id).await?;
        self.schedule_coordinator_wakes(&queued).await?;
        Ok((message, deliveries))
    }

    /// Replays durable delivery projection only after post-assignment recovery
    /// explicitly releases the startup barrier.
    pub async fn release_delivery_projection_after_recovery(&self) -> AppResult<usize> {
        if !self.startup_barrier().release_delivery_projection().await {
            return Err(AppError::Conflict(
                "managed Team recovery barrier did not settle successfully".to_string(),
            ));
        }
        let mut projected = 0usize;
        for session in self.team_repo.list_open_sessions().await? {
            let queued = self.project_actionable_deliveries(&session.id).await?;
            projected += queued.len();
            self.schedule_coordinator_wakes(&queued).await?;
        }
        Ok(projected)
    }

    /// Claims one durable wake batch without spawning. The preallocated Team
    /// binding is recorded only after the batch CAS succeeds, and before any
    /// later dispatcher can launch the coordinator turn.
    pub async fn claim_next_actionable_wake_batch(
        &self,
        team_id: &TeamSessionId,
    ) -> AppResult<Option<TeamWakeBatch>> {
        if !self.startup_barrier().delivery_projection_released().await {
            return Err(AppError::Conflict(
                "managed Team recovery barrier has not released wake scheduling".to_string(),
            ));
        }
        let session = self.open_session(team_id).await?;
        for batch in self
            .wake_batch_repo
            .list_queued_for_team(team_id, WAKE_BATCH_CLAIM_LIMIT)
            .await?
        {
            let planned_run_id = AgentRunId::new();
            let mut launching = batch.clone();
            launching
                .transition_to(TeamWakeBatchStatus::Launching, Utc::now())
                .map_err(AppError::Validation)?;
            launching.planned_agent_run_id = Some(planned_run_id.clone());
            launching.attempt_count += 1;
            launching.lease_token = Some(TeamWakeBatchId::new().0);
            launching.version += 1;
            if !self
                .wake_batch_repo
                .transition(
                    &batch.id,
                    batch.version,
                    TeamWakeBatchStatus::Queued,
                    launching.clone(),
                )
                .await?
            {
                continue;
            }
            let binding = new_coordinator_wake_run_binding(
                session.id,
                session.coordinator_conversation_id,
                planned_run_id,
                launching.first_message_sequence,
                launching.last_message_sequence,
            );
            if let Err(error) = self.run_binding_repo.create(binding).await {
                let mut failed = launching;
                failed
                    .transition_to(TeamWakeBatchStatus::Failed, Utc::now())
                    .map_err(AppError::Validation)?;
                failed.last_error = Some("wake_run_binding_create_failed".to_string());
                failed.version += 1;
                let _ = self
                    .wake_batch_repo
                    .transition(
                        &failed.id.clone(),
                        failed.version - 1,
                        TeamWakeBatchStatus::Launching,
                        failed,
                    )
                    .await;
                return Err(error);
            }
            return Ok(Some(launching));
        }
        Ok(None)
    }

    pub(super) async fn open_session(
        &self,
        team_id: &TeamSessionId,
    ) -> AppResult<crate::domain::entities::TeamSession> {
        let session = self
            .team_repo
            .get_session(team_id)
            .await?
            .ok_or_else(|| AppError::NotFound("managed Team was not found".to_string()))?;
        if session.status.is_closed() {
            return Err(AppError::Conflict("managed Team is closed".to_string()));
        }
        Ok(session)
    }

    async fn validate_message_sender(
        &self,
        session: &crate::domain::entities::TeamSession,
        sender: &ManagedTeamMessageSender,
    ) -> AppResult<Option<TeamMember>> {
        match sender {
            ManagedTeamMessageSender::Coordinator {
                conversation_id,
                source_run_id,
            } => {
                if session.coordinator_conversation_id != *conversation_id {
                    return Err(AppError::Conflict(
                        "coordinator message cannot cross Team boundaries".to_string(),
                    ));
                }
                if let Some(run_id) = source_run_id {
                    let binding = self
                        .run_binding_repo
                        .get_by_agent_run_id(run_id)
                        .await?
                        .ok_or_else(|| {
                            AppError::Conflict(
                                "coordinator message run has no Team binding".to_string(),
                            )
                        })?;
                    if binding.team_id != session.id
                        || binding.team_member_id.is_some()
                        || binding.conversation_id != *conversation_id
                        || !matches!(
                            binding.status,
                            TeamRunBindingStatus::Planned
                                | TeamRunBindingStatus::Launching
                                | TeamRunBindingStatus::Running
                        )
                    {
                        return Err(AppError::Conflict(
                            "coordinator message run is not current Team authority".to_string(),
                        ));
                    }
                }
                Ok(None)
            }
            ManagedTeamMessageSender::Member {
                member_id,
                generation,
                source_run_id,
            } => {
                let member = self.team_repo.get_member(member_id).await?.ok_or_else(|| {
                    AppError::NotFound("managed Team member was not found".to_string())
                })?;
                let binding = self
                    .run_binding_repo
                    .get_by_agent_run_id(source_run_id)
                    .await?
                    .ok_or_else(|| {
                        AppError::Conflict("member message run has no Team binding".to_string())
                    })?;
                if member.team_id != session.id
                    || !member.is_current_generation(*generation)
                    || !member.current_run_is_authoritative(*generation, source_run_id)
                    || !binding.is_current_member_authority(member_id, *generation)
                    || binding.team_id != session.id
                    || !matches!(
                        binding.status,
                        TeamRunBindingStatus::Planned
                            | TeamRunBindingStatus::Launching
                            | TeamRunBindingStatus::Running
                    )
                {
                    return Err(AppError::Conflict(
                        "member message authority is stale or cross-Team".to_string(),
                    ));
                }
                Ok(Some(member))
            }
            ManagedTeamMessageSender::System { assignment_id } => {
                let belongs_to_team = self
                    .run_binding_repo
                    .list_for_team(&session.id)
                    .await?
                    .iter()
                    .any(|binding| binding.assignment_id.as_ref() == Some(assignment_id));
                if !belongs_to_team {
                    return Err(AppError::Conflict(
                        "system message assignment does not belong to this Team".to_string(),
                    ));
                }
                Ok(None)
            }
        }
    }

    async fn resolve_message_recipients(
        &self,
        team_id: &TeamSessionId,
        target: &ManagedTeamMessageTarget,
    ) -> AppResult<(
        TeamMessageEnvelopeTarget,
        Option<TeamMemberId>,
        Vec<TeamMember>,
    )> {
        let members = self.team_repo.list_members(team_id).await?;
        match target {
            ManagedTeamMessageTarget::Coordinator => {
                Ok((TeamMessageEnvelopeTarget::Coordinator, None, Vec::new()))
            }
            ManagedTeamMessageTarget::MemberName(name) => {
                let normalized = normalize_team_member_name(name).map_err(AppError::Validation)?;
                let member = members
                    .into_iter()
                    .find(|member| {
                        member.normalized_name == normalized && member_can_receive(member)
                    })
                    .ok_or_else(|| {
                        AppError::NotFound("managed Team recipient was not found".to_string())
                    })?;
                Ok((
                    TeamMessageEnvelopeTarget::Member,
                    Some(member.id.clone()),
                    vec![member],
                ))
            }
            ManagedTeamMessageTarget::Broadcast => {
                let recipients: Vec<_> = members.into_iter().filter(member_can_receive).collect();
                Ok((TeamMessageEnvelopeTarget::Broadcast, None, recipients))
            }
        }
    }

    async fn create_message_envelope(
        &self,
        team_id: &TeamSessionId,
        request: &ManagedTeamMessageRequest,
    ) -> AppResult<(TeamMessage, Vec<TeamMessageDelivery>)> {
        for _ in 0..MESSAGE_CREATE_ATTEMPTS {
            if let Some(envelope) = self
                .message_repo
                .get_envelope_by_idempotency_key(team_id, &request.idempotency_key)
                .await?
            {
                return Ok(envelope);
            }
            let session = self.open_session(team_id).await?;
            let sender_member = self
                .validate_message_sender(&session, &request.sender)
                .await?;
            let (target_kind, target_member_id, recipients) = self
                .resolve_message_recipients(team_id, &request.target)
                .await?;
            if recipients.is_empty() && target_kind != TeamMessageEnvelopeTarget::Coordinator {
                return Err(AppError::Conflict(
                    "managed Team message has no current recipients".to_string(),
                ));
            }
            let message = TeamMessage {
                id: TeamMessageId::new(),
                team_id: team_id.clone(),
                sequence: self.message_repo.next_message_sequence(team_id).await?,
                sender_kind: match &request.sender {
                    ManagedTeamMessageSender::Coordinator { .. } => {
                        TeamMessageActorKind::Coordinator
                    }
                    ManagedTeamMessageSender::Member { .. } => TeamMessageActorKind::Member,
                    ManagedTeamMessageSender::System { .. } => TeamMessageActorKind::System,
                },
                sender_member_id: sender_member.as_ref().map(|member| member.id.clone()),
                target_kind,
                target_member_id,
                kind: request.kind,
                content: request.content.trim().to_string(),
                source_run_id: sender_source_run_id(&request.sender),
                assignment_id: sender_assignment_id(&request.sender),
                idempotency_key: request.idempotency_key.clone(),
                created_at: Utc::now(),
            };
            let deliveries = message_deliveries(&message, recipients);
            match self
                .message_repo
                .create_envelope_with_deliveries(message, deliveries)
                .await
            {
                Ok(envelope) => return Ok(envelope),
                Err(AppError::Conflict(_)) => continue,
                Err(error) => return Err(error),
            }
        }
        self.message_repo
            .get_envelope_by_idempotency_key(team_id, &request.idempotency_key)
            .await?
            .ok_or_else(|| {
                AppError::Conflict(
                    "managed Team message sequence contention exceeded retry budget".to_string(),
                )
            })
    }
}

pub(super) fn member_can_receive(member: &TeamMember) -> bool {
    matches!(
        member.status,
        TeamMemberStatus::Idle
            | TeamMemberStatus::Working
            | TeamMemberStatus::AwaitingInput
            | TeamMemberStatus::AwaitingApproval
    )
}

fn sender_source_run_id(sender: &ManagedTeamMessageSender) -> Option<AgentRunId> {
    match sender {
        ManagedTeamMessageSender::Coordinator { source_run_id, .. } => source_run_id.clone(),
        ManagedTeamMessageSender::Member { source_run_id, .. } => Some(source_run_id.clone()),
        ManagedTeamMessageSender::System { .. } => None,
    }
}

fn sender_assignment_id(sender: &ManagedTeamMessageSender) -> Option<AgentTaskAssignmentId> {
    match sender {
        ManagedTeamMessageSender::System { assignment_id } => Some(assignment_id.clone()),
        ManagedTeamMessageSender::Coordinator { .. } | ManagedTeamMessageSender::Member { .. } => {
            None
        }
    }
}

fn message_deliveries(
    message: &TeamMessage,
    recipients: Vec<TeamMember>,
) -> Vec<TeamMessageDelivery> {
    let now = Utc::now();
    match message.target_kind {
        TeamMessageEnvelopeTarget::Coordinator => vec![TeamMessageDelivery {
            id: TeamMessageDeliveryId::new(),
            message_id: message.id.clone(),
            recipient_kind: TeamMessageActorKind::Coordinator,
            recipient_member_id: None,
            recipient_generation: None,
            status: TeamMessageDeliveryStatus::Pending,
            queued_message_id: None,
            attempt_count: 0,
            next_retry_at: None,
            last_error: None,
            created_at: now,
            updated_at: now,
        }],
        TeamMessageEnvelopeTarget::Member | TeamMessageEnvelopeTarget::Broadcast => recipients
            .into_iter()
            .map(|member| TeamMessageDelivery {
                id: TeamMessageDeliveryId::new(),
                message_id: message.id.clone(),
                recipient_kind: TeamMessageActorKind::Member,
                recipient_member_id: Some(member.id),
                recipient_generation: Some(member.generation),
                status: TeamMessageDeliveryStatus::Pending,
                queued_message_id: None,
                attempt_count: 0,
                next_retry_at: None,
                last_error: None,
                created_at: now,
                updated_at: now,
            })
            .collect(),
    }
}
