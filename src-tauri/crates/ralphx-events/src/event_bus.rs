use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use tokio::sync::broadcast;

const DEFAULT_CAPACITY: usize = 256;

#[derive(Debug, Clone)]
pub struct EventBus<T: Clone> {
    sender: broadcast::Sender<T>,
    events_published: Arc<AtomicU64>,
}

impl<T: Clone> EventBus<T> {
    pub fn new() -> Self {
        Self::with_capacity(DEFAULT_CAPACITY)
    }

    pub fn with_capacity(capacity: usize) -> Self {
        let (sender, _) = broadcast::channel(capacity);
        Self {
            sender,
            events_published: Arc::new(AtomicU64::new(0)),
        }
    }

    pub fn publish(&self, event: T) -> Result<usize, T> {
        match self.sender.send(event) {
            Ok(count) => {
                self.events_published.fetch_add(1, Ordering::Relaxed);
                Ok(count)
            }
            Err(broadcast::error::SendError(event)) => Err(event),
        }
    }

    pub fn subscribe(&self) -> EventSubscriber<T> {
        EventSubscriber {
            receiver: self.sender.subscribe(),
        }
    }

    pub fn subscriber_count(&self) -> usize {
        self.sender.receiver_count()
    }

    pub fn events_published(&self) -> u64 {
        self.events_published.load(Ordering::Relaxed)
    }
}

impl<T: Clone> Default for EventBus<T> {
    fn default() -> Self {
        Self::new()
    }
}

pub struct EventSubscriber<T: Clone> {
    receiver: broadcast::Receiver<T>,
}

impl<T: Clone> EventSubscriber<T> {
    pub fn try_recv(&mut self) -> Result<T, broadcast::error::TryRecvError> {
        self.receiver.try_recv()
    }

    pub async fn recv(&mut self) -> Result<T, broadcast::error::RecvError> {
        self.receiver.recv().await
    }

    pub fn is_closed(&self) -> bool {
        self.receiver.is_closed()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn publish_without_subscribers_returns_event() {
        let bus = EventBus::new();
        assert_eq!(bus.publish("event".to_string()), Err("event".to_string()));
        assert_eq!(bus.events_published(), 0);
    }

    #[tokio::test]
    async fn subscribers_receive_published_events() {
        let bus = EventBus::new();
        let mut sub = bus.subscribe();

        assert_eq!(bus.publish("event".to_string()), Ok(1));

        assert_eq!(sub.recv().await.expect("event"), "event");
        assert_eq!(bus.events_published(), 1);
    }

    #[test]
    fn subscriber_reports_closed_after_bus_is_dropped() {
        let bus = EventBus::<String>::new();
        let sub = bus.subscribe();

        assert!(!sub.is_closed());

        drop(bus);

        assert!(sub.is_closed());
    }
}
