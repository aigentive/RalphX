//! Durable, exact-owner handoff staging for a retiring interactive runtime.
//!
//! This module deliberately does not launch a replacement itself. Once the durable
//! continuation is staged, the existing queue processor remains the sole launch
//! authority and therefore keeps Plan profile/session resolution single-sourced.

use std::sync::Arc;

use crate::application::interactive_process_registry::{
    InteractiveProcessKey, InteractiveProcessRegistry,
    InteractiveProcessRetireAfterTurnDisposition, InteractiveProcessRetireArmDisposition,
    InteractiveProcessToken,
};
use crate::domain::entities::ChatContextType;
use crate::domain::repositories::QueuedMessageRepository;
use crate::domain::services::{
    MessageQueue, QueueKey, QueuedMessage, RunningAgentKey, RunningAgentRegistry, TryRegisterError,
};

use super::{launch_reservation::LaunchReservationGuard, SendResult};

/// Exact runtime identity captured before the accepted answer is committed.
#[derive(Debug, Clone)]
pub struct RuntimeHandoffOwner {
    pub context_type: ChatContextType,
    pub runtime_context_id: String,
    pub agent_run_id: String,
    pub interactive_process_token: InteractiveProcessToken,
}

/// Exact short-lived registry ownership held while a no-owner handoff crosses
/// mode/session staging, durable enqueue, and answer commit.
pub struct RuntimeHandoffReservation {
    key: RunningAgentKey,
    agent_run_id: String,
    guard: LaunchReservationGuard,
}

/// Truthful result of releasing a request-owned no-owner reservation.
///
/// A failed or unreadable verification must not be treated as release success:
/// the request's PID-0 row may still exclude the canonical queue launch.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuntimeHandoffReleaseOutcome {
    Released,
    FailedOrUncertain,
}

/// The only safe interpretations of the two runtime registries during handoff
/// capture. `NoOwner` requires stable absence from both registries; every
/// disagreement or unreadable running-agent snapshot remains fail-closed.
#[derive(Debug, Clone)]
pub enum RuntimeHandoffCapture {
    Captured(RuntimeHandoffOwner),
    NoOwner,
    FailedOrUncertain,
}

/// Atomically claim a stable no-owner handoff slot before the caller crosses an
/// await boundary. The request-owned PID-0 row excludes a competing launch but
/// is never used as launch authority itself.
pub(super) async fn reserve_no_owner_runtime_handoff(
    running_agent_registry: &Arc<dyn RunningAgentRegistry>,
    context_type: ChatContextType,
    runtime_context_id: &str,
    request_id: &str,
) -> Result<RuntimeHandoffReservation, TryRegisterError> {
    let key = RunningAgentKey::new(context_type.to_string(), runtime_context_id);
    let agent_run_id = format!("plan-mode-handoff-reservation:{request_id}");
    running_agent_registry
        .try_register(
            key.clone(),
            runtime_context_id.to_string(),
            agent_run_id.clone(),
        )
        .await?;

    Ok(RuntimeHandoffReservation {
        guard: LaunchReservationGuard::new(
            Arc::clone(running_agent_registry),
            key.clone(),
            agent_run_id.clone(),
            std::time::Duration::from_secs(
                crate::infrastructure::agents::claude::stream_timeouts()
                    .launch_reservation_lease_secs,
            ),
        ),
        key,
        agent_run_id,
    })
}

/// Stop renewal and remove only this request's PID-0 reservation. It is safe to
/// call repeatedly: an exact-owner unregister cannot remove a competing launch.
pub(super) async fn release_no_owner_runtime_handoff(
    running_agent_registry: &Arc<dyn RunningAgentRegistry>,
    reservation: &RuntimeHandoffReservation,
) -> RuntimeHandoffReleaseOutcome {
    reservation.guard.stop();
    running_agent_registry
        .unregister(&reservation.key, &reservation.agent_run_id)
        .await;

    match running_agent_registry
        .list_by_context_type(&reservation.key.context_type)
        .await
    {
        Ok(entries)
            if entries.iter().any(|(key, info)| {
                key == &reservation.key && info.agent_run_id == reservation.agent_run_id
            }) =>
        {
            tracing::warn!(
                context_type = %reservation.key.context_type,
                context_id = %reservation.key.context_id,
                agent_run_id = %reservation.agent_run_id,
                "No-owner runtime-handoff reservation still owns its slot after release"
            );
            RuntimeHandoffReleaseOutcome::FailedOrUncertain
        }
        Ok(_) => RuntimeHandoffReleaseOutcome::Released,
        Err(error) => {
            tracing::warn!(
                context_type = %reservation.key.context_type,
                context_id = %reservation.key.context_id,
                agent_run_id = %reservation.agent_run_id,
                error = %error,
                "Could not verify no-owner runtime-handoff reservation release"
            );
            RuntimeHandoffReleaseOutcome::FailedOrUncertain
        }
    }
}

impl RuntimeHandoffOwner {
    fn queue_key(&self) -> QueueKey {
        QueueKey::new(self.context_type, self.runtime_context_id.clone())
    }

    fn interactive_key(&self) -> InteractiveProcessKey {
        InteractiveProcessKey::new(self.context_type.to_string(), &self.runtime_context_id)
    }
}

/// Explicit result of staging durable continuation authority.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuntimeHandoffOutcome {
    /// The continuation is durable and the exact runtime will retire at TurnComplete.
    AwaitingRetirement,
    /// The continuation is durable, but the captured owner changed; normal recovery
    /// must kick it rather than guessing which process to stop.
    DurablyRecoverable,
    /// No durable/reversible handoff authority was established.
    Failed,
}

/// Result of asking the canonical queued-send path to launch a durable handoff.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RuntimeHandoffKickOutcome {
    /// The queue row was consumed and a replacement run was started immediately.
    Started { agent_run_id: String },
    /// The continuation remains durable or a competing owner deferred its launch.
    DurablyRecoverable,
    /// The queue row could not be read or no launch/recovery authority remains.
    Failed,
}

/// Truthful launch mapping shared by post-commit and startup recovery callers.
pub(super) fn map_runtime_handoff_kick_send_result(
    result: Option<&SendResult>,
    stable_row_remains: bool,
) -> RuntimeHandoffKickOutcome {
    match result {
        Some(result) if !result.was_queued && !result.agent_run_id.trim().is_empty() => {
            RuntimeHandoffKickOutcome::Started {
                agent_run_id: result.agent_run_id.clone(),
            }
        }
        Some(result) if result.was_queued || stable_row_remains => {
            RuntimeHandoffKickOutcome::DurablyRecoverable
        }
        None if stable_row_remains => RuntimeHandoffKickOutcome::DurablyRecoverable,
        _ => RuntimeHandoffKickOutcome::Failed,
    }
}

/// Result of attempting to undo a pre-commit handoff stage.
///
/// A durable delete failure leaves the continuation and retirement authority intact
/// so the caller must fail closed and let normal recovery drain the row.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuntimeHandoffCompensationOutcome {
    Compensated,
    DurablyRecoverable,
}

/// Capture a handoff owner only when both runtime registries agree twice on the
/// same non-blank launch id. A key alone is never sufficient authority.
pub(super) async fn capture_runtime_handoff_owner(
    running_agent_registry: &Arc<dyn RunningAgentRegistry>,
    interactive_process_registry: &InteractiveProcessRegistry,
    context_type: ChatContextType,
    runtime_context_id: &str,
) -> RuntimeHandoffCapture {
    let running_key = RunningAgentKey::new(context_type.to_string(), runtime_context_id);
    let interactive_key = InteractiveProcessKey::new(context_type.to_string(), runtime_context_id);
    let context_name = context_type.to_string();
    let first_running = match running_agent_registry
        .list_by_context_type(&context_name)
        .await
    {
        Ok(entries) => entries
            .into_iter()
            .find_map(|(key, info)| (key == running_key).then_some(info)),
        Err(error) => {
            tracing::warn!(error = %error, runtime_context_id, "Could not capture runtime-handoff running owner");
            return RuntimeHandoffCapture::FailedOrUncertain;
        }
    };
    let first_interactive = interactive_process_registry
        .capture_owner(&interactive_key)
        .await;
    let second_running = match running_agent_registry
        .list_by_context_type(&context_name)
        .await
    {
        Ok(entries) => entries
            .into_iter()
            .find_map(|(key, info)| (key == running_key).then_some(info)),
        Err(error) => {
            tracing::warn!(error = %error, runtime_context_id, "Could not revalidate runtime-handoff running owner");
            return RuntimeHandoffCapture::FailedOrUncertain;
        }
    };
    let second_interactive = interactive_process_registry
        .capture_owner(&interactive_key)
        .await;

    if first_running.is_none()
        && first_interactive.is_none()
        && second_running.is_none()
        && second_interactive.is_none()
    {
        return RuntimeHandoffCapture::NoOwner;
    }

    let (
        Some(first_running),
        Some(first_interactive),
        Some(second_running),
        Some(second_interactive),
    ) = (
        first_running,
        first_interactive,
        second_running,
        second_interactive,
    )
    else {
        return RuntimeHandoffCapture::FailedOrUncertain;
    };

    if second_running.agent_run_id.trim().is_empty()
        || first_running.agent_run_id.trim().is_empty()
        || first_running.agent_run_id != second_running.agent_run_id
        || first_interactive.agent_run_id != second_interactive.agent_run_id
        || first_interactive.token != second_interactive.token
        || first_interactive.agent_run_id != first_running.agent_run_id
    {
        return RuntimeHandoffCapture::FailedOrUncertain;
    }

    RuntimeHandoffCapture::Captured(RuntimeHandoffOwner {
        context_type,
        runtime_context_id: runtime_context_id.to_owned(),
        agent_run_id: first_interactive.agent_run_id,
        interactive_process_token: first_interactive.token,
    })
}

/// Persist one stable continuation, mirror it in memory idempotently, then arm only
/// the captured IPR registration. The caller owns answer commit and must compensate
/// with [`compensate_runtime_handoff`] if that commit cannot proceed.
pub(super) async fn stage_runtime_handoff(
    queued_message_repo: Option<&Arc<dyn QueuedMessageRepository>>,
    message_queue: &MessageQueue,
    running_agent_registry: &Arc<dyn RunningAgentRegistry>,
    interactive_process_registry: &InteractiveProcessRegistry,
    owner: &RuntimeHandoffOwner,
    continuation: QueuedMessage,
) -> RuntimeHandoffOutcome {
    let Some(queued_message_repo) = queued_message_repo else {
        tracing::error!("Cannot stage runtime handoff without durable queue storage");
        return RuntimeHandoffOutcome::Failed;
    };
    let key = owner.queue_key();
    if let Err(error) = queued_message_repo.enqueue_back(&key, &continuation).await {
        tracing::error!(error = %error, queued_message_id = %continuation.id, "Failed to persist runtime-handoff continuation");
        return RuntimeHandoffOutcome::Failed;
    }
    let continuation_id = continuation.id.clone();
    message_queue.queue_back_existing(
        owner.context_type,
        owner.runtime_context_id.clone(),
        continuation,
    );

    let armed = interactive_process_registry
        .arm_retire_after_turn_if_owner(
            &owner.interactive_key(),
            owner.interactive_process_token,
            &owner.agent_run_id,
        )
        .await;

    let running_key =
        RunningAgentKey::new(owner.context_type.to_string(), &owner.runtime_context_id);
    let current_running = match running_agent_registry
        .list_by_context_type(&owner.context_type.to_string())
        .await
    {
        Ok(entries) => entries
            .into_iter()
            .find_map(|(key, info)| (key == running_key).then_some(info)),
        Err(error) => {
            tracing::warn!(error = %error, queued_message_id = %continuation_id, "Could not revalidate runtime-handoff running owner");
            return match compensate_runtime_handoff(
                Some(queued_message_repo),
                message_queue,
                interactive_process_registry,
                owner,
                &continuation_id,
            )
            .await
            {
                RuntimeHandoffCompensationOutcome::Compensated => RuntimeHandoffOutcome::Failed,
                RuntimeHandoffCompensationOutcome::DurablyRecoverable => {
                    RuntimeHandoffOutcome::DurablyRecoverable
                }
            };
        }
    };
    let disposition = interactive_process_registry
        .retire_after_turn_disposition_if_owner(
            &owner.interactive_key(),
            owner.interactive_process_token,
            &owner.agent_run_id,
        )
        .await;
    let current_interactive = interactive_process_registry
        .capture_owner(&owner.interactive_key())
        .await;

    let running_is_exact = current_running
        .as_ref()
        .is_some_and(|running| running.agent_run_id == owner.agent_run_id);
    let interactive_is_exact = current_interactive.as_ref().is_some_and(|interactive| {
        interactive.token == owner.interactive_process_token
            && interactive.agent_run_id == owner.agent_run_id
    });
    let both_authorities_gone = current_running.is_none() && current_interactive.is_none();

    if both_authorities_gone {
        // The old runtime completed between durable staging and revalidation. The
        // row is the remaining recovery authority; never guess a replacement stop.
        return RuntimeHandoffOutcome::DurablyRecoverable;
    }

    if !running_is_exact || !interactive_is_exact {
        // A foreign row, or a same-run registry paired with a missing/replaced IPR
        // entry, cannot safely retire this request's old runtime. Remove only the
        // row we wrote before the caller can commit the mode change.
        return match compensate_runtime_handoff(
            Some(queued_message_repo),
            message_queue,
            interactive_process_registry,
            owner,
            &continuation_id,
        )
        .await
        {
            RuntimeHandoffCompensationOutcome::Compensated => RuntimeHandoffOutcome::Failed,
            RuntimeHandoffCompensationOutcome::DurablyRecoverable => {
                RuntimeHandoffOutcome::DurablyRecoverable
            }
        };
    }

    match (armed, disposition) {
        (
            InteractiveProcessRetireArmDisposition::AwaitingTurn,
            InteractiveProcessRetireAfterTurnDisposition::Active { is_armed: true },
        )
        | (
            InteractiveProcessRetireArmDisposition::IdleReady,
            InteractiveProcessRetireAfterTurnDisposition::Idle { is_armed: true },
        ) => RuntimeHandoffOutcome::AwaitingRetirement,
        (_, InteractiveProcessRetireAfterTurnDisposition::Stale) => {
            RuntimeHandoffOutcome::DurablyRecoverable
        }
        _ => RuntimeHandoffOutcome::DurablyRecoverable,
    }
}

/// Undo only the request-owned staged continuation and exact retirement arm.
pub(super) async fn compensate_runtime_handoff(
    queued_message_repo: Option<&Arc<dyn QueuedMessageRepository>>,
    message_queue: &MessageQueue,
    interactive_process_registry: &InteractiveProcessRegistry,
    owner: &RuntimeHandoffOwner,
    continuation_id: &str,
) -> RuntimeHandoffCompensationOutcome {
    let key = owner.queue_key();
    if let Some(queued_message_repo) = queued_message_repo {
        if let Err(error) = queued_message_repo.delete(&key, continuation_id).await {
            tracing::warn!(error = %error, queued_message_id = continuation_id, "Failed to compensate durable runtime-handoff continuation");
            return RuntimeHandoffCompensationOutcome::DurablyRecoverable;
        }
    }
    let _ = message_queue.delete_with_key(&key, continuation_id);
    let _ = interactive_process_registry
        .disarm_retire_after_turn_if_owner(
            &owner.interactive_key(),
            owner.interactive_process_token,
            &owner.agent_run_id,
        )
        .await;
    RuntimeHandoffCompensationOutcome::Compensated
}

/// Start the post-commit retirement watchdog for the exact captured launch.
///
/// The watchdog never invokes a provider launch. It cancels the source stream
/// only while its exact IPR registration remains armed; that stream's ordinary
/// `mode_handoff_exit` path removes the idle row and invokes the canonical queue
/// processor with a fresh cancellation token.
pub(super) fn activate_runtime_handoff_watchdog(
    running_agent_registry: Arc<dyn RunningAgentRegistry>,
    interactive_process_registry: Arc<InteractiveProcessRegistry>,
    owner: RuntimeHandoffOwner,
) {
    let grace = std::time::Duration::from_secs(
        crate::infrastructure::agents::claude::stream_timeouts().completion_grace_secs,
    );
    tokio::spawn(async move {
        tokio::time::sleep(grace).await;
        let _ = cancel_armed_runtime_handoff_owner(
            &running_agent_registry,
            &interactive_process_registry,
            &owner,
        )
        .await;
    });
}

/// Cancel only an exact, still-armed owner. Kept separate so behavioral tests
/// can prove stale and unarmed rows never receive a background cancellation.
pub(super) async fn cancel_armed_runtime_handoff_owner(
    running_agent_registry: &Arc<dyn RunningAgentRegistry>,
    interactive_process_registry: &InteractiveProcessRegistry,
    owner: &RuntimeHandoffOwner,
) -> bool {
    if !matches!(
        interactive_process_registry
            .retire_after_turn_disposition_if_owner(
                &owner.interactive_key(),
                owner.interactive_process_token,
                &owner.agent_run_id,
            )
            .await,
        InteractiveProcessRetireAfterTurnDisposition::Active { is_armed: true }
            | InteractiveProcessRetireAfterTurnDisposition::Idle { is_armed: true }
    ) {
        return false;
    }

    let running_key =
        RunningAgentKey::new(owner.context_type.to_string(), &owner.runtime_context_id);
    let Some(info) = running_agent_registry.get(&running_key).await else {
        return false;
    };
    if info.agent_run_id != owner.agent_run_id {
        return false;
    }
    let Some(cancellation_token) = info.cancellation_token else {
        return false;
    };
    cancellation_token.cancel();
    true
}

/// Finalize a staged owner that was already idle after the answer is committed.
///
/// Returning `false` means a replacement or active turn won the race; callers must
/// leave the durable queue row intact for normal recovery/TurnComplete processing.
pub(super) async fn finalize_idle_runtime_handoff(
    interactive_process_registry: &InteractiveProcessRegistry,
    owner: &RuntimeHandoffOwner,
) -> Option<crate::application::interactive_process_registry::InteractiveProcess> {
    interactive_process_registry
        .retire_armed_idle_if_owner(
            &owner.interactive_key(),
            owner.interactive_process_token,
            &owner.agent_run_id,
        )
        .await
}
