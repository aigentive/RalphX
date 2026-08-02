use super::*;

use crate::domain::entities::{
    AgentRunId, AgentWorkspaceRepairAttempt, AgentWorkspaceRepairAttemptId,
    AgentWorkspaceRepairEffect, AgentWorkspaceRepairEffectStatus,
};
use crate::domain::repositories::{
    AgentWorkspaceRepairAttemptTransition, AgentWorkspaceRepairAttemptTransitionOutcome,
    AgentWorkspaceRepairCompatibilityProjection, AgentWorkspaceRepairRepository,
    BindAgentWorkspaceRepairAttemptRun, CompleteAgentWorkspaceRepairEffect,
    CompleteAgentWorkspaceRepairEffectOutcome, CreateAgentWorkspaceRepairEffect,
    CreateAgentWorkspaceRepairEffectOutcome, ImportLegacyAgentWorkspaceRepairAttempt,
    SettleAgentWorkspaceRepairAttempt, SettleAgentWorkspaceRepairAttemptOutcome,
    SettleAndStartAgentWorkspaceRepairSuccessor,
    SettleAndStartAgentWorkspaceRepairSuccessorOutcome, StartOrJoinAgentWorkspaceRepairAttempt,
    StartOrJoinAgentWorkspaceRepairAttemptOutcome,
};

fn validate_repair_events(
    conversation_id: &ChatConversationId,
    events: &[AgentConversationWorkspacePublicationEvent],
) -> AppResult<()> {
    if events
        .iter()
        .any(|event| event.conversation_id != *conversation_id)
    {
        return Err(AppError::Validation(
            "repair attempt events must belong to the repaired workspace".to_string(),
        ));
    }
    Ok(())
}

#[cfg(test)]
fn fail_for_forced_repair_event(
    repo: &MemoryAgentConversationWorkspaceRepository,
    events: &[AgentConversationWorkspacePublicationEvent],
) -> AppResult<()> {
    if events.is_empty() {
        return Ok(());
    }
    if let Some(message) = repo.next_publication_event_error.lock().unwrap().take() {
        return Err(AppError::Infrastructure(message));
    }
    let mut matching_error = repo.matching_publication_event_error.lock().unwrap();
    if let Some((_, _, message)) = matching_error.as_ref().filter(|(step, status, _)| {
        events
            .iter()
            .any(|event| event.step == *step && event.status == *status)
    }) {
        let message = message.clone();
        matching_error.take();
        return Err(AppError::Infrastructure(message));
    }
    Ok(())
}

fn apply_compatibility_projection(
    workspace: &mut AgentConversationWorkspace,
    projection: Option<&AgentWorkspaceRepairCompatibilityProjection>,
    updated_at: DateTime<Utc>,
) {
    let Some(projection) = projection else {
        return;
    };

    workspace.publication_push_status = projection.publication_push_status.clone();
    workspace.pr_supervision_status = projection.pr_supervision_status.clone();
    workspace.pr_supervision_summary = projection.pr_supervision_summary.clone();
    workspace.pr_supervision_updated_at = projection.pr_supervision_updated_at;
    workspace.pr_auto_merge_current = projection.pr_auto_merge_current;
    if let Some(pr_autofix_enabled) = projection.pr_autofix_enabled {
        workspace.pr_autofix_enabled = pr_autofix_enabled;
    }
    if let Some(pr_auto_merge_desired) = projection.pr_auto_merge_desired {
        workspace.pr_auto_merge_desired = pr_auto_merge_desired;
    }
    workspace.base_commit = projection.base_commit.clone();
    workspace.updated_at = updated_at;
}

fn append_repair_events(
    events_by_conversation: &mut HashMap<
        ChatConversationId,
        Vec<AgentConversationWorkspacePublicationEvent>,
    >,
    events: Vec<AgentConversationWorkspacePublicationEvent>,
) {
    for event in events {
        events_by_conversation
            .entry(event.conversation_id.clone())
            .or_default()
            .push(event);
    }
}

fn next_repair_generation(
    attempts: &HashMap<AgentWorkspaceRepairAttemptId, AgentWorkspaceRepairAttempt>,
    conversation_id: &ChatConversationId,
) -> AppResult<u64> {
    attempts
        .values()
        .filter(|attempt| attempt.conversation_id == *conversation_id)
        .map(|attempt| attempt.generation)
        .max()
        .unwrap_or_default()
        .checked_add(1)
        .ok_or_else(|| AppError::Validation("repair attempt generation overflow".to_string()))
}

fn validate_effect_shape(effect: &AgentWorkspaceRepairEffect) -> AppResult<()> {
    if effect.expected_remote_absent && effect.expected_remote_oid.is_some() {
        return Err(AppError::Validation(
            "repair effects cannot expect both a missing and concrete remote ref".to_string(),
        ));
    }
    match effect.status {
        AgentWorkspaceRepairEffectStatus::Observed => {
            let completed_at = effect.completed_at.ok_or_else(|| {
                AppError::Validation(
                    "observed repair effects require a completion time".to_string(),
                )
            })?;
            effect
                .can_complete_observed(effect.receipt_json.as_deref(), completed_at)
                .map_err(AppError::Validation)
        }
        AgentWorkspaceRepairEffectStatus::Pending
        | AgentWorkspaceRepairEffectStatus::InFlight
        | AgentWorkspaceRepairEffectStatus::Failed => {
            if effect.completed_at.is_some() {
                return Err(AppError::Validation(
                    "only observed repair effects may have a completion time".to_string(),
                ));
            }
            Ok(())
        }
    }
}

#[async_trait]
impl AgentWorkspaceRepairRepository for MemoryAgentConversationWorkspaceRepository {
    async fn get_current_repair_attempt(
        &self,
        conversation_id: &ChatConversationId,
    ) -> AppResult<Option<AgentWorkspaceRepairAttempt>> {
        Ok(self
            .repair_attempts
            .read()
            .await
            .values()
            .find(|attempt| {
                attempt.conversation_id == *conversation_id && attempt.settled_at.is_none()
            })
            .cloned())
    }

    async fn get_latest_repair_attempt_for_conversation(
        &self,
        conversation_id: &ChatConversationId,
    ) -> AppResult<Option<AgentWorkspaceRepairAttempt>> {
        Ok(self
            .repair_attempts
            .read()
            .await
            .values()
            .filter(|attempt| attempt.conversation_id == *conversation_id)
            .max_by_key(|attempt| attempt.generation)
            .cloned())
    }

    async fn get_repair_attempt(
        &self,
        attempt_id: &AgentWorkspaceRepairAttemptId,
    ) -> AppResult<Option<AgentWorkspaceRepairAttempt>> {
        Ok(self.repair_attempts.read().await.get(attempt_id).cloned())
    }

    async fn get_repair_attempt_for_run(
        &self,
        conversation_id: &ChatConversationId,
        run_id: &AgentRunId,
    ) -> AppResult<Option<AgentWorkspaceRepairAttempt>> {
        Ok(self
            .repair_attempts
            .read()
            .await
            .values()
            .find(|attempt| {
                attempt.conversation_id == *conversation_id
                    && attempt.reserved_agent_run_id.as_ref() == Some(run_id)
            })
            .cloned())
    }

    async fn list_repair_attempts_for_conversation(
        &self,
        conversation_id: &ChatConversationId,
    ) -> AppResult<Vec<AgentWorkspaceRepairAttempt>> {
        let mut attempts = self
            .repair_attempts
            .read()
            .await
            .values()
            .filter(|attempt| &attempt.conversation_id == conversation_id)
            .cloned()
            .collect::<Vec<_>>();
        attempts.sort_by(|left, right| left.generation.cmp(&right.generation));
        Ok(attempts)
    }

    async fn list_recoverable_repair_attempts(
        &self,
    ) -> AppResult<Vec<AgentWorkspaceRepairAttempt>> {
        let mut attempts = self
            .repair_attempts
            .read()
            .await
            .values()
            .filter(|attempt| attempt.settled_at.is_none())
            .cloned()
            .collect::<Vec<_>>();
        attempts.sort_by(|left, right| left.updated_at.cmp(&right.updated_at));
        Ok(attempts)
    }

    async fn start_or_join_repair_attempt(
        &self,
        request: StartOrJoinAgentWorkspaceRepairAttempt,
    ) -> AppResult<StartOrJoinAgentWorkspaceRepairAttemptOutcome> {
        validate_repair_events(&request.attempt.conversation_id, &request.events)?;
        #[cfg(test)]
        fail_for_forced_repair_event(self, &request.events)?;
        let conversation_id = request.attempt.conversation_id.clone();
        let mut attempts = self.repair_attempts.write().await;
        let mut workspaces = self.workspaces.write().await;
        let mut events_by_conversation = self.publication_events.write().await;
        let workspace = workspaces.get_mut(&conversation_id).ok_or_else(|| {
            AppError::NotFound(format!("workspace {} for repair attempt", conversation_id))
        })?;

        if let Some(current_id) = attempts
            .values()
            .find(|attempt| {
                attempt.conversation_id == conversation_id && attempt.settled_at.is_none()
            })
            .map(|attempt| attempt.id.clone())
        {
            let current = attempts
                .get_mut(&current_id)
                .expect("current repair attempt id came from the same map");
            if current.target_base_ref != request.attempt.target_base_ref {
                return Ok(
                    StartOrJoinAgentWorkspaceRepairAttemptOutcome::BlockedByCurrent(
                        current.clone(),
                    ),
                );
            }
            if !request.reason.trim().is_empty()
                && !current.pending_reasons.contains(&request.reason)
            {
                current.pending_reasons.push(request.reason);
            }
            if request.attempt.continuation.priority() > current.continuation.priority() {
                current.continuation = request.attempt.continuation;
            }
            if request.verified_newer_base
                && request.attempt.target_base_commit.is_some()
                && request.attempt.target_base_commit != current.target_base_commit
            {
                current.target_base_commit = request.attempt.target_base_commit.clone();
            }
            current.auto_publish_enabled = request.attempt.auto_publish_enabled;
            current.auto_merge_desired = request.attempt.auto_merge_desired;
            current.auto_merge_method = request.attempt.auto_merge_method.clone();
            current.updated_at = request.attempt.updated_at;
            return Ok(StartOrJoinAgentWorkspaceRepairAttemptOutcome::Joined(
                current.clone(),
            ));
        }

        let mut started = request.attempt;
        started.generation = next_repair_generation(&attempts, &conversation_id)?;
        if !request.reason.trim().is_empty() && !started.pending_reasons.contains(&request.reason) {
            started.pending_reasons.push(request.reason);
        }
        apply_compatibility_projection(
            workspace,
            request.compatibility_projection.as_ref(),
            started.updated_at,
        );
        append_repair_events(&mut events_by_conversation, request.events);
        attempts.insert(started.id.clone(), started.clone());
        Ok(StartOrJoinAgentWorkspaceRepairAttemptOutcome::Started(
            started,
        ))
    }

    async fn bind_repair_attempt_run(
        &self,
        request: BindAgentWorkspaceRepairAttemptRun,
    ) -> AppResult<AgentWorkspaceRepairAttemptTransitionOutcome> {
        let mut attempts = self.repair_attempts.write().await;
        let Some(current_snapshot) = attempts.get(&request.attempt_id).cloned() else {
            return Ok(AgentWorkspaceRepairAttemptTransitionOutcome::Missing);
        };
        if current_snapshot.settled_at.is_some()
            || current_snapshot.generation != request.generation
            || current_snapshot.phase != request.expected_phase
            || current_snapshot.updated_at != request.expected_updated_at
        {
            return Ok(AgentWorkspaceRepairAttemptTransitionOutcome::Stale(
                current_snapshot,
            ));
        }
        if current_snapshot.reserved_agent_run_id.as_ref() == Some(&request.run_id) {
            return Ok(AgentWorkspaceRepairAttemptTransitionOutcome::Applied(
                current_snapshot,
            ));
        }
        if current_snapshot.reserved_agent_run_id.is_some()
            || attempts.values().any(|attempt| {
                attempt.id != current_snapshot.id
                    && attempt.reserved_agent_run_id.as_ref() == Some(&request.run_id)
            })
        {
            return Ok(AgentWorkspaceRepairAttemptTransitionOutcome::Stale(
                current_snapshot,
            ));
        }
        let current = attempts
            .get_mut(&request.attempt_id)
            .expect("repair attempt existed while the write lock was held");
        current.reserved_agent_run_id = Some(request.run_id);
        current.updated_at = request.updated_at;
        Ok(AgentWorkspaceRepairAttemptTransitionOutcome::Applied(
            current.clone(),
        ))
    }

    async fn transition_repair_attempt(
        &self,
        request: AgentWorkspaceRepairAttemptTransition,
    ) -> AppResult<AgentWorkspaceRepairAttemptTransitionOutcome> {
        validate_repair_events(&request.attempt.conversation_id, &request.events)?;
        #[cfg(test)]
        fail_for_forced_repair_event(self, &request.events)?;
        let mut attempts = self.repair_attempts.write().await;
        let mut workspaces = self.workspaces.write().await;
        let mut events_by_conversation = self.publication_events.write().await;
        let Some(current) = attempts.get(&request.attempt.id).cloned() else {
            return Ok(AgentWorkspaceRepairAttemptTransitionOutcome::Missing);
        };
        if !request.matches_attempt(&current) {
            return Ok(AgentWorkspaceRepairAttemptTransitionOutcome::Stale(current));
        }
        let workspace = workspaces
            .get_mut(&request.attempt.conversation_id)
            .ok_or_else(|| {
                AppError::NotFound(format!(
                    "workspace {} for repair attempt",
                    request.attempt.conversation_id
                ))
            })?;
        apply_compatibility_projection(
            workspace,
            request.compatibility_projection.as_ref(),
            request.attempt.updated_at,
        );
        append_repair_events(&mut events_by_conversation, request.events);
        attempts.insert(request.attempt.id.clone(), request.attempt.clone());
        Ok(AgentWorkspaceRepairAttemptTransitionOutcome::Applied(
            request.attempt,
        ))
    }

    async fn settle_repair_attempt(
        &self,
        request: SettleAgentWorkspaceRepairAttempt,
    ) -> AppResult<SettleAgentWorkspaceRepairAttemptOutcome> {
        let mut attempts = self.repair_attempts.write().await;
        let mut workspaces = self.workspaces.write().await;
        let mut events_by_conversation = self.publication_events.write().await;
        let Some(current) = attempts.get(&request.attempt_id).cloned() else {
            return Ok(SettleAgentWorkspaceRepairAttemptOutcome::Missing);
        };
        if current.generation != request.generation
            || current.phase != request.expected_phase
            || current.updated_at != request.expected_updated_at
            || current.settled_at.is_some()
        {
            return Ok(SettleAgentWorkspaceRepairAttemptOutcome::Stale(current));
        }
        validate_repair_events(&current.conversation_id, &request.events)?;
        let workspace = workspaces
            .get_mut(&current.conversation_id)
            .ok_or_else(|| {
                AppError::NotFound(format!(
                    "workspace {} for repair attempt",
                    current.conversation_id
                ))
            })?;
        let mut settled = current;
        settled.outcome = Some(request.outcome);
        settled.settled_at = Some(request.settled_at);
        settled.updated_at = request.settled_at;
        apply_compatibility_projection(
            workspace,
            request.compatibility_projection.as_ref(),
            request.settled_at,
        );
        append_repair_events(&mut events_by_conversation, request.events);
        attempts.insert(settled.id.clone(), settled.clone());
        Ok(SettleAgentWorkspaceRepairAttemptOutcome::Applied(settled))
    }

    async fn settle_and_start_repair_successor(
        &self,
        request: SettleAndStartAgentWorkspaceRepairSuccessor,
    ) -> AppResult<SettleAndStartAgentWorkspaceRepairSuccessorOutcome> {
        validate_repair_events(&request.successor.attempt.conversation_id, &request.events)?;
        validate_repair_events(
            &request.successor.attempt.conversation_id,
            &request.successor.events,
        )?;
        let mut attempts = self.repair_attempts.write().await;
        let mut workspaces = self.workspaces.write().await;
        let mut events_by_conversation = self.publication_events.write().await;
        let Some(current) = attempts.get(&request.attempt_id).cloned() else {
            return Ok(SettleAndStartAgentWorkspaceRepairSuccessorOutcome::Missing);
        };
        if current.generation != request.generation
            || current.phase != request.expected_phase
            || current.updated_at != request.expected_updated_at
            || current.settled_at.is_some()
        {
            return Ok(SettleAndStartAgentWorkspaceRepairSuccessorOutcome::Stale(
                current,
            ));
        }
        if current.conversation_id != request.successor.attempt.conversation_id {
            return Err(AppError::Validation(
                "repair successor must belong to the settled workspace".to_string(),
            ));
        }
        let workspace = workspaces
            .get_mut(&current.conversation_id)
            .ok_or_else(|| {
                AppError::NotFound(format!(
                    "workspace {} for repair attempt",
                    current.conversation_id
                ))
            })?;

        let mut settled = current;
        settled.outcome = Some(request.outcome);
        settled.settled_at = Some(request.settled_at);
        settled.updated_at = request.settled_at;
        let mut successor = request.successor.attempt;
        successor.generation = next_repair_generation(&attempts, &settled.conversation_id)?;
        if !request.successor.reason.trim().is_empty()
            && !successor
                .pending_reasons
                .contains(&request.successor.reason)
        {
            successor.pending_reasons.push(request.successor.reason);
        }

        apply_compatibility_projection(
            workspace,
            request.compatibility_projection.as_ref(),
            request.settled_at,
        );
        apply_compatibility_projection(
            workspace,
            request.successor.compatibility_projection.as_ref(),
            successor.updated_at,
        );
        append_repair_events(&mut events_by_conversation, request.events);
        append_repair_events(&mut events_by_conversation, request.successor.events);
        attempts.insert(settled.id.clone(), settled);
        attempts.insert(successor.id.clone(), successor.clone());
        Ok(SettleAndStartAgentWorkspaceRepairSuccessorOutcome::Started(
            successor,
        ))
    }

    async fn create_repair_effect(
        &self,
        request: CreateAgentWorkspaceRepairEffect,
    ) -> AppResult<CreateAgentWorkspaceRepairEffectOutcome> {
        validate_effect_shape(&request.effect)?;
        #[cfg(test)]
        let forced_outcome = self
            .next_create_repair_effect_outcome
            .lock()
            .unwrap()
            .take();
        let attempts = self.repair_attempts.write().await;
        let mut effects = self.repair_effects.write().await;
        let mut workspaces = self.workspaces.write().await;
        let mut events_by_conversation = self.publication_events.write().await;
        #[cfg(test)]
        if let Some(outcome) = forced_outcome {
            return Ok(match outcome {
                ForcedCreateAgentWorkspaceRepairEffectOutcome::Stale => attempts
                    .get(&request.attempt_id)
                    .cloned()
                    .map(|a| CreateAgentWorkspaceRepairEffectOutcome::Stale(Box::new(a)))
                    .unwrap_or(CreateAgentWorkspaceRepairEffectOutcome::Missing),
                ForcedCreateAgentWorkspaceRepairEffectOutcome::Missing => {
                    CreateAgentWorkspaceRepairEffectOutcome::Missing
                }
            });
        }
        let Some(current) = attempts.get(&request.attempt_id).cloned() else {
            return Ok(CreateAgentWorkspaceRepairEffectOutcome::Missing);
        };
        validate_repair_events(&current.conversation_id, &request.events)?;
        if current.generation != request.generation
            || current.phase != request.expected_phase
            || current.updated_at != request.expected_attempt_updated_at
            || current.settled_at.is_some()
        {
            return Ok(CreateAgentWorkspaceRepairEffectOutcome::Stale(Box::new(
                current,
            )));
        }
        if request.effect.attempt_id != current.id {
            return Err(AppError::Validation(
                "repair effect does not belong to its requested attempt".to_string(),
            ));
        }
        if let Some(existing) = effects
            .values()
            .find(|effect| effect.attempt_id == current.id && effect.completed_at.is_none())
            .cloned()
        {
            return Ok(CreateAgentWorkspaceRepairEffectOutcome::OpenEffectExists(
                existing,
            ));
        }
        if effects
            .values()
            .any(|effect| effect.idempotency_key == request.effect.idempotency_key)
        {
            return Err(AppError::Conflict(
                "repair effect idempotency key already exists".to_string(),
            ));
        }
        if request.effect.completed_at.is_some()
            || matches!(
                request.effect.status,
                AgentWorkspaceRepairEffectStatus::Observed
            )
        {
            return Err(AppError::Validation(
                "new repair effects must start incomplete".to_string(),
            ));
        }
        let workspace = workspaces
            .get_mut(&current.conversation_id)
            .ok_or_else(|| {
                AppError::NotFound(format!(
                    "workspace {} for repair attempt",
                    current.conversation_id
                ))
            })?;
        apply_compatibility_projection(
            workspace,
            request.compatibility_projection.as_ref(),
            request.effect.updated_at,
        );
        append_repair_events(&mut events_by_conversation, request.events);
        effects.insert(request.effect.id.clone(), request.effect.clone());
        Ok(CreateAgentWorkspaceRepairEffectOutcome::Created(
            request.effect,
        ))
    }

    async fn get_repair_effect_by_idempotency_key(
        &self,
        idempotency_key: &str,
    ) -> AppResult<Option<AgentWorkspaceRepairEffect>> {
        Ok(self
            .repair_effects
            .read()
            .await
            .values()
            .find(|effect| effect.idempotency_key == idempotency_key)
            .cloned())
    }

    async fn get_open_repair_effect(
        &self,
        attempt_id: &AgentWorkspaceRepairAttemptId,
    ) -> AppResult<Option<AgentWorkspaceRepairEffect>> {
        Ok(self
            .repair_effects
            .read()
            .await
            .values()
            .filter(|effect| {
                effect.attempt_id == *attempt_id
                    && effect.completed_at.is_none()
                    && effect.status != AgentWorkspaceRepairEffectStatus::Failed
            })
            .min_by(|left, right| left.created_at.cmp(&right.created_at))
            .cloned())
    }

    async fn complete_repair_effect(
        &self,
        request: CompleteAgentWorkspaceRepairEffect,
    ) -> AppResult<CompleteAgentWorkspaceRepairEffectOutcome> {
        validate_effect_shape(&request.effect)?;
        let attempts = self.repair_attempts.read().await;
        let mut effects = self.repair_effects.write().await;
        let mut workspaces = self.workspaces.write().await;
        let mut events_by_conversation = self.publication_events.write().await;
        let Some(current) = attempts.get(&request.attempt_id).cloned() else {
            return Ok(CompleteAgentWorkspaceRepairEffectOutcome::Missing);
        };
        validate_repair_events(&current.conversation_id, &request.events)?;
        if request.effect.attempt_id != current.id {
            return Err(AppError::Validation(
                "repair effect does not belong to its requested attempt".to_string(),
            ));
        }
        let Some(existing) = effects.get(&request.effect.id).cloned() else {
            return Ok(CompleteAgentWorkspaceRepairEffectOutcome::Missing);
        };
        if existing.attempt_id != current.id {
            return Err(AppError::Validation(
                "repair effect belongs to another attempt".to_string(),
            ));
        }
        if existing.completed_at.is_some() && existing == request.effect {
            return Ok(CompleteAgentWorkspaceRepairEffectOutcome::Applied(
                Box::new(existing),
            ));
        }
        if current.generation != request.generation
            || current.phase != request.expected_phase
            || current.updated_at != request.expected_attempt_updated_at
            || current.settled_at.is_some()
            || existing.updated_at != request.expected_effect_updated_at
            || existing.status != request.expected_effect_status
        {
            return Ok(CompleteAgentWorkspaceRepairEffectOutcome::Stale(Box::new(
                current,
            )));
        }
        let workspace = workspaces
            .get_mut(&current.conversation_id)
            .ok_or_else(|| {
                AppError::NotFound(format!(
                    "workspace {} for repair attempt",
                    current.conversation_id
                ))
            })?;
        apply_compatibility_projection(
            workspace,
            request.compatibility_projection.as_ref(),
            request.effect.updated_at,
        );
        append_repair_events(&mut events_by_conversation, request.events);
        effects.insert(request.effect.id.clone(), request.effect.clone());
        Ok(CompleteAgentWorkspaceRepairEffectOutcome::Applied(
            Box::new(request.effect),
        ))
    }

    async fn import_legacy_repair_attempt(
        &self,
        request: ImportLegacyAgentWorkspaceRepairAttempt,
    ) -> AppResult<ImportLegacyAgentWorkspaceRepairAttemptOutcome> {
        validate_repair_events(&request.attempt.conversation_id, &request.events)?;
        if request.attempt.generation == 0 {
            return Err(AppError::Validation(
                "legacy repair attempts require a positive generation".to_string(),
            ));
        }
        let mut attempts = self.repair_attempts.write().await;
        let mut workspaces = self.workspaces.write().await;
        let mut events_by_conversation = self.publication_events.write().await;
        if let Some(existing) = attempts
            .values()
            .filter(|existing| existing.conversation_id == request.attempt.conversation_id)
            .max_by_key(|existing| existing.generation)
        {
            return Ok(
                ImportLegacyAgentWorkspaceRepairAttemptOutcome::ExistingDurable(existing.clone()),
            );
        }
        if let Some(existing) = attempts.get(&request.attempt.id) {
            return Err(AppError::Conflict(format!(
                "legacy repair attempt id {} belongs to a different conversation {}",
                existing.id, existing.conversation_id
            )));
        }
        let workspace = workspaces
            .get_mut(&request.attempt.conversation_id)
            .ok_or_else(|| {
                AppError::NotFound(format!(
                    "workspace {} for repair attempt",
                    request.attempt.conversation_id
                ))
            })?;
        apply_compatibility_projection(
            workspace,
            request.compatibility_projection.as_ref(),
            request.attempt.updated_at,
        );
        append_repair_events(&mut events_by_conversation, request.events);
        attempts.insert(request.attempt.id.clone(), request.attempt.clone());
        Ok(ImportLegacyAgentWorkspaceRepairAttemptOutcome::Imported(
            request.attempt,
        ))
    }
}
