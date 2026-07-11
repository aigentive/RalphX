use super::*;
use crate::domain::state_machine::transition_handler::metadata_builder;
use crate::domain::state_machine::{QaFailedData, TransitionHandler};

impl<'a> TransitionHandler<'a> {
    async fn ensure_qa_trigger_origin(&self) {
        if let Some(ref task_repo) = self.machine.context.services.task_repo {
            let task_id = TaskId::from_string(self.machine.context.task_id.clone());
            if let Ok(Some(task)) = task_repo.get_by_id(&task_id).await {
                if !MetadataUpdate::key_exists_in("trigger_origin", task.metadata.as_deref()) {
                    let metadata_update = metadata_builder::build_trigger_origin_metadata("qa");
                    let merged_metadata = metadata_update.merge_into(task.metadata.as_deref());

                    if let Err(e) = task_repo
                        .update_metadata(&task_id, Some(merged_metadata))
                        .await
                    {
                        tracing::error!(
                            task_id = %self.machine.context.task_id,
                            error = %e,
                            "Failed to set trigger_origin=qa in metadata"
                        );
                    }
                } else {
                    tracing::debug!(
                        task_id = %self.machine.context.task_id,
                        "Skipping metadata write - trigger_origin already present (pre-computed)"
                    );
                }
            }
        }
    }

    pub(super) async fn enter_qa_refining_state(&self) {
        self.ensure_qa_trigger_origin().await;

        if !self.machine.context.qa_prep_complete {
            self.machine
                .context
                .services
                .agent_spawner
                .wait_for("qa-prep", &self.machine.context.task_id)
                .await;
        }
        self.machine
            .context
            .services
            .agent_spawner
            .spawn("qa-refiner", &self.machine.context.task_id)
            .await;
    }

    pub(super) async fn enter_qa_testing_state(&self) {
        self.ensure_qa_trigger_origin().await;
        self.machine
            .context
            .services
            .agent_spawner
            .spawn("qa-tester", &self.machine.context.task_id)
            .await;
    }

    pub(super) async fn enter_qa_passed_state(&self) {
        self.machine
            .context
            .services
            .event_emitter
            .emit("qa_passed", &self.machine.context.task_id)
            .await;
    }

    pub(super) async fn enter_qa_failed_state(&self, data: &QaFailedData) {
        self.machine
            .context
            .services
            .event_emitter
            .emit("qa_failed", &self.machine.context.task_id)
            .await;

        let _ = data;
    }
}
