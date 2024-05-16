//! Events generated by `Node`

use std::fmt;
use std::panic::Location;
use std::time::Duration;

use instant::SystemTime;
use serde::Serialize;
use tokio::sync::broadcast;

/// An error returned from the `EventReceiver::recv`.
#[derive(Debug, thiserror::Error)]
pub enum RecvError {
    /// Node and all its event senders are closed.
    #[error("Event channel closed")]
    Closed,
}

/// An error returned from the `EventReceiver::try_recv`.
#[derive(Debug, thiserror::Error)]
pub enum TryRecvError {
    /// The event channel is currently empty.
    #[error("Event channel empty")]
    Empty,
    /// Node and all its event senders are closed.
    #[error("Event channel closed")]
    Closed,
}

/// A channel which users can subscribe for events.
#[derive(Debug)]
pub struct EventChannel {
    tx: broadcast::Sender<NodeEventInfo>,
}

/// `EventPublisher` is used to broadcast events generated by [`Node`] to [`EventSubscriber`]s.
///
/// [`Node`]: crate::node::Node
#[derive(Debug, Clone)]
pub struct EventPublisher {
    tx: broadcast::Sender<NodeEventInfo>,
}

/// `EventSubscriber` can be used by users to receive events from [`Node`].
///
/// [`Node`]: crate::node::Node
#[derive(Debug)]
pub struct EventSubscriber {
    rx: broadcast::Receiver<NodeEventInfo>,
}

impl EventChannel {
    /// Create a new `EventSubscription`.
    pub fn new() -> EventChannel {
        let (tx, _) = broadcast::channel(32);
        EventChannel { tx }
    }

    /// Creates a new [`EventPublisher`].
    pub fn publisher(&self) -> EventPublisher {
        EventPublisher {
            tx: self.tx.clone(),
        }
    }

    /// Creates a new [`EventSubscriber`].
    pub fn subscribe(&self) -> EventSubscriber {
        EventSubscriber {
            rx: self.tx.subscribe(),
        }
    }
}

impl Default for EventChannel {
    fn default() -> Self {
        EventChannel::new()
    }
}

impl EventPublisher {
    pub(crate) fn send(&self, event: NodeEvent) {
        let time = SystemTime::now();
        let location: &'static Location<'static> = Location::caller();

        // Error is produced if there aren't any subscribers. Since this is
        // a valid case, we ignore the error.
        let _ = self.tx.send(NodeEventInfo {
            event,
            time,
            file_path: location.file(),
            file_line: location.line(),
        });
    }
}

impl EventSubscriber {
    /// Receive an event from [`Node`].
    ///
    /// # Cancel safety
    ///
    /// This method is cancel-safe.
    ///
    /// [`Node`]: crate::node::Node
    pub async fn recv(&mut self) -> Result<NodeEventInfo, RecvError> {
        loop {
            match self.rx.recv().await {
                Ok(val) => return Ok(val),
                Err(broadcast::error::RecvError::Lagged(_)) => {
                    // Slow consumer. We will receive a message on the next call.
                    continue;
                }
                Err(broadcast::error::RecvError::Closed) => return Err(RecvError::Closed),
            }
        }
    }

    /// Attempts to receive an already queued event from [`Node`] without awaiting.
    ///
    /// If no events queued, `Err(TryRecvError::Empty)` is returned.
    ///
    /// [`Node`]: crate::node::Node
    pub fn try_recv(&mut self) -> Result<NodeEventInfo, TryRecvError> {
        loop {
            match self.rx.try_recv() {
                Ok(val) => return Ok(val),
                Err(broadcast::error::TryRecvError::Lagged(_)) => {
                    // Slow consumer. We will receive a message on the next call.
                    continue;
                }
                Err(broadcast::error::TryRecvError::Empty) => return Err(TryRecvError::Empty),
                Err(broadcast::error::TryRecvError::Closed) => return Err(TryRecvError::Closed),
            }
        }
    }
}

/// This struct include the [`NodeEvent`] and some extra information about the event.
#[derive(Debug, Clone, Serialize)]
pub struct NodeEventInfo {
    /// The event
    pub event: NodeEvent,
    #[cfg_attr(
        target_arch = "wasm32",
        serde(serialize_with = "serialize_system_time")
    )]
    /// When the event was generated.
    pub time: SystemTime,
    /// Which file generated the event.
    pub file_path: &'static str,
    /// Which line in the file generated the event.
    pub file_line: u32,
}

/// The events that [`Node`] can generate.
///
/// [`Node`]: crate::node::Node
#[derive(Debug, Clone, Serialize)]
#[non_exhaustive]
#[serde(tag = "type")]
#[serde(rename_all = "snake_case")]
pub enum NodeEvent {
    /// Sampling just started.
    SamplingStarted {
        /// The block height that will be sampled.
        height: u64,
        /// The square width of the block.
        square_width: u16,
        /// The coordinates of the shares that will be sampled.
        shares: Vec<(u16, u16)>,
    },

    /// A share was sampled.
    ShareSamplingResult {
        /// The block height of the share.
        height: u64,
        /// The square width of the block.
        square_width: u16,
        /// The row of the share.
        row: u16,
        /// The column of the share.
        column: u16,
        /// The result of the sampling of the share.
        accepted: bool,
    },

    /// Sampling just finished.
    SamplingFinished {
        /// The block height that was sampled.
        height: u64,
        /// The overall result of the sampling.
        accepted: bool,
        /// How much time sampling took.
        took: Duration,
    },

    /// Data sampling fatal error.
    FatalDaserError {
        /// A human readable error.
        error: String,
    },
}

impl fmt::Display for NodeEvent {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            NodeEvent::SamplingStarted {
                height,
                square_width,
                shares,
            } => {
                write!(f, "Sampling for {height} block started. Square: {square_width}x{square_width}, Shares: {shares:?}.")
            }
            NodeEvent::ShareSamplingResult {
                height,
                row,
                column,
                accepted,
                ..
            } => {
                let acc = if *accepted { "accepted" } else { "rejected" };
                write!(
                    f,
                    "Sampling for share [{row}, {column}] of {height} block was {acc}."
                )
            }
            NodeEvent::SamplingFinished {
                height,
                accepted,
                took,
            } => {
                let acc = if *accepted { "accepted" } else { "rejected" };
                write!(
                    f,
                    "Sampling for {height} block finished and {acc}. Took {took:?}."
                )
            }
            NodeEvent::FatalDaserError { error } => {
                write!(f, "Daser stopped because of a fatal error: {error}")
            }
        }
    }
}

#[cfg(target_arch = "wasm32")]
fn serialize_system_time<S>(value: &SystemTime, serializer: S) -> Result<S::Ok, S::Error>
where
    S: serde::ser::Serializer,
{
    // Javascript express time as f64 and in milliseconds.
    let js_time = value
        .duration_since(SystemTime::UNIX_EPOCH)
        .expect("SystemTime is before 1970")
        .as_secs_f64()
        * 1000.0;
    js_time.serialize(serializer)
}
