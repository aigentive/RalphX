use ralphx_remote_protocol::{EventDelivery, EventOrigin, EVENT_CLASSIFICATIONS};
use tauri::{Listener, Runtime};
use tokio::sync::mpsc::{self, error::TrySendError};

use crate::remote_server::sequencer::{EpochRollCause, SequencerControl};

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

/// The sync side of the capture bank: what a `listen_any` callback is allowed to touch.
///
/// Tokio channels rather than `std::sync::mpsc` because the drain side is the async sequencer
/// actor, and `std`'s receiver only blocks. The *sending* half is unchanged in character:
/// `try_send` and `UnboundedSender::send` are both plain non-blocking calls that need no runtime,
/// so the handler still does channel-send only — no DB, no fs, no await, no locks (§3.4).
#[derive(Clone)]
pub struct CaptureFeed {
    durable: mpsc::Sender<CapturedEvent>,
    transient: mpsc::UnboundedSender<CapturedEvent>,
    /// Separate and unbounded **because the bounded channel being full is what triggers it**.
    /// Signalling overload over the same channel that just overflowed could not work (§3.4 #3).
    control: mpsc::UnboundedSender<SequencerControl>,
}

pub struct CaptureReceivers {
    pub durable: mpsc::Receiver<CapturedEvent>,
    pub transient: mpsc::UnboundedReceiver<CapturedEvent>,
    pub control: mpsc::UnboundedReceiver<SequencerControl>,
    /// Spare control sender for the stream handle, so a roll can also be requested from outside
    /// the capture bank (listener/host paths) without reaching back into the feed.
    pub control_sender: mpsc::UnboundedSender<SequencerControl>,
}

impl CaptureFeed {
    pub fn channels(durable_capacity: usize) -> (Self, CaptureReceivers) {
        let (durable, durable_rx) = mpsc::channel(durable_capacity);
        let (transient, transient_rx) = mpsc::unbounded_channel();
        let (control, control_rx) = mpsc::unbounded_channel();
        (
            Self {
                durable,
                transient,
                control: control.clone(),
            },
            CaptureReceivers {
                durable: durable_rx,
                transient: transient_rx,
                control: control_rx,
                control_sender: control,
            },
        )
    }
}

/// Which of the two capture seams a classified event feeds.
///
/// Deliberately narrower than [`EventDelivery`]: `LocalOnly` is filtered out at registration,
/// so the handler cannot be handed a delivery class it has no seam for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CaptureSink {
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
                    feed.dispatch(
                        sink,
                        CapturedEvent {
                            name,
                            payload: raw_payload.to_owned(),
                        },
                    );
                }),
            );
        }
    }
}

impl CaptureFeed {
    /// The entire body of a capture handler: a non-blocking channel send, plus the overload
    /// signal. Extracted so the overload path is exercised by tests through the production code
    /// rather than a re-implementation of it.
    pub(crate) fn dispatch(&self, sink: CaptureSink, event: CapturedEvent) {
        let name = event.name;
        match sink {
            CaptureSink::Durable => match self.durable.try_send(event) {
                Ok(()) => {}
                // The event is DROPPED — never queued, never sequenced. Rather than let the
                // stream silently splice over the hole, the sequencer rolls the in-memory epoch
                // live and every connected client cold-hydrates (§3.4 #3, P-22(b)). The signal
                // goes on the separate unbounded control channel because this one is, by
                // definition, full.
                Err(TrySendError::Full(_)) => {
                    tracing::warn!(
                        event_name = name,
                        "Remote durable capture feed is full; rolling the stream epoch"
                    );
                    if self
                        .control
                        .send(SequencerControl::RollEpoch(EpochRollCause::CaptureOverload))
                        .is_err()
                    {
                        tracing::error!(
                            event_name = name,
                            "Remote sequencer control channel is closed; a dropped durable event \
                             cannot be signalled"
                        );
                    }
                }
                Err(TrySendError::Closed(_)) => tracing::warn!(
                    event_name = name,
                    "Remote durable capture feed is disconnected"
                ),
            },
            CaptureSink::Transient => {
                if self.transient.send(event).is_err() {
                    tracing::warn!(
                        event_name = name,
                        "Remote transient capture feed is disconnected"
                    );
                }
            }
        }
    }
}

// The pre-1.4 `install_if_host_mode_configured` placeholder is gone. Installation now lives in
// `remote_server::install_remote_stream_from_handle`, where the durable sequencer is started
// first and capture is installed against a live drain — and where the persisted host-mode setting
// can actually be read, which the pre-SQLite app-setup call site could not do.
