use ralphx_remote_protocol::{EventDelivery, EventOrigin, EVENT_CLASSIFICATIONS};
use std::sync::mpsc::{self, Receiver, SyncSender, TrySendError};
use tauri::{Listener, Runtime};

#[cfg(test)]
#[path = "capture_tests.rs"]
mod tests;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CapturedEvent {
    pub name: &'static str,
    /// Raw JSON payload text, exactly as Tauri delivered it.
    ///
    /// Deliberately unparsed. Tauri invokes `listen_any` callbacks INLINE on the emitting
    /// thread (`tauri::event::listener::emit_filter` → `(callback)(Event::new(…))`), so a
    /// `serde_json` parse here would run on the emit hot path of every classified event —
    /// including `agent:chunk` streaming. §3.4 pins the opposite contract ("parse cost … off
    /// the emit hot path"; "capture handlers stay sync and do channel-send"), so parsing
    /// belongs to the drain side: PR 1.4's sequencer/broadcast actor. `remote_event_log.payload`
    /// is TEXT, so the durable path stores this string without re-serializing it.
    pub payload: String,
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

/// Which of the two capture seams a classified event feeds.
///
/// Deliberately narrower than [`EventDelivery`]: `LocalOnly` is filtered out at registration,
/// so the handler cannot be handed a delivery class it has no seam for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CaptureSink {
    Durable,
    Transient,
}

pub trait EventRegistrar {
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
        for entry in EVENT_CLASSIFICATIONS.iter().filter(|entry| {
            // Local-only rows never get a handler, whatever their origin. The webview-origin
            // filter alone is not enough: §3.4's Local-only category explicitly includes
            // backend chrome ("window/dock/updater chrome"), and PR 1.4 adds backend-emitted
            // `remote:session_connected`/`remote:session_closed` as Local-only rows
            // (02-phase-1-host-mode.md). Filtering on delivery makes the `LocalOnly` arm of the
            // handler's match structurally unreachable instead of test-enforced.
            entry.origin == EventOrigin::Backend
                && entry.delivery != EventDelivery::LocalOnly
                && !entry.excluded_from_v1
        }) {
            let name = entry.name;
            let sink = match entry.delivery {
                EventDelivery::Durable => CaptureSink::Durable,
                EventDelivery::Transient => CaptureSink::Transient,
                // Filtered out above; `continue` rather than `unreachable!` keeps a table edit
                // from turning into a panic inside a Tauri event-dispatch callback.
                EventDelivery::LocalOnly => continue,
            };
            let feed = feed.clone();
            registrar.listen(
                name,
                Box::new(move |raw_payload| {
                    let event = CapturedEvent {
                        name,
                        payload: raw_payload.to_owned(),
                    };
                    match sink {
                        CaptureSink::Durable => match feed.durable.try_send(event) {
                            Ok(()) => {}
                            // PR 1.4: a full durable channel must mark the stream unhealthy and
                            // signal an epoch roll over the unbounded control channel (§3.4 #3)
                            // so no dropped event is ever silently spliced over. This warn is a
                            // pre-sequencer placeholder — it must not ship as the final behavior.
                            Err(TrySendError::Full(_)) => tracing::warn!(
                                event_name = name,
                                "Remote durable capture feed is full"
                            ),
                            Err(TrySendError::Disconnected(_)) => tracing::warn!(
                                event_name = name,
                                "Remote durable capture feed is disconnected"
                            ),
                        },
                        CaptureSink::Transient => {
                            if feed.transient.send(event).is_err() {
                                tracing::warn!(
                                    event_name = name,
                                    "Remote transient capture feed is disconnected"
                                );
                            }
                        }
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
    // Bounded so a wedged consumer can never block the emit thread; overflow means an epoch
    // roll, not backpressure (§3.4 #3).
    let (feed, receivers) = CaptureFeed::channels(1_024);
    RemoteEventCapture::install(app_handle, feed);
    // Placeholder drains: PR 1.4 replaces both receivers with the durable sequencer actor
    // (allocate → commit → publish) and the transient live-broadcast channel. Pre-1.4 there is
    // no sequencer, listener, or client, so discarded events are unobservable
    // (01-phase-0-foundations.md, Open question 2).
    std::thread::spawn(move || for _ in receivers.durable {});
    std::thread::spawn(move || for _ in receivers.transient {});
}
