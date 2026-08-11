use crate::domain::supervisor::SupervisorEvent;

#[cfg(test)]
use tokio::sync::broadcast;

pub type EventBus = ralphx_events::EventBus<SupervisorEvent>;
pub type EventSubscriber = ralphx_events::EventSubscriber<SupervisorEvent>;

#[cfg(test)]
#[path = "event_bus_tests.rs"]
mod tests;
