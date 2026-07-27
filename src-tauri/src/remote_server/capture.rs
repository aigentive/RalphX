use ralphx_remote_protocol::{EventDelivery, EventOrigin, EVENT_CLASSIFICATIONS};
use serde_json::Value;
use std::sync::mpsc::{self, Receiver, SyncSender, TrySendError};
use tauri::{Listener, Runtime};

#[cfg(test)]
#[path = "capture_tests.rs"]
mod tests;

#[derive(Debug, Clone, PartialEq)]
pub struct CapturedEvent {
    pub name: &'static str,
    pub payload: Value,
}

#[derive(Clone)]
pub struct CaptureFeed {
    durable: SyncSender<CapturedEvent>,
    transient: mpsc::Sender<CapturedEvent>,
}

pub struct CaptureReceivers {
    pub durable: Receiver<CapturedEvent>,
    pub transient: Receiver<CapturedEvent>,
}

impl CaptureFeed {
    pub fn channels(durable_capacity: usize) -> (Self, CaptureReceivers) {
        let (durable, durable_rx) = mpsc::sync_channel(durable_capacity);
        let (transient, transient_rx) = mpsc::channel();
        (
            Self { durable, transient },
            CaptureReceivers {
                durable: durable_rx,
                transient: transient_rx,
            },
        )
    }
}

pub trait EventRegistrar: Clone + Send + Sync + 'static {
    fn listen(&self, name: &'static str, handler: Box<dyn Fn(&str) + Send + Sync>);
}

struct TauriRegistrar<R: Runtime>(tauri::AppHandle<R>);

impl<R: Runtime> Clone for TauriRegistrar<R> {
    fn clone(&self) -> Self {
        Self(self.0.clone())
    }
}

impl<R: Runtime> EventRegistrar for TauriRegistrar<R> {
    fn listen(&self, name: &'static str, handler: Box<dyn Fn(&str) + Send + Sync>) {
        self.0
            .listen_any(name, move |event| handler(event.payload()));
    }
}

pub struct RemoteEventCapture;

impl RemoteEventCapture {
    pub fn install<R: Runtime>(app_handle: tauri::AppHandle<R>, feed: CaptureFeed) {
        Self::install_with_registrar(TauriRegistrar(app_handle), feed);
    }

    pub fn install_with_registrar<R: EventRegistrar>(registrar: R, feed: CaptureFeed) {
        for entry in EVENT_CLASSIFICATIONS
            .iter()
            .filter(|entry| entry.origin == EventOrigin::Backend && !entry.excluded_from_v1)
        {
            let name = entry.name;
            let delivery = entry.delivery;
            let feed = feed.clone();
            registrar.listen(
                name,
                Box::new(move |raw_payload| {
                    let Ok(payload) = serde_json::from_str(raw_payload) else {
                        tracing::warn!(
                            event_name = name,
                            "Remote event capture dropped malformed JSON payload"
                        );
                        return;
                    };
                    let event = CapturedEvent { name, payload };
                    match delivery {
                        EventDelivery::Durable => match feed.durable.try_send(event) {
                            Ok(()) => {}
                            Err(TrySendError::Full(_)) => tracing::warn!(
                                event_name = name,
                                "Remote durable capture feed is full"
                            ),
                            Err(TrySendError::Disconnected(_)) => tracing::warn!(
                                event_name = name,
                                "Remote durable capture feed is disconnected"
                            ),
                        },
                        EventDelivery::Transient => {
                            if feed.transient.send(event).is_err() {
                                tracing::warn!(
                                    event_name = name,
                                    "Remote transient capture feed is disconnected"
                                );
                            }
                        }
                        EventDelivery::LocalOnly => unreachable!("local events are not registered"),
                    }
                }),
            );
        }
    }
}

pub fn install_if_host_mode_configured<R: Runtime>(
    app_handle: tauri::AppHandle<R>,
    configured: bool,
) {
    if !configured {
        return;
    }
    let (feed, receivers) = CaptureFeed::channels(1_024);
    RemoteEventCapture::install(app_handle, feed);
    std::thread::spawn(move || for _ in receivers.durable {});
    std::thread::spawn(move || for _ in receivers.transient {});
}
