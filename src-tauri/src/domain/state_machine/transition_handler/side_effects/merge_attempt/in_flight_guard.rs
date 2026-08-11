use super::*;

impl<'a> TransitionHandler<'a> {
    /// Try to insert this task into the in-flight set. Returns false if already in flight.
    pub(in crate::domain::state_machine::transition_handler::side_effects) fn try_acquire_in_flight_guard(
        &self,
        task_id_str: &str,
    ) -> bool {
        let mut in_flight = self
            .machine
            .context
            .services
            .merges_in_flight
            .lock()
            .unwrap_or_else(|p| p.into_inner());
        if !in_flight.insert(task_id_str.to_string()) {
            tracing::info!(
                task_id = task_id_str,
                "Merge attempt skipped — already in flight for this task (self-dedup guard)"
            );
            return false;
        }
        true
    }
}
